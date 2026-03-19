import React from "react"
import { Box, Text, useStdout, useInput } from "ink"
import {
  type OcmInstance,
  type OcmSession,
  type SessionStatus,
  useStore,
} from "../store.js"
import { useDashboardKeys } from "../hooks/use-keybindings.js"
import { yieldToOpencode } from "../hooks/use-attach.js"
import { config } from "../config.js"
import { refreshNow, shortenModel } from "../poller.js"
import { killInstance } from "../registry/instances.js"
import { statusIcon } from "./helpers.js"
import {
  getChildSessions,
  countChildSessions,
  hasChildSessions,
  getSessionStatus,
  getLastMessagePreview,
  getSessionModel,
} from "../db/reader.js"

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

function relativeTime(ts: number): string {
  const seconds = Math.floor((Date.now() - ts) / 1000)
  if (seconds < 60) return "just now"
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  return `${days}d ago`
}

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

// ─── Flat row model ───────────────────────────────────────────────────────────

type InstanceRow = {
  kind: "instance"
  instance: OcmInstance
}

type ChildRow = {
  kind: "child"
  session: OcmSession
  agentType: string
  cleanedTitle: string
  depth: number
  isLast: boolean
  parentSessionId: string
}

type ScrollIndicatorRow = {
  kind: "scroll-indicator"
  direction: "above" | "below"
  count: number
  depth: number
  parentSessionId: string
}

type VisibleRow = InstanceRow | ChildRow | ScrollIndicatorRow

// ─── Build visible rows ───────────────────────────────────────────────────────

function buildRows(
  instances: OcmInstance[],
  expandedSessions: Set<string>,
  childSessions: Map<string, { children: OcmSession[]; totalCount: number }>,
  childScrollOffsets: Map<string, number>,
): VisibleRow[] {
  const rows: VisibleRow[] = []

  for (const instance of instances) {
    rows.push({ kind: "instance", instance })

    if (expandedSessions.has(instance.sessionId)) {
      insertChildren(rows, instance.sessionId, 1, expandedSessions, childSessions, childScrollOffsets)
    }
  }

  return rows
}

function insertChildren(
  rows: VisibleRow[],
  parentSessionId: string,
  depth: number,
  expandedSessions: Set<string>,
  childSessions: Map<string, { children: OcmSession[]; totalCount: number }>,
  childScrollOffsets: Map<string, number>,
): void {
  const data = childSessions.get(parentSessionId)
  if (!data) return

  const { children, totalCount } = data
  const offset = childScrollOffsets.get(parentSessionId) ?? 0

  if (offset > 0) {
    rows.push({ kind: "scroll-indicator", direction: "above", count: offset, depth, parentSessionId })
  }

  children.forEach((child, i) => {
    const isLast = i === children.length - 1 && offset + children.length >= totalCount
    rows.push({
      kind: "child",
      session: child,
      agentType: extractAgentType(child.title),
      cleanedTitle: cleanTitle(child.title),
      depth,
      isLast,
      parentSessionId,
    })
    if (expandedSessions.has(child.id)) {
      insertChildren(rows, child.id, depth + 1, expandedSessions, childSessions, childScrollOffsets)
    }
  })

  const remaining = totalCount - offset - children.length
  if (remaining > 0) {
    rows.push({ kind: "scroll-indicator", direction: "below", count: remaining, depth, parentSessionId })
  }
}

// Navigable rows exclude scroll indicators
function getNavigableIndices(rows: VisibleRow[]): number[] {
  return rows
    .map((r, i) => (r.kind === "scroll-indicator" ? -1 : i))
    .filter((i) => i >= 0)
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
  const { stdout } = useStdout()
  const termWidth = stdout?.columns ?? 80

  const [showHelp, setShowHelp] = React.useState(false)
  const [killConfirm, setKillConfirm] = React.useState<OcmInstance | null>(null)

  // Kill confirmation input handler
  useInput((input, key) => {
    if (!killConfirm) return
    if (input === "y" || input === "Y") {
      killInstance(killConfirm.worktree, killConfirm.sessionId)
      setKillConfirm(null)
      refreshNow()
    } else if (input === "n" || input === "N" || key.escape) {
      setKillConfirm(null)
    }
  })

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
    onRescan: () => { refreshNow() },
    onHelp: () => setShowHelp((v) => !v),
    onQuit: () => {
      if (showHelp) { setShowHelp(false); return }
      if (killConfirm) { setKillConfirm(null); return }
      process.exit(0)
    },
  })

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

      {/* Status bar */}
      <Box paddingX={2} paddingY={0} borderStyle="single" borderColor="gray">
        <Text bold color="cyan">OCMux</Text>
        <Text dimColor>  │  </Text>
        <Text bold>{instances.length}</Text><Text dimColor> {instances.length === 1 ? "instance" : "instances"}</Text>
        {statusCounts.working > 0 && <Text><Text dimColor>  │  </Text><Text color="green">▶ {statusCounts.working} working</Text></Text>}
        {statusCounts.needsInput > 0 && <Text><Text dimColor>  │  </Text><Text color="yellow">● {statusCounts.needsInput} needs input</Text></Text>}
        {statusCounts.error > 0 && <Text><Text dimColor>  │  </Text><Text color="red">✖ {statusCounts.error} error</Text></Text>}
      </Box>

      {/* Help overlay */}
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
            <Box><Box width={12}><Text bold color="white">{kb.kill}</Text></Box><Text dimColor>kill selected instance</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.rescan}</Text></Box><Text dimColor>refresh from database</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.help}</Text></Box><Text dimColor>close help</Text></Box>
            <Box><Box width={12}><Text bold color="white">{kb.quit}</Text></Box><Text dimColor>quit</Text></Box>
          </Box>
          <Box marginTop={1}>
            <Text dimColor>Press </Text><Text bold color="white">{kb.help}</Text><Text dimColor> or </Text><Text bold color="white">{kb.quit}</Text><Text dimColor> to close</Text>
          </Box>
        </Box>
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
              const canExpand = row.instance.hasChildren
              const isExpanded = expandedSessions.has(row.instance.sessionId)
              const expandChar = !canExpand ? "  " : isExpanded ? "▾ " : "▸ "

              let preview = row.instance.lastPreview
              if (row.instance.status === "working" && !preview) preview = "working..."

              // Fixed prefix: paddingLeft(1) + cursor(2) + icon(2) + expand(2) = 7 chars
              const model = row.instance.model
              const modelStr = model ? model + "  " : ""
              const modelLen = modelStr.length
              const labelLen = Math.min(36, Math.floor((termWidth - 7 - modelLen - 2) * 0.55))
              const label = `${row.instance.repoName} / ${row.instance.sessionTitle}`
              const truncLabel = label.length > labelLen ? label.slice(0, labelLen - 1) + "…" : label.padEnd(labelLen)
              const previewLen = Math.max(0, termWidth - 7 - labelLen - modelLen - 2)
              const truncPreview = preview.length > previewLen ? preview.slice(0, Math.max(0, previewLen - 1)) + (previewLen > 1 ? "…" : "") : preview

              return (
                <Box key={row.instance.id} paddingLeft={1}>
                  <Text color={isCursor ? "cyan" : undefined} bold={isCursor}>{isCursor ? "┃ " : "  "}</Text>
                  <Text color={color}>{char} </Text>
                  <Text dimColor>{expandChar}</Text>
                  <Text bold={isCursor}>{truncLabel}</Text>
                  {model && <Text color="cyan" dimColor>  {model}</Text>}
                  <Text dimColor wrap="truncate">  {truncPreview}</Text>
                </Box>
              )
            }

            if (row.kind === "child") {
              const { char, color } = statusIcon(row.session.status)
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
                  <Text dimColor>{indent + treeChar}</Text>
                  <Text color={color}>{char} </Text>
                  <Text color="magenta">{badge}</Text>
                  <Text bold={isCursor}> {truncChildTitle}</Text>
                  {row.session.model && <Text color="cyan" dimColor>  {row.session.model}</Text>}
                  <Text dimColor>{expandIndicator}  {timeAgo}</Text>
                </Box>
              )
            }

            return null
          })}

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
            <Box marginTop={1} paddingX={2} paddingY={0} borderStyle="single" borderColor="gray">
              <Box flexGrow={1}>
                <Text dimColor>
                  <Text bold color="white">{kb.up}/{kb.down}</Text> nav  <Text dimColor>│</Text>{" "}
                  <Text bold color="white">Enter</Text> open  <Text dimColor>│</Text>{" "}
                  <Text bold color="white">Tab</Text> expand  <Text dimColor>│</Text>{" "}
                  <Text bold color="white">{kb.attach}</Text> attach
                </Text>
              </Box>
              <Box>
                <Text dimColor>
                  <Text bold color="white">{kb.spawn}</Text> new  <Text dimColor>│</Text>{" "}
                  <Text bold color="white">{kb.kill}</Text> kill  <Text dimColor>│</Text>{" "}
                  <Text bold color="white">?</Text> help  <Text dimColor>│</Text>{" "}
                  <Text bold color="white">{kb.quit}</Text> quit
                </Text>
              </Box>
            </Box>
          )}
        </>
      )}
    </Box>
  )
}
