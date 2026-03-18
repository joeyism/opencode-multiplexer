import React from "react"
import { Box, Text } from "ink"
import TextInput from "ink-text-input"
import { execSync, spawn } from "child_process"
import { existsSync } from "fs"
import { createOpencodeClient } from "@opencode-ai/sdk"
import { useStore } from "../store.js"
import { useSpawnKeys } from "../hooks/use-keybindings.js"
import { refreshNow } from "../poller.js"
import { yieldToOpencode } from "../hooks/use-attach.js"
import {
  findNextPort,
  waitForServer,
  loadSpawnedInstances,
  saveSpawnedInstances,
} from "../registry/instances.js"

function expandHome(p: string): string {
  if (p.startsWith("~")) {
    return p.replace(/^~/, process.env.HOME ?? process.env.USERPROFILE ?? "")
  }
  return p
}

function isFzfAvailable(): boolean {
  try {
    execSync("which fzf", { stdio: "pipe" })
    return true
  } catch {
    return false
  }
}

function runFzf(): string | undefined {
  try {
    const searchPaths = [
      expandHome("~/Programming"),
      expandHome("~/repos"),
      expandHome("~/projects"),
      expandHome("~/code"),
      expandHome("~"),
    ]
      .filter((p) => p && existsSync(p))
      .join(" ")

    const cmd = `find ${searchPaths} -maxdepth 3 -type d 2>/dev/null | fzf --prompt="Select project folder: " --height=40%`
    const result = execSync(cmd, {
      stdio: ["inherit", "pipe", "inherit"],
      encoding: "utf-8",
    })
    return result.trim() || undefined
  } catch {
    return undefined
  }
}

export function Spawn() {
  const navigate = useStore((s) => s.navigate)
  const [manualPath, setManualPath] = React.useState("")
  const [status, setStatus] = React.useState<"idle" | "spawning" | "error">("idle")
  const [errorMsg, setErrorMsg] = React.useState("")
  const [hasFzf] = React.useState(() => isFzfAvailable())

  useSpawnKeys({
    onCancel: () => navigate("dashboard"),
  })

  const doSpawn = React.useCallback(
    async (cwd: string) => {
      const expanded = expandHome(cwd.trim())
      if (!expanded) return
      if (!existsSync(expanded)) {
        setErrorMsg(`Path does not exist: ${expanded}`)
        setStatus("error")
        return
      }

      setStatus("spawning")
      setErrorMsg("")

      try {
        // 1. Find next available port
        const port = await findNextPort()

        // 2. Spawn opencode serve --port {port} as a detached background process
        const proc = spawn("opencode", ["serve", "--port", String(port)], {
          cwd: expanded,
          detached: true,
          stdio: "ignore",
        })
        proc.unref() // don't block OCMux's event loop on this child

        // 3. Wait for the server to be ready
        await waitForServer(port)

        // 4. Create an initial session via SDK
        const client = createOpencodeClient({ baseUrl: `http://localhost:${port}` })
        const sessionResult = await client.session.create()
        const sessionId = (sessionResult as any).data?.id ?? null

        // 5. Persist the spawned instance info
        const instances = loadSpawnedInstances()
        instances.push({
          port,
          pid: proc.pid!,
          cwd: expanded,
          sessionId,
        })
        saveSpawnedInstances(instances)

        // 6. Refresh so the dashboard knows about the new instance
        refreshNow()

        // 7. Immediately attach to the new session
        if (sessionId) {
          yieldToOpencode(sessionId, expanded)
        }

        navigate("dashboard")
      } catch (e) {
        setErrorMsg(String(e))
        setStatus("error")
      }
    },
    [navigate],
  )

  const openFzf = React.useCallback(() => {
    const selected = runFzf()
    if (selected) {
      void doSpawn(selected)
    }
  }, [doSpawn])

  React.useEffect(() => {
    if (hasFzf) {
      setTimeout(() => openFzf(), 50)
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  if (status === "spawning") {
    return (
      <Box flexDirection="column" padding={1}>
        <Text bold>ocmux — spawning opencode server...</Text>
        <Text dimColor>Starting background server, please wait...</Text>
      </Box>
    )
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold>ocmux — open in opencode</Text>
      <Text dimColor>{"─".repeat(60)}</Text>

      {hasFzf ? (
        <Box flexDirection="column" marginTop={1}>
          <Text>Press Enter to open folder picker (fzf)</Text>
          <Text dimColor>or type a path manually below:</Text>
        </Box>
      ) : (
        <Box marginTop={1}>
          <Text dimColor>fzf not found. Enter path manually:</Text>
        </Box>
      )}

      <Box marginTop={1}>
        <Text dimColor>{">"} </Text>
        <TextInput
          value={manualPath}
          onChange={setManualPath}
          onSubmit={() => {
            if (manualPath.trim()) {
              void doSpawn(manualPath)
            } else if (hasFzf) {
              openFzf()
            }
          }}
          placeholder={
            hasFzf
              ? "Enter to open picker, or type path..."
              : "Enter path to project..."
          }
        />
      </Box>

      {status === "error" && (
        <Box marginTop={1}>
          <Text color="red">{errorMsg}</Text>
          <Text dimColor> (press Esc to go back)</Text>
        </Box>
      )}

      <Box marginTop={1}>
        <Text dimColor>Esc: back to dashboard</Text>
      </Box>
    </Box>
  )
}
