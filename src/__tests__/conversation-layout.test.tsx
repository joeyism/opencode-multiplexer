import { describe, test, expect } from "bun:test"
import React from "react"
import { render } from "ink-testing-library"
import { Text, Box } from "ink"

const SIDEBAR_WIDTH = 26
const HEADER_ROWS = 2   // title line + divider line
const FOOTER_ROWS = 3   // bordered box: top border + content + bottom border

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
  const bodyHeight = Math.max(5, termHeight - HEADER_ROWS - FOOTER_ROWS)
  const fullDivider = "─".repeat(termWidth)
  const contentWidth = Math.max(1, termWidth - SIDEBAR_WIDTH - 1)
  const divider = "─".repeat(contentWidth)

  return (
    <Box flexDirection="column" height={termHeight}>
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
