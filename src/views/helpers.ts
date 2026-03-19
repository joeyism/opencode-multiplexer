import type { SessionStatus } from "../store.js"

export function statusIcon(status: SessionStatus): { char: string; color: string } {
  switch (status) {
    case "working":     return { char: "▶", color: "green" }
    case "needs-input": return { char: "●", color: "yellow" }
    case "idle":        return { char: "✔", color: "gray" }
    case "error":       return { char: "✖", color: "red" }
  }
}
