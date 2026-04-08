import stripAnsi from "strip-ansi"
import { relative as pathRelative } from "path"
import type { SessionStatus } from "../store.js"
import type { DisplayLine } from "./display-lines.js"

/**
 * Format a timestamp as a compact relative time string.
 * Examples: "now", "2m", "1h", "3d", "2w"
 */
export function relativeTime(ts: number): string {
  const diff = Math.max(0, Date.now() - ts)
  const seconds = Math.floor(diff / 1000)
  if (seconds < 60) return "now"
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  return `${Math.floor(days / 7)}w`
}

export function statusIcon(status: SessionStatus): { char: string; color: string } {
  switch (status) {
    case "working":     return { char: "▶", color: "green" }
    case "needs-input": return { char: "●", color: "yellow" }
    case "idle":        return { char: "✔", color: "gray" }
    case "error":       return { char: "✖", color: "red" }
  }
}

/**
 * Highlight all case-insensitive occurrences of `query` in `text` with a bright background.
 * Works with ANSI-styled text by building a visible-char → original-index map.
 */
export function highlightMatches(text: string, query: string): string {
  if (!query) return text
  const plain = stripAnsi(text)
  const lowerPlain = plain.toLowerCase()
  const lowerQuery = query.toLowerCase()

  // Find all match positions in the plain (visible) text
  const matches: Array<{ start: number; end: number }> = []
  let searchFrom = 0
  while (true) {
    const idx = lowerPlain.indexOf(lowerQuery, searchFrom)
    if (idx === -1) break
    matches.push({ start: idx, end: idx + query.length })
    searchFrom = idx + 1
  }
  if (matches.length === 0) return text

  // Build a map from visible char index → original string index (skipping ANSI sequences)
  const ANSI_RE = /\x1b\[[0-9;]*m/g
  let visibleIdx = 0
  let originalIdx = 0
  const visibleToOriginal: number[] = []

  while (originalIdx < text.length) {
    ANSI_RE.lastIndex = originalIdx
    const ansiMatch = ANSI_RE.exec(text)
    if (ansiMatch && ansiMatch.index === originalIdx) {
      originalIdx += ansiMatch[0].length
      continue
    }
    visibleToOriginal[visibleIdx] = originalIdx
    visibleIdx++
    originalIdx++
  }

  // Inject highlight codes in reverse order to preserve indices
  const HIGHLIGHT_ON = "\x1b[30;43m"  // black text on yellow background
  const HIGHLIGHT_OFF = "\x1b[0m"
  let result = text
  for (let i = matches.length - 1; i >= 0; i--) {
    const m = matches[i]!
    const startOrig = visibleToOriginal[m.start]
    const endOrig = visibleToOriginal[m.end] ?? result.length
    if (startOrig === undefined) continue
    result =
      result.slice(0, startOrig) +
      HIGHLIGHT_ON +
      result.slice(startOrig, endOrig) +
      HIGHLIGHT_OFF +
      result.slice(endOrig)
  }
  return result
}

export function filterFilesForCwd(files: string[], cwd: string): string[] {
  return files.filter((file) => {
    if (!file.startsWith("/")) return true

    const relativePath = pathRelative(cwd, file)
    return relativePath === "" || (!relativePath.startsWith("..") && relativePath !== "..")
  })
}

function getDisplayLineSearchText(line: DisplayLine): string {
  switch (line.kind) {
    case "thinking":
    case "text":
      return line.text
    case "tool":
      return [line.name, line.title || line.input].filter(Boolean).join(" ")
    case "question":
      return [line.header, line.question].filter(Boolean).join(" ")
    default:
      return ""
  }
}

export function findDisplayLineMatches(lines: DisplayLine[], query: string): number[] {
  const trimmed = query.trim()
  if (!trimmed) return []

  const lowerQuery = trimmed.toLowerCase()
  return lines.flatMap((line, index) =>
    stripAnsi(getDisplayLineSearchText(line)).toLowerCase().includes(lowerQuery) ? [index] : []
  )
}

export function getSearchScrollOffset(totalLines: number, msgAreaHeight: number, lineIndex: number): number {
  const safeHeight = Math.max(1, msgAreaHeight)
  const maxStart = Math.max(0, totalLines - safeHeight)
  const targetStart = Math.max(0, Math.min(lineIndex, maxStart))
  return Math.max(0, totalLines - safeHeight - targetStart)
}
