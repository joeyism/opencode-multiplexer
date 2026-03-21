import React from "react"
import { Box } from "ink"
import { useStore } from "./store.js"
import { Dashboard } from "./views/dashboard.js"
import { Conversation } from "./views/conversation.js"
import { Spawn } from "./views/spawn.js"
import { Worktree } from "./views/worktree.js"
import { useWindowFocus } from "./hooks/use-window-focus.js"

export function App() {
  const view = useStore((s) => s.view)
  const windowFocused = useStore((s) => s.windowFocused)

  useWindowFocus()

  let content: React.ReactNode
  switch (view) {
    case "dashboard":
      content = <Dashboard />
      break
    case "conversation":
      content = <Conversation />
      break
    case "spawn":
      content = <Spawn />
      break
    case "worktree":
      content = <Worktree />
      break
    default:
      content = <Dashboard />
      break
  }

  return (
    <Box
      borderStyle="single"
      borderColor={windowFocused ? "cyan" : "gray"}
      flexDirection="column"
    >
      {content}
    </Box>
  )
}
