import { useInput } from "ink"
import React from "react"

export interface VimNavigationActions {
  onUp?: () => void
  onDown?: () => void
  onHalfPageUp?: () => void
  onHalfPageDown?: () => void
  onTop?: () => void
  onBottom?: () => void
  onOpen?: () => void
  onBack?: () => void
}

/**
 * Adds vim-style navigation keys on top of existing keybindings.
 * Call this alongside useDashboardKeys/useConversationKeys.
 */
export function useVimNavigation(actions: VimNavigationActions) {
  const [pendingG, setPendingG] = React.useState(false)

  useInput((input, key) => {
    if (input === "g") {
      if (pendingG) {
        setPendingG(false)
        actions.onTop?.()
      } else {
        setPendingG(true)
      }
      return
    }

    if (pendingG) setPendingG(false)

    if (input === "j") actions.onDown?.()
    else if (input === "k") actions.onUp?.()
    else if (input === "l") actions.onOpen?.()
    else if (input === "h") actions.onBack?.()
    else if (input === "G") actions.onBottom?.()
    else if (key.ctrl && input === "d") actions.onHalfPageDown?.()
    else if (key.ctrl && input === "u") actions.onHalfPageUp?.()
  })
}
