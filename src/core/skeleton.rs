use anyhow::{Context, Result};
use std::path::Path;

/// Default content for `settings.json` in the Claude skeleton.
pub const DEFAULT_SETTINGS: &str = "{}";

/// All directories that must exist in a valid Claude skeleton.
pub const SKELETON_DIRS: &[&str] = &[
    ".claude/hooks",
    "skills",
    "memory",
    "commands",
    "agents",
    "rules",
    "hooks",
];

/// All files that must exist in a valid Claude skeleton.
pub const SKELETON_FILES: &[&str] = &["settings.json", "CLAUDE.md", ".claude/settings.local.json"];

/// A problem found during skeleton verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkeletonIssue {
    /// A required file is missing.
    MissingFile(String),
    /// A required directory is missing.
    MissingDir(String),
}

/// Create the full Claude skeleton directory structure.
///
/// Builds every required directory and seeds `settings.json`,
/// `CLAUDE.md`, and `.claude/settings.local.json` with defaults.
///
/// # Errors
///
/// Returns an error if directory or file creation fails
/// (permissions, disk full, etc.).
pub fn create(claude_dir: &Path) -> Result<()> {
    for dir in SKELETON_DIRS {
        std::fs::create_dir_all(claude_dir.join(dir))
            .with_context(|| format!("creating skeleton dir: {dir}"))?;
    }

    // .claude/ inner dir (parent of settings.local.json) is created by
    // the .claude/hooks entry above, but be explicit.
    std::fs::create_dir_all(claude_dir.join(".claude")).context("creating .claude inner dir")?;

    // settings.json
    std::fs::write(claude_dir.join("settings.json"), DEFAULT_SETTINGS)
        .context("writing settings.json")?;

    // CLAUDE.md — empty
    std::fs::write(claude_dir.join("CLAUDE.md"), "").context("writing CLAUDE.md")?;

    // .claude/settings.local.json — empty JSON object
    std::fs::write(claude_dir.join(".claude/settings.local.json"), "{}")
        .context("writing settings.local.json")?;

    Ok(())
}

/// Verify that a directory has all skeleton components.
///
/// Returns a list of issues found. An empty list means the skeleton
/// is complete.
///
/// # Errors
///
/// Returns an error if the base directory itself cannot be inspected.
pub fn verify(claude_dir: &Path) -> Result<Vec<SkeletonIssue>> {
    let mut issues = Vec::new();

    for dir in SKELETON_DIRS {
        let path = claude_dir.join(dir);
        if !path.is_dir() {
            issues.push(SkeletonIssue::MissingDir((*dir).to_string()));
        }
    }

    for file in SKELETON_FILES {
        let path = claude_dir.join(file);
        if !path.is_file() {
            issues.push(SkeletonIssue::MissingFile((*file).to_string()));
        }
    }

    Ok(issues)
}
