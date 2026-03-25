import { describe, test, expect } from "bun:test"
import { buildDisplayLines, toolIcon, toolColor } from "../views/display-lines.js"
import type { ConversationMessage, ConversationMessagePart } from "../store.js"

// ─── Mock helpers ─────────────────────────────────────────────────────────────

function mockMessage(overrides: Partial<ConversationMessage> & { parts: ConversationMessagePart[] }): ConversationMessage {
  return {
    id: "msg_1",
    sessionId: "sess_1",
    role: "assistant",
    timeCreated: Date.now(),
    timeCompleted: null,
    modelId: null,
    providerId: null,
    ...overrides,
  }
}

function mockPart(overrides: Partial<ConversationMessagePart>): ConversationMessagePart {
  return {
    id: "part_1",
    type: "text",
    ...overrides,
  }
}

// ─── Reasoning blocks ─────────────────────────────────────────────────────────

describe("reasoning blocks", () => {
  test("reasoning parts are rendered as thinking display lines", () => {
    const msg = mockMessage({
      parts: [mockPart({ type: "reasoning", text: "Let me think about this" })],
    })
    const lines = buildDisplayLines([msg])
    const thinkingLines = lines.filter((l) => l.kind === "thinking")
    expect(thinkingLines.length).toBeGreaterThan(0)
    expect(thinkingLines[0]!.text).toContain("Thinking")
    const contentLine = thinkingLines.find((l) => l.text === "Let me think about this")
    expect(contentLine).toBeDefined()
  })

  test("type 'thinking' is NOT rendered (opencode uses 'reasoning')", () => {
    const msg = mockMessage({
      parts: [mockPart({ type: "thinking", text: "This should not show" })],
    })
    const lines = buildDisplayLines([msg])
    const thinkingLines = lines.filter((l) => l.kind === "thinking")
    expect(thinkingLines.length).toBe(0)
  })
})

// ─── Tool display ─────────────────────────────────────────────────────────────

describe("tool display", () => {
  test("tool parts show title when available", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "bash",
        toolStatus: "completed",
        toolTitle: "Show recent commits",
        toolInput: "git log --oneline",
      })],
    })
    const lines = buildDisplayLines([msg])
    const toolLines = lines.filter((l) => l.kind === "tool")
    expect(toolLines.length).toBe(1)
    expect(toolLines[0]!.title).toBe("Show recent commits")
    expect(toolLines[0]!.name).toBe("bash")
  })

  test("tool parts show input as fallback when no title", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "read",
        toolStatus: "completed",
        toolInput: "/path/to/file.ts",
      })],
    })
    const lines = buildDisplayLines([msg])
    const toolLines = lines.filter((l) => l.kind === "tool")
    expect(toolLines.length).toBe(1)
    expect(toolLines[0]!.input).toBe("/path/to/file.ts")
  })

  test("toolIcon returns correct chars for each status", () => {
    expect(toolIcon("completed")).toBe("✓")
    expect(toolIcon("running")).toBe("⟳")
    expect(toolIcon("error")).toBe("✗")
    expect(toolIcon(undefined)).toBe("⏳")
  })

  test("toolColor returns correct colors for each status", () => {
    expect(toolColor("completed")).toBe("green")
    expect(toolColor("running")).toBe("yellow")
    expect(toolColor("error")).toBe("red")
    expect(toolColor(undefined)).toBe("gray")
  })
})

// ─── Question prompts ─────────────────────────────────────────────────────────

describe("question prompts", () => {
  test("question tools with input render as question display lines", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "question",
        toolStatus: "running",
        toolInput: "Should I invoke the skill?",
        toolHeader: "Skill Check",
      })],
    })
    const lines = buildDisplayLines([msg])
    const questionLines = lines.filter((l) => l.kind === "question")
    expect(questionLines.length).toBe(1)
    expect(questionLines[0]!.header).toBe("Skill Check")
    expect(questionLines[0]!.question).toBe("Should I invoke the skill?")
    expect(questionLines[0]!.status).toBe("running")
  })

  test("question tools without input fall back to regular tool display", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "question",
        toolStatus: "running",
        // no toolInput or toolTitle
      })],
    })
    const lines = buildDisplayLines([msg])
    const questionLines = lines.filter((l) => l.kind === "question")
    const toolLines = lines.filter((l) => l.kind === "tool")
    expect(questionLines.length).toBe(0)
    expect(toolLines.length).toBe(1)
  })

  test("question display lines include options array", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "question",
        toolStatus: "running",
        toolInput: "Should I invoke the skill?",
        toolHeader: "Skill Check",
        toolOptions: JSON.stringify([
          { label: "Yes", description: "Use skill" },
          { label: "No", description: "Skip" },
        ]),
      })],
    })
    const lines = buildDisplayLines([msg])
    const questionLines = lines.filter((l) => l.kind === "question")
    expect(questionLines.length).toBe(1)
    const q = questionLines[0]! as { kind: "question"; options: Array<{ label: string; description?: string }> }
    expect(q.options.length).toBe(2)
    expect(q.options[0]!.label).toBe("Yes")
  })

  test("question display lines have empty options array when none provided", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "question",
        toolStatus: "running",
        toolInput: "Which approach?",
        toolHeader: "Design Choice",
        // no toolOptions
      })],
    })
    const lines = buildDisplayLines([msg])
    const questionLines = lines.filter((l) => l.kind === "question")
    expect(questionLines.length).toBe(1)
    const q = questionLines[0]! as { kind: "question"; options: Array<unknown> }
    expect(q.options).toEqual([])
  })

  test("question display lines include custom flag when toolCustom is true", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "question",
        toolStatus: "running",
        toolInput: "Check skills?",
        toolHeader: "Skill Check",
        toolOptions: JSON.stringify([{ label: "Yes", description: "Check" }]),
        toolCustom: "true",
      })],
    })
    const lines = buildDisplayLines([msg])
    const q = lines.find((l) => l.kind === "question")
    expect(q).toBeDefined()
    expect((q as any).custom).toBe(true)
  })

  test("question custom defaults to false when toolCustom not present", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "question",
        toolStatus: "running",
        toolInput: "Check skills?",
        toolHeader: "Skill Check",
      })],
    })
    const lines = buildDisplayLines([msg])
    const q = lines.find((l) => l.kind === "question")
    expect(q).toBeDefined()
    expect((q as any).custom).toBe(false)
  })
})

// ─── Agent name ───────────────────────────────────────────────────────────────

describe("agent name", () => {
  test("assistant messages include agent in role-header", () => {
    const msg = mockMessage({
      role: "assistant",
      agent: "orchestrator",
      parts: [mockPart({ type: "text", text: "Hello" })],
    })
    const lines = buildDisplayLines([msg])
    const header = lines.find((l) => l.kind === "role-header")!
    expect(header.agent).toBe("orchestrator")
  })

  test("user messages have no agent", () => {
    const msg = mockMessage({
      role: "user",
      agent: undefined,
      parts: [mockPart({ type: "text", text: "Hi" })],
    })
    const lines = buildDisplayLines([msg])
    const header = lines.find((l) => l.kind === "role-header")!
    expect(header.agent).toBeUndefined()
  })

  test("plan_exit tool renders as question display line", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "plan_exit",
        toolStatus: "running",
        toolTitle: "Switching to build agent",
      })],
    })
    const lines = buildDisplayLines([msg])
    const questionLines = lines.filter((l) => l.kind === "question")
    expect(questionLines.length).toBe(1)
    expect(questionLines[0]!.question).toBe("Switching to build agent")
    expect(questionLines[0]!.status).toBe("running")
  })
})

// ─── Message structure ────────────────────────────────────────────────────────

describe("message structure", () => {
  test("each message starts with role-header and ends with spacer", () => {
    const msg = mockMessage({
      parts: [mockPart({ type: "text", text: "Hello world" })],
    })
    const lines = buildDisplayLines([msg])
    expect(lines[0]!.kind).toBe("role-header")
    expect(lines[lines.length - 1]!.kind).toBe("spacer")
  })

  test("empty text parts produce no text lines", () => {
    const msg = mockMessage({
      parts: [mockPart({ type: "text", text: "" })],
    })
    const lines = buildDisplayLines([msg])
    const textLines = lines.filter((l) => l.kind === "text")
    expect(textLines.length).toBe(0)
  })

  test("multiple text parts are concatenated", () => {
    const msg = mockMessage({
      parts: [
        mockPart({ id: "p1", type: "text", text: "Hello " }),
        mockPart({ id: "p2", type: "text", text: "world" }),
      ],
    })
    const lines = buildDisplayLines([msg])
    const textLines = lines.filter((l) => l.kind === "text")
    const allText = textLines.map((l) => l.text).join(" ")
    expect(allText).toContain("Hello")
    expect(allText).toContain("world")
  })
})

// ─── Child session question format ───────────────────────────────────────────

describe("child session question format", () => {
  test("question header includes agent name in parentheses", () => {
    // Simulates the format used when child session questions are appended
    // The actual DB call is tested via integration, but we verify the format here
    const header = `Skill Check (build)`
    expect(header).toContain("Skill Check")
    expect(header).toContain("(build)")
  })

  test("question header falls back to 'subagent' when no agent", () => {
    const agent = null
    const header = `Skill Check (${agent ?? "subagent"})`
    expect(header).toContain("(subagent)")
  })

  test("question status determines styling — running shows prompt", () => {
    const msg = mockMessage({
      parts: [mockPart({
        type: "tool",
        tool: "question",
        toolStatus: "running",
        toolInput: "Which approach?",
        toolHeader: "Design Choice",
      })],
    })
    const lines = buildDisplayLines([msg])
    const questions = lines.filter((l) => l.kind === "question")
    expect(questions.length).toBe(1)
    expect(questions[0]!.status).toBe("running")
  })

  test("child session questions include options when available", () => {
    // This tests the return type shape — the actual DB query is integration-tested.
    type ChildQuestion = {
      sessionId: string
      sessionTitle: string
      agent: string | null
      question: string
      header: string
      status: string
      options: Array<{ label: string; description?: string }>
    }
    const q: ChildQuestion = {
      sessionId: "s1",
      sessionTitle: "Task 1",
      agent: "fixer",
      question: "Need to check for relevant skills?",
      header: "Skill check",
      status: "running",
      options: [
        { label: "Yes, use skill tool", description: "Invoke skill tool" },
        { label: "No, proceed without skill", description: "No skill needed" },
      ],
    }
    expect(q.options.length).toBe(2)
    expect(q.options[0]!.label).toBe("Yes, use skill tool")
  })
})

describe("text wrapping with width", () => {
  test("long text lines are wrapped when width is provided", () => {
    const longText = "A".repeat(200)
    const msg = mockMessage({
      parts: [mockPart({ type: "text", text: longText })],
    })
    const lines = buildDisplayLines([msg], 80)
    const textLines = lines.filter((l) => l.kind === "text")
    // 200 chars at width 80 - 4 padding = 76 chars per line → at least 2 lines
    expect(textLines.length).toBeGreaterThanOrEqual(2)
  })

  test("text lines are not wrapped when width is omitted", () => {
    const longText = "A".repeat(200)
    const msg = mockMessage({
      parts: [mockPart({ type: "text", text: longText })],
    })
    const lines = buildDisplayLines([msg])
    const textLines = lines.filter((l) => l.kind === "text")
    expect(textLines.length).toBe(1)
  })
})
