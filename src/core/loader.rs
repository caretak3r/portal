use crate::core::progress::ProgressReporter;
use crate::core::{backup, checksum, plugins, safety, skeleton};
use crate::storage::{manifest, paths::PortalPaths, plugins_manifest, state};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

use crate::core::profile::{LastOperation, OperationType};

/// Outcome of a successful profile load.
#[derive(Debug)]
pub struct LoadResult {
    /// Name of the loaded profile.
    pub profile: String,
    /// Number of files placed into `~/.claude/`.
    pub files_loaded: usize,
    /// Path to the pre-load backup archive.
    pub backup_path: PathBuf,
    /// Results from plugin reinstallation (empty when `no_plugins` is set).
    pub plugin_results: Vec<plugins::PluginInstallResult>,
}

/// Load a saved profile into `~/.claude/` via atomic swap.
///
/// Convenience wrapper around [`load_with_progress`] with a no-op reporter.
///
/// # Errors
///
/// Returns an error on pre-flight, checksum, or filesystem failures.
pub fn load(
    paths: &PortalPaths,
    profile_name: &str,
    no_plugins: bool,
    skip_claude_check: bool,
) -> Result<LoadResult> {
    load_with_progress(paths, profile_name, no_plugins, skip_claude_check, &super::progress::NoProgress)
}

/// Load a saved profile into `~/.claude/` via atomic swap with progress reporting.
///
/// # Errors
///
/// Returns an error if pre-flight checks fail, the profile manifest is
/// corrupt, checksum verification fails, filesystem operations (rename,
/// copy) fail, or state persistence fails.
#[allow(clippy::too_many_lines)]
pub fn load_with_progress(
    paths: &PortalPaths,
    profile_name: &str,
    no_plugins: bool,
    skip_claude_check: bool,
    progress: &dyn ProgressReporter,
) -> Result<LoadResult> {
    // 1. Pre-flight checks.
    if skip_claude_check {
        // Minimal validation — just verify the profile exists.
        let profile_dir = paths.profile_dir(profile_name);
        if !profile_dir.exists() {
            bail!("Profile \"{profile_name}\" not found.");
        }
        let manifest_path = paths.profile_manifest(profile_name);
        if !manifest_path.exists() {
            bail!("Profile \"{profile_name}\" is missing portal.json manifest.");
        }
    } else {
        safety::preflight_load(paths, profile_name)?;
    }

    // 2. Acquire lock.
    let _lock = safety::acquire_lock(paths)?;

    // 3. Read and verify profile manifest checksums against stored files.
    let manifest_path = paths.profile_manifest(profile_name);
    let mut manifest = manifest::read(&manifest_path)?;
    let files_dir = paths.profile_files_dir(profile_name);

    let mismatches = checksum::verify_manifest(&files_dir, &manifest.files)?;
    if !mismatches.is_empty() {
        let details: Vec<String> = mismatches
            .iter()
            .map(|m| format!("  {} (expected {}, got {})", m.path, m.expected, m.actual))
            .collect();
        bail!(
            "Profile \"{profile_name}\" has {} checksum mismatch(es):\n{}",
            mismatches.len(),
            details.join("\n")
        );
    }

    // 4. Back up current .claude/.
    let claude_dir = paths.claude_root();
    let backup_path = if claude_dir.exists() {
        backup::create(paths, "load", profile_name)?
    } else {
        // Nothing to back up — create a placeholder path.
        paths.backups_dir().join("no-backup")
    };

    // 5. Build target in tempdir.
    let tmp = tempfile::tempdir_in(paths.portal_root())
        .context("creating temp dir for profile build")?;
    let build_dir = tmp.path().join("claude");
    std::fs::create_dir_all(&build_dir).context("creating build dir")?;

    // Lay down skeleton first.
    skeleton::create(&build_dir)?;

    // Overlay profile files with progress.
    let file_count = manifest.files.len() as u64;
    progress.set_total(file_count);
    copy_dir_with_progress(&files_dir, &build_dir, progress)?;

    let files_loaded = manifest.files.len();

    // 6. Verify built checksums.
    let build_mismatches = checksum::verify_manifest(&build_dir, &manifest.files)?;
    if !build_mismatches.is_empty() {
        let details: Vec<String> = build_mismatches
            .iter()
            .map(|m| format!("  {} (expected {}, got {})", m.path, m.expected, m.actual))
            .collect();
        bail!(
            "Built directory has {} checksum mismatch(es):\n{}",
            build_mismatches.len(),
            details.join("\n")
        );
    }

    // 7. Atomic swap.
    let old_dir = paths.claude_old();
    if old_dir.exists() {
        std::fs::remove_dir_all(&old_dir).context("removing stale .portal-old")?;
    }

    if claude_dir.exists() {
        std::fs::rename(&claude_dir, &old_dir).context("moving .claude to .portal-old")?;
    }

    if let Err(e) = std::fs::rename(&build_dir, &claude_dir) {
        // Rollback: restore original.
        let _ = std::fs::rename(&old_dir, &claude_dir);
        return Err(e).context("atomic swap failed — restored original");
    }

    // Swap succeeded — clean up old dir.
    if old_dir.exists() {
        let _ = std::fs::remove_dir_all(&old_dir);
    }

    info!("atomic swap complete for profile: {profile_name}");

    // 8. Reinstall plugins.
    let plugin_results = if no_plugins {
        Vec::new()
    } else {
        let plugins_path = paths.profile_plugins(profile_name);
        if plugins_path.exists() {
            let blueprint = plugins_manifest::read(&plugins_path).unwrap_or_default();
            plugins::reinstall(&blueprint)
        } else {
            Vec::new()
        }
    };

    // 9. Update portal.state.json.
    let state_path = paths.state_file();
    let mut portal_state = state::read(&state_path)?;
    portal_state.active_profile = Some(profile_name.to_string());
    portal_state.last_operation = Some(LastOperation {
        op_type: OperationType::Load,
        profile: profile_name.to_string(),
        timestamp: Utc::now(),
        backup_path: backup_path.to_string_lossy().to_string(),
        plugins_installed: !no_plugins,
    });
    state::write(&state_path, &portal_state)?;

    // 10. Update manifest load count.
    manifest.load_count += 1;
    manifest.last_loaded = Some(Utc::now());
    manifest::write(&paths.profile_manifest(profile_name), &manifest)?;

    Ok(LoadResult {
        profile: profile_name.to_string(),
        files_loaded,
        backup_path,
        plugin_results,
    })
}

/// Recursively copy files from `src` to `dst` with progress reporting.
///
/// # Errors
///
/// Returns an error if directory traversal, creation, or copy fails.
fn copy_dir_with_progress(src: &Path, dst: &Path, progress: &dyn ProgressReporter) -> Result<()> {
    let mut count: u64 = 0;
    for entry in WalkDir::new(src).min_depth(1) {
        let entry =
            entry.with_context(|| format!("walking directory: {}", src.display()))?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .with_context(|| "stripping prefix during copy")?;
        let target = dst.join(rel);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)
                .with_context(|| format!("creating dir: {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating parent: {}", parent.display()))?;
            }
            std::fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "copying {} -> {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
            count += 1;
            progress.tick(count, &rel.to_string_lossy());
        }
    }
    Ok(())
}

/// Recursively copy all files from `src` to `dst`, creating parent
/// directories as needed.
///
/// Overwrites existing files at the destination. Preserves relative
/// directory structure.
///
/// # Errors
///
/// Returns an error if directory traversal, directory creation, or
/// file copy operations fail.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    for entry in WalkDir::new(src).min_depth(1) {
        let entry =
            entry.with_context(|| format!("walking directory: {}", src.display()))?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .with_context(|| "stripping prefix during copy")?;
        let target = dst.join(rel);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)
                .with_context(|| format!("creating dir: {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating parent: {}", parent.display()))?;
            }
            std::fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "copying {} -> {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}
