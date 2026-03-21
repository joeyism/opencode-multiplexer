import React from "react"
import { Box, Text } from "ink"
import TextInput from "ink-text-input"
import { execSync, spawn as spawnProcess } from "child_process"
import { existsSync } from "fs"
import { createOpencodeClient } from "@opencode-ai/sdk"
import { useStore } from "../store.js"
import { useSpawnKeys } from "../hooks/use-keybindings.js"
import { refreshNow } from "../poller.js"
import {
  findNextPort,
  waitForServer,
  loadSpawnedInstances,
  saveSpawnedInstances,
  createWorktree,
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

    // Search for .git directories and strip the /.git suffix to get repo roots
    // Use # as sed delimiter to avoid escaping issues with /
    const cmd = `find ${searchPaths} -maxdepth 4 -name .git -type d 2>/dev/null | sed 's#/.git$##' | sort -u | fzf --prompt="Select repo: " --height=40%`
    const result = execSync(cmd, {
      stdio: ["inherit", "pipe", "inherit"],
      encoding: "utf-8",
    })
    return result.trim() || undefined
  } catch {
    return undefined
  }
}

function isGitRepo(dir: string): boolean {
  try {
    execSync("git rev-parse --is-inside-work-tree", {
      cwd: dir,
      stdio: ["pipe", "pipe", "pipe"],
    })
    return true
  } catch {
    return false
  }
}

export function Worktree() {
  const navigate = useStore((s) => s.navigate)
  const [step, setStep] = React.useState<"repo" | "branch" | "spawning" | "error">("repo")
  const [repoDir, setRepoDir] = React.useState("")
  const [branchName, setBranchName] = React.useState("")
  const [errorMsg, setErrorMsg] = React.useState("")
  const [manualPath, setManualPath] = React.useState("")
  const [hasFzf] = React.useState(() => isFzfAvailable())

  useSpawnKeys({
    onCancel: () => navigate("dashboard"),
  })

  const pickRepo = React.useCallback((dir: string) => {
    const expanded = expandHome(dir.trim())
    if (!expanded || !existsSync(expanded)) {
      setErrorMsg(`Path does not exist: ${expanded}`)
      setStep("error")
      return
    }
    if (!isGitRepo(expanded)) {
      setErrorMsg(`Not a git repository: ${expanded}`)
      setStep("error")
      return
    }
    setRepoDir(expanded)
    setStep("branch")
  }, [])

  const openFzf = React.useCallback(() => {
    const selected = runFzf()
    if (selected) {
      pickRepo(selected)
    } else {
      navigate("dashboard")
    }
  }, [pickRepo, navigate])

  React.useEffect(() => {
    if (hasFzf && step === "repo") {
      setTimeout(() => openFzf(), 50)
    }
  }, []) // eslint-disable-line

  const doSpawn = React.useCallback(async (cwd: string) => {
    setStep("spawning")
    setErrorMsg("")

    try {
      const port = await findNextPort()
      const proc = spawnProcess("opencode", ["serve", "--port", String(port)], {
        cwd,
        detached: true,
        stdio: "ignore",
      })
      proc.unref()

      await waitForServer(port)

      const client = createOpencodeClient({ baseUrl: `http://localhost:${port}` })
      const sessionResult = await client.session.create()
      const sessionId = (sessionResult as any).data?.id ?? null

      const instances = loadSpawnedInstances()
      instances.push({ port, pid: proc.pid!, cwd, sessionId })
      saveSpawnedInstances(instances)

      refreshNow()

      if (sessionId) {
        const projectId = useStore.getState().instances.find(
          (i) => i.sessionId === sessionId
        )?.projectId ?? null
        if (projectId) {
          navigate("conversation", projectId, sessionId)
        } else {
          navigate("dashboard")
        }
      } else {
        navigate("dashboard")
      }
    } catch (e) {
      setErrorMsg(String(e))
      setStep("error")
    }
  }, [navigate])

  const handleBranchSubmit = React.useCallback((name: string) => {
    if (!name.trim()) {
      void doSpawn(repoDir)
      return
    }
    try {
      const worktreeDir = createWorktree(repoDir, name.trim())
      void doSpawn(worktreeDir)
    } catch (e) {
      setErrorMsg(`Failed to create worktree: ${String(e)}`)
      setStep("error")
    }
  }, [repoDir, doSpawn])

  if (step === "spawning") {
    return (
      <Box flexDirection="column" padding={1}>
        <Text bold>ocmux — creating worktree session...</Text>
        <Text dimColor>Starting background server, please wait...</Text>
      </Box>
    )
  }

  if (step === "branch") {
    return (
      <Box flexDirection="column" padding={1}>
        <Text bold>ocmux — new worktree session</Text>
        <Text dimColor>{"─".repeat(60)}</Text>
        <Box marginTop={1}>
          <Text>Repo: </Text>
          <Text bold color="cyan">{repoDir}</Text>
        </Box>
        <Box marginTop={1}>
          <Text dimColor>Branch name (empty = session in root directory):</Text>
        </Box>
        <Box marginTop={1}>
          <Text dimColor>branch: </Text>
          <TextInput
            value={branchName}
            onChange={setBranchName}
            onSubmit={handleBranchSubmit}
            placeholder="fix-auth-bug (or Enter for root)"
          />
        </Box>
        <Box marginTop={1}>
          <Text dimColor>Esc: back to dashboard</Text>
        </Box>
      </Box>
    )
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold>ocmux — new worktree session</Text>
      <Text dimColor>{"─".repeat(60)}</Text>

      {hasFzf ? (
        <Box flexDirection="column" marginTop={1}>
          <Text>Press Enter to open repo picker (fzf)</Text>
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
              pickRepo(manualPath)
            } else if (hasFzf) {
              openFzf()
            }
          }}
          placeholder={hasFzf ? "Enter to open picker, or type path..." : "Enter path to repo..."}
        />
      </Box>

      {step === "error" && (
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
