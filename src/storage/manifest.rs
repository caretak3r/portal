use crate::core::profile::ProfileManifest;
use anyhow::{Context, Result};
use std::path::Path;

/// Read a profile manifest from disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn read(path: &Path) -> Result<ProfileManifest> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading manifest: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing manifest: {}", path.display()))
}

/// Write a profile manifest to disk as pretty-printed JSON.
///
/// Creates parent directories if they don't exist.
///
/// # Errors
///
/// Returns an error if serialization or file I/O fails.
pub fn write(path: &Path, manifest: &ProfileManifest) -> Result<()> {
    let content = serde_json::to_string_pretty(manifest)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing manifest: {}", path.display()))
}
