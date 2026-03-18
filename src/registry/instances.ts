import { mkdirSync, readFileSync, writeFileSync } from "fs"
import { homedir } from "os"
import { join } from "path"
import { execSync } from "child_process"

const CONFIG_DIR = join(homedir(), ".config", "ocmux")
const INSTANCES_FILE = join(CONFIG_DIR, "instances.json")

export interface SpawnedInstance {
  port: number
  pid: number
  cwd: string
  sessionId: string | null
}

function ensureDir() {
  mkdirSync(CONFIG_DIR, { recursive: true })
}

export function loadSpawnedInstances(): SpawnedInstance[] {
  try {
    const raw = readFileSync(INSTANCES_FILE, "utf-8")
    return JSON.parse(raw) as SpawnedInstance[]
  } catch {
    return []
  }
}

export function saveSpawnedInstances(instances: SpawnedInstance[]): void {
  ensureDir()
  writeFileSync(INSTANCES_FILE, JSON.stringify(instances, null, 2))
}

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

async function isPortAlive(port: number): Promise<boolean> {
  try {
    const res = await fetch(`http://localhost:${port}/doc`, {
      signal: AbortSignal.timeout(1000),
    })
    return res.ok
  } catch {
    return false
  }
}

/**
 * Remove instances whose process is dead or port is unresponsive.
 * Called on startup to clean up stale entries.
 */
export async function cleanDeadInstances(): Promise<void> {
  const instances = loadSpawnedInstances()
  if (instances.length === 0) return

  const alive: SpawnedInstance[] = []
  for (const inst of instances) {
    if (isPidAlive(inst.pid) && (await isPortAlive(inst.port))) {
      alive.push(inst)
    }
  }

  if (alive.length !== instances.length) {
    saveSpawnedInstances(alive)
  }
}

/**
 * Find the next available port for a new opencode serve instance.
 */
export async function findNextPort(startPort = 4096, endPort = 4115): Promise<number> {
  const existing = new Set(loadSpawnedInstances().map((i) => i.port))
  for (let port = startPort; port <= endPort; port++) {
    if (existing.has(port)) continue
    // Check if anything is already listening
    const inUse = await isPortAlive(port)
    if (!inUse) return port
  }
  throw new Error(`No available ports in range ${startPort}-${endPort}`)
}

/**
 * Wait until opencode serve is ready to accept requests.
 */
export async function waitForServer(port: number, timeoutMs = 15000): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (await isPortAlive(port)) return
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`opencode server on port ${port} did not start within ${timeoutMs}ms`)
}

/**
 * Find the PID of an opencode process (TUI or serve) running in the given directory.
 * Used as a fallback when the instance isn't in instances.json.
 */
function findPidByWorktree(worktree: string): number | null {
  try {
    const psOutput = execSync("ps -eo pid,args 2>/dev/null", {
      encoding: "utf-8",
      timeout: 3000,
    })
    for (const line of psOutput.split("\n")) {
      const trimmed = line.trim()
      // Match both TUI and serve patterns
      // Linux: "node /path/to/opencode" wrapper. macOS: bare "opencode".
      const match = trimmed.match(/^(\d+)\s+(?:(?:node|bun|deno)\s+\S*\/opencode|opencode)(?:\s+serve|\s+-s\s+\S+)?/)
      if (!match) continue
      const pid = parseInt(match[1]!, 10)
      try {
        let cwd: string
        if (process.platform === "linux") {
          cwd = execSync(`readlink /proc/${pid}/cwd 2>/dev/null`, {
            encoding: "utf-8", timeout: 1000,
          }).trim()
        } else {
          const lsofOut = execSync(`lsof -p ${pid} 2>/dev/null`, {
            encoding: "utf-8", timeout: 2000,
          })
          const cwdLine = lsofOut.split("\n").find((l) => l.includes(" cwd "))
          cwd = cwdLine?.trim().split(/\s+/).slice(8).join(" ") ?? ""
        }
        // Match if: exact, cwd is under worktree, OR worktree is under cwd
        if (
          cwd === worktree ||
          cwd.startsWith(worktree + "/") ||
          worktree.startsWith(cwd + "/")
        ) return pid
      } catch {
        // process exited — skip
      }
    }
  } catch {
    // ps failed
  }
  return null
}

/**
 * Kill the opencode instance associated with the given worktree and optional sessionId.
 * Handles both spawned serve instances (from instances.json) and TUI instances.
 */
export function killInstance(worktree: string, sessionId: string | null): void {
  // 1. Check instances.json for a matching spawned serve process.
  //    Match by sessionId first (most reliable), then by cwd prefix.
  //    Note: instances.json cwd may differ from the project's SQLite worktree.
  const instances = loadSpawnedInstances()
  const idx = instances.findIndex((i) => {
    if (sessionId && i.sessionId === sessionId) return true
    return (
      i.cwd === worktree ||
      i.cwd.startsWith(worktree + "/") ||
      worktree.startsWith(i.cwd + "/")
    )
  })

  if (idx >= 0) {
    const inst = instances[idx]!
    try { process.kill(inst.pid, "SIGTERM") } catch { /* already dead */ }
    instances.splice(idx, 1)
    saveSpawnedInstances(instances)
    return
  }

  // 2. Fallback: find and kill TUI process by cwd/worktree
  const pid = findPidByWorktree(worktree)
  if (pid) {
    try { process.kill(pid, "SIGTERM") } catch { /* already dead */ }
  }
}
