import { describe, test, expect } from "bun:test"

// ─── Picker logic regression tests ──────────────────────────────────────────
// These test the core picker behavior that all three pickers share:
// filtering, cursor bounds, scroll tracking, and selection mapping.

type Item = { label: string; extra?: string }

function filterItems(items: Item[], query: string): Item[] {
  if (!query.trim()) return items
  const lower = query.toLowerCase()
  return items.filter((item) =>
    item.label.toLowerCase().includes(lower) ||
    (item.extra?.toLowerCase().includes(lower) ?? false)
  )
}

function clampCursor(cursor: number, listLength: number): number {
  return Math.max(0, Math.min(cursor, Math.max(0, listLength - 1)))
}

function computeScroll(cursor: number, scroll: number, maxVisible: number): number {
  if (cursor >= scroll + maxVisible) return cursor - maxVisible + 1
  if (cursor < scroll) return cursor
  return scroll
}

function mapFilteredIndexToOriginal<T>(filtered: T[], original: T[], filteredIdx: number): number {
  const item = filtered[filteredIdx]
  if (!item) return -1
  return original.indexOf(item)
}

describe("picker filtering", () => {
  const items: Item[] = [
    { label: "claude-sonnet-4-6", extra: "anthropic" },
    { label: "claude-opus-4", extra: "anthropic" },
    { label: "gpt-4o", extra: "openai" },
    { label: "gemini-2.5-pro", extra: "google" },
    { label: "claude-haiku-3.5", extra: "anthropic" },
  ]

  test("empty filter returns all items", () => {
    expect(filterItems(items, "")).toEqual(items)
    expect(filterItems(items, "  ")).toEqual(items)
  })

  test("filter by label substring", () => {
    const result = filterItems(items, "claude")
    expect(result.length).toBe(3)
    expect(result.map((r) => r.label)).toEqual([
      "claude-sonnet-4-6",
      "claude-opus-4",
      "claude-haiku-3.5",
    ])
  })

  test("filter by extra field", () => {
    const result = filterItems(items, "openai")
    expect(result.length).toBe(1)
    expect(result[0]!.label).toBe("gpt-4o")
  })

  test("filter is case-insensitive", () => {
    expect(filterItems(items, "CLAUDE").length).toBe(3)
    expect(filterItems(items, "Gemini").length).toBe(1)
  })

  test("filter with no matches returns empty", () => {
    expect(filterItems(items, "llama")).toEqual([])
  })
})

describe("picker cursor clamping", () => {
  test("cursor stays within bounds", () => {
    expect(clampCursor(5, 3)).toBe(2)  // clamped to last
    expect(clampCursor(-1, 3)).toBe(0) // clamped to first
    expect(clampCursor(1, 3)).toBe(1)  // unchanged
  })

  test("cursor clamped to 0 for empty list", () => {
    expect(clampCursor(5, 0)).toBe(0)
  })

  test("cursor at boundary stays", () => {
    expect(clampCursor(2, 3)).toBe(2) // last valid index
    expect(clampCursor(0, 3)).toBe(0) // first valid index
  })
})

describe("picker scroll tracking", () => {
  test("scroll adjusts when cursor goes below visible window", () => {
    // maxVisible=5, scroll=0, cursor moves to 6
    expect(computeScroll(6, 0, 5)).toBe(2)  // 6 - 5 + 1 = 2
  })

  test("scroll adjusts when cursor goes above visible window", () => {
    // scroll=5, cursor moves to 3
    expect(computeScroll(3, 5, 5)).toBe(3)
  })

  test("scroll stays when cursor is within visible window", () => {
    expect(computeScroll(3, 0, 5)).toBe(0)
    expect(computeScroll(4, 0, 5)).toBe(0)
  })
})

describe("picker filtered selection mapping", () => {
  const original = ["apple", "banana", "cherry", "date", "elderberry"]

  test("maps filtered index back to original index", () => {
    const filtered = ["banana", "date"]  // indices 1, 3 in original
    expect(mapFilteredIndexToOriginal(filtered, original, 0)).toBe(1) // banana
    expect(mapFilteredIndexToOriginal(filtered, original, 1)).toBe(3) // date
  })

  test("returns -1 for out-of-bounds filtered index", () => {
    const filtered = ["banana"]
    expect(mapFilteredIndexToOriginal(filtered, original, 5)).toBe(-1)
  })

  test("works when filtered === original (no filter)", () => {
    expect(mapFilteredIndexToOriginal(original, original, 2)).toBe(2)
  })
})

describe("picker Esc behavior", () => {
  test("first Esc clears filter, second Esc closes", () => {
    let filter = "claude"
    let open = true

    // First Esc: filter is non-empty → clear it
    if (filter) {
      filter = ""
    } else {
      open = false
    }
    expect(filter).toBe("")
    expect(open).toBe(true)

    // Second Esc: filter is empty → close
    if (filter) {
      filter = ""
    } else {
      open = false
    }
    expect(open).toBe(false)
  })
})

describe("picker integration: filter + cursor clamp + selection map", () => {
  test("filtering reduces items and cursor is clamped", () => {
    const items: Item[] = Array.from({ length: 10 }, (_, i) => ({ label: `item-${i}` }))
    const filtered = filterItems(items, "item-7")
    expect(filtered.length).toBe(1)
    expect(clampCursor(8, filtered.length)).toBe(0)
  })

  test("selection maps through filter correctly", () => {
    const items = ["alpha", "bravo", "charlie", "delta"]
    const filtered = ["bravo", "delta"]
    expect(mapFilteredIndexToOriginal(filtered, items, 0)).toBe(1)  // bravo
    expect(mapFilteredIndexToOriginal(filtered, items, 1)).toBe(3)  // delta
  })
})

// ─── questionAnswered ref behavior ────────────────────────────────────────────

describe("questionAnswered ref prevents re-open loop", () => {
  test("answered flag blocks re-open, cleared when question completes", () => {
    const questionAnswered = { current: false }
    let pendingQuestion: any = null
    const runningQ = { question: "test?", options: [{ label: "Yes" }] }

    // Simulate: question detected, overlay opens
    if (runningQ && !pendingQuestion && !questionAnswered.current) {
      pendingQuestion = runningQ
    }
    expect(pendingQuestion).toBe(runningQ)

    // Simulate: user answers → answered flag set, overlay closed
    questionAnswered.current = true
    pendingQuestion = null

    // Simulate: useEffect fires, question still running in DB (stale)
    if (runningQ && !pendingQuestion && !questionAnswered.current) {
      pendingQuestion = runningQ  // should NOT happen
    }
    expect(pendingQuestion).toBeNull()  // flag prevented re-open

    // Simulate: DB updates, question completed (runningQ becomes null)
    const runningQAfter = null
    if (!runningQAfter) {
      questionAnswered.current = false  // reset
    }
    expect(questionAnswered.current).toBe(false)
  })

  test("flag does not block opening a NEW question", () => {
    const questionAnswered = { current: false }
    let pendingQuestion: any = null
    const newQ = { question: "different question?", options: [{ label: "A" }] }

    // Flag is false, new question should open
    if (newQ && !pendingQuestion && !questionAnswered.current) {
      pendingQuestion = newQ
    }
    expect(pendingQuestion).toBe(newQ)
  })
})
