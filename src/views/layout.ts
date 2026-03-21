/**
 * Pure layout computation for the conversation view.
 * Single source of truth — used by both the component and tests.
 */

/** Rows/columns consumed by the App-level border wrapper */
export const APP_BORDER_ROWS = 2   // top + bottom border
export const APP_BORDER_COLS = 2   // left + right border

export function computeConversationLayout(
  termHeight: number,
  termWidth: number,
  options?: { killConfirm?: boolean },
) {
  const innerHeight = termHeight - APP_BORDER_ROWS
  const effectiveWidth = termWidth - APP_BORDER_COLS
  const HEADER_ROWS = 2   // title line + divider
  const FOOTER_ROWS = 3   // bordered footer: top border + content + bottom border
  const KILL_CONFIRM_ROWS = options?.killConfirm ? 3 : 0
  const bodyHeight = Math.max(5, innerHeight - HEADER_ROWS - FOOTER_ROWS - KILL_CONFIRM_ROWS)

  return {
    innerHeight,
    effectiveWidth,
    bodyHeight,
    HEADER_ROWS,
    FOOTER_ROWS,
  }
}
