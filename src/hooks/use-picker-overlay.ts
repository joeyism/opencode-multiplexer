import React from "react"

export interface PickerOverlayOptions<T> {
  /** All available items (unfiltered) */
  items: T[]
  /** Filter function — return true to include item */
  filterFn: (item: T, query: string) => boolean
  /** Called when user selects an item. Receives the item and its index in the ORIGINAL array. */
  onSelect: (item: T, originalIndex: number) => void
  /** Called when user closes the picker (Esc with empty filter) */
  onClose: () => void
  /** Max visible rows in the list */
  maxVisible: number
  /** Optional: extra key handler for picker-specific keys (e.g., 'a' for open-all in file picker).
   *  Return true if handled, false to fall through to default behavior. */
  onExtraKey?: (input: string, key: any) => boolean
}

export interface PickerOverlayState<T> {
  cursor: number
  scroll: number
  filter: string
  filteredItems: T[]
  /** Whether any items exist (original, not filtered) */
  hasItems: boolean
}

export function usePickerOverlay<T>(
  open: boolean,
  options: PickerOverlayOptions<T>,
) {
  const [cursor, setCursor] = React.useState(0)
  const [scroll, setScroll] = React.useState(0)
  const [filter, setFilter] = React.useState("")

  const filteredItems = React.useMemo(() => {
    if (!filter.trim()) return options.items
    return options.items.filter((item) => options.filterFn(item, filter))
  }, [options.items, filter, options.filterFn])

  // Clamp cursor when filtered list shrinks
  React.useEffect(() => {
    setCursor((c) => Math.max(0, Math.min(c, Math.max(0, filteredItems.length - 1))))
    setScroll((s) => Math.min(s, Math.max(0, filteredItems.length - options.maxVisible)))
  }, [filteredItems.length, options.maxVisible])

  // Reset state when picker opens
  React.useEffect(() => {
    if (open) {
      setFilter("")
      setCursor(0)
      setScroll(0)
    }
  }, [open])

  /** Call this from your useInput handler when the picker is open. Returns true = handled. */
  const handleInput = React.useCallback(
    (input: string, key: any): boolean => {
      if (!open) return false

      // Extra key handler first (e.g., 'a' for file picker)
      if (options.onExtraKey?.(input, key)) return true

      // Navigation: arrow keys + Ctrl-N/P only
      // ALL printable chars (including j/k) go to filter
      if (key.downArrow || (key.ctrl && input === "n")) {
        const next = Math.min(cursor + 1, Math.max(0, filteredItems.length - 1))
        setCursor(next)
        setScroll((s) => next >= s + options.maxVisible ? next - options.maxVisible + 1 : s)
        return true
      }
      if (key.upArrow || (key.ctrl && input === "p")) {
        const next = Math.max(cursor - 1, 0)
        setCursor(next)
        setScroll((s) => (next < s ? next : s))
        return true
      }
      if (key.return) {
        const item = filteredItems[cursor]
        if (item) {
          const originalIdx = options.items.indexOf(item)
          options.onSelect(item, originalIdx)
        }
        return true
      }
      if (key.escape) {
        if (filter) {
          setFilter("")
          setCursor(0)
          setScroll(0)
        } else {
          options.onClose()
        }
        return true
      }
      if (key.backspace || key.delete) {
        setFilter((f) => f.slice(0, -1))
        setCursor(0)
        setScroll(0)
        return true
      }
      // Printable chars → filter (includes j/k — no navigation via j/k)
      if (input && !key.ctrl && !key.meta) {
        setFilter((f) => f + input)
        setCursor(0)
        setScroll(0)
        return true
      }

      return false
    },
    [open, cursor, filter, filteredItems, options],
  )

  const state: PickerOverlayState<T> = {
    cursor,
    scroll,
    filter,
    filteredItems,
    hasItems: options.items.length > 0,
  }

  return { state, handleInput, setCursor, setScroll }
}
