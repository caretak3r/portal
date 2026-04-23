use crate::core::profile::PortalState;
use anyhow::{Context, Result};
use std::path::Path;

/// Read portal state from disk, returning a default if the file doesn't exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn read(path: &Path) -> Result<PortalState> {
    if !path.exists() {
        return Ok(PortalState::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading state: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing state: {}", path.display()))
}

/// Write portal state to disk as pretty-printed JSON.
///
/// Creates parent directories if they don't exist.
///
/// # Errors
///
/// Returns an error if serialization or file I/O fails.
pub fn write(path: &Path, state: &PortalState) -> Result<()> {
    let content = serde_json::to_string_pretty(state)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing state: {}", path.display()))
}
