use crate::core::profile::PluginBlueprint;
use anyhow::{Context, Result};
use std::path::Path;

/// Read a plugin blueprint from disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn read(path: &Path) -> Result<PluginBlueprint> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading plugins manifest: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing plugins manifest: {}", path.display()))
}

/// Write a plugin blueprint to disk as pretty-printed JSON.
///
/// Creates parent directories if they don't exist.
///
/// # Errors
///
/// Returns an error if serialization or file I/O fails.
pub fn write(path: &Path, blueprint: &PluginBlueprint) -> Result<()> {
    let content = serde_json::to_string_pretty(blueprint)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing plugins manifest: {}", path.display()))
}
