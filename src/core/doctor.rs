//! `portal doctor` — diagnostics and guided repairs.
//!
//! All logic here is pure data in / data out: [`diagnose`] inspects the
//! environment and returns a [`DoctorReport`]; [`apply_fix`] performs a single
//! opt-in repair. The CLI (and, later, a TUI panel) only render the report and
//! prompt — they never embed diagnostic logic. Every check reuses an existing
//! core utility rather than re-implementing it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::core::{backup, clone, skeleton, snapshot};
use crate::storage::{cas, manifest, meta, paths::PortalPaths, plugins_manifest, state};

/// How urgent a check is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Informational — nothing to do.
    Ok,
    /// Context, not a problem.
    Info,
    /// Should be addressed but not blocking.
    Warning,
    /// Broken; `portal doctor` exits non-zero if any remain.
    Error,
}

/// A repair `portal doctor --fix` can perform after explicit confirmation.
#[derive(Debug, Clone)]
pub enum FixAction {
    /// Import a legacy `~/.portal/profiles/<name>` profile into the active root.
    MigrateLegacyRoot { name: String, dir: PathBuf },
    /// Remove a legacy `~/.portal` directory (or one profile under it).
    DeleteLegacyRoot { dir: PathBuf },
    /// Delete specific zero-byte backup archives.
    PruneZeroByteBackups { paths: Vec<PathBuf> },
    /// Recreate missing skeleton directories/files in `~/.claude`.
    RecreateSkeleton,
    /// Remove a leftover `~/.claude.portal-old` from a crashed swap.
    ClearCrashLeftover { dir: PathBuf },
}

/// A single diagnostic result.
#[derive(Debug, Clone)]
pub struct Check {
    pub id: &'static str,
    pub title: String,
    pub detail: String,
    pub severity: Severity,
    /// `Some` when `--fix` can act on this check.
    pub fix: Option<FixAction>,
}

/// One row of the managed-directory table.
#[derive(Debug, Clone)]
pub struct ManagedDirRow {
    pub category: String,
    pub dir: String,
    pub exists: bool,
    pub file_count: usize,
}

/// Full diagnostic output.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
    pub managed_dirs: Vec<ManagedDirRow>,
    /// Paths portal deliberately ignores (never tracked into a profile).
    pub excluded_patterns: Vec<String>,
}

impl DoctorReport {
    /// True when any check is an unresolved [`Severity::Error`].
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.checks.iter().any(|c| c.severity == Severity::Error)
    }

    /// Checks that carry a fixable action, in report order.
    pub fn fixable(&self) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(|c| c.fix.is_some())
    }
}

/// Inspect the environment and produce a report. Read-only.
///
/// # Errors
///
/// Returns an error only if portal state or the backups directory cannot be
/// read; individual check failures are folded into the report, not bubbled.
pub fn diagnose(paths: &PortalPaths) -> Result<DoctorReport> {
    let mut checks = Vec::new();
    let claude = paths.claude_root();

    // 1. Storage root.
    checks.push(Check {
        id: "storage-root",
        title: "Storage root".into(),
        detail: paths.portal_root().display().to_string(),
        severity: Severity::Info,
        fix: None,
    });

    // 2. Active profile.
    let portal_state = state::read(&paths.state_file())?;
    let active = portal_state.active_profile.clone();
    let detail = match (&active, &portal_state.previous_profile) {
        (Some(a), Some(p)) => format!("{a} (previous: {p})"),
        (Some(a), None) => a.clone(),
        (None, _) => "(none)".into(),
    };
    checks.push(Check {
        id: "active-profile",
        title: "Active profile".into(),
        detail,
        severity: Severity::Info,
        fix: None,
    });

    // 3. Managed-vs-excluded directory table.
    let managed_dirs = build_managed_dirs(&claude);

    // 4. Skeleton completeness.
    checks.push(skeleton_check(&claude));

    // 5. Manifest integrity for the active profile (CAS-aware).
    if let Some(name) = &active {
        checks.push(active_profile_integrity(paths, name));

        // History depth — informational; absent/disabled history is not a fault.
        if let Ok(commits) = crate::core::git_history::log(paths, name) {
            checks.push(Check {
                id: "history",
                title: "History".into(),
                detail: format!("{} commit(s) on profile/{name}", commits.len()),
                severity: Severity::Info,
                fix: None,
            });
        }
    }

    // 6. Backup health — flag zero-byte archives.
    checks.push(backup_check(paths)?);

    // 7. Crash-recovery leftover.
    let old_dir = paths.claude_old();
    if old_dir.exists() {
        checks.push(Check {
            id: "crash-leftover",
            title: "Crash leftover".into(),
            detail: format!("{} exists — a swap may have crashed", old_dir.display()),
            severity: Severity::Warning,
            fix: Some(FixAction::ClearCrashLeftover { dir: old_dir }),
        });
    }

    // 8. Legacy ~/.portal root.
    for (name, dir) in legacy_profiles(paths) {
        checks.push(Check {
            id: "legacy-root",
            title: "Legacy root".into(),
            detail: format!(
                "~/.portal profile \"{name}\" — migrate into the active root or delete"
            ),
            severity: Severity::Warning,
            fix: Some(FixAction::MigrateLegacyRoot { name, dir }),
        });
    }

    Ok(DoctorReport {
        checks,
        managed_dirs,
        excluded_patterns: snapshot::EXCLUDED_PATTERNS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    })
}

/// Apply one repair. Returns a human-readable summary of what changed.
///
/// # Errors
///
/// Returns an error if the underlying filesystem/CAS operation fails, or if a
/// migration target name collides with an existing profile.
pub fn apply_fix(paths: &PortalPaths, action: &FixAction) -> Result<String> {
    match action {
        FixAction::MigrateLegacyRoot { name, dir } => migrate_legacy(paths, name, dir),
        FixAction::DeleteLegacyRoot { dir } => {
            std::fs::remove_dir_all(dir)
                .with_context(|| format!("removing legacy dir: {}", dir.display()))?;
            Ok(format!("deleted {}", dir.display()))
        }
        FixAction::PruneZeroByteBackups { paths: files } => {
            let mut removed = 0;
            for f in files {
                std::fs::remove_file(f)
                    .with_context(|| format!("removing backup: {}", f.display()))?;
                removed += 1;
            }
            Ok(format!("removed {removed} zero-byte backup(s)"))
        }
        FixAction::RecreateSkeleton => {
            skeleton::create(&paths.claude_root())?;
            Ok("recreated missing skeleton dirs/files".into())
        }
        FixAction::ClearCrashLeftover { dir } => {
            std::fs::remove_dir_all(dir)
                .with_context(|| format!("removing crash leftover: {}", dir.display()))?;
            Ok(format!("removed {}", dir.display()))
        }
    }
}

// ── internals ────────────────────────────────────────────────────────

/// Skeleton completeness check (reuses [`skeleton::verify`]).
fn skeleton_check(claude: &Path) -> Check {
    match skeleton::verify(claude) {
        Ok(issues) if issues.is_empty() => Check {
            id: "skeleton",
            title: "Skeleton".into(),
            detail: "all required dirs and files present".into(),
            severity: Severity::Ok,
            fix: None,
        },
        Ok(issues) => Check {
            id: "skeleton",
            title: "Skeleton".into(),
            detail: format!(
                "{} missing item(s): {}",
                issues.len(),
                describe_issues(&issues)
            ),
            severity: Severity::Warning,
            fix: Some(FixAction::RecreateSkeleton),
        },
        Err(e) => Check {
            id: "skeleton",
            title: "Skeleton".into(),
            detail: format!("could not inspect ~/.claude: {e}"),
            severity: Severity::Warning,
            fix: None,
        },
    }
}

/// Backup-health check — flags zero-byte (failed) archives.
fn backup_check(paths: &PortalPaths) -> Result<Check> {
    let backups = backup::list(paths)?;
    let zero: Vec<PathBuf> = backups
        .iter()
        .filter(|b| b.size == 0)
        .map(|b| b.path.clone())
        .collect();
    Ok(if zero.is_empty() {
        Check {
            id: "backups",
            title: "Backups".into(),
            detail: format!("{} archive(s), none empty", backups.len()),
            severity: Severity::Ok,
            fix: None,
        }
    } else {
        Check {
            id: "backups",
            title: "Backups".into(),
            detail: format!("{} zero-byte archive(s) — failed backups", zero.len()),
            severity: Severity::Warning,
            fix: Some(FixAction::PruneZeroByteBackups { paths: zero }),
        }
    })
}

/// Managed directories, paired with the clone category they belong to. Built
/// from the same source-of-truth lists the loader/snapshot use, so this table
/// can never drift from what portal actually tracks.
fn build_managed_dirs(claude: &Path) -> Vec<ManagedDirRow> {
    skeleton::SKELETON_DIRS
        .iter()
        .map(|dir| {
            let path = claude.join(dir);
            ManagedDirRow {
                category: category_label(dir),
                dir: (*dir).to_string(),
                exists: path.is_dir(),
                file_count: count_files(&path),
            }
        })
        .collect()
}

/// Map a managed directory to its clone [`Category`](clone::Category) label.
fn category_label(dir: &str) -> String {
    // `categorize_file` keys off path prefixes; feed it a representative path.
    let probe = format!("{}/x", dir.trim_start_matches(".claude/"));
    format!("{:?}", clone::categorize_file(&probe)).to_lowercase()
}

fn count_files(dir: &Path) -> usize {
    if !dir.is_dir() {
        return 0;
    }
    walkdir::WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .count()
}

fn describe_issues(issues: &[skeleton::SkeletonIssue]) -> String {
    issues
        .iter()
        .map(|i| match i {
            skeleton::SkeletonIssue::MissingFile(f) => f.clone(),
            skeleton::SkeletonIssue::MissingDir(d) => format!("{d}/"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Verify the active profile's manifest. Uses CAS object presence when the
/// profile has been migrated (no `files/` tree), otherwise re-hashes the
/// legacy tree. This avoids the false "all files differ" `cmd_status` reports
/// for CAS-mode profiles.
fn active_profile_integrity(paths: &PortalPaths, name: &str) -> Check {
    let mpath = paths.profile_manifest(name);
    if !mpath.exists() {
        return Check {
            id: "integrity",
            title: "Active integrity".into(),
            detail: format!("manifest for \"{name}\" not found"),
            severity: Severity::Error,
            fix: None,
        };
    }
    let m = match manifest::read(&mpath) {
        Ok(m) => m,
        Err(e) => {
            return Check {
                id: "integrity",
                title: "Active integrity".into(),
                detail: format!("could not read manifest: {e}"),
                severity: Severity::Error,
                fix: None,
            };
        }
    };

    let total = m.files.len();
    let files_dir = paths.profile_files_dir(name);
    let missing = if files_dir.is_dir() {
        // Legacy tree — re-hash on disk; an unreadable tree counts as all-failing.
        crate::core::checksum::verify_manifest(&files_dir, &m.files)
            .map_or(total, |mismatches| mismatches.len())
    } else {
        // CAS-mode — check object presence.
        m.files
            .values()
            .filter(|e| !cas::exists(paths, &e.checksum))
            .count()
    };

    if missing == 0 {
        Check {
            id: "integrity",
            title: "Active integrity".into(),
            detail: format!("all {total} files verified"),
            severity: Severity::Ok,
            fix: None,
        }
    } else {
        Check {
            id: "integrity",
            title: "Active integrity".into(),
            detail: format!("{missing}/{total} file(s) missing or corrupt"),
            severity: Severity::Error,
            fix: None,
        }
    }
}

/// List legacy `~/.portal/profiles/<name>` directories that hold a manifest.
fn legacy_profiles(paths: &PortalPaths) -> Vec<(String, PathBuf)> {
    let profiles = paths.legacy_root().join("profiles");
    let Ok(rd) = std::fs::read_dir(&profiles) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.filter_map(std::result::Result::ok) {
        let dir = entry.path();
        if dir.is_dir()
            && dir.join("portal.json").is_file()
            && let Some(name) = dir.file_name().and_then(|n| n.to_str())
        {
            out.push((name.to_string(), dir.clone()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Import a legacy `files/`-tree profile into the active root via CAS.
fn migrate_legacy(paths: &PortalPaths, name: &str, dir: &Path) -> Result<String> {
    let target = paths.profile_dir(name);
    if target.exists() {
        anyhow::bail!(
            "a profile named \"{name}\" already exists in the active root; rename or remove it first"
        );
    }

    let legacy_manifest = manifest::read(&dir.join("portal.json"))
        .with_context(|| format!("reading legacy manifest: {}", dir.display()))?;

    // Pull the legacy files/ tree into the shared CAS pool.
    let legacy_files = dir.join("files");
    let migrated = cas::migrate_profile_files(paths, &legacy_files, &legacy_manifest.files)
        .context("migrating legacy files into CAS")?;

    // Write the manifest (and optional sidecars) into the active root.
    std::fs::create_dir_all(&target)
        .with_context(|| format!("creating profile dir: {}", target.display()))?;
    manifest::write(&paths.profile_manifest(name), &legacy_manifest)?;

    let legacy_plugins = dir.join("plugins.json");
    if legacy_plugins.is_file()
        && let Ok(bp) = plugins_manifest::read(&legacy_plugins)
    {
        plugins_manifest::write(&paths.profile_plugins(name), &bp)?;
    }
    let legacy_meta = dir.join("meta.json");
    if legacy_meta.is_file()
        && let Ok(meta) = meta::read(&legacy_meta)
    {
        meta::write(&paths.profile_meta(name), &meta)?;
    }

    // Remove what's left of the legacy profile (files/ already consumed).
    let _ = std::fs::remove_dir_all(dir);

    Ok(format!(
        "migrated \"{name}\" ({migrated} object(s) into CAS)"
    ))
}
