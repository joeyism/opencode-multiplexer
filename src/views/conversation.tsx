import React from "react"
import { Box, Text, useInput, useStdout } from "ink"
import TextInput from "ink-text-input"
import { createOpencodeClient } from "@opencode-ai/sdk"
import { useStore, type ConversationMessage } from "../store.js"
import { useConversationKeys } from "../hooks/use-keybindings.js"
import { yieldToOpencode, yieldToShell, openInEditor, openFileInEditor, consumePendingEditorResult } from "../hooks/use-attach.js"
import { getMessages, getSessionById, getSessionStatus, getSessionAgent, getSessionModifiedFiles, getSessionModel, getChildSessionQuestions } from "../db/reader.js"
import { config } from "../config.js"
import { shortenModel, refreshNow } from "../poller.js"
import { ensureServeProcess, killInstance, untrackSession } from "../registry/instances.js"
import { statusIcon, relativeTime, filterFilesForCwd, findDisplayLineMatches, getSearchScrollOffset, highlightMatches } from "./helpers.js"
import { computeConversationLayout, APP_BORDER_COLS } from "./layout.js"


import { buildDisplayLines, type DisplayLine } from "./display-lines.js"
import { buildRows, getNavigableIndices, type VisibleRow } from "./tree-rows.js"
import { PickerOverlay } from "./picker-overlay.js"
import { usePickerOverlay } from "../hooks/use-picker-overlay.js"

function Sidebar({
  rows,
  currentSessionId,
  cursorIndex,
  focused,
  height,
  width,
  compact,
  expandedSessions,
  pinnedSessions,
}: {
  rows: VisibleRow[]
  currentSessionId: string | null
  cursorIndex: number
  focused: boolean
  height: number
  width: number
  compact: boolean
  expandedSessions: Set<string>
  pinnedSessions: Map<string, number>
}) {
  // Inner width: total - 2 border chars
  const innerWidth = Math.max(0, width - 2)
  const separatorLine = "─".repeat(innerWidth)

  return (
    <Box
      flexDirection="column"
      width={width}
      flexShrink={0}
      borderStyle="single"
      borderColor={focused ? "cyan" : "gray"}
      height={height}
      overflow="hidden"
    >
      {!compact && (
        <>
          {/* Header */}
          <Box paddingX={1} justifyContent="space-between">
            <Text bold color={focused ? "cyan" : "gray"}>sessions</Text>
            <Text dimColor>{rows.filter(r => r.kind === "instance").length}</Text>
          </Box>
          <Text dimColor>{separatorLine}</Text>
        </>
      )}

      {/* Instance list */}
      {rows.length === 0 && (
        <Box paddingX={1}>
          <Text dimColor>no instances</Text>
        </Box>
      )}
      {rows.map((row, i) => {
        if (row.kind === "scroll-indicator") {
          const indent = "  ".repeat(row.depth)
          return (
            <Box key={`scroll-${row.parentSessionId}-${row.direction}`} paddingLeft={1}>
              <Text dimColor>{indent}  {row.direction === "above" ? "↑" : "↓"} {row.count} more</Text>
            </Box>
          )
        }

        const isInstance = row.kind === "instance"
        const session = isInstance ? row.instance : row.session
        const sessionId = isInstance ? row.instance.sessionId : row.session.id
        const isCurrent = sessionId === currentSessionId
        const isCursor = focused && i === cursorIndex
        const isPinned = pinnedSessions.has(sessionId)
        const { char, color } = statusIcon(session.status)
        const cursorChar = isCursor ? "▸" : isCurrent ? "◆" : " "
        const expandChar = session.hasChildren
          ? (expandedSessions.has(sessionId) ? "▾" : "▸")
          : " "

        if (compact) {
          const rawTime = relativeTime(session.timeUpdated)
          const pinCols = isPinned ? 2 : 0
          // Overhead: paddingLeft(1) + cursor(1) + space(1) + status(1) + space(1) = 5
          const availableWidth = Math.max(0, innerWidth - 5 - pinCols)
          const timeLabel = availableWidth > 0 ? rawTime.slice(0, availableWidth) : ""
          return (
            <Box key={sessionId} paddingLeft={1} overflow="hidden">
              <Text color={isCursor ? "cyan" : isCurrent ? "white" : "gray"}>{cursorChar}</Text>
              <Text>{" "}</Text>
              {isPinned && <Text color="yellow">⊤ </Text>}
              <Text color={color}>{char}</Text>
              <Text>{" "}</Text>
              <Text
                bold={isCurrent || isCursor}
                color={isCursor ? "cyan" : isPinned ? "yellow" : isCurrent ? "white" : undefined}
                dimColor={!isCurrent && !isCursor && !isPinned}
              >
                {timeLabel}
              </Text>
            </Box>
          )
        }

        const timeAgo = relativeTime(session.timeUpdated)
        const indentWidth = isInstance ? 0 : row.depth * 2
        const pinCols = isPinned ? 2 : 0
        // Overhead: space(1) + cursor(1) + space(1) + status(1) + space(1) + expand(1) + space(1) + space-before-time(1) = 8
        const labelBudget = Math.max(0, innerWidth - indentWidth - 8 - timeAgo.length - pinCols)

        let paddedLabel = ""
        if (isInstance) {
          const sep = "/"
          const repoMax = Math.min(row.instance.repoName.length, Math.floor(labelBudget * 0.4))
          const repo = row.instance.repoName.length > repoMax
            ? row.instance.repoName.slice(0, Math.max(0, repoMax - 1)) + "…"
            : row.instance.repoName
          const titleMax = labelBudget - repo.length - sep.length
          const title = row.instance.sessionTitle.length > titleMax
            ? row.instance.sessionTitle.slice(0, Math.max(0, titleMax - 1)) + "…"
            : row.instance.sessionTitle
          paddedLabel = `${repo}${sep}${title}`.padEnd(labelBudget)
        } else {
          const titleMax = labelBudget
          const title = row.cleanedTitle.length > titleMax
            ? row.cleanedTitle.slice(0, Math.max(0, titleMax - 1)) + "…"
            : row.cleanedTitle
          paddedLabel = title.padEnd(labelBudget)
        }

        const indent = " ".repeat(indentWidth)

        return (
          <Box key={sessionId} height={1} overflow="hidden">
            <Text wrap="truncate">
              {" "}
              <Text color={isCursor ? "cyan" : isCurrent ? "white" : "gray"}>{cursorChar}</Text>
              {" "}
              {indent}
              {isPinned && <Text color="yellow">⊤ </Text>}
              <Text color={color}>{char}</Text>
              {" "}
              <Text dimColor>{expandChar}</Text>
              {" "}
              <Text
                bold={isCurrent || isCursor}
                color={isCursor ? "cyan" : isPinned ? "yellow" : isCurrent ? "white" : undefined}
                dimColor={!isCurrent && !isCursor && !isPinned}
              >{paddedLabel}</Text>
              {" "}
              <Text dimColor>{timeAgo}</Text>
            </Text>
          </Box>
        )
      })}
    </Box>
  )
}

// ─── Conversation component ───────────────────────────────────────────────────

export function Conversation() {
  const messages = useStore((s) => s.messages)
  const messagesLoading = useStore((s) => s.messagesLoading)
  const selectedSessionId = useStore((s) => s.selectedSessionId)
  const instances = useStore((s) => s.instances)
  const navigate = useStore((s) => s.navigate)
  const setMessages = useStore((s) => s.setMessages)
  const setMessagesLoading = useStore((s) => s.setMessagesLoading)
  const expandedSessions = useStore((s) => s.expandedSessions)
  const childSessions = useStore((s) => s.childSessions)
  const childScrollOffsets = useStore((s) => s.childScrollOffsets)
  const toggleExpanded = useStore((s) => s.toggleExpanded)
  const pinnedSessions = useStore((s) => s.pinnedSessions)
  const togglePin = useStore((s) => s.togglePin)

  const rows = React.useMemo(
    () => buildRows(instances, expandedSessions, childSessions, childScrollOffsets),
    [instances, expandedSessions, childSessions, childScrollOffsets]
  )
  const navigableIndices = React.useMemo(() => getNavigableIndices(rows), [rows])

  const [scrollOffset, setScrollOffset] = React.useState(0)
  const [error, setError] = React.useState<string | null>(null)
  const [inputText, setInputText] = React.useState("")
  const [sending, setSending] = React.useState(false)
  const [sendError, setSendError] = React.useState<string | null>(null)
  // vim modal: "normal" (navigate) or "insert" (type into input)
  const [mode, setMode] = React.useState<"normal" | "insert">("normal")
  // Pane focus: "conversation" (default) or "sidebar"
  const [focus, setFocus] = React.useState<"conversation" | "sidebar">("conversation")
  // Auto-spawned serve process port (for non-OCMux sessions)
  const [autoSpawnedPort, setAutoSpawnedPort] = React.useState<number | null>(null)
  const [autoSpawning, setAutoSpawning] = React.useState(false)
  const [autoSpawnError, setAutoSpawnError] = React.useState<string | null>(null)
  // Loading state: covers the gap between promptAsync returning and SSE status arriving
  const [waitingForResponse, setWaitingForResponse] = React.useState(false)
  // Kill confirmation
  const [killConfirm, setKillConfirm] = React.useState<import("../store.js").OcmInstance | null>(null)
  // Help overlay
  const [showHelp, setShowHelp] = React.useState(false)
  // Commit mode
  const [commitMode, setCommitMode] = React.useState(false)
  const [commitText, setCommitText] = React.useState("")
  const [commitStatus, setCommitStatus] = React.useState<string | null>(null)
  const [searchMode, setSearchMode] = React.useState(false)
  const [searchInput, setSearchInput] = React.useState("")
  const [searchQuery, setSearchQuery] = React.useState("")
  const [searchMatchCursor, setSearchMatchCursor] = React.useState(0)
  // Title rename mode
  const [titleMode, setTitleMode] = React.useState(false)
  const [titleText, setTitleText] = React.useState("")
  // For sidebar-initiated renames: which session/port/worktree to target
  const [titleRenameSessionId, setTitleRenameSessionId] = React.useState<string | null>(null)
  const [titleRenamePort, setTitleRenamePort] = React.useState<number | null>(null)
  const [titleRenameWorktree, setTitleRenameWorktree] = React.useState<string | null>(null)
  // Inline question answering
type QuestionOption = { label: string; description?: string }
type PendingQuestion = {
  questions: Array<{ question: string; header: string; options: QuestionOption[]; custom?: boolean }>
}
  const [pendingQuestion, setPendingQuestion] = React.useState<PendingQuestion | null>(null)
  const [questionCursor, setQuestionCursor] = React.useState(0)
  const [questionCustomMode, setQuestionCustomMode] = React.useState(false)
  const [questionCustomText, setQuestionCustomText] = React.useState("")
  // Prevents the auto-open useEffect from re-opening the overlay immediately after
  // submitAnswer closes it — the serve process needs time to update question status in DB.
  const questionAnswered = React.useRef(false)
  // Tracks pendingQuestion + questionAnswered for the setInterval closure (can't see React state)
  const pendingQuestionRef = React.useRef(false)
  const questionOpts = React.useMemo(() => {
    if (!pendingQuestion) return []
    const raw = pendingQuestion.questions[0]?.options ?? []
    return [...raw, { label: "Type your own answer", description: "" }]
  }, [pendingQuestion])
  pendingQuestionRef.current = !!pendingQuestion || questionAnswered.current
  React.useEffect(() => {
    if (questionOpts.length === 0) {
      setQuestionCursor(0)
      return
    }
    setQuestionCursor((c) => Math.min(c, questionOpts.length - 1))
  }, [questionOpts.length])

  // Auto-open/close question overlay from DB-sourced options (handles child session questions
  // which the tui.control.next() serve API doesn't surface)
  React.useEffect(() => {
    const lines = buildDisplayLines(messages)
    const runningQ = lines.find(
      (l): l is Extract<DisplayLine, { kind: "question" }> =>
        l.kind === "question" && l.status === "running" && l.options.length > 0
    )
    if (runningQ && !pendingQuestion && !questionAnswered.current) {
      setPendingQuestion({
        questions: [{
          question: runningQ.question,
          header: runningQ.header,
          options: runningQ.options,
          custom: runningQ.custom,
        }],
      })
      setQuestionCursor(0)
    } else if (!runningQ) {
      // Question completed in DB — reset answered flag and clear overlay if open
      questionAnswered.current = false
      if (pendingQuestion) {
        setPendingQuestion(null)
        setQuestionCustomMode(false)
        setQuestionCustomText("")
      }
    }
  }, [messages, pendingQuestion])

  // Model picker overlay
  const [modelPickerOpen, setModelPickerOpen] = React.useState(false)
  // File picker overlay
  const [filePickerOpen, setFilePickerOpen] = React.useState(false)
  const [filePickerFiles, setFilePickerFiles] = React.useState<string[]>([])
  // Tick counter to force sessionStatus re-read when SSE session events arrive
  const [statusTick, setStatusTick] = React.useState(0)
  // Only update messages state if content actually changed (avoids expensive re-renders)
  const updateMessagesIfChanged = React.useCallback((newMessages: ConversationMessage[]) => {
    const current = useStore.getState().messages
    if (
      newMessages.length === current.length &&
      newMessages[newMessages.length - 1]?.id === current[current.length - 1]?.id &&
      newMessages[newMessages.length - 1]?.parts.length === current[current.length - 1]?.parts.length
    ) return  // nothing changed
    setMessages(newMessages)
  }, [setMessages])

  // Sidebar cursor (index into instances array)
  const [sidebarCursor, setSidebarCursor] = React.useState(0)
  // Ctrl-W combo: track first Ctrl-W press (normal mode only)
  const [pendingCtrlW, setPendingCtrlW] = React.useState(false)
  const pendingCtrlWTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null)

  // Agent/model selection (live instances only)
  type AgentOption = { name: string; model?: { providerID: string; modelID: string } }
  type ModelOption = { providerID: string; modelID: string; label: string }
  const [availableAgents, setAvailableAgents] = React.useState<AgentOption[]>([])
  const [availableModels, setAvailableModels] = React.useState<ModelOption[]>([])
  const [selectedAgentIdx, setSelectedAgentIdx] = React.useState(0)
  const [modelOverrideIdx, setModelOverrideIdx] = React.useState<number | null>(null)
  // For read-only instances: agent from last assistant message
  const [readOnlyAgent, setReadOnlyAgent] = React.useState<string | null>(null)
  // gg combo: track first 'g' press
  const [pendingG, setPendingG] = React.useState(false)
  const pendingGTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  // Ctrl-X E combo (only active in insert mode)
  const [pendingCtrlX, setPendingCtrlX] = React.useState(false)
  const pendingCtrlXTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  // Ref to block TextInput from receiving Ctrl+letter combos.
  // Set to true whenever ctrl is pressed in insert mode — cleared in onChange.
  const ctrlPressed = React.useRef(false)
  // Ref to synchronously block TextInput during multi-key chords (Ctrl-X E, Ctrl-X M).
  // Unlike pendingCtrlX (React state, async), this ref is set immediately in the same
  // tick as the keystroke, preventing the character from leaking into TextInput's onChange.
  const pendingChord = React.useRef(false)

  const { stdout } = useStdout()
  const termWidth = stdout?.columns ?? 80
  const termHeight = stdout?.rows ?? 24
  const effectiveWidth = termWidth - APP_BORDER_COLS   // account for App border (left + right)
  const { innerHeight, bodyHeight } = computeConversationLayout(
    termHeight, termWidth, { killConfirm: !!killConfirm }
  )
  const modelPickerMaxVisible = Math.min(20, Math.max(5, termHeight - 10))
  const modelPicker = usePickerOverlay(modelPickerOpen, {
    items: availableModels,
    filterFn: (m, q) => {
      const lower = q.toLowerCase()
      return m.label.toLowerCase().includes(lower) || m.providerID.toLowerCase().includes(lower)
    },
    onSelect: (_item, originalIdx) => {
      setModelOverrideIdx(originalIdx)
      setModelPickerOpen(false)
    },
    onClose: () => setModelPickerOpen(false),
    maxVisible: modelPickerMaxVisible,
  })
  const [sidebarCompact, setSidebarCompact] = React.useState(false)

  const filePickerMaxVisible = Math.min(20, Math.max(5, termHeight - 10))
  const filePicker = usePickerOverlay(filePickerOpen, {
    items: filePickerFiles,
    filterFn: (f, q) => f.toLowerCase().includes(q.toLowerCase()),
    onSelect: (file) => {
      setFilePickerOpen(false)
      openFileInEditor(file)
    },
    onClose: () => setFilePickerOpen(false),
    maxVisible: filePickerMaxVisible,
    onExtraKey: (input, _key) => {
      if (input === "a") {
        setFilePickerOpen(false)
        openFileInEditor(filePickerFiles)
        return true
      }
      return false
    },
  })

  const sidebarWidthWide = Math.max(20, Math.floor(termWidth * 0.15))
  const sidebarWidthCompact = 10
  const sidebarWidth = sidebarCompact ? sidebarWidthCompact : sidebarWidthWide

  // Track whether this mount was triggered by the editor returning
  const hadEditorResult = React.useRef(false)

  // On mount: check for editor result from openInEditor.
  // The pending result is set before remount, so it's available on first render.
  React.useEffect(() => {
    const pending = consumePendingEditorResult()
    if (pending !== null) {
      hadEditorResult.current = true
      setInputText(pending)
      setMode("insert")
    }
  }, [])  // only fires on initial mount

  // Load messages when session changes, reset state for fresh navigation
  React.useEffect(() => {
    if (!selectedSessionId) return
    // Don't reset if we just returned from the editor — preserve the edited text
    if (!hadEditorResult.current) {
      setMode("normal")
      setInputText("")
      setSendError(null)
    }
    hadEditorResult.current = false
    setMessagesLoading(true)
    setMessages([])
    setError(null)
    setScrollOffset(0)
    try {
      const dbMessages = getMessages(selectedSessionId)
      setMessages(dbMessages as ConversationMessage[])
    } catch (e) {
      setError(String(e))
    } finally {
      setMessagesLoading(false)
    }
  }, [selectedSessionId, setMessages, setMessagesLoading])

  // Instance/session info
  const instance = instances.find((i) => i.sessionId === selectedSessionId)
  const sessionInfo = React.useMemo(() => {
    if (!selectedSessionId) return null
    return getSessionById(selectedSessionId)
  }, [selectedSessionId])

  const sessionStatus = React.useMemo(() => {
    // Always re-read from DB on every statusTick so we pick up changes
    // regardless of whether the poller has run yet.
    if (!selectedSessionId) return "idle" as const
    return getSessionStatus(selectedSessionId)
  }, [selectedSessionId, statusTick])

  // Clear waitingForResponse when status catches up OR when new messages arrive
  // (handles case where agent responds so fast the poll never catches "working")
  React.useEffect(() => {
    if (sessionStatus !== "idle") setWaitingForResponse(false)
  }, [sessionStatus])
  React.useEffect(() => {
    if (waitingForResponse && messages.length > 0) {
      const lastMsg = messages[messages.length - 1]
      if (lastMsg && lastMsg.role === "assistant") {
        setWaitingForResponse(false)
      }
    }
  }, [messages.length, waitingForResponse])

  // Unconditional status tick — ensures sessionStatus re-evaluates even without instancePort
  // (e.g. sessions opened via session picker with no serve process yet)
  React.useEffect(() => {
    const id = setInterval(() => setStatusTick((t) => t + 1), 1000)
    return () => clearInterval(id)
  }, [])

  // Spinner animation for working state
  const SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
  const [spinnerIdx, setSpinnerIdx] = React.useState(0)
  React.useEffect(() => {
    if (sessionStatus !== "working" && !waitingForResponse) return
    const id = setInterval(() => setSpinnerIdx((i) => (i + 1) % SPINNER_FRAMES.length), 80)
    return () => clearInterval(id)
  }, [sessionStatus, waitingForResponse])

  const sessionTitle = instance?.sessionTitle ?? sessionInfo?.title ?? selectedSessionId?.slice(0, 20) ?? "session"
  const repoName = instance?.repoName ?? ""
  const sessionCwd = sessionInfo?.directory ?? instance?.worktree ?? process.cwd()
  const model = instance?.model ?? null

  // Determine if this is an SDK-capable live instance
  const isLive = !!(instance?.port || autoSpawnedPort)
  const instancePort = instance?.port ?? autoSpawnedPort

  // Fetch agents + models from SDK (live) or read agent from SQLite (read-only)
  React.useEffect(() => {
    if (!selectedSessionId) return

    // Always read agent from SQLite as fallback (available before SDK loads)
    setReadOnlyAgent(getSessionAgent(selectedSessionId))

    if (instancePort) {
      const client = createOpencodeClient({ baseUrl: `http://localhost:${instancePort}` })

      ;(client.app as any).agents().then((result: any) => {
        const all = (result?.data ?? []) as Array<{ name: string; mode: string; model?: { providerID: string; modelID: string } }>
        const primary = all.filter((a) => a.mode === "primary" || a.mode === "all")
        setAvailableAgents(primary)
        // Default to last-used agent from this session, fall back to first
        const lastAgent = getSessionAgent(selectedSessionId!)
        const agentIdx = lastAgent ? primary.findIndex((a) => a.name === lastAgent) : 0
        setSelectedAgentIdx(agentIdx >= 0 ? agentIdx : 0)
      }).catch(() => {})

      ;(client as any).provider.list().then((result: any) => {
        const data = result?.data as any
        const connected = new Set<string>(data?.connected ?? [])
        const models: ModelOption[] = []
        for (const provider of data?.all ?? []) {
          if (!connected.has(provider.id)) continue
          for (const modelId of Object.keys(provider.models ?? {})) {
            models.push({ providerID: provider.id, modelID: modelId, label: shortenModel(modelId) })
          }
        }
        setAvailableModels(models)
        // Default to last-used model from this session
        const lastModel = getSessionModel(selectedSessionId!)
        if (lastModel) {
          const modelIdx = models.findIndex((m) => m.modelID === lastModel)
          setModelOverrideIdx(modelIdx >= 0 ? modelIdx : null)
        } else {
          setModelOverrideIdx(null)
        }
      }).catch(() => {})
    }
  }, [selectedSessionId, instancePort])

  // Subscribe to SSE events + always-on 1s polling as safety net
  React.useEffect(() => {
    if (!instancePort || !selectedSessionId) return
    const sessionId = selectedSessionId

    let cancelled = false

    // Client used for both polling and SSE
    const client = createOpencodeClient({ baseUrl: `http://localhost:${instancePort}` })

    // Always-on polling — catches SSE stalls, external TUI writes, and status changes
    const pollInterval = setInterval(() => {
      if (cancelled) return
      try {
        const dbMessages = getMessages(sessionId)
        updateMessagesIfChanged(dbMessages as ConversationMessage[])
      } catch {}
      setStatusTick((t) => t + 1)
    }, config.conversationPollIntervalMs)

    // SSE for real-time updates (best-effort, faster than polling)

    async function listen() {
      try {
        const { stream } = await (client as any).event.subscribe()
        for await (const event of stream) {
          if (cancelled) break

          const type = event?.type
          const props = event?.properties

          // Filter to events relevant to our session
          const eventSessionId =
            props?.info?.sessionID ??
            props?.sessionID ??
            null

          if (eventSessionId && eventSessionId !== sessionId) continue

          if (
            type === "message.updated" ||
            type === "message.part.updated" ||
            type === "message.part.delta" ||
            type === "message.removed"
          ) {
            try {
              const dbMessages = getMessages(sessionId)
              updateMessagesIfChanged(dbMessages as ConversationMessage[])
            } catch {}
          }

          if (type === "session.status" || type === "session.idle") {
            setStatusTick((t) => t + 1)
          }
        }
      } catch {
        // SSE failed — polling is already running as safety net
      }
    }

    listen()

    return () => {
      cancelled = true
      clearInterval(pollInterval)
    }
  }, [instancePort, selectedSessionId])  // setMessages intentionally omitted — stable Zustand fn

  // Auto-spawn a serve process for non-live instances so we can chat via SDK
  React.useEffect(() => {
    if (instance?.port || !selectedSessionId) return  // already has a port

    let cancelled = false

    async function spawn() {
      try {
        setAutoSpawning(true)
        setAutoSpawnError(null)
        const cwd = sessionCwd
        if (!cwd) return
        const port = await ensureServeProcess(cwd)
        if (cancelled) return
        setAutoSpawnedPort(port)
      } catch (e) {
        if (!cancelled) setAutoSpawnError(String(e))
      } finally {
        if (!cancelled) setAutoSpawning(false)
      }
    }

    spawn()

    return () => { cancelled = true }
  }, [instance?.port, selectedSessionId, sessionCwd])

  // Reset auto-spawned port when session changes (new session may need different serve)
  React.useEffect(() => {
    setAutoSpawnedPort(null)
    setAutoSpawnError(null)
  }, [selectedSessionId])

  const openInOpencode = React.useCallback(() => {
    if (!selectedSessionId) return
    // Smart attach: if a child session has a running question, attach there instead
    const childQuestions = getChildSessionQuestions(selectedSessionId)
    const targetSessionId = childQuestions.length > 0
      ? childQuestions[0]!.sessionId
      : selectedSessionId
    yieldToOpencode(targetSessionId, sessionCwd, instancePort)
  }, [selectedSessionId, sessionCwd, instancePort])

  // Computed current agent and model
  const currentAgent = availableAgents[selectedAgentIdx]
  const currentModel: ModelOption | null = React.useMemo(() => {
    if (modelOverrideIdx !== null) return availableModels[modelOverrideIdx] ?? null
    if (currentAgent?.model) {
      return {
        providerID: currentAgent.model.providerID,
        modelID: currentAgent.model.modelID,
        label: shortenModel(currentAgent.model.modelID),
      }
    }
    return null
  }, [currentAgent, modelOverrideIdx, availableModels])

  // Send message via SDK
  const sendMessage = React.useCallback(async (text: string) => {
    if (!text.trim() || !selectedSessionId || !instancePort) return
    const sessionId = selectedSessionId
    setSending(true)
    setSendError(null)
    try {
      const client = createOpencodeClient({ baseUrl: `http://localhost:${instancePort}` })
      await (client.session as any).promptAsync({
        path: { id: selectedSessionId },
        body: {
          parts: [{ type: "text", text: text.trim() }],
          ...(currentAgent ? { agent: currentAgent.name } : {}),
          ...(currentModel ? { model: { providerID: currentModel.providerID, modelID: currentModel.modelID } } : {}),
        },
      })
      setInputText("")
      setMode("normal")
      setWaitingForResponse(true)
      // Show the user's message immediately (SSE will handle assistant updates)
      try {
        const dbMessages = getMessages(sessionId)
        setMessages(dbMessages as ConversationMessage[])
      } catch {}
    } catch (e) {
      setSendError(String(e))
    } finally {
      setSending(false)
    }
  }, [selectedSessionId, instancePort, currentAgent, currentModel, setMessages])

  // Commit session-modified files
  const doCommit = React.useCallback(async (message: string) => {
    if (!message.trim() || !selectedSessionId) return
    setCommitStatus("committing...")

    try {
      const { getSessionModifiedFiles } = await import("../db/reader.js")
      const { execSync: exec } = await import("child_process")
      const { existsSync } = await import("fs")
      const cwd = sessionCwd

      const sessionFiles = filterFilesForCwd(getSessionModifiedFiles(selectedSessionId), cwd)
      if (sessionFiles.length === 0) {
        setCommitStatus("✗ no files modified in this session")
        setTimeout(() => { setCommitStatus(null); setCommitMode(false); setMode("normal") }, 2500)
        return
      }

      // Filter out gitignored files before staging
      const { spawnSync } = await import("child_process")
      const ignoredSet = new Set<string>()
      if (sessionFiles.length > 0) {
        const result = spawnSync(
          "git", ["check-ignore", "--stdin"],
          { input: sessionFiles.join("\n"), cwd, encoding: "utf8" }
        )
        if (result.stdout) {
          for (const line of result.stdout.split("\n")) {
            const f = line.trim()
            if (f) ignoredSet.add(f)
          }
        }
      }
      const committableFiles = sessionFiles.filter((f) => !ignoredSet.has(f))
      if (committableFiles.length === 0) {
        setCommitStatus("✗ no files to commit (all ignored or outside repo)")
        setTimeout(() => { setCommitStatus(null); setCommitMode(false); setMode("normal") }, 2500)
        return
      }

      const existingFiles = committableFiles.filter((f) => existsSync(f))
      if (existingFiles.length > 0) {
        exec(`git add ${existingFiles.map((f) => `"${f}"`).join(" ")}`, { cwd, stdio: "pipe" })
      }
      const deletedFiles = committableFiles.filter((f) => !existsSync(f))
      if (deletedFiles.length > 0) {
        try {
          exec(`git add ${deletedFiles.map((f) => `"${f}"`).join(" ")}`, { cwd, stdio: "pipe" })
        } catch { /* not tracked — ignore */ }
      }

      exec(`git commit -m ${JSON.stringify(message.trim())}`, { cwd, stdio: "pipe" })

      let pushOk = true
      try {
        exec("git push origin HEAD", { cwd, stdio: "pipe" })
      } catch { pushOk = false }

      const n = committableFiles.length
      const noun = `${n} file${n === 1 ? "" : "s"}`
      setCommitStatus(pushOk
        ? `✓ committed ${noun} and pushed`
        : `✓ committed ${noun} (push failed — run manually)`)

      setTimeout(() => {
        setCommitStatus(null); setCommitMode(false); setCommitText(""); setMode("normal")
      }, 2500)
    } catch (e) {
      setCommitStatus(`✗ ${String(e)}`)
      setTimeout(() => { setCommitStatus(null); setCommitMode(false); setMode("normal") }, 3000)
    }
  }, [selectedSessionId, sessionCwd])

  // Rename session title via SDK
  const doRenameTitle = React.useCallback(async (newTitle: string) => {
    if (!newTitle.trim()) return
    // Use sidebar-targeted session/port, or fall back to current conversation session
    const targetId = titleRenameSessionId ?? selectedSessionId
    if (!targetId) return
    try {
      let port: number | null = titleRenamePort ?? instancePort
      if (!port && titleRenameWorktree) {
        // Need to spin up a serve process for this session's directory
        port = await ensureServeProcess(titleRenameWorktree)
      }
      if (!port) return
      const client = createOpencodeClient({ baseUrl: `http://localhost:${port}` })
      await (client.session as any).update({
        path: { id: targetId },
        body: { title: newTitle.trim() },
      })
      setCommitStatus("✓ title updated")
      setTimeout(() => setCommitStatus(null), 1500)
      refreshNow()
    } catch (e) {
      setCommitStatus(`✗ ${String(e)}`)
      setTimeout(() => setCommitStatus(null), 2500)
    } finally {
      setTitleMode(false)
      setTitleText("")
      setTitleRenameSessionId(null)
      setTitleRenamePort(null)
      setTitleRenameWorktree(null)
      setMode("normal")
    }
  }, [selectedSessionId, instancePort, titleRenameSessionId, titleRenamePort, titleRenameWorktree])

  // Open in $EDITOR (Ctrl-X E)
  const handleEditorOpen = React.useCallback(() => {
    openInEditor(inputText, (edited) => setInputText(edited))
  }, [inputText])

  const openModelPicker = React.useCallback(() => {
    if (!isLive || availableModels.length === 0) return
    setModelPickerOpen(true)
  }, [isLive, availableModels])

  React.useEffect(() => {
    if (modelPickerOpen && currentModel) {
      const idx = availableModels.findIndex(
        (m) => m.providerID === currentModel.providerID && m.modelID === currentModel.modelID
      )
      if (idx >= 0) {
        modelPicker.setCursor(idx)
        modelPicker.setScroll(Math.max(0, idx - Math.floor(modelPickerMaxVisible / 2)))
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- intentionally only on open
  }, [modelPickerOpen])

  // Content width (used both for display line wrapping and layout)
  const contentWidth = Math.max(1, effectiveWidth - sidebarWidth - 1)

  // Build display lines — wrap text at contentWidth so 1 display line = 1 terminal row
  const displayLines = React.useMemo<DisplayLine[]>(
    () => buildDisplayLines(messages, contentWidth),
    [messages, contentWidth]
  )
  const totalLines = displayLines.length

  // Estimate visible message lines for scroll/slice calculations.
  // Uses bodyHeight (which accounts for border + header + footer) not raw termHeight.
  const INNER_OVERHEAD = isLive ? 3 : 0  // input divider + prefix line + input line (live only)
  const SPINNER_ROW = 1  // reserve space for the working spinner
  const msgAreaHeight = Math.max(5, bodyHeight - INNER_OVERHEAD - SPINNER_ROW)
  const maxScroll = Math.max(0, totalLines - msgAreaHeight)
  const halfPage = Math.max(1, Math.floor(msgAreaHeight / 2))
  const fullPage = Math.max(1, msgAreaHeight - 2)

  const clampScroll = (v: number) => Math.max(0, Math.min(v, maxScroll))

  const scrollBy = React.useCallback((delta: number) => {
    setScrollOffset((o) => clampScroll(o + delta))
  }, [maxScroll])

  const prevMessageCount = React.useRef(messages.length)
  React.useEffect(() => {
    if (messages.length > prevMessageCount.current && scrollOffset <= 2) {
      setScrollOffset(0)
    }
    prevMessageCount.current = messages.length
  }, [messages.length, scrollOffset])

  // Keep sidebar cursor in bounds when instances change
  React.useEffect(() => {
    setSidebarCursor((c) => Math.min(c, Math.max(0, instances.length - 1)))
  }, [instances.length])

  // Visible lines — padded to constant count so Ink's rendering stays stable
  const startIdx = Math.max(0, totalLines - msgAreaHeight - scrollOffset)
  const endIdx = Math.max(0, totalLines - scrollOffset)
  const visibleLines: typeof displayLines = displayLines.slice(startIdx, endIdx)

  const searchMatches = React.useMemo(
    () => findDisplayLineMatches(displayLines, searchQuery),
    [displayLines, searchQuery]
  )

  React.useEffect(() => {
    if (searchMatches.length === 0) {
      setSearchMatchCursor(0)
      return
    }
    setSearchMatchCursor((cursor) => Math.min(cursor, searchMatches.length - 1))
  }, [searchMatches.length])

  React.useEffect(() => {
    if (!searchQuery || searchMatches.length === 0) return
    const lineIndex = searchMatches[searchMatchCursor]
    if (lineIndex === undefined) return
    setScrollOffset(getSearchScrollOffset(totalLines, msgAreaHeight, lineIndex))
  }, [searchMatchCursor, searchMatches, searchQuery, totalLines, msgAreaHeight])

  React.useEffect(() => {
    setSearchMode(false)
    setSearchInput("")
    setSearchQuery("")
    setSearchMatchCursor(0)
  }, [selectedSessionId])


  // Scroll position indicator
  const scrollPct = totalLines <= msgAreaHeight
    ? 100
    : Math.round(((totalLines - msgAreaHeight - scrollOffset) / Math.max(1, totalLines - msgAreaHeight)) * 100)
  const scrollIndicator = totalLines <= msgAreaHeight
    ? "ALL"
    : `${Math.max(0, Math.min(100, scrollPct))}%`

  // Combined useInput: mode switching + gg + Ctrl-X E
  useInput((input, key) => {
    // ── OVERLAYS (capture all keys regardless of mode) ────────────────────────
      if (pendingQuestion) {
        const opts = questionOpts
        const submitAnswer = async (label: string) => {
          let port = instancePort
          if (!port) {
            try {
              port = await ensureServeProcess(sessionCwd)
            } catch {
              setPendingQuestion(null)
              setQuestionCustomMode(false)
              setQuestionCustomText("")
              return
            }
          }

          try {
            const listRes = await fetch(`http://localhost:${port}/question`)
            if (!listRes.ok) {
              console.error("[DEBUG] question list failed:", listRes.status)
              return
            }
            const questions = await listRes.json() as Array<{ id: string; sessionID: string }>
            console.error("[DEBUG] pending questions:", JSON.stringify(questions.map((q) => ({ id: q.id, sessionID: q.sessionID }))))

            // Find question for THIS session specifically, or fall back to first
            const match = questions.find((q) => q.sessionID === selectedSessionId) ?? questions[0]

            if (!match) {
              console.error("[DEBUG] no pending questions found (stale question)")
              // Stale question — DB says running but serve process has no memory of it.
              // Set answered flag so the useEffect doesn't re-open the overlay.
              questionAnswered.current = true
              setPendingQuestion(null)
              setQuestionCustomMode(false)
              setQuestionCustomText("")
              return
            }

            const requestID = match.id
            const replyRes = await fetch(`http://localhost:${port}/question/${requestID}/reply`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ answers: [[label]] }),
            })
            console.error("[DEBUG] reply result:", replyRes.status, await replyRes.text())

            if (replyRes.ok) {
              questionAnswered.current = true
            }
          } catch (e) {
            console.error("[DEBUG] submitAnswer error:", String(e))
          }

          // Always dismiss overlay after attempting to answer
          setPendingQuestion(null)
          setQuestionCustomMode(false)
          setQuestionCustomText("")
        }
      if (questionCustomMode) {
        if (key.return) {
          if (questionCustomText.trim()) {
            void submitAnswer(questionCustomText.trim())
            setQuestionCustomMode(false)
            setQuestionCustomText("")
          }
        } else if (key.escape) {
          setQuestionCustomMode(false)
          setQuestionCustomText("")
        } else if (key.backspace || key.delete) {
          setQuestionCustomText((t) => t.slice(0, -1))
        } else if (input && !key.ctrl && !key.meta) {
          setQuestionCustomText((t) => t + input)
        }
        return
      }
      if (input === "j" || key.downArrow) {
        setQuestionCursor((c) => Math.min(c + 1, Math.max(opts.length - 1, 0)))
      } else if (input === "k" || key.upArrow) {
        setQuestionCursor((c) => Math.max(c - 1, 0))
      } else if (key.return) {
        const isCustomEntry = questionCursor === opts.length - 1
        if (isCustomEntry) {
          setQuestionCustomMode(true)
          setQuestionCustomText("")
        } else {
          const selected = opts[questionCursor]
          if (selected) void submitAnswer(selected.label)
        }
      } else if (key.escape) {
        setPendingQuestion(null)
        setQuestionCustomMode(false)
        setQuestionCustomText("")
      } else {
        const num = parseInt(input, 10)
        if (!isNaN(num) && num >= 1 && num <= opts.length) {
          const idx = num - 1
          const isCustomEntry = idx === opts.length - 1
          if (isCustomEntry) {
            setQuestionCustomMode(true)
            setQuestionCustomText("")
          } else {
            void submitAnswer(opts[idx]!.label)
          }
        }
      }
      return
    }
    if (killConfirm) {
      if (input === "y") {
        const killed = killConfirm
        setKillConfirm(null)
        killInstance(killed.worktree, killed.sessionId)
        untrackSession(killed.sessionId)
        refreshNow()
        const remaining = instances.filter((i) => i.sessionId !== killed.sessionId)
        if (remaining.length > 0) {
          const next = remaining[0]!
          navigate("conversation", next.projectId, next.sessionId)
        } else {
          navigate("dashboard")
        }
      } else if (input === "n" || key.escape) {
        setKillConfirm(null)
      }
      return
    }
    if (modelPickerOpen) {
      modelPicker.handleInput(input, key)
      return
    }
    if (filePickerOpen) {
      filePicker.handleInput(input, key)
      return
    }
    if (searchMode) {
      if (key.return) {
        setSearchQuery(searchInput.trim())
        setSearchMatchCursor(0)
        setSearchMode(false)
      } else if (key.escape) {
        setSearchMode(false)
        setSearchInput(searchQuery)
      } else if (key.backspace || key.delete) {
        setSearchInput((text) => text.slice(0, -1))
      } else if (input && !key.ctrl && !key.meta) {
        setSearchInput((text) => text + input)
      }
      return
    }
    // ── INSERT MODE ──────────────────────────────────────────────────────────
    if (mode === "insert") {
      // Track any Ctrl press — prevents TextInput from receiving the character
      if (key.ctrl) ctrlPressed.current = true

      // Esc or Ctrl-C exits insert mode (does NOT go back to dashboard)
      if (key.escape || (key.ctrl && input === "c")) {
        pendingChord.current = false
        setMode("normal")
        setCommitMode(false)
        setCommitText("")
        setTitleMode(false)
        setTitleText("")
        setTitleRenameSessionId(null)
        setTitleRenamePort(null)
        setTitleRenameWorktree(null)
        // Clear pending combos
        if (pendingCtrlXTimer.current) clearTimeout(pendingCtrlXTimer.current)
        setPendingCtrlX(false)
        return
      }

      // Ctrl-X E: open in editor (only from insert mode)
      if (key.ctrl && input === "x") {
        pendingChord.current = true
        if (pendingCtrlXTimer.current) clearTimeout(pendingCtrlXTimer.current)
        setPendingCtrlX(true)
        pendingCtrlXTimer.current = setTimeout(() => { setPendingCtrlX(false); pendingChord.current = false }, 1000)
        return
      }
      if (pendingCtrlX) {
        pendingChord.current = false
        if (pendingCtrlXTimer.current) clearTimeout(pendingCtrlXTimer.current)
        setPendingCtrlX(false)
        if ((input === "e" || input === "E") && !key.ctrl) {
          handleEditorOpen()
        } else if ((input === "m" || input === "M") && !key.ctrl) {
          openModelPicker()
        }
        return
      }

      // Tab: cycle agent (insert mode)
      if (key.tab && !key.shift && isLive && availableAgents.length > 0) {
        setSelectedAgentIdx((prev) => (prev + 1) % availableAgents.length)
        setModelOverrideIdx(null)
        return
      }
      // Shift-Tab: cycle agent backward (insert mode)
      if (key.tab && key.shift && isLive && availableAgents.length > 0) {
        setSelectedAgentIdx((prev) => (prev - 1 + availableAgents.length) % availableAgents.length)
        setModelOverrideIdx(null)
        return
      }

      // All other keys in insert mode go to TextInput — don't intercept
      return
    }

    // ── NORMAL MODE ──────────────────────────────────────────────────────────


    // Help overlay: any key closes it
    if (showHelp) {
      setShowHelp(false)
      return
    }

    // ?: toggle help overlay (don't require key.shift — some terminals don't set it for printable chars)
    if (input === "?") {
      setShowHelp(true)
      return
    }

    // x: kill session (context-dependent: sidebar cursor or current session)
    if (input === "x") {
      if (focus === "sidebar") {
        const target = instances[sidebarCursor]
        if (target) setKillConfirm(target)
      } else {
        if (instance) setKillConfirm(instance)
      }
      return
    }

    // Ctrl-W Ctrl-W: toggle focus between sidebar and conversation
    if (key.ctrl && input === "w") {
      if (pendingCtrlW) {
        if (pendingCtrlWTimer.current) clearTimeout(pendingCtrlWTimer.current)
        setPendingCtrlW(false)
        setFocus((f) => f === "sidebar" ? "conversation" : "sidebar")
      } else {
        setPendingCtrlW(true)
        pendingCtrlWTimer.current = setTimeout(() => setPendingCtrlW(false), 500)
      }
      return
    }
    // Clear pending Ctrl-W on any other key
    if (pendingCtrlW) {
      if (pendingCtrlWTimer.current) clearTimeout(pendingCtrlWTimer.current)
      setPendingCtrlW(false)
    }

    // ── SIDEBAR FOCUSED ───────────────────────────────────────────────────────
    if (focus === "sidebar") {
      if (input === "j" || key.downArrow) {
        const currentIndex = navigableIndices.indexOf(sidebarCursor)
        const nextIndex = Math.min(currentIndex + 1, navigableIndices.length - 1)
        setSidebarCursor(navigableIndices[nextIndex] ?? 0)
        return
      }
      if (input === "k" || key.upArrow) {
        const currentIndex = navigableIndices.indexOf(sidebarCursor)
        const nextIndex = Math.max(currentIndex - 1, 0)
        setSidebarCursor(navigableIndices[nextIndex] ?? 0)
        return
      }
      if (key.return) {
        const targetRow = rows[sidebarCursor]
        if (targetRow && targetRow.kind !== "scroll-indicator") {
          const targetSessionId = targetRow.kind === "instance" ? targetRow.instance.sessionId : targetRow.session.id
          const targetProjectId = targetRow.kind === "instance" ? targetRow.instance.projectId : targetRow.session.projectId
          if (targetSessionId !== selectedSessionId) {
            navigate("conversation", targetProjectId, targetSessionId)
          }
        }
        setFocus("conversation")
        return
      }
      if (key.tab) {
        const targetRow = rows[sidebarCursor]
        if (targetRow && targetRow.kind !== "scroll-indicator") {
          const sessionId = targetRow.kind === "instance" ? targetRow.instance.sessionId : targetRow.session.id
          const session = targetRow.kind === "instance" ? targetRow.instance : targetRow.session
          if (session.hasChildren) {
            toggleExpanded(sessionId)
          }
        }
        return
      }
      if (input === "q" || key.escape) {
        navigate("dashboard")
        return
      }
      if (input === "s") {
        setSidebarCompact((c) => !c)
        return
      }
      if (input === "p") {
        const targetRow = rows[sidebarCursor]
        if (targetRow && targetRow.kind !== "scroll-indicator") {
          const sessionId = targetRow.kind === "instance" ? targetRow.instance.sessionId : targetRow.session.id
          togglePin(sessionId)
          refreshNow()
        }
        return
      }
      if (input === "t") {
        const target = instances[sidebarCursor]
        if (target) {
          setTitleMode(true)
          setTitleText(target.sessionTitle)
          setTitleRenameSessionId(target.sessionId)
          setTitleRenamePort(target.port)
          setTitleRenameWorktree(target.worktree)
          setMode("insert")
          setFocus("conversation")
        }
        return
      }
      // When sidebar is focused, block all other keys
      return
    }

    // s: toggle sidebar compact mode
    if (input === "s") {
      setSidebarCompact((c) => !c)
      return
    }

    if (input === "/") {
      setSearchMode(true)
      setSearchInput(searchQuery)
      return
    }

    // ── CONVERSATION FOCUSED — session management keys ────────────────────

    // Ctrl-N: jump to next needs-input session (must be before 'n' check)
    if (key.ctrl && input === "n") {
      const needsInput = instances.filter((i) => i.status === "needs-input")
      if (needsInput.length > 0) {
        const currentIdx = instances.findIndex((i) => i.sessionId === selectedSessionId)
        const next = needsInput.find((_, j) => {
          const idx = instances.indexOf(needsInput[j]!)
          return idx > currentIdx
        }) ?? needsInput[0]!
        navigate("conversation", next.projectId, next.sessionId)
      }
      return
    }

    if (searchQuery && (input === "n" || input === "N" || (input === "n" && key.shift))) {
      if (searchMatches.length === 0) return
      setSearchMatchCursor((cursor) => {
        if (input === "N" || key.shift) return (cursor - 1 + searchMatches.length) % searchMatches.length
        return (cursor + 1) % searchMatches.length
      })
      return
    }

    // n: spawn new session
    if (input === "n") {
      navigate("spawn")
      return
    }

    // e: open editor with current input (shortcut for i + Ctrl-X E)
    if (input === "e" && isLive) {
      handleEditorOpen()
      setMode("insert")
      setFocus("conversation")
      return
    }

    // c: enter commit mode
    if (input === "c" && isLive) {
      setCommitMode(true)
      setCommitText("")
      setCommitStatus(null)
      setMode("insert")
      setFocus("conversation")
      return
    }

    // t: rename session title (live sessions only)
    if (input === "t" && isLive && selectedSessionId) {
      setTitleMode(true)
      setTitleText(sessionTitle)
      setMode("insert")
      setFocus("conversation")
      return
    }

    // m: open model picker
    if (input === "m") {
      openModelPicker()
      return
    }

    // f: open file picker (session-modified files)
    if (input === "f" && selectedSessionId) {
      const files = getSessionModifiedFiles(selectedSessionId)
      if (files.length === 0) {
        setCommitStatus("no files modified in this session")
        setTimeout(() => setCommitStatus(null), 2000)
        return
      }
      setFilePickerFiles(files)
      setFilePickerOpen(true)
      return
    }

    // w: new worktree session
    if (input === "w") {
      navigate("worktree")
      return
    }

    // r: refresh instances and messages
    if (input === "r") {
      refreshNow()
      if (selectedSessionId) {
        try {
          const dbMessages = getMessages(selectedSessionId)
          setMessages(dbMessages as ConversationMessage[])
        } catch {}
      }
      return
    }

    // Tab: cycle agent (live only), resets model to agent's default
    if (key.tab && !key.shift && isLive && availableAgents.length > 0) {
      setSelectedAgentIdx((prev) => (prev + 1) % availableAgents.length)
      setModelOverrideIdx(null)
      return
    }
    // Shift-Tab: cycle agent backward (live only)
    if (key.tab && key.shift && isLive && availableAgents.length > 0) {
      setSelectedAgentIdx((prev) => (prev - 1 + availableAgents.length) % availableAgents.length)
      setModelOverrideIdx(null)
      return
    }

    // 'i': enter insert mode for live instances, or attach for read-only
    if (input === "i") {
      if (isLive) {
        setMode("insert")
        setFocus("conversation")
        setScrollOffset(0)  // auto-scroll to bottom
      } else {
        openInOpencode()  // attach to TUI to reply
      }
      return
    }

    // gg combo
    if (key.escape || key.ctrl || key.return) {
      if (pendingGTimer.current) clearTimeout(pendingGTimer.current)
      setPendingG(false)
      return
    }
    if (input === "g" && !key.shift) {
      if (pendingG) {
        if (pendingGTimer.current) clearTimeout(pendingGTimer.current)
        setPendingG(false)
        setScrollOffset(maxScroll)
      } else {
        setPendingG(true)
        pendingGTimer.current = setTimeout(() => setPendingG(false), 500)
      }
    } else if (pendingG) {
      if (pendingGTimer.current) clearTimeout(pendingGTimer.current)
      setPendingG(false)
    }
  })

  // When any overlay is open, hide the body entirely — prevents background bleed-through in Ink
  const anyOverlayOpen = showHelp || modelPickerOpen || filePickerOpen || !!pendingQuestion
  const searchStatus = searchQuery
    ? (searchMatches.length > 0 ? `${searchMatchCursor + 1}/${searchMatches.length}` : "0/0")
    : null

  // Normal mode keybindings — all disabled in insert mode
  useConversationKeys(mode === "normal" && focus === "conversation" && !anyOverlayOpen && !searchMode ? {
    onBack: () => navigate("dashboard"),
    onAttach: openInOpencode,
    onSend: isLive ? undefined : openInOpencode,  // Enter attaches for read-only
    onScrollUp:           () => scrollBy(1),
    onScrollDown:         () => scrollBy(-1),
    onScrollHalfPageUp:   () => scrollBy(halfPage),
    onScrollHalfPageDown: () => scrollBy(-halfPage),
    onScrollPageUp:       () => scrollBy(fullPage),
    onScrollPageDown:     () => scrollBy(-fullPage),
    onScrollBottom:       () => setScrollOffset(0),
    onScrollTop:          () => setScrollOffset(maxScroll),
    onShell:              () => yieldToShell(sessionCwd),
  } : {})

  // Status indicator
  const statusInfo = (() => {
    if (sessionStatus === "working") return { char: "●", color: "green" }
    if (sessionStatus === "needs-input") return { char: "◐", color: "yellow" }
    if (sessionStatus === "error") return { char: "✗", color: "red" }
    return { char: "○", color: "white" }
  })()

  const fullDivider = "─".repeat(Math.max(1, effectiveWidth))
  const divider = "─".repeat(contentWidth)


  return (
    <Box flexDirection="column" height={innerHeight}>
      {/* Header — full width, outside the sidebar/content row */}
      <Box paddingLeft={1} justifyContent="space-between">
        <Box>
          <Text bold color={focus === "conversation" ? "cyan" : "gray"}>{repoName}</Text>
          <Text dimColor> / </Text>
          <Text bold color={focus === "conversation" ? undefined : "gray"}>{sessionTitle}</Text>
        </Box>
        <Box>
          <Text color={statusInfo.color as any}>{statusInfo.char}</Text>
          {currentAgent ? (
            <Text color="yellow" dimColor>  [{currentAgent.name}]</Text>
          ) : readOnlyAgent ? (
            <Text color="yellow" dimColor>  [{readOnlyAgent}]</Text>
          ) : null}
          {currentModel ? (
            <Text color="cyan" dimColor>  {currentModel.label}</Text>
          ) : model ? (
            <Text color="cyan" dimColor>  {model}</Text>
          ) : null}
          {isLive && mode === "insert" && <Text bold color="green">  [INSERT]  </Text>}
          {isLive && mode === "normal" && <Text bold color="gray">  [NORMAL]  </Text>}
          {!isLive && <Text bold color="yellow">  [read-only]  </Text>}
          <Text dimColor>{scrollIndicator}</Text>
        </Box>
      </Box>
      <Text color={focus === "conversation" ? "cyan" : "gray"} dimColor>{fullDivider}</Text>

      {/* Body row: sidebar + message area — hidden when any overlay is open */}
      {!anyOverlayOpen && <Box flexDirection="row" height={bodyHeight}>
<Sidebar
  rows={rows}
  currentSessionId={selectedSessionId}
  cursorIndex={sidebarCursor}
  focused={focus === "sidebar"}
  height={bodyHeight}
  width={sidebarWidth}
  compact={sidebarCompact}
  expandedSessions={expandedSessions}
  pinnedSessions={pinnedSessions}
/>

        <Box flexDirection="column" flexGrow={1} height={bodyHeight} width={contentWidth}>
          {/* Messages area — fixed height, clips any text wrapping */}
          <Box flexDirection="column" flexGrow={1} overflow="hidden" justifyContent="flex-end">
            {messagesLoading && (
              <Box paddingLeft={2}>
                <Text dimColor>Loading messages...</Text>
              </Box>
            )}
            {!messagesLoading && messages.length === 0 && !error && (
              <Box paddingLeft={2}>
                <Text dimColor>No messages in this session yet.</Text>
              </Box>
            )}
            {error && (
              <Box paddingLeft={2}>
                <Text color="red">Error: {error}</Text>
              </Box>
            )}

            {visibleLines.map((line, i) => {
              if (line.kind === "spacer") {
                return <Box key={`sp-${i}`}><Text> </Text></Box>
              }
              if (line.kind === "role-header") {
                const isUser = line.role === "user"
                return (
                  <Box key={`rh-${i}`} paddingLeft={4} height={1} overflow="hidden">
                    <Text bold color={isUser ? "blue" : "magenta"}>
                      {isUser ? "▶ YOU" : "◆ ASSISTANT"}
                    </Text>
                    {line.agent && <Text color="yellow" dimColor>  [{line.agent}]</Text>}
                    <Text dimColor>  {line.time}</Text>
                  </Box>
                )
              }
              if (line.kind === "thinking") {
                const text = searchQuery ? highlightMatches(line.text, searchQuery) : line.text
                return (
                  <Box key={`th-${i}`} paddingLeft={4} height={1} overflow="hidden">
                    <Text dimColor color="yellow" wrap="truncate">{text}</Text>
                  </Box>
                )
              }
              if (line.kind === "question") {
                const isRunning = line.status === "running"
                const header = searchQuery ? highlightMatches(line.header, searchQuery) : line.header
                const question = searchQuery ? highlightMatches(line.question, searchQuery) : line.question
                return (
                  <Box key={`q-${i}`} flexDirection="column" paddingLeft={4}>
                    <Box>
                      <Text color="yellow" bold>{isRunning ? "❓ " : "✓ "}</Text>
                      <Text color={isRunning ? "yellow" : "gray"} bold>{header}</Text>
                    </Box>
                    <Box paddingLeft={3}>
                      <Text color={isRunning ? "yellow" : "gray"} wrap="truncate">{question}</Text>
                    </Box>
                    {isRunning && line.options.length === 0 && (
                      <Box paddingLeft={3}>
                        <Text dimColor>press </Text>
                        <Text color="cyan" bold>a</Text>
                        <Text dimColor> to attach and answer</Text>
                      </Box>
                    )}
                  </Box>
                )
              }
              if (line.kind === "tool") {
                const detail = line.title || line.input || ""
                const name = searchQuery ? highlightMatches(line.name, searchQuery) : line.name
                const highlightedDetail = searchQuery ? highlightMatches(detail, searchQuery) : detail
                return (
                  <Box key={`tool-${i}`} paddingLeft={4} height={1} overflow="hidden">
                    <Text color={line.color as any}>{line.icon} </Text>
                    <Text dimColor>{name}</Text>
                    {detail && <Text dimColor wrap="truncate">  {highlightedDetail}</Text>}
                  </Box>
                )
              }
              if (line.kind === "text") {
                const text = searchQuery ? highlightMatches(line.text, searchQuery) : line.text
                return (
                  <Box key={`txt-${i}`} paddingLeft={4} height={1} overflow="hidden">
                    <Text wrap="truncate">{text}</Text>
                  </Box>
                )
              }
              return null
            })}
            {(sessionStatus === "working" || waitingForResponse) && scrollOffset === 0 && (
              <Box paddingLeft={3}>
                <Text color="green">{SPINNER_FRAMES[spinnerIdx]}</Text>
              </Box>
            )}
          </Box>

          {/* Input area — pinned below messages, position independent of scroll */}
          {isLive && (
            <>
              <Text dimColor>{divider}</Text>
              <Box paddingLeft={1}>
                {sending ? (
                  <Text dimColor>Sending...</Text>
                ) : mode === "insert" ? (
                  <Box>
                    {titleMode ? (
                      <>
                        <Text color="cyan" bold>title {">"} </Text>
                        <TextInput
                          value={titleText}
                          onChange={(val) => {
                            if (ctrlPressed.current) { ctrlPressed.current = false; return }
                            setTitleText(val)
                          }}
                          onSubmit={(text) => { void doRenameTitle(text) }}
                          placeholder="new session title..."
                          focus={true}
                        />
                      </>
                    ) : commitMode ? (
                      <>
                        <Text color="yellow" bold>commit {">"} </Text>
                        <TextInput
                          value={commitText}
                          onChange={(val) => {
                            if (ctrlPressed.current) { ctrlPressed.current = false; return }
                            setCommitText(val)
                          }}
                          onSubmit={(text) => { void doCommit(text) }}
                          placeholder="commit message..."
                          focus={true}
                        />
                      </>
                    ) : (
                      <Box flexDirection="column">
                        <Text><Text color="yellow">{currentAgent?.name ?? "agent"}</Text><Text dimColor> [{currentModel?.label ?? "model"}]</Text></Text>
                        <Box>
                          <Text color={focus === "conversation" ? "cyan" : "gray"}>❯ </Text>
                          <TextInput
                            value={inputText}
                            onChange={(val) => {
                              if (ctrlPressed.current || pendingChord.current) { ctrlPressed.current = false; return }
                              setInputText(val)
                            }}
                            onSubmit={(text) => { void sendMessage(text) }}
                            placeholder="Type a message...  (^X E: editor  ^X M: model)"
                            focus={!pendingCtrlX && !modelPickerOpen}
                          />
                        </Box>
                      </Box>
                    )}
                  </Box>
                ) : (
                  <Box flexDirection="column">
                    <Text><Text color="yellow" dimColor>{currentAgent?.name ?? "agent"}</Text><Text dimColor> [{currentModel?.label ?? "model"}]</Text></Text>
                    <Text dimColor>› Press <Text color={focus === "conversation" ? "cyan" : "gray"} bold>i</Text> to type a message</Text>
                  </Box>
                )}
              </Box>
              {commitStatus && (
                <Box paddingLeft={1}>
                  <Text color={commitStatus.startsWith("✓") ? "green" : commitStatus.startsWith("✗") ? "red" : "yellow"}>
                    {commitStatus}
                  </Text>
                </Box>
              )}
              {sendError && (
                <Box paddingLeft={1}>
                  <Text color="red">{sendError}</Text>
                </Box>
              )}
            </>
          )}
          {!isLive && autoSpawning && (
            <>
              <Text dimColor>{divider}</Text>
              <Box paddingLeft={1}>
                <Text dimColor>Starting background server...</Text>
              </Box>
            </>
          )}
          {!isLive && autoSpawnError && (
            <>
              <Text dimColor>{divider}</Text>
              <Box paddingLeft={1}>
                <Text color="red">Server error: {autoSpawnError}</Text>
              </Box>
            </>
          )}
        </Box>
      </Box>}

      {/* Overlays — rendered instead of body when open */}
      {anyOverlayOpen && <Box flexDirection="column" height={bodyHeight} overflow="hidden">

      {/* Help overlay */}
      {showHelp && (
        <Box flexDirection="column" paddingX={2} paddingY={0} borderStyle="round" borderColor="cyan" width={effectiveWidth}>
          <Box><Text bold color="cyan">Conversation Keybindings</Text></Box>
          <Box flexDirection="column" paddingLeft={2}>
            <Box><Box width={16}><Text bold color="white">i</Text></Box><Text dimColor>insert mode (type message)</Text></Box>
            <Box><Box width={16}><Text bold color="white">e</Text></Box><Text dimColor>open editor (normal mode)</Text></Box>
            <Box><Box width={16}><Text bold color="white">c</Text></Box><Text dimColor>commit session files + push</Text></Box>
            <Box><Box width={16}><Text bold color="white">t</Text></Box><Text dimColor>rename session title</Text></Box>
            <Box><Box width={16}><Text bold color="white">p</Text></Box><Text dimColor>pin/unpin session (sidebar)</Text></Box>
            <Box><Box width={16}><Text bold color="white">Esc</Text></Box><Text dimColor>normal mode</Text></Box>
            <Box><Box width={16}><Text bold color="white">^W ^W</Text></Box><Text dimColor>toggle sidebar focus</Text></Box>
            <Box><Box width={16}><Text bold color="white">j/k</Text></Box><Text dimColor>scroll messages</Text></Box>
            <Box><Box width={16}><Text bold color="white">^U/^D</Text></Box><Text dimColor>half page up/down</Text></Box>
            <Box><Box width={16}><Text bold color="white">G/gg</Text></Box><Text dimColor>scroll to bottom/top</Text></Box>
            <Box><Box width={16}><Text bold color="white">Tab/S-Tab</Text></Box><Text dimColor>cycle agent</Text></Box>
            <Box><Box width={16}><Text bold color="white">m</Text></Box><Text dimColor>select model</Text></Box>
            <Box><Box width={16}><Text bold color="white">^X M</Text></Box><Text dimColor>model picker (insert)</Text></Box>
<Box><Box width={16}><Text bold color="white">f</Text></Box><Text dimColor>edit session files</Text></Box>
<Box><Box width={16}><Text bold color="white">/</Text></Box><Text dimColor>search text</Text></Box>
<Box><Box width={16}><Text bold color="white">n / N</Text></Box><Text dimColor>next / prev match</Text></Box>
<Box><Box width={16}><Text bold color="white">a</Text></Box><Text dimColor>attach opencode TUI</Text></Box>
            <Box><Box width={16}><Text bold color="white">n</Text></Box><Text dimColor>spawn new session</Text></Box>
            <Box><Box width={16}><Text bold color="white">w</Text></Box><Text dimColor>worktree session</Text></Box>
            <Box><Box width={16}><Text bold color="white">!</Text></Box><Text dimColor>shell in session cwd</Text></Box>
            <Box><Box width={16}><Text bold color="white">x</Text></Box><Text dimColor>kill session</Text></Box>
            <Box><Box width={16}><Text bold color="white">r</Text></Box><Text dimColor>refresh</Text></Box>
            <Box><Box width={16}><Text bold color="white">Ctrl-N</Text></Box><Text dimColor>next needs-input session</Text></Box>
            <Box><Box width={16}><Text bold color="white">q</Text></Box><Text dimColor>back to dashboard</Text></Box>
          </Box>
          <Text dimColor>Press any key to close</Text>
        </Box>
      )}

      {/* Inline question answering overlay */}
      {pendingQuestion && pendingQuestion.questions[0] && (() => {
        const q = pendingQuestion.questions[0]!
        return (
          <Box flexDirection="column" paddingX={2} paddingY={0} borderStyle="round" borderColor="yellow" width={effectiveWidth}>
            <Box><Text bold color="yellow">❓ {q.header}</Text></Box>
            <Box paddingLeft={2}><Text color="yellow">{q.question}</Text></Box>
            <Box><Text> </Text></Box>
            {questionCustomMode ? (
              <Box paddingLeft={3} marginTop={1}>
                <Text color="cyan">› </Text>
                <Text>{questionCustomText}</Text>
                <Text color="cyan" dimColor>│</Text>
              </Box>
            ) : (
              questionOpts.map((opt, i) => {
                const isCursor = i === questionCursor
                return (
                  <Box key={`opt-${i}`} paddingLeft={1}>
                    <Text color={isCursor ? "yellow" : "gray"}>{isCursor ? "▸" : " "} </Text>
                    <Text dimColor>{String(i + 1)}. </Text>
                    <Text bold={isCursor} color={isCursor ? "yellow" : undefined} dimColor={!isCursor}>{opt.label}</Text>
                    {opt.description && <Text dimColor>  {opt.description}</Text>}
                  </Box>
                )
              })
            )}
            <Text dimColor>
              {questionCustomMode
                ? "Enter: submit  Esc: back"
                : `j/k: nav  Enter: select  1-${questionOpts.length}: quick  Esc: dismiss`}
            </Text>
          </Box>
        )
      })()}

      {/* Model picker overlay */}
      {modelPickerOpen && (
        <PickerOverlay
          title="Select Model"
          state={modelPicker.state}
          maxVisible={modelPickerMaxVisible}
          width={effectiveWidth}
          renderItem={(m, i, isCursor) => {
            const isCurrent = currentModel &&
              m.providerID === currentModel.providerID &&
              m.modelID === currentModel.modelID
            return (
              <Box paddingLeft={1}>
                <Text color={isCursor ? "cyan" : "gray"}>{isCursor ? "▸" : " "} </Text>
                <Text bold={!!isCurrent} color={isCursor ? "cyan" : isCurrent ? "white" : undefined} dimColor={!isCursor && !isCurrent}>
                  {m.label}
                </Text>
                <Text dimColor>  {m.providerID}</Text>
                {isCurrent && <Text color="green" dimColor>  ✓</Text>}
              </Box>
            )
          }}
        />
      )}

      {/* File picker overlay */}
      {filePickerOpen && (
        <PickerOverlay
          title="Session Files"
          state={filePicker.state}
          maxVisible={filePickerMaxVisible}
          width={effectiveWidth}
          countLabel="modified"
          renderItem={(f, i, isCursor) => {
            const display = f.startsWith(sessionCwd + "/")
              ? f.slice(sessionCwd.length + 1)
              : f
            return (
              <Box paddingLeft={1}>
                <Text color={isCursor ? "cyan" : "gray"}>{isCursor ? "▸" : " "} </Text>
                <Text color={isCursor ? "cyan" : undefined} dimColor={!isCursor} wrap="truncate">{display}</Text>
              </Box>
            )
          }}
          hint={`↑↓: nav  Enter: open  a: open all  Esc: ${filePicker.state.filter ? "clear" : "close"}  type to filter`}
        />
      )}

      </Box>}

      {/* Kill confirmation */}
      {killConfirm && (
        <Box paddingX={1} paddingY={0} borderStyle="single" borderColor="red">
          <Text color="red">Kill </Text>
          <Text bold color="red">{killConfirm.repoName} / {killConfirm.sessionTitle.slice(0, 30)}</Text>
          <Text color="red">? </Text>
          <Text bold color="white">y</Text>
          <Text dimColor> confirm  </Text>
          <Text bold color="white">n</Text>
          <Text dimColor>/</Text>
          <Text bold color="white">Esc</Text>
          <Text dimColor> cancel</Text>
        </Box>
      )}

      {/* Footer — full width, outside the sidebar/content row */}
      <Box paddingX={1} paddingY={0} borderStyle="single" borderColor="gray">
        <Text dimColor wrap="truncate">
          {searchMode
            ? `/` + searchInput + "  Enter: search  Esc: cancel"
            : searchQuery
            ? `/${searchQuery}  ${searchStatus}  n/N: next/prev  /: edit search`
            : isLive && mode === "insert"
            ? (titleMode ? `Esc: cancel  Enter: rename` : commitMode ? `Esc: cancel  Enter: commit and push` : `Esc: normal  Enter: send  ^XE: editor`)
            : isLive
            ? `q: back  i: insert  e: edit  c: commit  t: title  p: pin  m: model  f: files  s: sidebar  /:search  ^W^W: focus  Tab: agent  a: attach  !: shell  j/k: scroll  ? help`
            : `q: back  a/i/Enter: attach  j/k: scroll  ^U/^D: ½pg  G/gg: nav`
          }
        </Text>
      </Box>
    </Box>
  )
}
