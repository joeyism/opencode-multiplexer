import { Database } from "bun:sqlite"
import { homedir } from "os"
import { join } from "path"
import { existsSync } from "fs"
import type { SessionStatus } from "../store.js"

const DB_PATH = join(homedir(), ".local", "share", "opencode", "opencode.db")

let _db: Database | null = null

function getDb(): Database {
  if (_db) return _db
  if (!existsSync(DB_PATH)) {
    throw new Error(`opencode database not found at ${DB_PATH}`)
  }
  _db = new Database(DB_PATH, { readonly: true })
  return _db
}

// ─── Project types ────────────────────────────────────────────────────────────

export interface DbProject {
  id: string
  worktree: string
  name: string | null
  timeCreated: number
  timeUpdated: number
}

// ─── Session types ────────────────────────────────────────────────────────────

export interface DbSession {
  id: string
  projectId: string
  title: string
  directory: string
  permission: string | null
  timeCreated: number
  timeUpdated: number
}

// ─── Message / Part types ─────────────────────────────────────────────────────

export interface DbMessagePart {
  id: string
  type: string
  text?: string
  tool?: string
  toolStatus?: string
  callId?: string
}

export interface DbMessage {
  id: string
  sessionId: string
  role: "user" | "assistant"
  timeCreated: number
  timeCompleted: number | null
  modelId: string | null
  providerId: string | null
  parts: DbMessagePart[]
}

// ─── Queries ──────────────────────────────────────────────────────────────────

export function getProjects(): DbProject[] {
  const db = getDb()
  const rows = db
    .query<
      { id: string; worktree: string; name: string | null; time_created: number; time_updated: number },
      []
    >(
      `SELECT id, worktree, name, time_created, time_updated
       FROM project
       ORDER BY time_updated DESC`,
    )
    .all()

  return rows.map((r) => ({
    id: r.id,
    worktree: r.worktree,
    name: r.name,
    timeCreated: r.time_created,
    timeUpdated: r.time_updated,
  }))
}

export function getSessionsForProject(projectId: string): DbSession[] {
  const db = getDb()
  const rows = db
    .query<
      {
        id: string
        project_id: string
        title: string
        directory: string
        permission: string | null
        time_created: number
        time_updated: number
      },
      [string]
    >(
      `SELECT id, project_id, title, directory, permission, time_created, time_updated
       FROM session
       WHERE project_id = ?
         AND time_archived IS NULL
       ORDER BY time_updated DESC`,
    )
    .all(projectId)

  return rows.map((r) => ({
    id: r.id,
    projectId: r.project_id,
    title: r.title,
    directory: r.directory,
    permission: r.permission,
    timeCreated: r.time_created,
    timeUpdated: r.time_updated,
  }))
}

export function countSessionsForProject(projectId: string): number {
  const db = getDb()
  const row = db
    .query<{ cnt: number }, [string]>(
      `SELECT COUNT(*) as cnt FROM session WHERE project_id = ? AND time_archived IS NULL`,
    )
    .get(projectId)
  return row?.cnt ?? 0
}

export function getMostRecentSessionForProject(projectId: string, offset = 0): DbSession | null {
  const db = getDb()
  const row = db
    .query<
      {
        id: string
        project_id: string
        title: string
        directory: string
        permission: string | null
        time_created: number
        time_updated: number
      },
      [string, number]
    >(
      `SELECT id, project_id, title, directory, permission, time_created, time_updated
       FROM session
       WHERE project_id = ? AND time_archived IS NULL AND parent_id IS NULL
       ORDER BY time_updated DESC
       LIMIT 1 OFFSET ?`,
    )
     .get(projectId, offset)
  if (!row) return null
  return {
    id: row.id,
    projectId: row.project_id,
    title: row.title,
    directory: row.directory,
    permission: row.permission,
    timeCreated: row.time_created,
    timeUpdated: row.time_updated,
  }
}

export function getSessionStatus(sessionId: string): SessionStatus {
  const db = getDb()

  // Check for a running question tool in the latest message → needs-input
  // (agent is showing a question/multiselect prompt to the user)
  const questionRow = db
    .query<{ cnt: number }, [string, string]>(
      `SELECT COUNT(*) as cnt
       FROM part p
       WHERE p.session_id = ?
         AND json_extract(p.data, '$.type') = 'tool'
         AND json_extract(p.data, '$.tool') = 'question'
         AND json_extract(p.data, '$.state.status') = 'running'
         AND p.message_id = (
           SELECT id FROM message WHERE session_id = ? ORDER BY time_created DESC LIMIT 1
         )`,
    )
    .get(sessionId, sessionId)
  if ((questionRow?.cnt ?? 0) > 0) return "needs-input"

  // Check for error tool parts in latest message → error
  const errorRow = db
    .query<{ cnt: number }, [string, string]>(
      `SELECT COUNT(*) as cnt
       FROM part p
       WHERE p.session_id = ?
         AND json_extract(p.data, '$.type') = 'tool'
         AND json_extract(p.data, '$.state.status') = 'error'
         AND p.message_id = (
           SELECT id FROM message WHERE session_id = ? ORDER BY time_created DESC LIMIT 1
         )`,
    )
    .get(sessionId, sessionId)
  if ((errorRow?.cnt ?? 0) > 0) return "error"

  // Check latest message
  const lastMsg = db
    .query<{ role: string; completed: number | null }, [string]>(
      `SELECT json_extract(data, '$.role') as role,
              json_extract(data, '$.time.completed') as completed
       FROM message
       WHERE session_id = ?
       ORDER BY time_created DESC
       LIMIT 1`,
    )
    .get(sessionId)

  if (!lastMsg) return "idle"
  // Agent is generating (incomplete assistant message)
  if (lastMsg.role === "assistant" && lastMsg.completed === null) return "working"
  // User sent a message, agent hasn't responded yet
  if (lastMsg.role === "user") return "working"
  // Assistant finished — session is idle (not "needs-input")
  // "needs-input" only comes from an active question tool (checked above)
  return "idle"
}

export function getLastMessagePreview(sessionId: string): { text: string; role: "user" | "assistant" } {
  const db = getDb()

  // Get the last meaningful text part — skip tool output XML (starts with '<')
  // and internal markers. Prefer assistant prose or user messages.
  const row = db
    .query<
      { text: string; role: string },
      [string]
    >(
      `SELECT json_extract(p.data, '$.text') as text,
              json_extract(m.data, '$.role') as role
       FROM part p
       JOIN message m ON p.message_id = m.id
       WHERE p.session_id = ?
         AND json_extract(p.data, '$.type') = 'text'
         AND json_extract(p.data, '$.text') IS NOT NULL
         AND json_extract(p.data, '$.text') != ''
         AND json_extract(p.data, '$.text') NOT LIKE '<%'
       ORDER BY m.time_created DESC, p.time_created DESC
       LIMIT 1`,
    )
    .get(sessionId)

  // Strip markdown syntax and collapse to a single line for dashboard preview
  const raw = row?.text ?? ""
  const clean = raw
    .replace(/\*\*(.+?)\*\*/g, "$1")                    // **bold** → bold
    .replace(/(?<!\*)\*([^*\n]+)\*(?!\*)/g, "$1")       // *italic* → italic
    .replace(/`([^`]+)`/g, "$1")                         // `code` → code
    .replace(/^#{1,6}\s+/gm, "")                         // ## heading → heading
    .replace(/^[-*]\s+/gm, "• ")                         // - list → • list
    .replace(/\n+/g, " ")                                // collapse newlines to spaces
    .replace(/\s{2,}/g, " ")                             // collapse multiple spaces
    .trim()

  return {
    text: clean,
    role: (row?.role ?? "user") as "user" | "assistant",
  }
}

export function getMessages(sessionId: string): DbMessage[] {
  const db = getDb()

  // Get all messages for the session
  const messages = db
    .query<
      {
        id: string
        role: string
        time_created: number
        completed: number | null
        model_id: string | null
        provider_id: string | null
      },
      [string]
    >(
      `SELECT id,
              json_extract(data, '$.role') as role,
              time_created,
              json_extract(data, '$.time.completed') as completed,
              json_extract(data, '$.modelID') as model_id,
              json_extract(data, '$.providerID') as provider_id
       FROM message
       WHERE session_id = ?
       ORDER BY time_created ASC`,
    )
    .all(sessionId)

  if (messages.length === 0) return []

  // Get all parts for these messages in one query
  const parts = db
    .query<
      {
        id: string
        message_id: string
        part_type: string
        text: string | null
        tool: string | null
        tool_status: string | null
        call_id: string | null
      },
      [string]
    >(
      `SELECT p.id,
              p.message_id,
              json_extract(p.data, '$.type') as part_type,
              json_extract(p.data, '$.text') as text,
              json_extract(p.data, '$.tool') as tool,
              json_extract(p.data, '$.state.status') as tool_status,
              json_extract(p.data, '$.callID') as call_id
       FROM part p
       WHERE p.session_id = ?
       ORDER BY p.time_created ASC`,
    )
    .all(sessionId)

  // Group parts by message_id
  const partsByMessage = new Map<string, DbMessagePart[]>()
  for (const p of parts) {
    const list = partsByMessage.get(p.message_id) ?? []
    list.push({
      id: p.id,
      type: p.part_type,
      text: p.text ?? undefined,
      tool: p.tool ?? undefined,
      toolStatus: p.tool_status ?? undefined,
      callId: p.call_id ?? undefined,
    })
    partsByMessage.set(p.message_id, list)
  }

  return messages.map((m) => ({
    id: m.id,
    sessionId,
    role: m.role as "user" | "assistant",
    timeCreated: m.time_created,
    timeCompleted: m.completed ?? null,
    modelId: m.model_id,
    providerId: m.provider_id,
    parts: partsByMessage.get(m.id) ?? [],
  }))
}

export function getSessionById(sessionId: string): DbSession | null {
  const db = getDb()
  const row = db
    .query<
      {
        id: string
        project_id: string
        title: string
        directory: string
        permission: string | null
        time_created: number
        time_updated: number
      },
      [string]
    >(
      `SELECT id, project_id, title, directory, permission, time_created, time_updated
       FROM session WHERE id = ?`,
    )
    .get(sessionId)

  if (!row) return null
  return {
    id: row.id,
    projectId: row.project_id,
    title: row.title,
    directory: row.directory,
    permission: row.permission,
    timeCreated: row.time_created,
    timeUpdated: row.time_updated,
  }
}

// ─── Child session queries (for subagent tree) ───────────────────────────────

export function getChildSessions(parentSessionId: string, limit = 10, offset = 0): DbSession[] {
  const db = getDb()
  const rows = db
    .query<
      {
        id: string
        project_id: string
        title: string
        directory: string
        permission: string | null
        time_created: number
        time_updated: number
      },
      [string, number, number]
    >(
      `SELECT id, project_id, title, directory, permission, time_created, time_updated
       FROM session
       WHERE parent_id = ?
         AND time_archived IS NULL
       ORDER BY time_created DESC
       LIMIT ? OFFSET ?`,
    )
    .all(parentSessionId, limit, offset)

  return rows.map((r) => ({
    id: r.id,
    projectId: r.project_id,
    title: r.title,
    directory: r.directory,
    permission: r.permission,
    timeCreated: r.time_created,
    timeUpdated: r.time_updated,
  }))
}

export function countChildSessions(parentSessionId: string): number {
  const db = getDb()
  const row = db
    .query<{ cnt: number }, [string]>(
      `SELECT COUNT(*) as cnt FROM session WHERE parent_id = ? AND time_archived IS NULL`,
    )
    .get(parentSessionId)
  return row?.cnt ?? 0
}

export function hasChildSessions(sessionId: string): boolean {
  const db = getDb()
  const row = db
    .query<{ cnt: number }, [string]>(
      `SELECT COUNT(*) as cnt FROM session WHERE parent_id = ? AND time_archived IS NULL LIMIT 1`,
    )
    .get(sessionId)
  return (row?.cnt ?? 0) > 0
}

export function getSessionModel(sessionId: string): string | null {
  const db = getDb()
  const row = db
    .query<{ model_id: string }, [string]>(
      `SELECT json_extract(data, '$.modelID') as model_id
       FROM message
       WHERE session_id = ?
         AND json_extract(data, '$.role') = 'assistant'
         AND json_extract(data, '$.modelID') IS NOT NULL
       ORDER BY time_created DESC
       LIMIT 1`,
    )
    .get(sessionId)
  return row?.model_id ?? null
}

export function getSessionAgent(sessionId: string): string | null {
  const db = getDb()
  const row = db
    .query<{ agent: string }, [string]>(
      `SELECT json_extract(data, '$.agent') as agent
       FROM message
       WHERE session_id = ?
         AND json_extract(data, '$.role') = 'assistant'
         AND json_extract(data, '$.agent') IS NOT NULL
       ORDER BY time_created DESC
       LIMIT 1`,
    )
    .get(sessionId)
  return row?.agent ?? null
}

// Close DB on process exit to avoid WAL lock issues
process.on("exit", () => { try { _db?.close() } catch {} })
