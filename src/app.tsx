import React from "react"
import { useStore } from "./store.js"
import { Dashboard } from "./views/dashboard.js"
import { Conversation } from "./views/conversation.js"
import { Spawn } from "./views/spawn.js"
import { Worktree } from "./views/worktree.js"

export function App() {
  const view = useStore((s) => s.view)

  switch (view) {
    case "dashboard":
      return <Dashboard />
    case "conversation":
      return <Conversation />
    case "spawn":
      return <Spawn />
    case "worktree":
      return <Worktree />
  }
}
