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
