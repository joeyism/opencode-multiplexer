pub mod models;
pub mod reader;
pub mod writer;

use std::path::PathBuf;
use anyhow::Context;

pub(crate) fn default_db_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/share/opencode/opencode.db"))
}
