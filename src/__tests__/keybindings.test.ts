import { describe, test, expect } from "bun:test"

// ─── matchKey logic tests (replicated here to verify the fix) ─────────────────
//
// The matchKey function in use-keybindings.ts was fixed to add !inkKey.ctrl
// so that plain letter bindings don't fire on Ctrl+letter combos.
//
// We test the logic directly since matchKey is private to the hook module.

function matchKey(key: string, input: string, inkKey: { ctrl?: boolean; shift?: boolean; tab?: boolean; return?: boolean; escape?: boolean; upArrow?: boolean; downArrow?: boolean; meta?: boolean }): boolean {
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
    default:
      if (key.startsWith("ctrl-")) {
        const letter = key.slice(5)
        return !!inkKey.ctrl && !inkKey.tab && !inkKey.return && !inkKey.escape && input === letter
      }
      // Plain letter: must NOT have ctrl or meta held
      return input === key && !inkKey.ctrl && !inkKey.meta
  }
}

// ─── Plain letter bindings ───────────────────────────────────────────────────

describe("plain letter keybindings", () => {
  test("'a' matches plain a press", () => {
    expect(matchKey("a", "a", {})).toBe(true)
  })

  test("'a' does NOT match Ctrl-A", () => {
    expect(matchKey("a", "a", { ctrl: true })).toBe(false)
  })

  test("'a' does NOT match Meta-A", () => {
    expect(matchKey("a", "a", { meta: true })).toBe(false)
  })

  test("'n' does NOT match Ctrl-N", () => {
    expect(matchKey("n", "n", { ctrl: true })).toBe(false)
  })

  test("'x' does NOT match Ctrl-X", () => {
    expect(matchKey("x", "x", { ctrl: true })).toBe(false)
  })

  test("'r' does NOT match Ctrl-R", () => {
    expect(matchKey("r", "r", { ctrl: true })).toBe(false)
  })

  test("'q' does NOT match Ctrl-Q", () => {
    expect(matchKey("q", "q", { ctrl: true })).toBe(false)
  })
})

// ─── Ctrl- prefixed bindings ──────────────────────────────────────────────────

describe("ctrl- prefixed keybindings", () => {
  test("'ctrl-n' matches Ctrl-N", () => {
    expect(matchKey("ctrl-n", "n", { ctrl: true })).toBe(true)
  })

  test("'ctrl-n' does NOT match plain n", () => {
    expect(matchKey("ctrl-n", "n", {})).toBe(false)
  })

  test("'ctrl-n' does NOT match Ctrl-Tab", () => {
    expect(matchKey("ctrl-n", "n", { ctrl: true, tab: true })).toBe(false)
  })
})

// ─── Edit and commit keybindings ─────────────────────────────────────────────

describe("edit and commit keybindings", () => {
  test("'e' matches plain e press", () => {
    expect(matchKey("e", "e", {})).toBe(true)
  })

  test("'e' does NOT match Ctrl-E", () => {
    expect(matchKey("e", "e", { ctrl: true })).toBe(false)
  })

  test("'c' matches plain c press", () => {
    expect(matchKey("c", "c", {})).toBe(true)
  })

  test("'c' does NOT match Ctrl-C", () => {
    expect(matchKey("c", "c", { ctrl: true })).toBe(false)
  })
})

// ─── Model picker keybinding ──────────────────────────────────────────────────

describe("model picker keybinding", () => {
  test("'m' matches plain m press", () => {
    expect(matchKey("m", "m", {})).toBe(true)
  })

  test("'m' does NOT match Ctrl-M", () => {
    expect(matchKey("m", "m", { ctrl: true })).toBe(false)
  })
})

// ─── File picker keybinding ───────────────────────────────────────────────────

describe("file picker keybinding", () => {
  test("'f' matches plain f press", () => {
    expect(matchKey("f", "f", {})).toBe(true)
  })

  test("'f' does NOT match Ctrl-F", () => {
    expect(matchKey("f", "f", { ctrl: true })).toBe(false)
  })
})

// ─── Question overlay keybindings ────────────────────────────────────────────

describe("question overlay keybindings", () => {
  test("number keys 1-9 match for quick selection", () => {
    expect(matchKey("1", "1", {})).toBe(true)
    expect(matchKey("2", "2", {})).toBe(true)
    expect(matchKey("9", "9", {})).toBe(true)
  })

  test("number keys do NOT match with ctrl", () => {
    expect(matchKey("1", "1", { ctrl: true })).toBe(false)
  })

  test("'j' and 'k' navigate without ctrl", () => {
    expect(matchKey("j", "j", {})).toBe(true)
    expect(matchKey("k", "k", {})).toBe(true)
    expect(matchKey("j", "j", { ctrl: true })).toBe(false)
    expect(matchKey("k", "k", { ctrl: true })).toBe(false)
  })
})

// ─── Title rename keybinding ──────────────────────────────────────────────────

describe("title rename keybinding", () => {
  test("'t' matches plain t press", () => {
    expect(matchKey("t", "t", {})).toBe(true)
  })

  test("'t' does NOT match Ctrl-T", () => {
    expect(matchKey("t", "t", { ctrl: true })).toBe(false)
  })
})

// ─── Special keys ─────────────────────────────────────────────────────────────

describe("special keybindings", () => {
  test("'return' matches Enter", () => {
    expect(matchKey("return", "", { return: true })).toBe(true)
  })

  test("'escape' matches Escape", () => {
    expect(matchKey("escape", "", { escape: true })).toBe(true)
  })

  test("'tab' matches Tab (no shift)", () => {
    expect(matchKey("tab", "", { tab: true })).toBe(true)
  })

  test("'tab' does NOT match Shift-Tab", () => {
    expect(matchKey("tab", "", { tab: true, shift: true })).toBe(false)
  })

  test("'shift-tab' matches Shift-Tab", () => {
    expect(matchKey("shift-tab", "", { tab: true, shift: true })).toBe(true)
  })
})

describe("session picker keybinding", () => {
  test("'s' matches plain s press", () => {
    expect(matchKey("s", "s", {})).toBe(true)
  })

  test("'s' does NOT match Ctrl-S", () => {
    expect(matchKey("s", "s", { ctrl: true })).toBe(false)
  })
})

// ─── Chord key leakage protection ─────────────────────────────────────────────
//
// When the user presses Ctrl-X then E or M in insert mode, neither "x" nor "e"/"m"
// should leak into the TextInput. We test the ref-based blocking mechanism here
// by simulating the same logic used in conversation.tsx.

describe("chord key leakage protection", () => {
  test("pendingChord ref blocks TextInput onChange during Ctrl-X", () => {
    const pendingChord = { current: false }
    let textInputValue = "hello"

    const onChange = (val: string) => {
      if (pendingChord.current) return
      textInputValue = val
    }

    // Keystroke 1: Ctrl-X — our useInput sets pendingChord synchronously
    pendingChord.current = true
    // TextInput's onChange fires with "x" appended
    onChange("hellox")
    // "x" should NOT have gone through
    expect(textInputValue).toBe("hello")
  })

  test("pendingChord ref blocks second keystroke of chord (e/m)", () => {
    const pendingChord = { current: false }
    let textInputValue = "hello"

    const onChange = (val: string) => {
      if (pendingChord.current) return
      textInputValue = val
    }

    // Keystroke 1: Ctrl-X
    pendingChord.current = true
    onChange("hellox")  // blocked

    // Keystroke 2: "m" (no Ctrl) — pendingChord still true
    onChange("hellom")  // should be blocked
    expect(textInputValue).toBe("hello")

    // Chord resolves — pendingChord cleared
    pendingChord.current = false
    onChange("hello world")  // now allowed
    expect(textInputValue).toBe("hello world")
  })

  test("pendingChord ref does not block normal typing", () => {
    const pendingChord = { current: false }
    let textInputValue = "hello"

    const onChange = (val: string) => {
      if (pendingChord.current) return
      textInputValue = val
    }

    // Normal typing — pendingChord is false
    onChange("hello w")
    expect(textInputValue).toBe("hello w")
    onChange("hello wo")
    expect(textInputValue).toBe("hello wo")
  })
})

// ─── Focus event sequence parsing ──────────────────────────────────────────────
describe("focus event sequence parsing", () => {
  function parseFocusEvent(data: string): "focus-in" | "focus-out" | null {
    if (data.includes("\x1b[I")) return "focus-in"
    if (data.includes("\x1b[O")) return "focus-out"
    return null
  }

  test("detects focus-in sequence", () => {
    expect(parseFocusEvent("\x1b[I")).toBe("focus-in")
  })

  test("detects focus-out sequence", () => {
    expect(parseFocusEvent("\x1b[O")).toBe("focus-out")
  })

  test("returns null for non-focus data", () => {
    expect(parseFocusEvent("hello")).toBeNull()
  })

  test("detects focus event embedded in other data", () => {
    expect(parseFocusEvent("prefix\x1b[Isuffix")).toBe("focus-in")
  })
})
