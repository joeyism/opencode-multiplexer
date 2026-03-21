import React from "react"
import { Box, Text, useInput, useStdout } from "ink"
import TextInput from "ink-text-input"
import { createOpencodeClient } from "@opencode-ai/sdk"
import { useStore, type ConversationMessage } from "../store.js"
import { useConversationKeys } from "../hooks/use-keybindings.js"
import { yieldToOpencode, openInEditor, consumePendingEditorResult } from "../hooks/use-attach.js"
import { getMessages, getSessionById, getSessionStatus, getSessionAgent } from "../db/reader.js"
import { config } from "../config.js"
import { shortenModel, refreshNow } from "../poller.js"
import { ensureServeProcess, killInstance } from "../registry/instances.js"
import { statusIcon } from "./helpers.js"


import { buildDisplayLines, type DisplayLine } from "./display-lines.js"

const SIDEBAR_WIDTH = 26

function Sidebar({
  instances,
  currentSessionId,
  cursorIndex,
  focused,
  height,
}: {
  instances: import("../store.js").OcmInstance[]
  currentSessionId: string | null
  cursorIndex: number
  focused: boolean
  height: number
}) {
  // Inner width: total - 2 border chars
  const innerWidth = SIDEBAR_WIDTH - 2
  // Max chars for repo/title after cursor(1) + space(1) + icon(1) + space(1) = 4
  const maxLabelWidth = innerWidth - 4

  return (
    <Box
      flexDirection="column"
      width={SIDEBAR_WIDTH}
      flexShrink={0}
      borderStyle="single"
      borderColor={focused ? "cyan" : "gray"}
      height={height}
      overflow="hidden"
    >
      {/* Header */}
      <Box paddingX={1} justifyContent="space-between">
        <Text bold color={focused ? "cyan" : "gray"}>sessions</Text>
        <Text dimColor>{instances.length}</Text>
      </Box>
      <Text dimColor>{"─".repeat(innerWidth)}</Text>

      {/* Instance list */}
      {instances.length === 0 && (
        <Box paddingX={1}>
          <Text dimColor>no instances</Text>
        </Box>
      )}
      {instances.map((inst, i) => {
        const isCurrent = inst.sessionId === currentSessionId
        const isCursor = focused && i === cursorIndex
        const { char, color } = statusIcon(inst.status)

        // Compact single-line: "▸ ▶ repo/title"
        const sep = "/"
        const maxTotal = maxLabelWidth
        const repoMax = Math.min(inst.repoName.length, Math.floor(maxTotal * 0.4))
        const repo = inst.repoName.length > repoMax
          ? inst.repoName.slice(0, repoMax - 1) + "…"
          : inst.repoName
        const titleMax = maxTotal - repo.length - sep.length
        const title = inst.sessionTitle.length > titleMax
          ? inst.sessionTitle.slice(0, Math.max(0, titleMax - 1)) + "…"
          : inst.sessionTitle

        return (
          <Box key={inst.id} paddingLeft={1}>
            <Text color={isCursor ? "cyan" : isCurrent ? "white" : "gray"}>
              {isCursor ? "▸" : isCurrent ? "◆" : " "}
            </Text>
            <Text>{" "}</Text>
            <Text color={color}>{char}</Text>
            <Text>{" "}</Text>
            <Text
              bold={isCurrent || isCursor}
              color={isCursor ? "cyan" : isCurrent ? "white" : undefined}
              dimColor={!isCurrent && !isCursor}
            >
              {repo}
            </Text>
            <Text dimColor>{sep}</Text>
            <Text
              dimColor={!isCurrent && !isCursor}
              color={isCursor ? "cyan" : undefined}
            >
              {title}
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
  // Loading state: covers the gap between promptAsync returning and SSE status arriving
  const [waitingForResponse, setWaitingForResponse] = React.useState(false)
  // Kill confirmation
  const [killConfirm, setKillConfirm] = React.useState<import("../store.js").OcmInstance | null>(null)
  // Help overlay
  const [showHelp, setShowHelp] = React.useState(false)
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

  const { stdout } = useStdout()
  const termWidth = stdout?.columns ?? 80
  const termHeight = stdout?.rows ?? 24

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
    if (instance) return instance.status
    if (!selectedSessionId) return "idle" as const
    return getSessionStatus(selectedSessionId)
  }, [instance, selectedSessionId, statusTick])

  // Clear waitingForResponse once session status catches up
  React.useEffect(() => {
    if (sessionStatus !== "idle") setWaitingForResponse(false)
  }, [sessionStatus])

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
        setSelectedAgentIdx(0)
        setModelOverrideIdx(null)
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
      }).catch(() => {})
    }
  }, [selectedSessionId, instancePort])

  // Subscribe to SSE events + always-on 1s polling as safety net
  React.useEffect(() => {
    if (!instancePort || !selectedSessionId) return
    const sessionId = selectedSessionId

    let cancelled = false

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
    const client = createOpencodeClient({ baseUrl: `http://localhost:${instancePort}` })

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
        const cwd = sessionCwd
        if (!cwd) return
        const port = await ensureServeProcess(cwd)
        if (cancelled) return
        setAutoSpawnedPort(port)
      } catch (e) {
        // Failed to spawn — instance stays read-only
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
  }, [selectedSessionId])

  const openInOpencode = React.useCallback(() => {
    if (!selectedSessionId) return
    yieldToOpencode(selectedSessionId, sessionCwd, instancePort)
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

  // Open in $EDITOR (Ctrl-X E)
  const handleEditorOpen = React.useCallback(() => {
    openInEditor(inputText, (edited) => setInputText(edited))
  }, [inputText])

  // Build display lines
  const displayLines = React.useMemo<DisplayLine[]>(() => buildDisplayLines(messages), [messages])
  const totalLines = displayLines.length

  // Estimate visible message lines for scroll/slice calculations.
  // Layout is flexbox-driven; this only controls how many lines we render.
  // Err on the side of too many — overflow="hidden" clips any excess.
  const INNER_OVERHEAD = isLive ? 2 : 0  // input divider + input line (live only)
  const msgAreaHeight = Math.max(5, termHeight - INNER_OVERHEAD)
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
  while (visibleLines.length < msgAreaHeight) {
    visibleLines.unshift({ kind: "spacer" as const })
  }

  // Scroll position indicator
  const scrollPct = totalLines <= msgAreaHeight
    ? 100
    : Math.round(((totalLines - msgAreaHeight - scrollOffset) / Math.max(1, totalLines - msgAreaHeight)) * 100)
  const scrollIndicator = totalLines <= msgAreaHeight
    ? "ALL"
    : `${Math.max(0, Math.min(100, scrollPct))}%`

  // Combined useInput: mode switching + gg + Ctrl-X E
  useInput((input, key) => {
    // ── INSERT MODE ──────────────────────────────────────────────────────────
    if (mode === "insert") {
      // Track any Ctrl press — prevents TextInput from receiving the character
      if (key.ctrl) ctrlPressed.current = true

      // Esc or Ctrl-C exits insert mode (does NOT go back to dashboard)
      if (key.escape || (key.ctrl && input === "c")) {
        setMode("normal")
        // Clear pending combos
        if (pendingCtrlXTimer.current) clearTimeout(pendingCtrlXTimer.current)
        setPendingCtrlX(false)
        return
      }

      // Ctrl-X E: open in editor (only from insert mode)
      if (key.ctrl && input === "x") {
        if (pendingCtrlXTimer.current) clearTimeout(pendingCtrlXTimer.current)
        setPendingCtrlX(true)
        pendingCtrlXTimer.current = setTimeout(() => setPendingCtrlX(false), 1000)
        return
      }
      if (pendingCtrlX) {
        if (pendingCtrlXTimer.current) clearTimeout(pendingCtrlXTimer.current)
        setPendingCtrlX(false)
        if ((input === "e" || input === "E") && !key.ctrl) {
          handleEditorOpen()
        }
        return
      }

      // All other keys in insert mode go to TextInput — don't intercept
      return
    }

    // ── NORMAL MODE ──────────────────────────────────────────────────────────

    // Kill confirmation: y to confirm, n/Esc to cancel (captures all keys)
    if (killConfirm) {
      if (input === "y") {
        const killed = killConfirm
        setKillConfirm(null)
        killInstance(killed.worktree, killed.sessionId)
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

    // Help overlay: any key closes it
    if (showHelp) {
      setShowHelp(false)
      return
    }

    // ?: toggle help overlay
    if (input === "?" && key.shift) {
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
        const maxIdx = Math.max(0, instances.length - 1)
        setSidebarCursor((c) => Math.min(c + 1, maxIdx))
        return
      }
      if (input === "k" || key.upArrow) {
        setSidebarCursor((c) => Math.max(c - 1, 0))
        return
      }
      if (key.return) {
        const target = instances[sidebarCursor]
        if (target && target.sessionId !== selectedSessionId) {
          navigate("conversation", target.projectId, target.sessionId)
        }
        // Switch focus back to conversation pane regardless
        setFocus("conversation")
        return
      }
      if (input === "q" || key.escape) {
        navigate("dashboard")
        return
      }
      // When sidebar is focused, block all other keys
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

    // n: spawn new session
    if (input === "n") {
      navigate("spawn")
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

  // Normal mode keybindings — all disabled in insert mode
  useConversationKeys(mode === "normal" && focus === "conversation" ? {
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
  } : {})

  // Status indicator
  const statusInfo = (() => {
    if (sessionStatus === "working") return { char: "●", color: "green" }
    if (sessionStatus === "needs-input") return { char: "◐", color: "yellow" }
    if (sessionStatus === "error") return { char: "✗", color: "red" }
    return { char: "○", color: "white" }
  })()

  const contentWidth = Math.max(1, termWidth - SIDEBAR_WIDTH - 1)
  const fullDivider = "─".repeat(Math.max(1, termWidth))
  const divider = "─".repeat(contentWidth)

  // Explicit layout heights — deterministic, avoids flexGrow + border interactions
  const HEADER_ROWS = 2   // title line + divider
  const FOOTER_ROWS = 3   // bordered footer: top border + content + bottom border
  const KILL_CONFIRM_ROWS = killConfirm ? 3 : 0  // conditional confirmation box
  const bodyHeight = Math.max(5, termHeight - HEADER_ROWS - FOOTER_ROWS - KILL_CONFIRM_ROWS)


  return (
    <Box flexDirection="column" height={termHeight}>
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

      {/* Body row: sidebar + message area side by side */}
        <Box flexDirection="row" height={bodyHeight}>
          <Sidebar
            instances={instances}
            currentSessionId={selectedSessionId}
            cursorIndex={sidebarCursor}
            focused={focus === "sidebar"}
            height={bodyHeight}
          />

        <Box flexDirection="column" flexGrow={1} height={bodyHeight}>
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
                  <Box key={`rh-${i}`} paddingLeft={1}>
                    <Text bold color={isUser ? "blue" : "magenta"}>
                      {isUser ? "▶ YOU" : "◆ ASSISTANT"}
                    </Text>
                    {line.agent && <Text color="yellow" dimColor>  [{line.agent}]</Text>}
                    <Text dimColor>  {line.time}</Text>
                  </Box>
                )
              }
              if (line.kind === "thinking") {
                return (
                  <Box key={`th-${i}`} paddingLeft={4}>
                    <Text dimColor color="yellow" wrap="truncate">{line.text}</Text>
                  </Box>
                )
              }
              if (line.kind === "question") {
                const isRunning = line.status === "running"
                return (
                  <Box key={`q-${i}`} flexDirection="column" paddingLeft={3}>
                    <Box>
                      <Text color="yellow" bold>{isRunning ? "❓ " : "✓ "}</Text>
                      <Text color={isRunning ? "yellow" : "gray"} bold>{line.header}</Text>
                    </Box>
                    <Box paddingLeft={3}>
                      <Text color={isRunning ? "yellow" : "gray"} wrap="truncate">{line.question}</Text>
                    </Box>
                    {isRunning && (
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
                return (
                  <Box key={`tool-${i}`} paddingLeft={4}>
                    <Text color={line.color as any}>{line.icon} </Text>
                    <Text dimColor>{line.name}</Text>
                    {detail && <Text dimColor wrap="truncate">  {detail}</Text>}
                  </Box>
                )
              }
              if (line.kind === "text") {
                return (
                  <Box key={`txt-${i}`} paddingLeft={3}>
                    <Text wrap="truncate">{line.text}</Text>
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
                    <Text color={focus === "conversation" ? "cyan" : "gray"}>❯ </Text>
                    <TextInput
                      value={inputText}
                      onChange={(val) => {
                        if (ctrlPressed.current) {
                          ctrlPressed.current = false
                          return  // discard any character added by a Ctrl combo
                        }
                        setInputText(val)
                      }}
                      onSubmit={(text) => { void sendMessage(text) }}
                      placeholder="Type a message...  (^X E: editor)"
                      focus={!pendingCtrlX}
                    />
                  </Box>
                ) : (
                  <Text dimColor>› Press <Text color={focus === "conversation" ? "cyan" : "gray"} bold>i</Text> to type a message</Text>
                )}
              </Box>
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
        </Box>
      </Box>

      {/* Help overlay */}
      {showHelp && (
        <Box flexDirection="column" paddingX={2} paddingY={0} borderStyle="round" borderColor="cyan">
          <Box><Text bold color="cyan">Conversation Keybindings</Text></Box>
          <Box flexDirection="column" paddingLeft={2}>
            <Box><Box width={16}><Text bold color="white">i</Text></Box><Text dimColor>insert mode (type message)</Text></Box>
            <Box><Box width={16}><Text bold color="white">Esc</Text></Box><Text dimColor>normal mode</Text></Box>
            <Box><Box width={16}><Text bold color="white">^W ^W</Text></Box><Text dimColor>toggle sidebar focus</Text></Box>
            <Box><Box width={16}><Text bold color="white">j/k</Text></Box><Text dimColor>scroll messages</Text></Box>
            <Box><Box width={16}><Text bold color="white">^U/^D</Text></Box><Text dimColor>half page up/down</Text></Box>
            <Box><Box width={16}><Text bold color="white">G/gg</Text></Box><Text dimColor>scroll to bottom/top</Text></Box>
            <Box><Box width={16}><Text bold color="white">Tab/S-Tab</Text></Box><Text dimColor>cycle agent/model</Text></Box>
            <Box><Box width={16}><Text bold color="white">a</Text></Box><Text dimColor>attach opencode TUI</Text></Box>
            <Box><Box width={16}><Text bold color="white">n</Text></Box><Text dimColor>spawn new session</Text></Box>
            <Box><Box width={16}><Text bold color="white">w</Text></Box><Text dimColor>worktree session</Text></Box>
            <Box><Box width={16}><Text bold color="white">x</Text></Box><Text dimColor>kill session</Text></Box>
            <Box><Box width={16}><Text bold color="white">r</Text></Box><Text dimColor>refresh</Text></Box>
            <Box><Box width={16}><Text bold color="white">Ctrl-N</Text></Box><Text dimColor>next needs-input session</Text></Box>
            <Box><Box width={16}><Text bold color="white">q</Text></Box><Text dimColor>back to dashboard</Text></Box>
          </Box>
          <Text dimColor>Press any key to close</Text>
        </Box>
      )}

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
          {isLive && mode === "insert"
            ? `Esc: normal  Enter: send  ^XE: editor`
            : isLive
            ? `q: back  i: insert  ^W^W: sidebar  Tab: agent  a: attach  j/k: scroll  w: worktree  ? help`
            : `q: back  a/i/Enter: attach  j/k: scroll  ^U/^D: ½pg  G/gg: nav`
          }
        </Text>
      </Box>
    </Box>
  )
}
