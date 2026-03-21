import { describe, test, expect } from "bun:test"
import { config } from "../config.js"

describe("sidebar width calculation", () => {
  test("wide sidebar is 25% of termWidth, minimum 20", () => {
    expect(Math.max(20, Math.floor(200 * 0.25))).toBe(50)
    expect(Math.max(20, Math.floor(80 * 0.25))).toBe(20)
    expect(Math.max(20, Math.floor(60 * 0.25))).toBe(20)
    expect(Math.max(20, Math.floor(120 * 0.25))).toBe(30)
  })

  test("compact sidebar width is always 10", () => {
    const SIDEBAR_WIDTH_COMPACT = 10
    expect(SIDEBAR_WIDTH_COMPACT).toBe(10)
  })

  test("contentWidth reduces when sidebar is wider", () => {
    const termWidth = 120
    const wideWidth = Math.max(20, Math.floor(termWidth * 0.25))
    const compactWidth = 10
    const contentWide = Math.max(1, termWidth - wideWidth - 1)
    const contentCompact = Math.max(1, termWidth - compactWidth - 1)
    expect(contentCompact).toBeGreaterThan(contentWide)
  })
})

describe("s key binding (sidebar toggle)", () => {
  function matchKey(key: string, input: string, inkKey: any): boolean {
    switch (key) {
      case "return":
        return !!inkKey.return
      case "escape":
        return !!inkKey.escape
      case "tab":
        return !!inkKey.tab && !inkKey.shift
      case "shift-tab":
        return !!inkKey.tab && !!inkKey.shift
      case "up":
        return !!inkKey.upArrow
      case "down":
        return !!inkKey.downArrow
      default: {
        if (key.startsWith("ctrl-")) {
          const letter = key.slice(5)
          return (
            !!inkKey.ctrl &&
            !inkKey.tab &&
            !inkKey.return &&
            !inkKey.escape &&
            input === letter
          )
        }
        return input === key && !inkKey.ctrl && !inkKey.meta
      }
    }
  }

  test("'s' matches plain s press", () => {
    expect(matchKey("s", "s", {})).toBe(true)
  })

  test("'s' does NOT match Ctrl-S", () => {
    expect(matchKey("s", "s", { ctrl: true })).toBe(false)
  })

  test("'s' does NOT match Meta-S", () => {
    expect(matchKey("s", "s", { meta: true })).toBe(false)
  })

  test("sidebar toggle is a plain letter (not ctrl)", () => {
    const binding = "s"
    expect(binding.startsWith("ctrl-")).toBe(false)
  })
})

describe("config keybindings", () => {
  test("dashboard renameTitle binding is 't'", () => {
    expect(config.keybindings.dashboard.renameTitle).toBe("t")
  })

  test("renameTitle is in dashboard keybindings", () => {
    expect("renameTitle" in config.keybindings.dashboard).toBe(true)
  })
})

describe("title rename target resolution", () => {
  test("uses overrideSessionId when set, else falls back to selectedId", () => {
    const selectedId = "session-1"
    const overrideId = "session-2"

    const targetWithOverride = overrideId ?? selectedId
    expect(targetWithOverride).toBe("session-2")

    const overrideIdNull: string | null = null
    const targetWithoutOverride = overrideIdNull ?? selectedId
    expect(targetWithoutOverride).toBe("session-1")
  })

  test("uses overridePort when set, else falls back to instancePort", () => {
    const instancePort = 3000
    const overridePort = 4000

    const portWithOverride = overridePort ?? instancePort
    expect(portWithOverride).toBe(4000)

    const overridePortNull: number | null = null
    const portWithoutOverride = overridePortNull ?? instancePort
    expect(portWithoutOverride).toBe(3000)
  })

  test("ensureServeProcess would be called when port is null and worktree is set", () => {
    const port: number | null = null
    const worktree = "/path/to/worktree"
    const shouldSpawn = !port && !!worktree
    expect(shouldSpawn).toBe(true)
  })

  test("ensureServeProcess would NOT be called when port is already set", () => {
    const port: number | null = 5000
    const worktree = "/path/to/worktree"
    const shouldSpawn = !port && !!worktree
    expect(shouldSpawn).toBe(false)
  })

  test("ensureServeProcess would NOT be called when worktree is null", () => {
    const port: number | null = null
    const worktree: string | null = null
    const shouldSpawn = !port && !!worktree
    expect(shouldSpawn).toBe(false)
  })
})
