import { describe, test, expect } from "bun:test"
import React from "react"
import { render } from "ink-testing-library"
import { Text, Box } from "ink"
import { computeConversationLayout, APP_BORDER_ROWS, APP_BORDER_COLS } from "../views/layout.js"

const SIDEBAR_WIDTH = 26

function ConversationLayoutHarness({
  termHeight,
  termWidth,
  headerText,
  footerText,
  messageCount,
  isLive,
}: {
  termHeight: number
  termWidth: number
  headerText: string
  footerText: string
  messageCount: number
  isLive: boolean
}) {
  const { innerHeight, effectiveWidth, bodyHeight } = computeConversationLayout(termHeight, termWidth)
  const fullDivider = "─".repeat(effectiveWidth)
  const contentWidth = Math.max(1, effectiveWidth - SIDEBAR_WIDTH - 1)
  const divider = "─".repeat(contentWidth)

  return (
    <Box flexDirection="column" height={innerHeight}>
      {/* Header */}
      <Box paddingLeft={1}>
        <Text bold color="cyan">{headerText}</Text>
      </Box>
      <Text dimColor>{fullDivider}</Text>

      {/* Body row — explicit height */}
      <Box flexDirection="row" height={bodyHeight}>
        {/* Sidebar — explicit height */}
        <Box
          flexDirection="column"
          width={SIDEBAR_WIDTH}
          flexShrink={0}
          borderStyle="single"
          borderColor="gray"
          height={bodyHeight}
          overflow="hidden"
        >
          <Text>sessions</Text>
          <Text>item 1</Text>
          <Text>item 2</Text>
        </Box>

        {/* Message pane */}
        <Box flexDirection="column" flexGrow={1} height={bodyHeight}>
          <Box flexDirection="column" flexGrow={1} overflow="hidden" justifyContent="flex-end">
            {Array.from({ length: messageCount }, (_, i) => (
              <Box key={i} paddingLeft={3}>
                <Text wrap="truncate">Message line {i + 1}</Text>
              </Box>
            ))}
          </Box>

          {isLive && (
            <>
              <Text dimColor>{divider}</Text>
              <Box paddingLeft={1}>
                <Text dimColor>› Press i to type</Text>
              </Box>
            </>
          )}
        </Box>
      </Box>

      {/* Footer */}
      <Box paddingX={1} paddingY={0} borderStyle="single" borderColor="gray">
        <Text dimColor>{footerText}</Text>
      </Box>
    </Box>
  )
}

describe("computeConversationLayout formula", () => {
  test("innerHeight accounts for App border", () => {
    const layout = computeConversationLayout(24, 80)
    expect(layout.innerHeight).toBe(24 - APP_BORDER_ROWS)
  })

  test("effectiveWidth accounts for App border", () => {
    const layout = computeConversationLayout(24, 80)
    expect(layout.effectiveWidth).toBe(80 - APP_BORDER_COLS)
  })

  test("header + body + footer fit within innerHeight", () => {
    for (const h of [12, 15, 20, 24, 30, 40, 50]) {
      const layout = computeConversationLayout(h, 80)
      const total = layout.bodyHeight + layout.HEADER_ROWS + layout.FOOTER_ROWS
      expect(total).toBeLessThanOrEqual(layout.innerHeight)
    }
  })

  test("header + body + footer + killConfirm fit within innerHeight", () => {
    for (const h of [15, 20, 24, 30]) {
      const layout = computeConversationLayout(h, 80, { killConfirm: true })
      const total = layout.bodyHeight + layout.HEADER_ROWS + layout.FOOTER_ROWS + 3
      expect(total).toBeLessThanOrEqual(layout.innerHeight)
    }
  })

  test("bodyHeight never goes below minimum of 5", () => {
    const layout = computeConversationLayout(10, 80)
    expect(layout.bodyHeight).toBeGreaterThanOrEqual(5)
  })

  test("very small terminal still produces valid layout", () => {
    const layout = computeConversationLayout(8, 40)
    expect(layout.innerHeight).toBe(6)
    expect(layout.bodyHeight).toBeGreaterThanOrEqual(5)
    expect(layout.effectiveWidth).toBe(38)
  })
})

describe("conversation layout", () => {
  test("header is visible with few messages", () => {
    const { lastFrame } = render(
      <ConversationLayoutHarness
        termHeight={24}
        termWidth={80}
        headerText="myrepo / my session"
        footerText="q: back  i: insert"
        messageCount={3}
        isLive={true}
      />
    )
    const lines = lastFrame()!.split("\n")
    expect(lines[0]).toContain("myrepo / my session")
  })

  test("header is visible with many messages", () => {
    const { lastFrame } = render(
      <ConversationLayoutHarness
        termHeight={24}
        termWidth={80}
        headerText="myrepo / my session"
        footerText="q: back  i: insert"
        messageCount={100}
        isLive={true}
      />
    )
    const lines = lastFrame()!.split("\n")
    expect(lines[0]).toContain("myrepo / my session")
  })

  test("footer is visible", () => {
    const { lastFrame } = render(
      <ConversationLayoutHarness
        termHeight={24}
        termWidth={80}
        headerText="myrepo / my session"
        footerText="q: back  i: insert"
        messageCount={100}
        isLive={true}
      />
    )
    const output = lastFrame()!
    expect(output).toContain("q: back  i: insert")
  })

  test("header visible for non-live sessions", () => {
    const { lastFrame } = render(
      <ConversationLayoutHarness
        termHeight={24}
        termWidth={80}
        headerText="myrepo / read only session"
        footerText="q: back"
        messageCount={50}
        isLive={false}
      />
    )
    const lines = lastFrame()!.split("\n")
    expect(lines[0]).toContain("myrepo / read only session")
  })

  test("header visible on small terminal", () => {
    const { lastFrame } = render(
      <ConversationLayoutHarness
        termHeight={15}
        termWidth={60}
        headerText="repo / session"
        footerText="q: back"
        messageCount={50}
        isLive={true}
      />
    )
    const lines = lastFrame()!.split("\n")
    expect(lines[0]).toContain("repo / session")
  })

  test("output does not exceed termHeight lines", () => {
    const termHeight = 24
    const { lastFrame } = render(
      <ConversationLayoutHarness
        termHeight={termHeight}
        termWidth={80}
        headerText="myrepo / my session"
        footerText="q: back"
        messageCount={100}
        isLive={true}
      />
    )
    const lines = lastFrame()!.split("\n")
    expect(lines.length).toBeLessThanOrEqual(termHeight)
  })
})
