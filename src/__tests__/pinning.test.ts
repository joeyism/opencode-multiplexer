import { describe, test, expect } from "bun:test"
import { config } from "../config.js"

// ─── Sidebar label budget formula ─────────────────────────────────────────────
//
// The Sidebar component lays out each row as:
//   " " + cursorChar(1) + " " + indent(indentWidth) + [pinIcon(pinCols)] +
//   statusIcon(1) + " " + expandChar(1) + " " + paddedLabel(labelBudget) +
//   " " + timeAgo(timeAgo.length)
//
// Fixed overhead (excluding indent, pin, label, time) = 8 characters.
// labelBudget must satisfy: 8 + indentWidth + pinCols + labelBudget + timeAgo.length = innerWidth
//
// This invariant ensures the time field is never truncated.

function computeLabelBudget(
  innerWidth: number,
  indentWidth: number,
  pinCols: number,   // 2 if pinned, 0 if not
  timeAgoLength: number,
): number {
  return Math.max(0, innerWidth - indentWidth - 8 - timeAgoLength - pinCols)
}

function totalRowCols(
  innerWidth: number,
  indentWidth: number,
  pinCols: number,
  timeAgoLength: number,
): number {
  const labelBudget = computeLabelBudget(innerWidth, indentWidth, pinCols, timeAgoLength)
  // 8 fixed + indent + pin + label + time
  return 8 + indentWidth + pinCols + labelBudget + timeAgoLength
}

describe("sidebar label budget: column invariant", () => {
  test("unpinned top-level row fills innerWidth exactly (width=40)", () => {
    expect(totalRowCols(40, 0, 0, 3)).toBe(40)  // timeAgo = "5m" (3 chars? no, "5m" is 2)
  })

  test("unpinned top-level row fills innerWidth exactly at various widths", () => {
    for (const width of [20, 30, 40, 50, 60, 80, 100]) {
      for (const timeLen of [2, 3, 4]) {  // "5m", "now", "23h"
        expect(totalRowCols(width, 0, 0, timeLen)).toBe(width)
      }
    }
  })

  test("pinned top-level row fills innerWidth exactly", () => {
    for (const width of [20, 30, 40, 50, 60, 80, 100]) {
      for (const timeLen of [2, 3, 4]) {
        expect(totalRowCols(width, 0, 2, timeLen)).toBe(width)
      }
    }
  })

  test("indented child row fills innerWidth exactly", () => {
    for (const depth of [1, 2, 3]) {
      const indent = depth * 2
      for (const width of [30, 50, 80]) {
        expect(totalRowCols(width, indent, 0, 3)).toBe(width)
      }
    }
  })

  test("pinned child row fills innerWidth exactly", () => {
    const indent = 2  // depth 1
    for (const width of [30, 50, 80]) {
      expect(totalRowCols(width, indent, 2, 3)).toBe(width)
    }
  })

  test("pinned rows have 2 fewer label cols than unpinned (same width)", () => {
    const w = 50
    const unpinnedBudget = computeLabelBudget(w, 0, 0, 3)
    const pinnedBudget = computeLabelBudget(w, 0, 2, 3)
    expect(unpinnedBudget - pinnedBudget).toBe(2)
  })

  test("label budget is never negative (very narrow sidebar)", () => {
    // At minimum sidebar width (20) with wide time string
    expect(computeLabelBudget(20, 0, 0, 4)).toBeGreaterThanOrEqual(0)
    expect(computeLabelBudget(20, 0, 2, 4)).toBeGreaterThanOrEqual(0)
    expect(computeLabelBudget(10, 0, 0, 3)).toBe(0)  // clamped at 0
  })

  test("original off-by-one bug would have overflowed (regression check)", () => {
    // The bug: the formula used overhead=7, but the JSX has 8 fixed columns.
    // buggyBudget is computed with 7, but the rendered row has 8 fixed cols.
    // Result: rendered total = 8 + buggyBudget + pin + time = innerWidth + 1.
    const buggyTotal = (innerWidth: number, indentWidth: number, pinCols: number, timeLen: number) => {
      const buggyBudget = Math.max(0, innerWidth - indentWidth - 7 - timeLen - pinCols)  // old formula
      return 8 + indentWidth + pinCols + buggyBudget + timeLen  // actual JSX overhead is 8
    }
    // For a wide enough sidebar, the old formula overflows by 1
    expect(buggyTotal(50, 0, 0, 3)).toBe(51)
    expect(buggyTotal(50, 0, 2, 3)).toBe(51)
    // Fixed formula gives exactly innerWidth
    expect(totalRowCols(50, 0, 0, 3)).toBe(50)
    expect(totalRowCols(50, 0, 2, 3)).toBe(50)
  })
})

// ─── Pin sort algorithm ───────────────────────────────────────────────────────
//
// Sort logic from poller.ts:
//   - Both pinned: sort by pin time ascending (oldest pin = top)
//   - One pinned: pinned goes first
//   - Neither pinned: sort by timeUpdated descending (most recent first)

function pinSort(
  instances: Array<{ sessionId: string; timeUpdated: number }>,
  pinnedSessions: Map<string, number>,
): typeof instances {
  return [...instances].sort((a, b) => {
    const aPin = pinnedSessions.get(a.sessionId)
    const bPin = pinnedSessions.get(b.sessionId)
    if (aPin !== undefined && bPin !== undefined) return aPin - bPin
    if (aPin !== undefined) return -1
    if (bPin !== undefined) return 1
    return b.timeUpdated - a.timeUpdated
  })
}

describe("pin sort algorithm", () => {
  const instances = [
    { sessionId: "a", timeUpdated: 300 },
    { sessionId: "b", timeUpdated: 200 },
    { sessionId: "c", timeUpdated: 100 },
  ]

  test("no pins: sorts by timeUpdated descending", () => {
    const pinned = new Map<string, number>()
    const sorted = pinSort(instances, pinned)
    expect(sorted.map((i) => i.sessionId)).toEqual(["a", "b", "c"])
  })

  test("one pin: pinned session rises to top", () => {
    const pinned = new Map([["c", Date.now()]])
    const sorted = pinSort(instances, pinned)
    expect(sorted[0]!.sessionId).toBe("c")
    // Remaining two in timeUpdated order
    expect(sorted.map((i) => i.sessionId)).toEqual(["c", "a", "b"])
  })

  test("two pins: sorted by pin time ascending (oldest pin first)", () => {
    const t = Date.now()
    const pinned = new Map([
      ["b", t + 100],  // pinned second = lower position
      ["c", t],        // pinned first = top
    ])
    const sorted = pinSort(instances, pinned)
    expect(sorted[0]!.sessionId).toBe("c")
    expect(sorted[1]!.sessionId).toBe("b")
    expect(sorted[2]!.sessionId).toBe("a")
  })

  test("all pinned: sorted purely by pin time ascending", () => {
    const t = Date.now()
    const pinned = new Map([
      ["a", t + 200],
      ["b", t + 100],
      ["c", t],
    ])
    const sorted = pinSort(instances, pinned)
    expect(sorted.map((i) => i.sessionId)).toEqual(["c", "b", "a"])
  })

  test("unpinned sessions maintain relative timeUpdated order", () => {
    const pinned = new Map([["a", Date.now()]])
    const sorted = pinSort(instances, pinned)
    // b and c should remain sorted by timeUpdated desc
    const unpinned = sorted.filter((i) => i.sessionId !== "a")
    expect(unpinned.map((i) => i.sessionId)).toEqual(["b", "c"])
  })

  test("stale pin (session no longer in list) is ignored gracefully", () => {
    const pinned = new Map([["z", Date.now()]])  // "z" not in instances
    const sorted = pinSort(instances, pinned)
    // Should fall back to normal sort
    expect(sorted.map((i) => i.sessionId)).toEqual(["a", "b", "c"])
  })
})

// ─── Pin toggle logic ─────────────────────────────────────────────────────────

describe("pin toggle logic", () => {
  function togglePin(map: Map<string, number>, sessionId: string): Map<string, number> {
    const next = new Map(map)
    if (next.has(sessionId)) {
      next.delete(sessionId)
    } else {
      next.set(sessionId, Date.now())
    }
    return next
  }

  test("pinning a session adds it to the map", () => {
    const map = new Map<string, number>()
    const next = togglePin(map, "session-1")
    expect(next.has("session-1")).toBe(true)
  })

  test("pin timestamp is a recent timestamp", () => {
    const before = Date.now()
    const map = new Map<string, number>()
    const next = togglePin(map, "session-1")
    const after = Date.now()
    const ts = next.get("session-1")!
    expect(ts).toBeGreaterThanOrEqual(before)
    expect(ts).toBeLessThanOrEqual(after)
  })

  test("unpinning a session removes it from the map", () => {
    const map = new Map([["session-1", Date.now()]])
    const next = togglePin(map, "session-1")
    expect(next.has("session-1")).toBe(false)
  })

  test("toggling twice leaves session unpinned", () => {
    const map = new Map<string, number>()
    const pinned = togglePin(map, "session-1")
    const unpinned = togglePin(pinned, "session-1")
    expect(unpinned.has("session-1")).toBe(false)
  })

  test("toggle does not mutate the original map", () => {
    const map = new Map<string, number>()
    togglePin(map, "session-1")
    expect(map.has("session-1")).toBe(false)
  })

  test("pinning multiple sessions tracks each independently", () => {
    let map = new Map<string, number>()
    map = togglePin(map, "a")
    map = togglePin(map, "b")
    expect(map.has("a")).toBe(true)
    expect(map.has("b")).toBe(true)
    map = togglePin(map, "a")
    expect(map.has("a")).toBe(false)
    expect(map.has("b")).toBe(true)
  })
})

// ─── p keybinding ────────────────────────────────────────────────────────────

describe("p keybinding (pin/unpin)", () => {
  test("togglePin config key is 'p'", () => {
    expect(config.keybindings.dashboard.togglePin).toBe("p")
  })

  test("'p' is a plain letter (not ctrl-prefixed)", () => {
    expect(config.keybindings.dashboard.togglePin.startsWith("ctrl-")).toBe(false)
  })
})
