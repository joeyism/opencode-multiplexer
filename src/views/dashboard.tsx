import React from "react"
import { Box, Text, useStdout, useInput } from "ink"
import TextInput from "ink-text-input"
import {
  type OcmInstance,
  type OcmSession,
  type SessionStatus,
  useStore,
} from "../store.js"
import { buildRows, getNavigableIndices, type VisibleRow } from "./tree-rows.js"
import { useDashboardKeys } from "../hooks/use-keybindings.js"
import { yieldToOpencode } from "../hooks/use-attach.js"
import { config } from "../config.js"
import { refreshNow, shortenModel, deriveRepoName } from "../poller.js"
import { ensureServeProcess, killInstance, untrackSession } from "../registry/instances.js"
import { statusIcon, relativeTime } from "./helpers.js"
import { APP_BORDER_COLS } from "./layout.js"
import {
  getAllSessions,
  type DbSessionWithProject,
  getChildSessions,
  countChildSessions,
  hasChildSessions,
  getSessionStatus,
  getLastMessagePreview,
  getSessionModel,
} from "../db/reader.js"
import { usePickerOverlay } from "../hooks/use-picker-overlay.js"
import { PickerOverlay } from "./picker-overlay.js"

// ─── Status helpers ───────────────────────────────────────────────────────────

const STATUS_ORDER: Record<SessionStatus, number> = {
  "needs-input": 0,
  error: 1,
  working: 2,
  idle: 3,
}

// ─── Child session builder ────────────────────────────────────────────────────

function buildChildOcmSession(c: { id: string; projectId: string; title: string; directory: string; timeUpdated: number }): OcmSession {
  const preview = getLastMessagePreview(c.id)
  return {
    id: c.id,
    projectId: c.projectId,
    title: c.title,
    directory: c.directory,
    status: getSessionStatus(c.id),
    lastMessagePreview: preview.text,
    lastMessageRole: preview.role,
    model: (() => { const m = getSessionModel(c.id); return m ? shortenModel(m) : null })(),
    timeUpdated: c.timeUpdated,
    hasChildren: hasChildSessions(c.id),
  }
}

// ─── Time helpers ─────────────────────────────────────────────────────────────

// ─── Agent type extraction ────────────────────────────────────────────────────

function extractAgentType(title: string): string {
  const match = title.match(/\(@(\w+)\s+subagent\)$/)
  if (match) return `${match[1]}`
  if (title.startsWith("Task:")) return "task"
  if (title.startsWith("Background:")) return "bg"
  return "agent"
}

function cleanTitle(title: string): string {
  let cleaned = title.replace(/\s*\(@\w+\s+subagent\)\s*$/, "").trim()
  cleaned = cleaned.replace(/^Task:\s*/, "").trim()
  cleaned = cleaned.replace(/^Background:\s*/, "").trim()
  return cleaned
}

// ─── Dashboard component ──────────────────────────────────────────────────────

export function Dashboard() {
  const instances = useStore((s) => s.instances)
  const cursorIndex = useStore((s) => s.cursorIndex)
  const setCursorIndex = useStore((s) => s.setCursorIndex)
  const navigate = useStore((s) => s.navigate)
  const expandedSessions = useStore((s) => s.expandedSessions)
  const toggleExpanded = useStore((s) => s.toggleExpanded)
  const collapseSession = useStore((s) => s.collapseSession)
  const childSessions = useStore((s) => s.childSessions)
  const childScrollOffsets = useStore((s) => s.childScrollOffsets)
  const setChildSessions = useStore((s) => s.setChildSessions)
  const setChildScrollOffset = useStore((s) => s.setChildScrollOffset)
  const pinnedSessions = useStore((s) => s.pinnedSessions)
  const togglePin = useStore((s) => s.togglePin)
  const { stdout } = useStdout()
  const termWidth = stdout?.columns ?? 80
  const effectiveWidth = termWidth - APP_BORDER_COLS   // account for App border (left + right)

  const [showHelp, setShowHelp] = React.useState(false)
  const [killConfirm, setKillConfirm] = React.useState<OcmInstance | null>(null)
  const [titleRenameTarget, setTitleRenameTarget] = React.useState<{
    sessionId: string
    port: number | null
    worktree: string
    currentTitle: string
  } | null>(null)
  const [titleText, setTitleText] = React.useState("")
  const [titleStatus, setTitleStatus] = React.useState<string | null>(null)
  // Session picker overlay
  const [sessionPickerOpen, setSessionPickerOpen] = React.useState(false)
  const [sessionPickerSessions, setSessionPickerSessions] = React.useState<DbSessionWithProject[]>([])
  const [sessionPickerLoading, setSessionPickerLoading] = React.useState(false)
  const sessionPickerMaxVisible = Math.min(20, Math.max(5, (stdout?.rows ?? 24) - 10))
  const sessionPicker = usePickerOverlay(sessionPickerOpen, {
    items: sessionPickerSessions,
    filterFn: (s, q) => {
      const lower = q.toLowerCase()
      const repo = deriveRepoName(s.projectWorktree).toLowerCase()
      return s.title.toLowerCase().includes(lower) || repo.includes(lower) || s.directory.toLowerCase().includes(lower)
    },
    onSelect: (session) => {
      if (sessionPickerLoading) return
      setSessionPickerOpen(false)
      setSessionPickerLoading(true)
      ensureServeProcess(session.projectWorktree)
        .then(() => navigate("conversation", session.projectId, session.id))
        .catch((e) => {
          setTitleStatus(`✗ Failed to start session: ${String(e)}`)
          setTimeout(() => setTitleStatus(null), 2500)
        })
        .finally(() => setSessionPickerLoading(false))
    },
    onClose: () => setSessionPickerOpen(false),
    maxVisible: sessionPickerMaxVisible,
  })

  // Kill confirmation input handler
  useInput((input, key) => {
    // Ctrl-C: exit app (exitOnCtrlC is disabled globally so we handle it here)
    if (key.ctrl && input === "c") {
      process.exit(0)
    }
    if (titleRenameTarget) {
      if (key.escape || (key.ctrl && input === "c")) {
        setTitleRenameTarget(null)
        setTitleText("")
      }
      return
    }
    if (!killConfirm) return
    if (input === "y" || input === "Y") {
      const inst = killConfirm
      setKillConfirm(null)
      killInstance(inst.worktree, inst.sessionId)
      untrackSession(inst.sessionId)
      refreshNow()
    } else if (input === "n" || input === "N" || key.escape) {
      setKillConfirm(null)
    }
  })

  useInput((input, key) => {
    if (sessionPickerOpen) {
      sessionPicker.handleInput(input, key)
    }
  })

  const doRenameTitle = React.useCallback(async (newTitle: string) => {
    if (!titleRenameTarget || !newTitle.trim()) return
    try {
      let port = titleRenameTarget.port
      if (!port) {
        port = await ensureServeProcess(titleRenameTarget.worktree)
      }
      if (!port) throw new Error("unable to determine serve port")
      const res = await fetch(
        `http://localhost:${port}/session/${titleRenameTarget.sessionId}`,
        {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ title: newTitle.trim() }),
          signal: AbortSignal.timeout(3000),
        },
      )
      if (!res.ok) {
        const text = await res.text().catch(() => "")
        throw new Error(`HTTP ${res.status}: ${text}`)
      }
      setTitleStatus("✓ title updated")
      setTimeout(() => setTitleStatus(null), 1500)
      refreshNow()
    } catch (e) {
      setTitleStatus(`✗ ${String(e)}`)
      setTimeout(() => setTitleStatus(null), 2500)
    } finally {
      setTitleRenameTarget(null)
      setTitleText("")
    }
  }, [titleRenameTarget])

  const visibleRows = React.useMemo(
    () => buildRows(instances, expandedSessions, childSessions, childScrollOffsets),
    [instances, expandedSessions, childSessions, childScrollOffsets],
  )

  const navigableIndices = React.useMemo(() => getNavigableIndices(visibleRows), [visibleRows])

  const rowToNavIdx = React.useMemo(() => {
    const map = new Map<number, number>()
    navigableIndices.forEach((rowIdx, navIdx) => map.set(rowIdx, navIdx))
    return map
  }, [navigableIndices])

  const safeNavIndex = Math.min(cursorIndex, Math.max(0, navigableIndices.length - 1))
  const safeRowIndex = navigableIndices[safeNavIndex] ?? 0
  const currentRow = visibleRows[safeRowIndex]

  const statusCounts = React.useMemo(() => {
    const statuses = navigableIndices.map((i) => {
      const row = visibleRows[i]
      if (!row) return "idle" as SessionStatus
      if (row.kind === "instance") return row.instance.status
      if (row.kind === "child") return row.session.status
      return "idle" as SessionStatus
    })
    return {
      working: statuses.filter((s) => s === "working").length,
      needsInput: statuses.filter((s) => s === "needs-input").length,
      error: statuses.filter((s) => s === "error").length,
    }
  }, [visibleRows, navigableIndices])

  const attentionNavIndices = React.useMemo(
    () =>
      navigableIndices
        .map((rowIdx, navIdx) => {
          const row = visibleRows[rowIdx]
          if (!row) return -1
          const status =
            row.kind === "instance" ? row.instance.status
            : row.kind === "child" ? row.session.status
            : "idle"
          return status === "needs-input" ? navIdx : -1
        })
        .filter((i) => i >= 0),
    [visibleRows, navigableIndices],
  )

  useDashboardKeys({
    onUp: () => {
      if (navigableIndices.length === 0) return
      const currentRowIdx = navigableIndices[safeNavIndex] ?? 0
      const currentVisRow = visibleRows[currentRowIdx]

      if (currentVisRow?.kind === "child") {
        const rowAbove = visibleRows[currentRowIdx - 1]
        if (rowAbove?.kind === "scroll-indicator" && rowAbove.direction === "above") {
          const parentId = rowAbove.parentSessionId
          const currentOffset = childScrollOffsets.get(parentId) ?? 0
          if (currentOffset > 0) {
            const newOffset = currentOffset - 1
            setChildScrollOffset(parentId, newOffset)
            const children = getChildSessions(parentId, 10, newOffset)
            const totalCount = childSessions.get(parentId)?.totalCount ?? 0
            setChildSessions(parentId, children.map(buildChildOcmSession), totalCount)
            return
          }
        }
      }
      setCursorIndex(Math.max(0, safeNavIndex - 1))
    },
    onDown: () => {
      if (navigableIndices.length === 0) return
      const currentRowIdx = navigableIndices[safeNavIndex] ?? 0
      const currentVisRow = visibleRows[currentRowIdx]

      if (currentVisRow?.kind === "child") {
        const rowBelow = visibleRows[currentRowIdx + 1]
        if (rowBelow?.kind === "scroll-indicator" && rowBelow.direction === "below") {
          const parentId = rowBelow.parentSessionId
          const currentOffset = childScrollOffsets.get(parentId) ?? 0
          const totalCount = childSessions.get(parentId)?.totalCount ?? 0
          if (currentOffset + 10 < totalCount) {
            const newOffset = currentOffset + 1
            setChildScrollOffset(parentId, newOffset)
            const children = getChildSessions(parentId, 10, newOffset)
            setChildSessions(parentId, children.map(buildChildOcmSession), totalCount)
            return
          }
        }
      }
      setCursorIndex(Math.min(navigableIndices.length - 1, safeNavIndex + 1))
    },
    onOpen: () => {
      if (!currentRow) return
      if (currentRow.kind === "instance") {
        navigate("conversation", currentRow.instance.projectId, currentRow.instance.sessionId)
      } else if (currentRow.kind === "child") {
        navigate("conversation", currentRow.session.projectId, currentRow.session.id)
      }
    },
    onAttach: () => {
      if (!currentRow) return
      if (currentRow.kind === "instance") {
        yieldToOpencode(currentRow.instance.sessionId, currentRow.instance.worktree, currentRow.instance.port)
      } else if (currentRow.kind === "child") {
        yieldToOpencode(currentRow.session.id, currentRow.session.directory)
      }
    },
    onExpand: () => {
      if (!currentRow) return
      const loadAndExpand = (sessionId: string) => {
        toggleExpanded(sessionId)
        if (!expandedSessions.has(sessionId)) {
          const children = getChildSessions(sessionId, 10, 0)
          const totalCount = countChildSessions(sessionId)
          setChildSessions(sessionId, children.map(buildChildOcmSession), totalCount)
        }
      }
      if (currentRow.kind === "instance" && currentRow.instance.hasChildren) {
        loadAndExpand(currentRow.instance.sessionId)
      } else if (currentRow.kind === "child" && currentRow.session.hasChildren) {
        loadAndExpand(currentRow.session.id)
      }
    },
    onCollapse: () => {
      if (!currentRow) return
      if (currentRow.kind === "instance") {
        collapseSession(currentRow.instance.sessionId)
      } else if (currentRow.kind === "child") {
        collapseSession(currentRow.session.id)
      }
    },
    onWorktree: () => navigate("worktree"),
    onSpawn: () => navigate("spawn"),
    onNextNeedsInput: () => {
      if (attentionNavIndices.length === 0) return
      const next = attentionNavIndices.find((i) => i > safeNavIndex) ?? attentionNavIndices[0]!
      setCursorIndex(next)
    },
    onKill: () => {
      if (!currentRow) return
      if (currentRow.kind === "instance") {
        setKillConfirm(currentRow.instance)
      }
    },
    onRenameTitle: () => {
      if (!currentRow) return
      if (currentRow.kind === "instance") {
        setTitleRenameTarget({
          sessionId: currentRow.instance.sessionId,
          port: currentRow.instance.port,
          worktree: currentRow.instance.worktree,
          currentTitle: currentRow.instance.sessionTitle,
        })
        setTitleText(currentRow.instance.sessionTitle)
      } else if (currentRow.kind === "child") {
        const parentInst = (() => {
          for (let i = safeRowIndex; i >= 0; i--) {
            const row = visibleRows[i]
            if (row?.kind === "instance") {
              return row.instance
            }
          }
          return undefined
        })()
        if (!parentInst) return
        setTitleRenameTarget({
          sessionId: currentRow.session.id,
          port: parentInst.port,
          worktree: parentInst.worktree,
          currentTitle: currentRow.session.title,
        })
        setTitleText(currentRow.session.title)
      }
    },
    onRescan: () => { refreshNow() },
    onHelp: () => setShowHelp((v) => !v),
    onTogglePin: () => {
      if (!currentRow || currentRow.kind === "scroll-indicator") return
      const sessionId = currentRow.kind === "instance" ? currentRow.instance.sessionId : currentRow.session.id
      togglePin(sessionId)
      refreshNow()
    },
    onSessions: () => {
      const sessions = getAllSessions(500)
      setSessionPickerSessions(sessions)
      setSessionPickerOpen(true)
    },
    onQuit: () => {
      if (showHelp) { setShowHelp(false); return }
      if (killConfirm) { setKillConfirm(null); return }
      process.exit(0)
    },
  }, !titleRenameTarget && !sessionPickerOpen)

  const kb = config.keybindings.dashboard

  // ASCII logo — only show when terminal is tall enough (≥15 rows)
  const showLogo = (stdout?.rows ?? 24) >= 20
  const LOGO = [
    " █▀▀█ █▀▀▀ █▄▀▄█ █  █ ▄  ▄",
    " █  █ █___ █ ▀ █ █__█ _▀▀_",
    " ▀▀▀▀ ▀▀▀▀ ▀   ▀ ▀▀▀▀ ▀  ▀",
  ]

  return (
    <Box flexDirection="column">
      {/* ASCII logo */}
      {showLogo && (
        <Box flexDirection="column" paddingLeft={1}>
          {LOGO.map((line, i) => (
            <Text key={i} color="cyan">{line}</Text>
          ))}
        </Box>
      )}

      {/* Status bar — top border only, no redundant "OCMux" when logo is visible */}
      <Box paddingX={1} paddingY={0} borderStyle="single" borderTop={false} borderLeft={false} borderRight={false} borderColor="gray">
        {!showLogo && <><Text bold color="cyan">OCMux</Text><Text dimColor>  │  </Text></>}
        <Text bold>{instances.length}</Text><Text dimColor> {instances.length === 1 ? "instance" : "instances"}</Text>
        {statusCounts.working > 0 && <Text><Text dimColor>  │  </Text><Text color="green">▶ {statusCounts.working} working</Text></Text>}
        {statusCounts.needsInput > 0 && <Text><Text dimColor>  │  </Text><Text color="yellow">● {statusCounts.needsInput} needs input</Text></Text>}
        {statusCounts.error > 0 && <Text><Text dimColor>  │  </Text><Text color="red">✖ {statusCounts.error} error</Text></Text>}
      </Box>

      {/* Help overlay and session picker */}
      {showHelp ? (
        <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="round" borderColor="cyan" marginX={2} marginY={1}>
          <Box marginBottom={1}><Text bold color="cyan">Dashboard Keybindings</Text></Box>
          <Box flexDirection="column" paddingLeft={2}>
            <Box><Box width={12}><Text bold color="white">{kb.up}/{kb.down}</Text></Box><Text dimColor>navigate</Text></Box>
            <Box><Box width={12}><Text bold color="white">Enter</Text></Box><Text dimColor>open conversation</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.attach}</Text></Box><Text dimColor>open in opencode (attach)</Text></Box>
            <Box><Box width={12}><Text bold color="white">Tab/S-Tab</Text></Box><Text dimColor>expand/collapse subagents</Text></Box>
            <Box><Box width={12}><Text bold color="white">Ctrl-N</Text></Box><Text dimColor>jump to next needs-input</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.spawn}</Text></Box><Text dimColor>spawn new opencode (background)</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.worktree}</Text></Box><Text dimColor>create worktree session</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.kill}</Text></Box><Text dimColor>kill selected instance</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.rescan}</Text></Box><Text dimColor>refresh from database</Text></Box>
            <Box><Box width={12}><Text bold color="white">t</Text></Box><Text dimColor>rename title</Text></Box>
            <Box><Box width={12}><Text bold color="white">p</Text></Box><Text dimColor>pin/unpin to top</Text></Box>
            <Box><Box width={12}><Text bold color="white">s</Text></Box><Text dimColor>browse past sessions</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.help}</Text></Box><Text dimColor>close help</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.quit}</Text></Box><Text dimColor>quit</Text></Box>
          </Box>
          <Box marginTop={1}>
            <Text dimColor>Press </Text><Text bold color="white">{kb.help}</Text><Text dimColor> or </Text><Text bold color="white">{kb.quit}</Text><Text dimColor> to close</Text>
          </Box>
        </Box>
      ) : sessionPickerOpen ? (
        <PickerOverlay
          title="Sessions"
          state={sessionPicker.state}
          maxVisible={sessionPickerMaxVisible}
          width={effectiveWidth}
          renderItem={(session, i, isCursor) => {
            const repo = deriveRepoName(session.projectWorktree)
            const timeAgo = relativeTime(session.timeUpdated)
            const isRunning = instances.some((inst) => inst.sessionId === session.id)
            const label = `${repo} / ${session.title}`
            const maxLabelLen = Math.max(20, effectiveWidth - 16 - timeAgo.length)
            const truncLabel = label.length > maxLabelLen ? label.slice(0, maxLabelLen - 1) + "…" : label
            return (
              <Box paddingLeft={1} key={session.id}>
                <Text color={isCursor ? "cyan" : "gray"}>{isCursor ? "▸" : " "} </Text>
                {isRunning ? <Text color="green">▶ </Text> : <Text dimColor>○ </Text>}
                <Text bold={isCursor} dimColor={!isCursor}>{truncLabel}</Text>
                <Text dimColor>  {timeAgo}</Text>
              </Box>
            )
          }}
        />
      ) : (
        <>
          {instances.length === 0 && (
            <Box paddingX={2} paddingY={1} borderStyle="round" borderColor="gray" marginX={2} marginY={1}>
              <Text dimColor>No opencode instances running. Press </Text>
              <Text bold color="cyan">{kb.spawn}</Text>
              <Text dimColor> to start one.</Text>
            </Box>
          )}

          {visibleRows.map((row, rowIdx) => {
            const navIdx = rowToNavIdx.get(rowIdx) ?? -1
            const isCursor = navIdx >= 0 && navIdx === safeNavIndex

            if (row.kind === "scroll-indicator") {
              const indent = "  ".repeat(row.depth)
              const arrow = row.direction === "above" ? "▲" : "▼"
              return (
                <Box key={`scroll-${row.direction}-${row.parentSessionId}-${row.depth}`} paddingLeft={1}>
                  <Text>  </Text>
                  <Text dimColor>{indent + "⋮ "}</Text>
                  <Box paddingLeft={1}>
                    <Text dimColor>{arrow + " " + row.count} {row.direction === "above" ? "above" : "more below"}</Text>
                  </Box>
                </Box>
              )
            }

            if (row.kind === "instance") {
              const { char, color } = statusIcon(row.instance.status)
              const isPinned = pinnedSessions.has(row.instance.sessionId)
              const canExpand = row.instance.hasChildren
              const isExpanded = expandedSessions.has(row.instance.sessionId)
              const expandChar = !canExpand ? "  " : isExpanded ? "▾ " : "▸ "

              let preview = (row.instance.lastPreview || "").replace(/\r?\n/g, " ")
              if (row.instance.status === "working" && !preview) preview = "working..."

              // Fixed prefix: paddingLeft(1) + cursor(2) + icon(2) + expand(2) = 7 chars
              // Plus pin icon(2) if pinned
              const prefixLen = isPinned ? 9 : 7
              const model = row.instance.model
              const timeAgoStr = relativeTime(row.instance.timeUpdated)
              const modelStr = model ? model + "  " : ""
              const modelLen = modelStr.length
              const timeLen = timeAgoStr.length + 2  // "  2m"
              const labelLen = Math.min(36, Math.floor((effectiveWidth - prefixLen - modelLen - timeLen - 2) * 0.55))
              const label = `${row.instance.repoName} / ${row.instance.sessionTitle}`
              const truncLabel = label.length > labelLen ? label.slice(0, labelLen - 1) + "…" : label.padEnd(labelLen)
              const previewLen = Math.max(0, effectiveWidth - prefixLen - labelLen - modelLen - timeLen - 2)
              const truncPreview = preview.length > previewLen ? preview.slice(0, Math.max(0, previewLen - 1)) + (previewLen > 1 ? "…" : "") : preview

              return (
                <Box key={row.instance.id} paddingLeft={1}>
                  <Text color={isCursor ? "cyan" : undefined} bold={isCursor}>{isCursor ? "┃ " : "  "}</Text>
                  {isPinned && <Text color="yellow">⊤ </Text>}
                  <Text color={color}>{char} </Text>
                  <Text dimColor>{expandChar}</Text>
                  <Text bold={isCursor} color={isPinned ? "yellow" : undefined}>{truncLabel}</Text>
                  {model && <Text color="cyan" dimColor>  {model}</Text>}
                  <Text dimColor>  {timeAgoStr}</Text>
                  <Text dimColor wrap="truncate">  {truncPreview}</Text>
                </Box>
              )
            }

            if (row.kind === "child") {
              const { char, color } = statusIcon(row.session.status)
              const isPinned = pinnedSessions.has(row.session.id)
              const indent = "  ".repeat(row.depth)
              const treeChar = row.isLast ? "└─ " : "├─ "
              const expandIndicator = row.session.hasChildren
                ? (expandedSessions.has(row.session.id) ? " ▾" : " ▸")
                : ""
              const timeAgo = relativeTime(row.session.timeUpdated)

              const titleLen = 30
              const truncChildTitle = row.cleanedTitle.length > titleLen
                ? row.cleanedTitle.slice(0, titleLen - 1) + "…"
                : row.cleanedTitle.padEnd(titleLen)
              const badge = ("[" + row.agentType + "]").padEnd(10).slice(0, 10)

              return (
                <Box key={`child-${row.parentSessionId}-${row.session.id}`} paddingLeft={1}>
                  <Text color={isCursor ? "cyan" : undefined} bold={isCursor}>{isCursor ? "┃ " : "  "}</Text>
                  {isPinned && <Text color="yellow">⊤ </Text>}
                  <Text dimColor>{indent + treeChar}</Text>
                  <Text color={color}>{char} </Text>
                  <Text color="magenta">{badge}</Text>
                  <Text bold={isCursor} color={isPinned ? "yellow" : undefined}> {truncChildTitle}</Text>
                  {row.session.model && <Text color="cyan" dimColor>  {row.session.model}</Text>}
                  <Text dimColor>{expandIndicator}  {timeAgo}</Text>
                </Box>
              )
            }

            return null
          })}

          {titleRenameTarget && (
            <Box paddingX={1} borderStyle="single" borderColor="yellow" marginTop={1}>
              <Text color="yellow" bold>rename › </Text>
              <TextInput
                value={titleText}
                onChange={setTitleText}
                onSubmit={(text) => { void doRenameTitle(text) }}
                placeholder="new title..."
                focus={true}
              />
            </Box>
          )}
          {titleStatus && (
            <Box paddingX={1}>
              <Text color={titleStatus.startsWith("✓") ? "green" : "red"}>{titleStatus}</Text>
            </Box>
          )}

          {killConfirm ? (
            <Box marginTop={1} paddingX={2} paddingY={0} borderStyle="single" borderColor="red">
              <Text color="red">Kill </Text>
              <Text bold color="red">
                {killConfirm.repoName} / {killConfirm.sessionTitle.slice(0, 40)}
              </Text>
              <Text color="red">? </Text>
              <Text bold color="white">y</Text>
              <Text dimColor> confirm  </Text>
              <Text bold color="white">n</Text>
              <Text dimColor>/</Text>
              <Text bold color="white">Esc</Text>
              <Text dimColor> cancel</Text>
            </Box>
          ) : (
            <Box marginTop={1} paddingX={1} paddingY={0} borderStyle="single" borderBottom={false} borderLeft={false} borderRight={false} borderColor="gray">
              <Box flexGrow={1} gap={3}>
                <Text><Text bold color="white">{kb.up}/{kb.down}</Text> <Text dimColor>nav</Text></Text>
                <Text><Text bold color="white">Enter</Text> <Text dimColor>open</Text></Text>
                <Text><Text bold color="white">Tab</Text> <Text dimColor>expand</Text></Text>
                <Text><Text bold color="white">{kb.attach}</Text> <Text dimColor>attach</Text></Text>
              </Box>
               <Box gap={3}>
                 <Text><Text bold color="white">{kb.spawn}</Text> <Text dimColor>new</Text></Text>
                 <Text><Text bold color="white">{kb.worktree}</Text> <Text dimColor>worktree</Text></Text>
                 <Text><Text bold color="white">s</Text> <Text dimColor>sessions</Text></Text>
                 <Text><Text bold color="white">{kb.kill}</Text> <Text dimColor>kill</Text></Text>
                  <Text><Text bold color="white">t</Text> <Text dimColor>rename</Text></Text>
                  <Text><Text bold color="white">p</Text> <Text dimColor>pin</Text></Text>
                  <Text><Text bold color="white">?</Text> <Text dimColor>help</Text></Text>
                 <Text><Text bold color="white">{kb.quit}</Text> <Text dimColor>quit</Text></Text>
               </Box>
            </Box>
          )}
        </>
      )}
    </Box>
  )
}
