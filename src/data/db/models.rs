use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbProject {
    pub id: String,
    pub worktree: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSession {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub directory: PathBuf,
    pub time_updated: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPreview {
    pub text: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSessionSummary {
    pub id: String,
    pub title: String,
    pub directory: PathBuf,
    pub worktree: PathBuf,
    pub time_updated: i64,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbConversationMessage {
    pub id: String,
    pub role: String,
    pub time_created: i64,
    pub completed: Option<i64>,
    pub model_id: Option<String>,
    pub agent: Option<String>,
    pub parts: Vec<DbConversationPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbConversationPart {
    pub id: String,
    pub part_type: String,
    pub text: Option<String>,
    pub tool: Option<String>,
    pub tool_status: Option<String>,
    pub tool_title: Option<String>,
    pub tool_input: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbUserMessage {
    pub id: String,
    pub session_id: String,
    pub session_title: String,
    pub time_created: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbManagedSession {
    pub id: String,
    pub title: String,
    pub directory: PathBuf,
    pub worktree: PathBuf,
    pub user_message_count: i64,
    pub time_updated: i64,
}
