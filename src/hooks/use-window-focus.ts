import React from "react"
import { useStore } from "../store.js"

const ENABLE_FOCUS_REPORTING = "\x1b[?1004h"
const DISABLE_FOCUS_REPORTING = "\x1b[?1004l"
const FOCUS_IN = "\x1b[I"
const FOCUS_OUT = "\x1b[O"

/**
 * Hook that enables terminal focus reporting and tracks window focus state.
 * Works with kitty, iTerm2, and tmux (requires `set -g focus-events on`).
 *
 * Must be called exactly once, in the root App component.
 */
export function useWindowFocus() {
  const setWindowFocused = useStore((s) => s.setWindowFocused)

  React.useEffect(() => {
    if (!process.stdin.isTTY) return

    // Enable focus reporting
    process.stdout.write(ENABLE_FOCUS_REPORTING)

    const onData = (data: Buffer) => {
      const str = data.toString()
      if (str.includes(FOCUS_IN)) {
        setWindowFocused(true)
      } else if (str.includes(FOCUS_OUT)) {
        setWindowFocused(false)
      }
    }

    process.stdin.on("data", onData)

    return () => {
      process.stdin.off("data", onData)
      process.stdout.write(DISABLE_FOCUS_REPORTING)
    }
  }, [setWindowFocused])
}
