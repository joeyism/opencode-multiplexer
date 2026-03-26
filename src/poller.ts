import { basename } from "path"
import { execSync } from "child_process"
import {
  getProjects,
  getSessionById,
  getSessionStatus,
  getLastMessagePreview,
  getSessionModel,
  getChildSessions,
  countChildSessions,
  hasChildSessions,
  getMostRecentSessionForProject,
  isTopLevelSession,
} from "./db/reader.js"

/**
 * Derive a display-friendly repo name from a directory path.
 * For ocmux-created worktrees at `<repo>/.worktrees/<branch>`, returns
 * the repo directory name instead of the branch directory name.
 */
export function deriveRepoName(dir: string): string {
  const idx = dir.indexOf("/.worktrees/")
  if (idx !== -1) return basename(dir.slice(0, idx))
  return basename(dir)
}

export function shortenModel(model: string): string {
  let s = model
  // Strip org prefix e.g. "deepseek-ai/deepseek-v3.2" → "deepseek-v3.2"
  if (s.includes("/")) s = s.split("/").pop()!
  // Strip "claude-" prefix
  s = s.replace(/^claude-/, "")
  // Strip "antigravity-" prefix
  s = s.replace(/^antigravity-/, "")
  // Strip "codex-" from gpt models
  s = s.replace(/codex-/, "")
  // Strip "-preview" suffix
  s = s.replace(/-preview$/, "")
  return s
}
import { useStore, type OcmInstance, type OcmSession, type SessionStatus } from "./store.js"
import { loadSpawnedInstances } from "./registry/instances.js"

interface RunningProcess {
  cwd: string
  sessionId: string | null  // from -s flag, instances.json, or null
  port?: number              // only for opencode serve processes
}

/**
 * Get CWD for a PID. Cross-platform: macOS uses lsof, Linux uses /proc.
 */
function getCwdForPid(pid: number): string {
  try {
    if (process.platform === "linux") {
      return execSync(`readlink /proc/${pid}/cwd 2>/dev/null`, {
        encoding: "utf-8", timeout: 1000,
      }).trim()
    } else {
      const lsofOutput = execSync(`lsof -p ${pid} 2>/dev/null`, {
        encoding: "utf-8", timeout: 2000,
      })
      const cwdLine = lsofOutput.split("\n").find((l) => l.includes(" cwd "))
      return cwdLine?.trim().split(/\s+/).slice(8).join(" ") ?? ""
    }
  } catch {
    return ""
  }
}

/**
 * Find all currently running opencode processes (TUI and serve) with cwds and session IDs.
 * TUI pattern:   opencode [-s {sessionId}]
 * Serve pattern: opencode serve --port {port}
 */
function getRunningOpencodeProcesses(): RunningProcess[] {
  try {
    const psOutput = execSync("ps -eo pid,args 2>/dev/null", {
      encoding: "utf-8",
      timeout: 3000,
    })

    // Load spawned instances to resolve sessionIds for serve processes
    const spawnedInstances = loadSpawnedInstances()
    const spawnedByPort = new Map(spawnedInstances.map((i) => [i.port, i]))

    const results: RunningProcess[] = []
    for (const line of psOutput.split("\n")) {
      const trimmed = line.trim()

      // Match TUI: "opencode" or "opencode -s {sessionId}"
      // Linux: "node /path/to/opencode" wrapper is 1:1 with real sessions. macOS: bare "opencode".
      // Never match ".opencode" — it's always a child process or orphaned subagent.
      // NOTE: Does not match standalone binaries ("/usr/local/bin/opencode") — assumes npm/nvm/bun wrapper on Linux.
      const tuiMatch = trimmed.match(/^(\d+)\s+(?:(?:node|bun|deno)\s+\S*\/opencode|opencode)(?:\s+-s\s+(\S+))?$/)
      if (tuiMatch) {
        const pid = parseInt(tuiMatch[1]!, 10)
        const sessionId = tuiMatch[2] ?? null
        const cwd = getCwdForPid(pid)
        if (cwd) results.push({ cwd, sessionId })
        continue
      }

      // Match serve: "opencode serve --port {port} ..."
      // Linux: "node /path/to/opencode" wrapper. macOS: bare "opencode".
      const serveMatch = trimmed.match(/^(\d+)\s+(?:(?:node|bun|deno)\s+\S*\/opencode|opencode)\s+serve\s+.*--port\s+(\d+)/)
      if (serveMatch) {
        const pid = parseInt(serveMatch[1]!, 10)
        const port = parseInt(serveMatch[2]!, 10)
        const spawned = spawnedByPort.get(port)
        const cwd = spawned?.cwd ?? getCwdForPid(pid)
        if (cwd) results.push({ cwd, sessionId: spawned?.sessionId ?? null, port })
        continue
      }

    }

    return results
  } catch {
    return []
  }
}

const STATUS_PRIORITY: Record<SessionStatus, number> = {
  "needs-input": 0,
  error: 1,
  working: 2,
  idle: 3,
}

/**
 * Find the most specific project for a given cwd.
 * If /Users/joey/repos/project and /Users/joey both match,
 * prefer /Users/joey/repos/project (longer = more specific).
 */
function findBestProject(
  cwd: string,
  projects: Array<{ id: string; worktree: string }>,
): { id: string; worktree: string } | null {
  let best: { id: string; worktree: string } | null = null
  let bestLen = -1
  for (const p of projects) {
    const isMatch =
      cwd === p.worktree ||
      cwd.startsWith(p.worktree + "/") ||
      p.worktree.startsWith(cwd + "/")
    if (isMatch && p.worktree.length > bestLen) {
      best = p
      bestLen = p.worktree.length
    }
  }
  return best
}

let _intervalId: ReturnType<typeof setInterval> | null = null
let _lastPollTime = 0

/**
 * Query a serve process's /session endpoint to discover all active sessions it hosts.
 * A single serve process can serve multiple sessions (e.g. via `opencode attach`).
 * Filters to only top-level sessions that were recently active (updated within the
 * last 24 hours) to avoid flooding the dashboard with old/stale sessions.
 */
async function getActiveServeSessionIds(port: number): Promise<string[]> {
  try {
    const res = await fetch(`http://localhost:${port}/session`, {
      signal: AbortSignal.timeout(1000),
    })
    if (!res.ok) return []
    const sessions = (await res.json()) as Array<{
      id: string
      time?: { updated?: number }
    }>
    const cutoff = Date.now() - 24 * 60 * 60 * 1000
    return sessions
      .filter((s) => {
        const updated = s.time?.updated ?? 0
        return updated > cutoff
      })
      .map((s) => s.id)
  } catch {
    return []
  }
}

async function loadFromDb(): Promise<void> {
  try {
    const dbProjects = getProjects()
    const runningProcesses = getRunningOpencodeProcesses()

    // Build ONE OcmInstance per running process.
    // Processes with -s get their explicit session.
    // Processes without -s get assigned the Nth most-recent session for their project
    // (where N is how many other flag-less processes in the same project came before).
    // This way two flag-less processes in the same dir show different sessions.
    const ocmInstances: OcmInstance[] = []
    const seenSessionIds = new Set<string>()
    // Track how many flag-less processes we've assigned per project
    const flaglessCountByProject = new Map<string, number>()
    // Track serve processes so we can query them for additional sessions
    const serveProcesses: Array<{ port: number; cwd: string }> = []

    for (const proc of runningProcesses) {
      const project = findBestProject(proc.cwd, dbProjects)
      if (!project) continue

      if (proc.port) {
        serveProcesses.push({ port: proc.port, cwd: proc.cwd })
      }

      // Resolve the session ID
      let sessionId = proc.sessionId
      if (!sessionId) {
        // Assign the Nth most-recent session to avoid all flag-less processes
        // in the same directory collapsing to the same session
        const offset = flaglessCountByProject.get(project.id) ?? 0
        flaglessCountByProject.set(project.id, offset + 1)
        const recent = getMostRecentSessionForProject(project.id, offset)
        sessionId = recent?.id ?? null
      }
      if (!sessionId) continue

      // Still deduplicate if two processes explicitly target the same session
      if (seenSessionIds.has(sessionId)) continue
      seenSessionIds.add(sessionId)

      const session = getSessionById(sessionId)
      if (!session) continue

      const status = getSessionStatus(sessionId)
      const preview = getLastMessagePreview(sessionId)
      const rawModel = getSessionModel(sessionId)

      // For serve instances, use the actual process cwd as worktree — it's the
      // authoritative source and must match what's stored in instances.json.
      // For TUI instances, use the SQLite project worktree (more stable).
      const instanceWorktree = proc.port ? proc.cwd : project.worktree

      ocmInstances.push({
        id: `${project.id}-${sessionId}`,
        sessionId,
        sessionTitle: session.title || sessionId.slice(0, 20),
        projectId: project.id,
        worktree: instanceWorktree,
        repoName: deriveRepoName(instanceWorktree),
        status,
        lastPreview: preview.text,
        lastPreviewRole: preview.role,
        hasChildren: hasChildSessions(sessionId),
        model: rawModel ? shortenModel(rawModel) : null,
        port: proc.port ?? null,
        timeUpdated: session.timeUpdated,
      })
    }

    // A single serve process can host multiple sessions (via `opencode attach`).
    // Query each serve's /session API to discover sessions not yet accounted for.
    for (const serve of serveProcesses) {
      const allIds = await getActiveServeSessionIds(serve.port)
      for (const sid of allIds) {
        if (seenSessionIds.has(sid)) continue
        if (!isTopLevelSession(sid)) continue
        seenSessionIds.add(sid)

        const session = getSessionById(sid)
        if (!session) continue

        const project = findBestProject(serve.cwd, dbProjects)
        if (!project) continue

        const status = getSessionStatus(sid)
        const preview = getLastMessagePreview(sid)
        const rawModel = getSessionModel(sid)

        ocmInstances.push({
          id: `${project.id}-${sid}`,
          sessionId: sid,
          sessionTitle: session.title || sid.slice(0, 20),
          projectId: project.id,
          worktree: serve.cwd,
          repoName: deriveRepoName(serve.cwd),
          status,
          lastPreview: preview.text,
          lastPreviewRole: preview.role,
          hasChildren: hasChildSessions(sid),
          model: rawModel ? shortenModel(rawModel) : null,
          port: serve.port,
          timeUpdated: session.timeUpdated,
        })
      }
    }

    // Sort: pinned first (by pin time), then by most recently updated
    const pinned = useStore.getState().pinnedSessions
    ocmInstances.sort((a, b) => {
      const aPin = pinned.get(a.sessionId)
      const bPin = pinned.get(b.sessionId)
      if (aPin !== undefined && bPin !== undefined) return aPin - bPin
      if (aPin !== undefined) return -1
      if (bPin !== undefined) return 1
      return b.timeUpdated - a.timeUpdated
    })

    useStore.getState().setInstances(ocmInstances)

    // Refresh children for expanded sessions
    const expandedSessions = useStore.getState().expandedSessions
    const childScrollOffsets = useStore.getState().childScrollOffsets
    for (const sessionId of expandedSessions) {
      try {
        const offset = childScrollOffsets.get(sessionId) ?? 0
        const children = getChildSessions(sessionId, 10, offset)
        const totalCount = countChildSessions(sessionId)
        const childOcmSessions: OcmSession[] = children.map((c) => {
          const status = getSessionStatus(c.id)
          const preview = getLastMessagePreview(c.id)
          return {
            id: c.id,
            projectId: c.projectId,
            title: c.title,
            directory: c.directory,
            status,
            lastMessagePreview: preview.text,
            lastMessageRole: preview.role,
            model: (() => { const m = getSessionModel(c.id); return m ? shortenModel(m) : null })(),
            timeUpdated: c.timeUpdated,
            hasChildren: hasChildSessions(c.id),
          }
        })
        useStore.getState().setChildSessions(sessionId, childOcmSessions, totalCount)
      } catch {
        // Skip on error
      }
    }

    _lastPollTime = Date.now()
  } catch {
    // DB may be locked briefly — skip this poll cycle
  }
}

export function startPoller(intervalMs = 2000): void {
  if (_intervalId) return
  loadFromDb()
  _intervalId = setInterval(() => { loadFromDb() }, intervalMs)
}

export function stopPoller(): void {
  if (_intervalId) {
    clearInterval(_intervalId)
    _intervalId = null
  }
}

export function refreshNow(): void {
  loadFromDb()
}

export function getLastPollTime(): number {
  return _lastPollTime
}
