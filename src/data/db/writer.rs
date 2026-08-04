use std::path::Path;
use anyhow::Context;
use rusqlite::{Connection, OpenFlags};

pub struct DbWriter {
    conn: Connection,
}

impl DbWriter {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| format!("failed to open sqlite db for write at {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        Ok(Self { conn })
    }

    pub fn open_default() -> anyhow::Result<Self> {
        Self::open(&super::default_db_path()?)
    }

    pub fn pragma_foreign_keys(&self) -> anyhow::Result<i64> {
        Ok(self.conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?)
    }

    /// Returns the busy_timeout in milliseconds. Used for tests and diagnostics.
    pub fn busy_timeout_ms(&self) -> anyhow::Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))?)
    }

    pub fn delete_sessions(
        &mut self,
        ids: &[String],
        live_ids: &std::collections::HashSet<String>,
    ) -> anyhow::Result<DeleteSessionsResult> {
        let mut skipped_live_ids = Vec::new();
        let mut to_delete = std::collections::HashSet::new();

        for id in ids {
            if live_ids.contains(id) {
                skipped_live_ids.push(id.clone());
                continue;
            }

            // Recursive CTE to find all descendants
            let mut stmt = self.conn.prepare(
                "WITH RECURSIVE tree(id) AS (
                    SELECT id FROM session WHERE id = ?1
                    UNION ALL
                    SELECT s.id FROM session s JOIN tree t ON s.parent_id = t.id
                ) SELECT id FROM tree",
            )?;
            let rows = stmt.query_map([id], |row| row.get::<_, String>(0))?;
            let mut found_live = false;
            let mut subtree_ids = Vec::new();
            for row in rows {
                let descendant_id = row?;
                if live_ids.contains(&descendant_id) {
                    found_live = true;
                }
                subtree_ids.push(descendant_id);
            }

            if found_live {
                skipped_live_ids.push(id.clone());
            } else {
                for sid in subtree_ids {
                    to_delete.insert(sid);
                }
            }
        }

        if to_delete.is_empty() {
            return Ok(DeleteSessionsResult {
                deleted_session_ids: Vec::new(),
                skipped_live_ids,
            });
        }

        let mut sorted_ids: Vec<String> = to_delete.into_iter().collect();
        sorted_ids.sort();

        // Perform deletion in transaction
        let tx = self.conn.transaction()?;

        {
            let id_list = format!("('{}')", sorted_ids.join("','"));
            
            // Whitelist of tables to clean up. 
            let tables = [
                "session_share",
                "session_message",
                "session_input",
                "session_context_epoch",
                "todo",
                "part",
                "message",
                "session",
            ];

            for table in tables {
                let exists: bool = tx.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get::<_, i64>(0).map(|n| n > 0),
                )?;
                if exists {
                    tx.execute(&format!("DELETE FROM {} WHERE {} IN {}", 
                        table, 
                        if table == "session" { "id" } else { "session_id" },
                        id_list), [])?;
                }
            }
        }

        tx.commit()?;
        
        Ok(DeleteSessionsResult {
            deleted_session_ids: sorted_ids,
            skipped_live_ids,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteSessionsResult {
    pub deleted_session_ids: Vec<String>,
    pub skipped_live_ids: Vec<String>,
}
