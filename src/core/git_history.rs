//! Per-profile git history (additive layer).
//!
//! Each profile gets an **orphan branch** `profile/<name>` in a single git repo
//! under `~/.config/portal/history/`. Saving a profile commits its file set to
//! that branch, giving independent per-profile history and diff.
//!
//! This is deliberately *not* the load mechanism: the CAS + atomic-swap engine
//! remains the sole authority over `~/.claude`. Git here records history and
//! never drives the live config. Every entry point is best-effort — a git
//! failure logs a warning and is swallowed so it can never block a save/load.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::core::profile::ProfileManifest;
use crate::storage::{cas, paths::PortalPaths};

/// One commit on a profile's history branch.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub timestamp: String,
    pub summary: String,
}

/// Branch name for a profile's history.
fn branch_for(profile: &str) -> String {
    format!("profile/{profile}")
}

/// Whether history recording is enabled (config flag, default on).
#[must_use]
pub fn enabled(paths: &PortalPaths) -> bool {
    crate::config::load(&paths.config_file()).map_or(true, |c| c.history.enabled)
}

/// Record a snapshot, swallowing (but logging) any failure. Use this at hook
/// points so git problems never break a save/load.
pub fn record_snapshot_best_effort(paths: &PortalPaths, profile: &str, manifest: &ProfileManifest) {
    if !enabled(paths) {
        return;
    }
    if let Err(e) = record_snapshot(paths, profile, manifest) {
        tracing::warn!("git history record for \"{profile}\" failed (non-fatal): {e:#}");
    }
}

/// Initialize the history repo if it doesn't exist yet. Sets local identity so
/// commits never depend on the user's global git config (important in CI).
///
/// # Errors
///
/// Returns an error if `git init`/`git config` fails.
pub fn ensure_repo(paths: &PortalPaths) -> Result<()> {
    let dir = paths.history_dir();
    if dir.join(".git").is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating history dir: {}", dir.display()))?;
    git(&dir, &["init", "--quiet"])?;
    git(&dir, &["config", "user.name", "portal"])?;
    git(&dir, &["config", "user.email", "portal@localhost"])?;
    git(&dir, &["config", "commit.gpgsign", "false"])?;
    Ok(())
}

/// Commit the profile's file set to its orphan branch. Skips committing when
/// nothing changed since the last snapshot.
///
/// # Errors
///
/// Returns an error if any git invocation or CAS placement fails.
pub fn record_snapshot(
    paths: &PortalPaths,
    profile: &str,
    manifest: &ProfileManifest,
) -> Result<()> {
    ensure_repo(paths)?;
    let dir = paths.history_dir();
    let branch = branch_for(profile);

    // Point HEAD at the profile's branch (create as an orphan if it's new).
    if branch_exists(&dir, &branch)? {
        git(&dir, &["checkout", "--quiet", &branch])?;
    } else {
        git(&dir, &["switch", "--quiet", "--orphan", &branch])?;
    }

    // Rebuild the working tree from CAS so it exactly mirrors the manifest,
    // regardless of whatever branch we were just on.
    clear_worktree(&dir)?;
    for (rel, entry) in &manifest.files {
        let dest = dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating history parent dir for {rel}"))?;
        }
        cas::place(paths, &entry.checksum, &dest)
            .with_context(|| format!("placing {rel} into history tree"))?;
    }

    git(&dir, &["add", "-A"])?;

    // Nothing staged → identical to last snapshot; don't create an empty commit.
    if git(&dir, &["status", "--porcelain"])?.trim().is_empty() {
        return Ok(());
    }

    let msg = format!("{profile}: snapshot ({} files)", manifest.files.len());
    git(&dir, &["commit", "--quiet", "--no-gpg-sign", "-m", &msg])?;
    Ok(())
}

/// Return the commit history for a profile's branch, newest first. An absent
/// branch yields an empty list rather than an error.
///
/// # Errors
///
/// Returns an error if the repo cannot be read.
pub fn log(paths: &PortalPaths, profile: &str) -> Result<Vec<CommitInfo>> {
    let dir = paths.history_dir();
    if !dir.join(".git").is_dir() {
        return Ok(Vec::new());
    }
    let branch = branch_for(profile);
    if !branch_exists(&dir, &branch)? {
        return Ok(Vec::new());
    }
    // Unit separator (\x1f) between fields — safe against spaces in summaries.
    let out = git(&dir, &["log", &branch, "--pretty=format:%H\x1f%cI\x1f%s"])?;
    let commits = out
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\u{1f}');
            Some(CommitInfo {
                hash: parts.next()?.to_string(),
                timestamp: parts.next()?.to_string(),
                summary: parts.next().unwrap_or("").to_string(),
            })
        })
        .collect();
    Ok(commits)
}

/// Diff a profile's branch against an arbitrary revision (e.g. `HEAD~1`).
///
/// # Errors
///
/// Returns an error if the repo or revision cannot be read.
pub fn diff(paths: &PortalPaths, profile: &str, rev: &str) -> Result<String> {
    let dir = paths.history_dir();
    git(&dir, &["diff", rev, &branch_for(profile)])
}

// ── internals ────────────────────────────────────────────────────────

fn branch_exists(dir: &Path, branch: &str) -> Result<bool> {
    let refspec = format!("refs/heads/{branch}");
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--verify", "--quiet", &refspec])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("git rev-parse in {}", dir.display()))?;
    Ok(output.success())
}

/// Remove everything in the working tree except `.git`.
fn clear_worktree(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading history dir: {}", dir.display()))?
        .filter_map(std::result::Result::ok)
    {
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("clearing {}", path.display()))?;
        } else {
            std::fs::remove_file(&path).with_context(|| format!("clearing {}", path.display()))?;
        }
    }
    Ok(())
}

/// Run a git command in `dir`, capturing stdio. Returns stdout on success.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
}
