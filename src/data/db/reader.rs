#![allow(clippy::type_complexity)]
use std::path::{Path, PathBuf};

use anyhow::Context;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::{
    app::sessions::SessionStatus,
    data::db::models::{
        DbConversationMessage, DbConversationPart, DbManagedSession, DbProject, DbSession,
        DbSessionSummary, DbUserMessage, SessionPreview,
    },
};

pub struct DbReader {
    conn: Connection,
}

impl DbReader {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| format!("failed to open sqlite db at {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self { conn })
    }

    /// Returns the busy_timeout in milliseconds. Used for tests and diagnostics.
    pub fn busy_timeout_ms(&self) -> anyhow::Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))?)
    }

    pub fn open_default() -> anyhow::Result<Self> {
        Self::open(&super::default_db_path()?)
    }

    pub fn get_all_sessions(&self) -> anyhow::Result<Vec<DbSessionSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.title, s.directory,
                    COALESCE(
                        (SELECT time_created FROM message WHERE session_id = s.id AND json_extract(data, '$.role') = 'user' ORDER BY time_created DESC LIMIT 1),
                        s.time_created
                    ) as last_interaction,
                    s.time_archived, p.worktree
             FROM session s
             JOIN project p ON p.id = s.project_id
             WHERE s.parent_id IS NULL
             ORDER BY last_interaction DESC
             LIMIT 500"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DbSessionSummary {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                directory: PathBuf::from(row.get::<_, Option<String>>(2)?.unwrap_or_default()),
                time_updated: row.get(3)?,
                archived: row.get::<_, Option<i64>>(4)?.is_some(),
                worktree: PathBuf::from(row.get::<_, String>(5)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn list_sessions_for_manager(&self) -> anyhow::Result<Vec<DbManagedSession>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.title, s.directory, p.worktree,
              COALESCE((
                SELECT COUNT(*) FROM message m
                WHERE m.session_id IN (
                  WITH RECURSIVE tree(id) AS (
                    SELECT s.id
                    UNION ALL
                    SELECT c.id FROM session c JOIN tree t ON c.parent_id = t.id
                  )
                  SELECT id FROM tree
                )
                AND json_extract(m.data, '$.role') = 'user'
              ), 0) AS user_msg_count,
              COALESCE((
                SELECT time_created FROM message
                WHERE session_id = s.id AND json_extract(data, '$.role') = 'user'
                ORDER BY time_created DESC LIMIT 1
              ), s.time_created) AS last_interaction
            FROM session s
            JOIN project p ON p.id = s.project_id
            WHERE s.parent_id IS NULL
            ORDER BY last_interaction DESC
            LIMIT 500"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DbManagedSession {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                directory: PathBuf::from(row.get::<_, Option<String>>(2)?.unwrap_or_default()),
                worktree: PathBuf::from(row.get::<_, String>(3)?),
                user_message_count: row.get(4)?,
                time_updated: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_projects(&self) -> anyhow::Result<Vec<DbProject>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, worktree FROM project ORDER BY time_updated DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(DbProject {
                id: row.get(0)?,
                worktree: PathBuf::from(row.get::<_, String>(1)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_most_recent_session(
        &self,
        project_id: &str,
        offset: usize,
    ) -> anyhow::Result<Option<DbSession>> {
        self.conn
            .query_row(
                "SELECT id, project_id, title, directory,
                        COALESCE(
                            (SELECT time_created FROM message WHERE session_id = session.id AND json_extract(data, '$.role') = 'user' ORDER BY time_created DESC LIMIT 1),
                            time_created
                        ) as last_interaction
                 FROM session 
                 WHERE project_id = ?1 AND time_archived IS NULL AND parent_id IS NULL 
                 ORDER BY last_interaction DESC 
                 LIMIT 1 OFFSET ?2",
                params![project_id, offset as i64],
                |row| {
                    Ok(DbSession {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        directory: PathBuf::from(row.get::<_, Option<String>>(3)?.unwrap_or_default()),
                        time_updated: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_session_by_id(&self, session_id: &str) -> anyhow::Result<Option<DbSession>> {
        self.conn
            .query_row(
                "SELECT id, project_id, title, directory,
                        COALESCE(
                            (SELECT time_created FROM message WHERE session_id = session.id AND json_extract(data, '$.role') = 'user' ORDER BY time_created DESC LIMIT 1),
                            time_created
                        ) as last_interaction
                 FROM session WHERE id = ?1",
                [session_id],
                |row| {
                    Ok(DbSession {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        directory: PathBuf::from(
                            row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        ),
                        time_updated: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_child_sessions(
        &self,
        parent_session_id: &str,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<DbSession>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, title, directory, time_updated FROM session WHERE parent_id = ?1 AND time_archived IS NULL ORDER BY time_created DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(
            params![parent_session_id, limit as i64, offset as i64],
            |row| {
                Ok(DbSession {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    directory: PathBuf::from(row.get::<_, Option<String>>(3)?.unwrap_or_default()),
                    time_updated: row.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn has_child_sessions(&self, session_id: &str) -> anyhow::Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM session WHERE parent_id = ?1 AND time_archived IS NULL LIMIT 1",
            [session_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn is_top_level_session(&self, session_id: &str) -> anyhow::Result<bool> {
        let result: Option<(Option<String>, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT parent_id, time_archived FROM session WHERE id = ?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(matches!(result, Some((None, None))))
    }

    /// Returns true if the session exists in the DB and is not archived.
    /// Used by `apply_session_event` defensive check to detect when an
    /// existing managed `session_id` is still the user's active session,
    /// to avoid having plugin-created auxiliary sessions displace it.
    pub fn session_is_active(&self, session_id: &str) -> anyhow::Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM session WHERE id = ?1 AND time_archived IS NULL LIMIT 1",
            [session_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn get_session_status(
        &self,
        session_id: &str,
        process_start_time: Option<i64>,
    ) -> anyhow::Result<SessionStatus> {
        self.get_session_status_recursive(
            session_id,
            process_start_time,
            None,
            &mut std::collections::HashSet::new(),
        )
    }

    fn get_session_status_recursive(
        &self,
        session_id: &str,
        process_start_time: Option<i64>,
        parent_user_msg_time: Option<i64>,
        visited: &mut std::collections::HashSet<String>,
    ) -> anyhow::Result<SessionStatus> {
        if !visited.insert(session_id.to_string()) {
            return Ok(SessionStatus::Idle);
        }

        let local = self.get_local_session_status(session_id, process_start_time)?;

        // If parent has a newer user message than this subagent's last update,
        // treat subagent as abandoned/idle.
        if let Some(cutoff) = parent_user_msg_time {
            let last_update: Option<i64> = self
                .conn
                .query_row(
                    "SELECT time_updated FROM session WHERE id = ?1",
                    [session_id],
                    |row| row.get(0),
                )
                .optional()?;

            if let Some(t) = last_update {
                // OpenCode uses milliseconds for time_updated
                if t < cutoff {
                    return Ok(SessionStatus::Idle);
                }
            }
        }

        if local == SessionStatus::NeedsInput {
            return Ok(SessionStatus::NeedsInput);
        }

        // Get this session's latest user message time to pass to children
        let latest_user_msg_time: Option<i64> = self
            .conn
            .query_row(
                "SELECT time_created FROM message WHERE session_id = ?1 AND json_extract(data, '$.role') = 'user' ORDER BY time_created DESC LIMIT 1",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;

        let mut best_child = SessionStatus::Idle;
        let children = self.get_child_sessions(session_id, 100, 0)?;
        for child in children {
            let child_status = self.get_session_status_recursive(
                &child.id,
                process_start_time,
                latest_user_msg_time,
                visited,
            )?;
            best_child = self.rollup_status(best_child, child_status, true);
            if best_child == SessionStatus::NeedsInput {
                return Ok(SessionStatus::NeedsInput);
            }
        }

        Ok(self.rollup_status(local, best_child, false))
    }

    fn rollup_status(
        &self,
        base: SessionStatus,
        new: SessionStatus,
        is_child: bool,
    ) -> SessionStatus {
        let new_effective = if is_child {
            match new {
                SessionStatus::Working | SessionStatus::SubagentsWorking => {
                    SessionStatus::SubagentsWorking
                }
                _ => new,
            }
        } else {
            new
        };

        // Precedence: NeedsInput > Error > Working > SubagentsWorking > Idle
        match (base, new_effective) {
            (SessionStatus::NeedsInput, _) | (_, SessionStatus::NeedsInput) => {
                SessionStatus::NeedsInput
            }
            (SessionStatus::Error, _) | (_, SessionStatus::Error) => SessionStatus::Error,
            (SessionStatus::Working, _) | (_, SessionStatus::Working) => SessionStatus::Working,
            (SessionStatus::SubagentsWorking, _) | (_, SessionStatus::SubagentsWorking) => {
                SessionStatus::SubagentsWorking
            }
            _ => SessionStatus::Idle,
        }
    }

    fn get_local_session_status(
        &self,
        session_id: &str,
        process_start_time: Option<i64>,
    ) -> anyhow::Result<SessionStatus> {
        let latest_message_id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 1",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(message_id) = latest_message_id.as_deref() {
            // Check for NeedsInput: tool status 'running' or 'pending'
            let latest_running: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM part p WHERE p.session_id = ?1 AND p.message_id = ?2 AND json_extract(p.data, '$.type') = 'tool' AND json_extract(p.data, '$.tool') IN ('question', 'plan_exit') AND json_extract(p.data, '$.state.status') = 'running'",
                params![session_id, message_id],
                |row| row.get(0),
            )?;
            if latest_running > 0 {
                // Check if this tool part is stale relative to process lifetime
                if let Some(cutoff) = process_start_time {
                    let part_time: Option<i64> = self
                        .conn
                        .query_row(
                            "SELECT COALESCE(json_extract(data, '$.state.time.start'), time_created) FROM part WHERE session_id = ?1 AND message_id = ?2 AND json_extract(data, '$.type') = 'tool' AND json_extract(data, '$.tool') IN ('question', 'plan_exit') AND json_extract(data, '$.state.status') = 'running' LIMIT 1",
                            params![session_id, message_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if part_time.is_none_or(|t| t >= cutoff) {
                        return Ok(SessionStatus::NeedsInput);
                    }
                } else {
                    return Ok(SessionStatus::NeedsInput);
                }
            }

            // Detect 'pending' status on ANY tool call part (permission required)
            let latest_pending: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM part p WHERE p.session_id = ?1 AND p.message_id = ?2 AND json_extract(p.data, '$.type') = 'tool' AND json_extract(p.data, '$.state.status') = 'pending'",
                params![session_id, message_id],
                |row| row.get(0),
            )?;
            if latest_pending > 0 {
                if let Some(cutoff) = process_start_time {
                    let part_time: Option<i64> = self
                        .conn
                        .query_row(
                            "SELECT time_created FROM part WHERE session_id = ?1 AND message_id = ?2 AND json_extract(data, '$.type') = 'tool' AND json_extract(data, '$.state.status') = 'pending' LIMIT 1",
                            params![session_id, message_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if part_time.is_none_or(|t| t >= cutoff) {
                        return Ok(SessionStatus::NeedsInput);
                    }
                } else {
                    return Ok(SessionStatus::NeedsInput);
                }
            }

            // Check for Error
            let latest_error: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM part p WHERE p.session_id = ?1 AND p.message_id = ?2 AND json_extract(p.data, '$.type') = 'tool' AND json_extract(p.data, '$.state.status') = 'error'",
                params![session_id, message_id],
                |row| row.get(0),
            )?;
            if latest_error > 0 {
                return Ok(SessionStatus::Error);
            }

            let message_error: Option<String> = self
                .conn
                .query_row(
                    "SELECT json_extract(data, '$.error.name') FROM message WHERE id = ?1 AND json_extract(data, '$.error') IS NOT NULL",
                    [message_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if message_error
                .as_deref()
                .is_some_and(|name| name != "MessageAbortedError")
            {
                return Ok(SessionStatus::Error);
            }
        }

        let latest_message: Option<(Option<String>, Option<i64>, i64)> = self.conn.query_row(
            "SELECT json_extract(data, '$.role') as role, json_extract(data, '$.time.completed') as completed, time_created FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;

        match latest_message {
            Some((Some(role), _completed, time)) if role == "user" => {
                if let Some(cutoff) = process_start_time {
                    if time < cutoff {
                        Ok(SessionStatus::Idle)
                    } else {
                        Ok(SessionStatus::Working)
                    }
                } else {
                    Ok(SessionStatus::Working)
                }
            }
            Some((Some(role), completed, time)) if role == "assistant" && completed.is_none() => {
                if let Some(cutoff) = process_start_time {
                    if time < cutoff {
                        Ok(SessionStatus::Idle)
                    } else {
                        Ok(SessionStatus::Working)
                    }
                } else {
                    Ok(SessionStatus::Working)
                }
            }
            _ => Ok(SessionStatus::Idle),
        }
    }

    pub fn get_session_modified_files(&self, session_id: &str) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT file_path FROM (
                SELECT COALESCE(
                    json_extract(data, '$.state.input.filePath'),
                    json_extract(data, '$.state.input.path'),
                    json_extract(data, '$.state.metadata.filepath'),
                    json_extract(data, '$.state.metadata.filediff.file'),
                    json_extract(data, '$.input.filePath'),
                    json_extract(data, '$.input.path')
                ) AS file_path
                FROM part
                WHERE session_id = ?1
                  AND json_extract(data, '$.type') = 'tool'
                  AND json_extract(data, '$.tool') IN ('edit', 'write', 'apply_patch', 'github_create_or_update_file')
            ) WHERE file_path IS NOT NULL
            ORDER BY file_path"
        )?;
        let rows = stmt.query_map([session_id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    }
    pub fn get_session_model(&self, session_id: &str) -> anyhow::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT json_extract(data, '$.modelID') as model_id FROM message WHERE session_id = ?1 AND json_extract(data, '$.role') = 'assistant' AND json_extract(data, '$.modelID') IS NOT NULL ORDER BY time_created DESC LIMIT 1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_last_message_preview(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Option<SessionPreview>> {
        self.conn
            .query_row(
                "SELECT json_extract(p.data, '$.text') as text, json_extract(m.data, '$.role') as role FROM part p JOIN message m ON p.message_id = m.id WHERE p.session_id = ?1 AND json_extract(p.data, '$.type') = 'text' AND json_extract(p.data, '$.text') IS NOT NULL AND json_extract(p.data, '$.text') != '' AND json_extract(p.data, '$.text') NOT LIKE '<%' ORDER BY m.time_created DESC, p.time_created DESC LIMIT 1",
                [session_id],
                |row| {
                    Ok(SessionPreview {
                        text: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        role: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_conversation(&self, session_id: &str) -> anyhow::Result<Vec<DbConversationMessage>> {
        let mut msg_stmt = self.conn.prepare(
            "SELECT id, time_created,
                    json_extract(data, '$.role'),
                    json_extract(data, '$.time.completed'),
                    json_extract(data, '$.modelID'),
                    json_extract(data, '$.agent')
             FROM message WHERE session_id = ?1 ORDER BY time_created ASC",
        )?;
        let msg_rows: Vec<(
            String,
            i64,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        )> = msg_stmt
            .query_map([session_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut part_stmt = self.conn.prepare(
            "SELECT p.id, p.message_id,
                    json_extract(p.data, '$.type'),
                    json_extract(p.data, '$.text'),
                    json_extract(p.data, '$.tool'),
                    json_extract(p.data, '$.state.status'),
                    json_extract(p.data, '$.state.title'),
                    COALESCE(
                        json_extract(p.data, '$.state.input.filePath'),
                        json_extract(p.data, '$.state.input.path'),
                        json_extract(p.data, '$.state.input.command'),
                        json_extract(p.data, '$.state.input.query'),
                        json_extract(p.data, '$.state.input.pattern'),
                        json_extract(p.data, '$.state.input.url'),
                        json_extract(p.data, '$.state.input.description')
                    )
             FROM part p WHERE p.session_id = ?1 ORDER BY p.time_created ASC",
        )?;
        let part_rows: Vec<(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = part_stmt
            .query_map([session_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut parts_by_msg: std::collections::HashMap<String, Vec<DbConversationPart>> =
            std::collections::HashMap::new();
        for (id, message_id, part_type, text, tool, tool_status, tool_title, tool_input) in
            part_rows
        {
            parts_by_msg
                .entry(message_id)
                .or_default()
                .push(DbConversationPart {
                    id,
                    part_type: part_type.unwrap_or_default(),
                    text,
                    tool,
                    tool_status,
                    tool_title,
                    tool_input,
                });
        }

        let mut messages = Vec::new();
        for (id, time_created, role, completed, model_id, agent) in msg_rows {
            let parts = parts_by_msg.remove(&id).unwrap_or_default();
            messages.push(DbConversationMessage {
                id,
                role: role.unwrap_or_default(),
                time_created,
                completed,
                model_id,
                agent,
                parts,
            });
        }

        Ok(messages)
    }

    pub fn get_all_user_messages(&self) -> anyhow::Result<Vec<DbUserMessage>> {
        let mut stmt = self.conn.prepare(
            "WITH recent_user_messages AS (
                SELECT m.id, m.session_id, m.time_created,
                       COALESCE(s.title, '') AS session_title
                FROM message m
                JOIN session s ON s.id = m.session_id
                WHERE json_extract(m.data, '$.role') = 'user'
                ORDER BY m.time_created DESC, m.id DESC
                LIMIT 500
            )
            SELECT m.id, m.session_id, m.session_title, m.time_created,
                COALESCE((
                    SELECT group_concat(part_text, '')
                    FROM (
                        SELECT json_extract(p.data, '$.text') AS part_text
                        FROM part p
                        WHERE p.message_id = m.id
                          AND json_extract(p.data, '$.type') = 'text'
                          AND json_extract(p.data, '$.text') IS NOT NULL
                        ORDER BY p.time_created ASC, p.id ASC
                    )
                ), '') AS text
            FROM recent_user_messages m
            ORDER BY m.time_created DESC, m.id DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(DbUserMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                session_title: row.get(2)?,
                time_created: row.get(3)?,
                text: row.get(4)?,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            let msg = row?;
            if !msg.text.trim().is_empty() {
                messages.push(msg);
            }
        }

        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::sessions::SessionStatus;
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ocmux-rs-reader-{label}-{nanos}.db"))
    }

    fn init_db(path: &PathBuf) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE project (
                id TEXT PRIMARY KEY,
                worktree TEXT NOT NULL,
                name TEXT,
                time_created INTEGER,
                time_updated INTEGER
            );
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                parent_id TEXT,
                title TEXT,
                directory TEXT,
                permission TEXT,
                time_created INTEGER,
                time_updated INTEGER,
                time_archived INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL,
                time_created INTEGER
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                data TEXT NOT NULL,
                time_created INTEGER
            );
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn get_session_status_treats_message_error_as_error() {
        let db_path = temp_db_path("message-error");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess1', 'proj1', NULL, 'Title', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO message VALUES ('msg1', 'sess1', '{"role":"assistant","error":{"name":"ContextOverflowError"}}', 200)"#,
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        assert_eq!(
            reader.get_session_status("sess1", None).unwrap(),
            SessionStatus::Error
        );
    }

    #[test]
    fn get_session_status_ignores_message_aborted_error() {
        let db_path = temp_db_path("message-aborted");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess1', 'proj1', NULL, 'Title', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO message VALUES ('msg1', 'sess1', '{"role":"assistant","time":{"completed":200},"error":{"name":"MessageAbortedError"}}', 200)"#,
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        assert_eq!(
            reader.get_session_status("sess1", None).unwrap(),
            SessionStatus::Idle
        );
    }

    #[test]
    fn session_is_active_returns_true_for_unarchived_session() {
        let db_path = temp_db_path("is-active-true");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess1', 'proj1', NULL, 'Title', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        assert!(reader.session_is_active("sess1").unwrap());
    }

    #[test]
    fn session_is_active_returns_false_for_archived_session() {
        let db_path = temp_db_path("is-active-archived");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        // time_archived = 1000 (non-null) — session is archived
        conn.execute(
            "INSERT INTO session VALUES ('sess1', 'proj1', NULL, 'Title', '/tmp/proj', NULL, 100, 200, 1000)",
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        assert!(!reader.session_is_active("sess1").unwrap());
    }

    #[test]
    fn get_session_status_detects_any_pending_tool_as_needs_input() {
        let db_path = temp_db_path("pending-tool");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess1', 'proj1', NULL, 'Title', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO message VALUES ('msg1', 'sess1', '{"role":"assistant"}', 200)"#,
            [],
        )
        .unwrap();
        // A tool that is NOT question/plan_exit, but is pending
        conn.execute(
            r#"INSERT INTO part VALUES ('part1', 'sess1', 'msg1', '{"type":"tool","tool":"edit","state":{"status":"pending"}}', 200)"#,
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        assert_eq!(
            reader.get_session_status("sess1", None).unwrap(),
            SessionStatus::NeedsInput
        );
    }

    #[test]
    fn get_session_status_rollups_child_status() {
        let db_path = temp_db_path("rollup");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        
        // Parent session (Idle)
        conn.execute(
            "INSERT INTO session VALUES ('parent', 'proj1', NULL, 'Parent', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO message VALUES ('msg-p', 'parent', '{"role":"assistant","time":{"completed":200}}', 200)"#,
            [],
        )
        .unwrap();

        // Child session (Working)
        conn.execute(
            "INSERT INTO session VALUES ('child', 'proj1', 'parent', 'Child', '/tmp/proj/child', NULL, 150, 250, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO message VALUES ('msg-c', 'child', '{"role":"assistant"}', 250)"#,
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        
        // Child should be Working
        assert_eq!(
            reader.get_session_status("child", None).unwrap(),
            SessionStatus::Working
        );
        
        // Parent should be SubagentsWorking
        assert_eq!(
            reader.get_session_status("parent", None).unwrap(),
            SessionStatus::SubagentsWorking
        );
        
        // If child becomes NeedsInput, parent should become NeedsInput
        conn.execute(
            r#"INSERT INTO part VALUES ('part-c', 'child', 'msg-c', '{"type":"tool","tool":"question","state":{"status":"running"}}', 260)"#,
            [],
        )
        .unwrap();
        
        assert_eq!(
            reader.get_session_status("parent", None).unwrap(),
            SessionStatus::NeedsInput
        );
    }

    #[test]
    fn get_session_status_ignores_archived_children() {
        let db_path = temp_db_path("rollup-archived");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        
        // Parent session (Idle)
        conn.execute(
            "INSERT INTO session VALUES ('parent', 'proj1', NULL, 'Parent', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();

        // Archived Child session (Working)
        conn.execute(
            "INSERT INTO session VALUES ('child', 'proj1', 'parent', 'Child', '/tmp/proj/child', NULL, 150, 250, 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO message VALUES ('msg-c', 'child', '{"role":"assistant"}', 250)"#,
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        
        // Parent should be Idle because child is archived
        assert_eq!(
            reader.get_session_status("parent", None).unwrap(),
            SessionStatus::Idle
        );
    }
}
