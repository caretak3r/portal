use crate::core::profile::ProfileMeta;
use anyhow::{Context, Result};
use std::path::Path;

/// Read profile metadata from disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn read(path: &Path) -> Result<ProfileMeta> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading meta: {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parsing meta: {}", path.display()))
}

/// Write profile metadata to disk as pretty-printed JSON.
///
/// Creates parent directories if they don't exist.
///
/// # Errors
///
/// Returns an error if serialization or file I/O fails.
pub fn write(path: &Path, meta: &ProfileMeta) -> Result<()> {
    let content = serde_json::to_string_pretty(meta)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content).with_context(|| format!("writing meta: {}", path.display()))
}
