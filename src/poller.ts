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
} from "./db/reader.js"

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
      const tuiMatch = trimmed.match(/^(\d+)\s+(?:\S+\s+)*\S*\.?opencode(?:\s+-s\s+(\S+))?$/)
      if (tuiMatch) {
        const pid = parseInt(tuiMatch[1]!, 10)
        const sessionId = tuiMatch[2] ?? null
        const cwd = getCwdForPid(pid)
        if (cwd) results.push({ cwd, sessionId })
        continue
      }

      // Match serve: "opencode serve --port {port} ..."
      const serveMatch = trimmed.match(/^(\d+)\s+(?:\S+\s+)*\S*\.?opencode\s+serve\s+.*--port\s+(\d+)/)
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

function loadFromDb(): void {
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

    for (const proc of runningProcesses) {
      const project = findBestProject(proc.cwd, dbProjects)
      if (!project) continue

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
        repoName: basename(instanceWorktree),
        status,
        lastPreview: preview.text,
        lastPreviewRole: preview.role,
        hasChildren: hasChildSessions(sessionId),
        model: rawModel ? shortenModel(rawModel) : null,
        port: proc.port ?? null,
      })
    }

    // Sort: needs-input first, then working, then idle, then error
    ocmInstances.sort((a, b) => {
      const pa = STATUS_PRIORITY[a.status]
      const pb = STATUS_PRIORITY[b.status]
      if (pa !== pb) return pa - pb
      // Secondary sort: repo name alphabetically
      return a.repoName.localeCompare(b.repoName)
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
