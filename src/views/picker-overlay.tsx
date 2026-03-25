import React from "react"
import { Box, Text } from "ink"
import type { PickerOverlayState } from "../hooks/use-picker-overlay.js"

export interface PickerOverlayProps<T> {
  /** Title displayed in the header */
  title: string
  /** Overlay state from usePickerOverlay */
  state: PickerOverlayState<T>
  /** Max visible items */
  maxVisible: number
  /** Width of the overlay box */
  width: number
  /** Render function for a single item */
  renderItem: (item: T, index: number, isCursor: boolean) => React.ReactNode
  /** Hint text at the bottom. Defaults to "↑↓: nav  Enter: select  Esc: clear/close  type to filter" */
  hint?: string
  /** Optional label appended to the item count (e.g., "modified") */
  countLabel?: string
  /** Border color (default: "cyan") */
  borderColor?: string
}

export function PickerOverlay<T>({
  title,
  state,
  maxVisible,
  width,
  renderItem,
  hint,
  countLabel,
  borderColor = "cyan",
}: PickerOverlayProps<T>) {
  const { cursor, scroll, filter, filteredItems } = state
  const visibleItems = filteredItems.slice(scroll, scroll + maxVisible)
  const showScrollUp = scroll > 0
  const showScrollDown = scroll + maxVisible < filteredItems.length

  return (
    <Box flexDirection="column" paddingX={2} paddingY={0} borderStyle="round" borderColor={borderColor} width={width}>
      <Box>
        <Text bold color={borderColor}>{title}</Text>
        <Text dimColor>  {filteredItems.length}{countLabel ? ` ${countLabel}` : ""}</Text>
      </Box>
      {/* Filter input line */}
      <Box>
        <Text color={borderColor}>› </Text>
        <Text>{filter}</Text>
        <Text color={borderColor} dimColor>│</Text>
      </Box>
      {showScrollUp && (
        <Box paddingLeft={1}><Text dimColor>  ↑ {scroll} more</Text></Box>
      )}
      {visibleItems.map((item, vi) => {
        const i = vi + scroll
        const isCursor = i === cursor
        return (
          <Box key={`picker-${i}`}>
            {renderItem(item, i, isCursor)}
          </Box>
        )
      })}
      {showScrollDown && (
        <Box paddingLeft={1}><Text dimColor>  ↓ {filteredItems.length - scroll - maxVisible} more</Text></Box>
      )}
      <Text dimColor>{hint ?? `↑↓: nav  Enter: select  Esc: ${filter ? "clear" : "close"}  type to filter`}</Text>
    </Box>
  )
}
