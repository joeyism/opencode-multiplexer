import type { ConversationMessage, ConversationMessagePart } from "../store.js"
import { getChildSessionQuestions } from "../db/reader.js"
import { marked } from "marked"
import TerminalRenderer from "marked-terminal"
import wrapAnsi from "wrap-ansi"

marked.setOptions({ renderer: new (TerminalRenderer as any)({ reflowText: false, width: 10000 }) })

const BOLD_RE   = /\*\*(.+?)\*\*/g
const ITALIC_RE = /(?<!\*)\*([^*\n]+)\*(?!\*)/g
const CODE_RE   = /(?<!`)`([^`\n]+)`(?!`)/g

export function fixInlineMarkdown(text: string): string {
  return text
    .replace(BOLD_RE, "\x1b[1m$1\x1b[22m")
    .replace(ITALIC_RE, "\x1b[3m$1\x1b[23m")
    .replace(CODE_RE, "\x1b[2m$1\x1b[22m")
}

export function toolIcon(status: string | undefined): string {
  switch (status) {
    case "completed": return "✓"
    case "running":   return "⟳"
    case "error":     return "✗"
    default:           return "⏳"
  }
}

export function toolColor(status: string | undefined): string {
  switch (status) {
    case "completed": return "green"
    case "running":   return "yellow"
    case "error":     return "red"
    default:           return "gray"
  }
}

export function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
}

export function getTextFromParts(parts: ConversationMessagePart[]): string {
  return parts
    .filter((p) => p.type === "text" && p.text)
    .map((p) => p.text as string)
    .join("")
}

export function getThinkingFromParts(parts: ConversationMessagePart[]): string {
  return parts
    .filter((p) => p.type === "reasoning" && p.text)
    .map((p) => p.text as string)
    .join("")
}

export function getToolParts(parts: ConversationMessagePart[]) {
  return parts.filter((p) => p.type === "tool" && p.tool)
}

export type DisplayLine =
  | { kind: "role-header"; role: "user" | "assistant"; time: string; agent?: string }
  | { kind: "thinking"; text: string }
  | { kind: "text"; text: string }
  | { kind: "tool"; icon: string; color: string; name: string; title?: string; input?: string }
  | { kind: "question"; header: string; question: string; status: string; options: Array<{ label: string; description?: string }>; custom: boolean }
  | { kind: "spacer" }

export function buildDisplayLines(messages: ConversationMessage[], width?: number): DisplayLine[] {
  const lines: DisplayLine[] = []

  for (const msg of messages) {
    lines.push({ kind: "role-header", role: msg.role, time: formatTime(msg.timeCreated), agent: msg.agent })

    const thinking = getThinkingFromParts(msg.parts)
    if (thinking) {
      lines.push({ kind: "thinking", text: "💭 Thinking" })
      for (const line of thinking.split("\n")) {
        lines.push({ kind: "thinking", text: line })
      }
    }

    const text = getTextFromParts(msg.parts)
    if (text) {
      try {
        const rendered = fixInlineMarkdown((marked(text) as string).trimEnd())
        for (const rawLine of rendered.split("\n")) {
          if (width && rawLine.length > 0) {
            // Wrap long lines at content width (accounting for paddingLeft=4 in the JSX)
            const maxLineWidth = Math.max(10, width - 4)
            const wrapped = wrapAnsi(rawLine, maxLineWidth, { hard: true, trim: false })
            for (const wLine of wrapped.split("\n")) {
              lines.push({ kind: "text", text: wLine })
            }
          } else {
            lines.push({ kind: "text", text: rawLine })
          }
        }
      } catch {
        lines.push({ kind: "text", text })
      }
    }

    for (const part of getToolParts(msg.parts)) {
      if ((part.tool === "question" || part.tool === "plan_exit") && (part.toolInput || part.toolTitle)) {
        let options: Array<{ label: string; description?: string }> = []
        if (part.toolOptions) {
          try { options = JSON.parse(part.toolOptions) } catch {}
        }
        let custom = false
        if (part.toolCustom) {
          custom = part.toolCustom === "true" || part.toolCustom === "1"
        }
        lines.push({
          kind: "question",
          header: part.toolHeader || "Question",
          question: part.toolInput || part.toolTitle || "",
          status: part.toolStatus || "running",
          options,
          custom,
        })
        continue
      }

      lines.push({
        kind: "tool",
        icon: toolIcon(part.toolStatus),
        color: toolColor(part.toolStatus),
        name: part.tool as string,
        title: part.toolTitle,
        input: part.toolInput,
      })
    }

    lines.push({ kind: "spacer" })
  }

  // Append running questions from direct child sessions (subagent prompts)
  if (messages.length > 0) {
    const parentSessionId = messages[0]!.sessionId
      try {
        const childQuestions = getChildSessionQuestions(parentSessionId)
        for (const q of childQuestions) {
          lines.push({
            kind: "question",
            header: `${q.header} (${q.agent ?? "subagent"})`,
            question: q.question,
            status: q.status,
            options: q.options,
            custom: q.custom,
          })
        }
    } catch {
      // DB may be briefly locked — skip child questions this render
    }
  }

  return lines
}
