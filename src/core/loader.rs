use crate::core::profile::ProfileManifest;
use crate::core::progress::ProgressReporter;
use crate::core::{backup, checksum, plugins, safety, skeleton};
use crate::storage::{cas, manifest, paths::PortalPaths, plugins_manifest, state};
use anyhow::{Context, Result, bail};
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
/// Convenience wrapper around [`load_with_progress`] with a no-op reporter
/// and the safe defaults (backup on, plugins reinstalled).
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
    load_with_progress(
        paths,
        profile_name,
        no_plugins,
        false, // no_backup — keep the safe default for callers using `load`
        skip_claude_check,
        &super::progress::NoProgress,
    )
}

/// Load a saved profile into `~/.claude/` via atomic swap with progress reporting.
///
/// # Errors
///
/// Returns an error if pre-flight checks fail, the profile manifest is
/// corrupt, checksum verification fails, filesystem operations (rename,
/// copy) fail, or state persistence fails.
#[allow(clippy::too_many_lines, clippy::fn_params_excessive_bools)]
pub fn load_with_progress(
    paths: &PortalPaths,
    profile_name: &str,
    no_plugins: bool,
    no_backup: bool,
    skip_claude_check: bool,
    progress: &dyn ProgressReporter,
) -> Result<LoadResult> {
    // 1. Pre-flight checks.
    progress.phase("Verifying profile");
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

    // 3. Read profile manifest. Verification of stored content depends on
    //    layout: CAS-mode profiles verify by checking objects exist (cheap);
    //    legacy `files/` profiles verify by re-hashing each file.
    let manifest_path = paths.profile_manifest(profile_name);
    let mut manifest = manifest::read(&manifest_path)?;
    let files_dir = paths.profile_files_dir(profile_name);

    // Pick CAS path if every referenced object is in the pool. Otherwise fall
    // back to legacy `files/` (and migrate on the way through).
    let cas_ready = !files_dir.exists() || all_objects_present(paths, &manifest);
    if cas_ready {
        verify_manifest_objects(paths, &manifest)?;
    } else {
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
    }

    // 3.5. Snapshot the *current* live plugin blueprint (before the swap
    //      mutates ~/.claude/). This lets step 8 reinstall only the delta
    //      between active and target — for the toggle-back case where the
    //      two profiles share most plugins, the entire reinstall becomes
    //      a no-op. Best-effort: if extraction fails we fall through to
    //      a full reinstall, matching pre-diff behaviour.
    let claude_dir = paths.claude_root();
    let active_blueprint = if claude_dir.exists() && !no_plugins {
        plugins::extract_blueprint(&claude_dir).ok()
    } else {
        None
    };

    // 4. Back up current .claude/.
    progress.phase("Backing up current config");
    let backup_path = if no_backup {
        // Caller explicitly opted out of backups — record a sentinel path so
        // the LoadResult and state file still serialize cleanly.
        paths.backups_dir().join("no-backup-skipped")
    } else if claude_dir.exists() {
        backup::create(paths, "load", profile_name)?
    } else {
        // Nothing to back up — create a placeholder path.
        paths.backups_dir().join("no-backup")
    };

    // 5. Build target in tempdir.
    progress.phase("Building target");
    let tmp =
        tempfile::tempdir_in(paths.portal_root()).context("creating temp dir for profile build")?;
    let build_dir = tmp.path().join("claude");
    std::fs::create_dir_all(&build_dir).context("creating build dir")?;

    // Lay down skeleton first.
    skeleton::create(&build_dir)?;

    // Overlay profile files. Prefer reflink-from-CAS (constant-time per file
    // on APFS / btrfs / xfs); fall back to copying from the legacy files/ tree
    // and opportunistically populate the CAS pool as we go.
    let file_count = manifest.files.len() as u64;
    progress.set_total(file_count);

    let cas_sourced = if cas_ready {
        place_from_cas_parallel(paths, &manifest, &build_dir, progress)?;
        true
    } else {
        copy_dir_with_progress(&files_dir, &build_dir, progress)?;
        // Populate CAS for next time. Errors here are not fatal — we still
        // loaded successfully, the profile just won't be deduped yet.
        let _ = cas::migrate_profile_files(paths, &files_dir, &manifest.files);
        false
    };

    let files_loaded = manifest.files.len();

    // 6. Verify built checksums.
    //    When CAS-sourced, every byte we placed came from an object whose name
    //    IS its hash, so the build dir is correct by construction. Skip the
    //    re-hash pass entirely; saves one full read of every file.
    if !cas_sourced {
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
    }

    // 7. Atomic swap.
    progress.phase("Atomic swap");
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

    // 8. Reinstall plugins (delta only — see step 3.5 for active capture).
    progress.phase("Reinstalling plugins");
    let plugin_results = if no_plugins {
        Vec::new()
    } else {
        let plugins_path = paths.profile_plugins(profile_name);
        if plugins_path.exists() {
            let blueprint = plugins_manifest::read(&plugins_path).unwrap_or_default();
            plugins::reinstall_with_diff(&blueprint, active_blueprint.as_ref())
        } else {
            Vec::new()
        }
    };

    // 9. Update portal.state.json.
    let state_path = paths.state_file();
    let mut portal_state = state::read(&state_path)?;
    // Capture the outgoing active profile as `previous_profile` so `portal
    // toggle` can swap straight back. Loading the active profile onto itself
    // is a no-op for toggle history — preserve whatever previous was already
    // recorded rather than clobbering it with the same name.
    if portal_state.active_profile.as_deref() != Some(profile_name) {
        portal_state.previous_profile = portal_state.active_profile.clone();
    }
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

/// True when every file referenced by the manifest has its object in CAS.
fn all_objects_present(paths: &PortalPaths, manifest: &ProfileManifest) -> bool {
    manifest
        .files
        .values()
        .all(|entry| cas::exists(paths, &entry.checksum))
}

/// Verify every CAS object referenced by the manifest exists. Cheap: a stat per
/// file, no read of file contents. We trust the CAS pool's content-addressing
/// invariant — the object's name IS its hash, so existence implies correctness.
fn verify_manifest_objects(paths: &PortalPaths, manifest: &ProfileManifest) -> Result<()> {
    let missing: Vec<String> = manifest
        .files
        .iter()
        .filter(|(_, entry)| !cas::exists(paths, &entry.checksum))
        .map(|(rel, entry)| format!("  {} ({})", rel, entry.checksum))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        bail!(
            "Profile is missing {} CAS object(s):\n{}",
            missing.len(),
            missing.join("\n")
        )
    }
}

/// Place every manifest entry from the CAS pool into `build_dir`.
/// Each placement uses reflink when the filesystem supports it (APFS, btrfs,
/// xfs) and falls back to a plain copy otherwise. Reflinks are metadata-only,
/// so this is dominated by mkdir + clonefile syscalls. Serial — rayon's
/// work-stealing overhead dominates per-file work this small.
fn place_from_cas_parallel(
    paths: &PortalPaths,
    manifest: &ProfileManifest,
    build_dir: &Path,
    progress: &dyn ProgressReporter,
) -> Result<()> {
    // Pre-create every parent directory once so each placement only does a
    // single clonefile, not clonefile + create_dir_all per file.
    let mut parents: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for rel in manifest.files.keys() {
        let target = build_dir.join(rel);
        if let Some(parent) = target.parent() {
            parents.insert(parent.to_path_buf());
        }
    }
    for parent in &parents {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dir: {}", parent.display()))?;
    }

    let total = manifest.files.len() as u64;
    let mut n: u64 = 0;
    for (rel, entry) in &manifest.files {
        let dest = build_dir.join(rel);
        cas::place(paths, &entry.checksum, &dest)?;
        n += 1;
        progress.tick(n.min(total), rel);
    }
    Ok(())
}

/// Recursively copy files from `src` to `dst` with progress reporting.
///
/// # Errors
///
/// Returns an error if directory traversal, creation, or copy fails.
fn copy_dir_with_progress(src: &Path, dst: &Path, progress: &dyn ProgressReporter) -> Result<()> {
    let mut count: u64 = 0;
    for entry in WalkDir::new(src).min_depth(1) {
        let entry = entry.with_context(|| format!("walking directory: {}", src.display()))?;
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
                format!("copying {} -> {}", entry.path().display(), target.display())
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
        let entry = entry.with_context(|| format!("walking directory: {}", src.display()))?;
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
                format!("copying {} -> {}", entry.path().display(), target.display())
            })?;
        }
    }
    Ok(())
}
