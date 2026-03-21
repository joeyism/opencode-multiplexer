import type { SessionStatus } from "../store.js"

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
