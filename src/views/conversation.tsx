import React from "react"
import { Box, Text, useInput, useStdout } from "ink"
import TextInput from "ink-text-input"
import { marked } from "marked"
import { markedTerminal } from "marked-terminal"
import { createOpencodeClient } from "@opencode-ai/sdk"
import { useStore, type ConversationMessage, type ConversationMessagePart } from "../store.js"
import { useConversationKeys } from "../hooks/use-keybindings.js"
import { yieldToOpencode, openInEditor, consumePendingEditorResult } from "../hooks/use-attach.js"
import { getMessages, getSessionById, getSessionStatus, getSessionAgent } from "../db/reader.js"
import { config } from "../config.js"
import { shortenModel } from "../poller.js"

// ─── Markdown setup ───────────────────────────────────────────────────────────

marked.use(markedTerminal({ reflowText: true }))

// ─── Inline markdown fix ──────────────────────────────────────────────────────
// marked-terminal has a known bug: inline formatting (**bold**, `code`, *italic*)
// is not applied inside list items. Post-process to catch remaining raw markers.

const BOLD_RE   = /\*\*(.+?)\*\*/g
const ITALIC_RE = /(?<!\*)\*([^*\n]+)\*(?!\*)/g
const CODE_RE   = /(?<!`)`([^`\n]+)`(?!`)/g

function fixInlineMarkdown(text: string): string {
  return text
    .replace(BOLD_RE,   "\x1b[1m$1\x1b[22m")
    .replace(ITALIC_RE, "\x1b[3m$1\x1b[23m")
    .replace(CODE_RE,   "\x1b[2m$1\x1b[22m")
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function toolIcon(status: string | undefined): string {
  switch (status) {
    case "completed": return "✓"
    case "running":   return "⟳"
    case "error":     return "✗"
    default:          return "⏳"
  }
}

function toolColor(status: string | undefined): string {
  switch (status) {
    case "completed": return "green"
    case "running":   return "yellow"
    case "error":     return "red"
    default:          return "gray"
  }
}

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
}

function getTextFromParts(parts: ConversationMessagePart[]): string {
  return parts
    .filter((p) => p.type === "text" && p.text && !p.text.trimStart().startsWith("<"))
    .map((p) => p.text as string)
    .join("")
}

function getToolParts(parts: ConversationMessagePart[]) {
  return parts.filter((p) => p.type === "tool" && p.tool)
}

// ─── Display line types ───────────────────────────────────────────────────────

type DisplayLine =
  | { kind: "role-header"; role: "user" | "assistant"; time: string }
  | { kind: "text"; text: string }
  | { kind: "tool"; icon: string; color: string; name: string; callId?: string }
  | { kind: "spacer" }

function buildDisplayLines(messages: ConversationMessage[]): DisplayLine[] {
  const lines: DisplayLine[] = []

  for (const msg of messages) {
    lines.push({ kind: "role-header", role: msg.role, time: formatTime(msg.timeCreated) })

    const text = getTextFromParts(msg.parts)
    if (text) {
      try {
        const rendered = fixInlineMarkdown((marked(text) as string).trimEnd())
        for (const line of rendered.split("\n")) {
          lines.push({ kind: "text", text: line })
        }
      } catch {
        lines.push({ kind: "text", text })
      }
    }

    for (const part of getToolParts(msg.parts)) {
      lines.push({
        kind: "tool",
        icon: toolIcon(part.toolStatus),
        color: toolColor(part.toolStatus),
        name: part.tool as string,
        callId: part.callId,
      })
    }

    lines.push({ kind: "spacer" })
  }

  return lines
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
  // Ref (not state) to block the onChange that TextInput fires for the Ctrl-X keystroke.
  // Must be a ref because TextInput's onChange fires synchronously in the same tick —
  // React batched state wouldn't be visible yet.
  const blockNextInputChange = React.useRef(false)

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
  }, [instance, selectedSessionId])

  const sessionTitle = instance?.sessionTitle ?? sessionInfo?.title ?? selectedSessionId?.slice(0, 20) ?? "session"
  const repoName = instance?.repoName ?? ""
  const sessionCwd = sessionInfo?.directory ?? instance?.worktree ?? process.cwd()
  const model = instance?.model ?? null

  // Determine if this is an SDK-capable live instance
  const isLive = !!(instance?.port)
  const instancePort = instance?.port ?? null

  // Fetch agents + models from SDK (live) or read agent from SQLite (read-only)
  React.useEffect(() => {
    if (!selectedSessionId) return

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
    } else {
      setReadOnlyAgent(getSessionAgent(selectedSessionId))
    }
  }, [selectedSessionId, instancePort])

  const openInOpencode = React.useCallback(() => {
    if (!selectedSessionId) return
    yieldToOpencode(selectedSessionId, sessionCwd)
  }, [selectedSessionId, sessionCwd])

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
    setSending(true)
    setSendError(null)
    try {
      const client = createOpencodeClient({ baseUrl: `http://localhost:${instancePort}` })
      await (client.session as any).prompt({
        path: { id: selectedSessionId },
        body: {
          parts: [{ type: "text", text: text.trim() }],
          ...(currentAgent ? { agent: currentAgent.name } : {}),
          ...(currentModel ? { model: { providerID: currentModel.providerID, modelID: currentModel.modelID } } : {}),
        },
      })
      setInputText("")
      const dbMessages = getMessages(selectedSessionId)
      setMessages(dbMessages as ConversationMessage[])
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
  const displayLines = React.useMemo(() => buildDisplayLines(messages), [messages])
  const totalLines = displayLines.length

  // Layout — live instances need 2 extra rows for the input box
  const HEADER_ROWS = 3
  const FOOTER_ROWS = isLive ? 5 : 3
  const msgAreaHeight = Math.max(5, termHeight - HEADER_ROWS - FOOTER_ROWS)
  const maxScroll = Math.max(0, totalLines - msgAreaHeight)
  const halfPage = Math.max(1, Math.floor(msgAreaHeight / 2))
  const fullPage = Math.max(1, msgAreaHeight - 2)

  const clampScroll = (v: number) => Math.max(0, Math.min(v, maxScroll))

  const scrollBy = React.useCallback((delta: number) => {
    setScrollOffset((o) => clampScroll(o + delta))
  }, [maxScroll])

  // Visible lines
  const startIdx = Math.max(0, totalLines - msgAreaHeight - scrollOffset)
  const endIdx = Math.max(0, totalLines - scrollOffset)
  const visibleLines = displayLines.slice(startIdx, endIdx)

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
      // Esc exits insert mode (does NOT go back to dashboard)
      if (key.escape) {
        setMode("normal")
        // Clear pending combos
        if (pendingCtrlXTimer.current) clearTimeout(pendingCtrlXTimer.current)
        setPendingCtrlX(false)
        return
      }

      // Ctrl-X E: open in editor (only from insert mode)
      if (key.ctrl && input === "x") {
        // Block synchronously — TextInput's onChange fires in the same tick
        // before React can re-render with the updated pendingCtrlX state
        blockNextInputChange.current = true
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

    // Tab: cycle agent (live only), resets model to agent's default
    if (key.tab && !key.shift && isLive && availableAgents.length > 0) {
      setSelectedAgentIdx((prev) => (prev + 1) % availableAgents.length)
      setModelOverrideIdx(null)
      return
    }
    // Shift-Tab: cycle model override (live only)
    if (key.tab && key.shift && isLive && availableModels.length > 0) {
      setModelOverrideIdx((prev) => ((prev ?? -1) + 1) % availableModels.length)
      return
    }

    // 'i': enter insert mode for live instances, or attach for read-only
    if (input === "i") {
      if (isLive) {
        setMode("insert")
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
  useConversationKeys(mode === "normal" ? {
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

  const divider = "─".repeat(termWidth)

  return (
    <Box flexDirection="column">
      {/* Header */}
      <Box paddingLeft={1} justifyContent="space-between">
        <Box>
          <Text bold color="cyan">{repoName}</Text>
          <Text dimColor> / </Text>
          <Text bold>{sessionTitle}</Text>
        </Box>
        <Box>
          <Text color={statusInfo.color as any}>{statusInfo.char}</Text>
          {/* Agent indicator */}
          {isLive && currentAgent && (
            <Text color="yellow" dimColor>  [{currentAgent.name}]</Text>
          )}
          {!isLive && readOnlyAgent && (
            <Text color="yellow" dimColor>  [{readOnlyAgent}]</Text>
          )}
          {/* Model indicator: current model override or agent default or dashboard model */}
          {isLive && currentModel ? (
            <Text color="cyan" dimColor>  {currentModel.label}</Text>
          ) : model ? (
            <Text color="cyan" dimColor>  {model}</Text>
          ) : null}
          <Text dimColor>  {isLive ? (mode === "insert" ? "[INSERT]" : "[NORMAL]") : "[read-only]"}  </Text>
          <Text dimColor>{scrollIndicator}</Text>
        </Box>
      </Box>
      <Text dimColor>{divider}</Text>

      {/* Messages area */}
      {messagesLoading && (
        <Box paddingLeft={2} marginTop={1}>
          <Text dimColor>Loading messages...</Text>
        </Box>
      )}
      {!messagesLoading && messages.length === 0 && !error && (
        <Box paddingLeft={2} marginTop={1}>
          <Text dimColor>No messages in this session yet.</Text>
        </Box>
      )}
      {error && (
        <Box paddingLeft={2} marginTop={1}>
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
            <Box key={`rh-${i}`} paddingLeft={1} marginTop={1}>
              <Text bold color={isUser ? "blue" : "magenta"}>
                {isUser ? "▶ YOU" : "◆ ASSISTANT"}
              </Text>
              <Text dimColor>  {line.time}</Text>
            </Box>
          )
        }
        if (line.kind === "tool") {
          return (
            <Box key={`tool-${i}`} paddingLeft={4}>
              <Text color={line.color as any}>{line.icon} </Text>
              <Text dimColor>{line.name}</Text>
              {line.callId && <Text dimColor>  {line.callId.slice(0, 20)}</Text>}
            </Box>
          )
        }
        return (
          <Box key={`txt-${i}`} paddingLeft={3}>
            <Text>{line.text}</Text>
          </Box>
        )
      })}

      {/* Input area — only for live (SDK-capable) instances */}
      {isLive && (
        <>
          <Text dimColor>{divider}</Text>
          <Box paddingLeft={1}>
            {sending ? (
              <Text dimColor>Sending...</Text>
            ) : mode === "insert" ? (
              <Box>
                <Text color="cyan">❯ </Text>
                <TextInput
                  value={inputText}
                  onChange={(val) => {
                    if (blockNextInputChange.current) {
                      blockNextInputChange.current = false
                      return  // discard the 'x' that TextInput added from Ctrl-X
                    }
                    setInputText(val)
                  }}
                  onSubmit={(text) => { void sendMessage(text) }}
                  placeholder="Type a message...  (^X E: editor)"
                  focus={!pendingCtrlX}
                />
              </Box>
            ) : (
              <Text dimColor>○ Press <Text color="cyan" bold>i</Text> to type a message</Text>
            )}
          </Box>
          {sendError && (
            <Box paddingLeft={1}>
              <Text color="red">{sendError}</Text>
            </Box>
          )}
        </>
      )}

      {/* Footer */}
      <Text dimColor>{divider}</Text>
      <Box paddingLeft={1}>
        <Text dimColor wrap="truncate">
          {isLive && mode === "insert"
            ? `Esc: normal mode  Enter: send  ^XE: editor  [INSERT]  [${scrollIndicator}]`
            : isLive
            ? `q: back  i: insert  Tab: agent  S-Tab: model  a: attach  j/k: scroll  [NORMAL]  [${scrollIndicator}]`
            : `q: back  a/i/Enter: open in opencode to reply  j/k: scroll  ^U/^D: ½ page  G/gg: nav  [${scrollIndicator}]`
          }
        </Text>
      </Box>
    </Box>
  )
}
