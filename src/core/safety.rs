use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::warn;

use crate::storage::paths::PortalPaths;

/// Results of pre-flight validation before a `load` operation.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct PreflightReport {
    /// Claude Code is not running.
    pub claude_not_running: bool,
    /// `~/.claude/` exists on disk.
    pub claude_dir_exists: bool,
    /// The requested profile directory and manifest exist.
    pub profile_exists: bool,
    /// `~/.claude.portal-old` was found, hinting at a previous crash.
    pub crash_recovery_needed: bool,
}

/// Pre-flight checks before a `load` operation.
///
/// Validates that Claude is not running, the claude directory exists,
/// and the target profile is present with a valid manifest.
///
/// # Errors
///
/// Returns an error if any check fails (Claude running, missing dirs,
/// missing profile).
pub fn preflight_load(paths: &PortalPaths, profile_name: &str) -> Result<PreflightReport> {
    let mut report = PreflightReport::default();

    if is_claude_running() {
        bail!("Claude is running. Close all Claude Code sessions first.");
    }
    report.claude_not_running = true;

    let claude_dir = paths.claude_root();
    if !claude_dir.exists() {
        bail!("~/.claude/ does not exist. Run `portal reset` to create a skeleton first.");
    }
    report.claude_dir_exists = true;

    let profile_dir = paths.profile_dir(profile_name);
    if !profile_dir.exists() {
        bail!("Profile \"{profile_name}\" not found. Run `portal list` to see available profiles.");
    }
    let manifest_path = paths.profile_manifest(profile_name);
    if !manifest_path.exists() {
        bail!("Profile \"{profile_name}\" is missing portal.json manifest.");
    }
    report.profile_exists = true;

    if paths.claude_old().exists() {
        warn!("Found ~/.claude.portal-old -- previous operation may have crashed");
        report.crash_recovery_needed = true;
    }

    Ok(report)
}

/// Pre-flight checks before a `save` operation.
///
/// Validates that `~/.claude/` exists and contains a `settings.json`.
///
/// # Errors
///
/// Returns an error if the claude directory or its `settings.json` is missing.
pub fn preflight_save(paths: &PortalPaths) -> Result<()> {
    let claude_dir = paths.claude_root();
    if !claude_dir.exists() {
        bail!("~/.claude/ does not exist. Nothing to save.");
    }
    if !claude_dir.join("settings.json").exists() {
        bail!("~/.claude/settings.json not found. Is this a valid Claude configuration?");
    }
    Ok(())
}

/// Check whether a `claude` process is currently running.
///
/// Uses `pgrep -x claude` for exact-match to avoid matching our own
/// process tree.
fn is_claude_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "claude"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// File-based lock to prevent concurrent portal operations.
///
/// The lock file is automatically removed when this guard is dropped.
pub struct PortalLock {
    path: PathBuf,
}

impl Drop for PortalLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Acquire an exclusive lock for portal operations.
///
/// If a lock file already exists and is older than 300 seconds it is
/// considered stale and removed. A fresh lock within that window causes
/// an error.
///
/// # Errors
///
/// Returns an error if another operation holds the lock or the lock
/// file cannot be created.
pub fn acquire_lock(paths: &PortalPaths) -> Result<PortalLock> {
    let lock_path = paths.lock_file();
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if lock_path.exists() {
        if let Ok(meta) = std::fs::metadata(&lock_path) {
            if let Ok(modified) = meta.modified() {
                let age = SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                if age.as_secs() > 300 {
                    warn!("removing stale lock file ({}s old)", age.as_secs());
                    std::fs::remove_file(&lock_path)?;
                } else {
                    bail!(
                        "Another portal operation is in progress (lock file exists). \
                         If this is stale, delete: {}",
                        lock_path.display()
                    );
                }
            }
        }
    }

    std::fs::write(&lock_path, format!("{}", std::process::id()))
        .context("creating lock file")?;

    Ok(PortalLock { path: lock_path })
}
