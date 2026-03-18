import React from "react"
import { render, Text, Box, useInput } from "ink"
import { execSync } from "child_process"

function App() {
  const [status, setStatus] = React.useState(
    "Ready. Press 'a' to attach to 'spike-test' tmux session, 'q' to quit."
  )

  useInput((input) => {
    if (input === "a") {
      setStatus("Attaching to tmux...")
      setTimeout(() => {
        try {
          execSync("tmux attach-session -t spike-test", { stdio: "inherit" })
          setStatus("Returned from tmux successfully. Ink recovered! Press 'q' to quit.")
        } catch (e: any) {
          setStatus(`Error or detached: ${e?.message ?? e}. Press 'q' to quit.`)
        }
      }, 100)
    }
    if (input === "q") process.exit(0)
  })

  return (
    <Box flexDirection="column">
      <Text>{status}</Text>
    </Box>
  )
}

render(<App />)
