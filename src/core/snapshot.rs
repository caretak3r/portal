use crate::core::checksum;
use crate::core::plugins;
use crate::core::profile::{FileEntry, FileSource, ProfileManifest, ProfileMeta};
use crate::core::progress::ProgressReporter;
use crate::storage::{manifest, meta, paths::PortalPaths, plugins_manifest};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Paths that are never saved into a profile snapshot.
pub const EXCLUDED_PATTERNS: &[&str] = &[
    "session-env",
    "sessions",
    "shell-snapshots",
    "history.jsonl",
    "todos",
    "file-history",
    "telemetry",
    "statsig",
    "paste-cache",
    "debug",
    "stats-cache.json",
    "mcp-needs-auth-cache.json",
    "plans",
    "projects",
    "repositories",
    "plugins/cache",
    "plugins/marketplaces",
    "plugins/data",
    "plugins/blocklist.json",
    "plugins/install-counts-cache.json",
    "plugins/known_marketplaces.json",
    ".DS_Store",
];

/// Path segments that indicate a nested `.git/` directory (inside plugins,
/// skills, etc.) and should always be excluded regardless of depth.
const EXCLUDED_SEGMENTS: &[&str] = &[".git", "node_modules", "__pycache__", ".venv"];

/// Check whether a relative path matches any exclusion pattern.
#[must_use]
pub fn is_excluded(rel_path: &str) -> bool {
    // Check prefix-based patterns (top-level exclusions).
    if EXCLUDED_PATTERNS
        .iter()
        .any(|pat| rel_path == *pat || rel_path.starts_with(&format!("{pat}/")))
    {
        return true;
    }
    // Check segment-based patterns (`.git/` at any depth).
    rel_path
        .split('/')
        .any(|seg| EXCLUDED_SEGMENTS.contains(&seg))
}

/// Walk `claude_dir`, filter out excluded paths, return sorted trackable files.
///
/// # Errors
///
/// Returns an error if the directory walk encounters I/O errors.
pub fn scan_trackable_files(claude_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(claude_dir).min_depth(1) {
        let entry =
            entry.with_context(|| format!("walking directory: {}", claude_dir.display()))?;

        if !entry.file_type().is_file() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(claude_dir)
            .with_context(|| "stripping prefix")?;

        let rel_str = rel.to_string_lossy();
        if !is_excluded(&rel_str) {
            files.push(rel.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

/// Determine the file source based on skeleton defaults.
fn classify_source(rel_path: &str, content: &[u8]) -> FileSource {
    match rel_path {
        "settings.json" | ".claude/settings.local.json" => {
            // If the file still has default content, it's skeleton-sourced.
            let trimmed = String::from_utf8_lossy(content);
            let trimmed = trimmed.trim();
            if trimmed == "{}" || trimmed.is_empty() {
                FileSource::Skeleton
            } else {
                FileSource::User
            }
        }
        "CLAUDE.md" => {
            if content.is_empty() {
                FileSource::Skeleton
            } else {
                FileSource::User
            }
        }
        _ => FileSource::User,
    }
}

/// Snapshot the current `~/.claude/` into a named profile.
///
/// Convenience wrapper around [`save_with_progress`] that uses a no-op reporter.
///
/// # Errors
///
/// Returns an error if the `.claude/` directory doesn't exist, or if
/// any file copy, checksum, or manifest write fails.
pub fn save(
    paths: &PortalPaths,
    name: &str,
    description: &str,
    tags: &[String],
) -> Result<ProfileManifest> {
    save_with_progress(paths, name, description, tags, &super::progress::NoProgress)
}

/// Snapshot the current `~/.claude/` into a named profile with progress reporting.
///
/// # Errors
///
/// Returns an error if the `.claude/` directory doesn't exist, or if
/// any file copy, checksum, or manifest write fails.
pub fn save_with_progress(
    paths: &PortalPaths,
    name: &str,
    description: &str,
    tags: &[String],
    progress: &dyn ProgressReporter,
) -> Result<ProfileManifest> {
    let claude_dir = paths.claude_root();
    if !claude_dir.is_dir() {
        bail!(".claude/ directory not found at {}", claude_dir.display());
    }

    // If overwriting an existing profile, read its manifest so we can preserve
    // metadata (created_at, load_count, last_loaded) and any description/tags
    // the caller didn't explicitly override.
    let manifest_path = paths.profile_manifest(name);
    let existing = if manifest_path.exists() {
        manifest::read(&manifest_path).ok()
    } else {
        None
    };

    // Clear stale files so an overwrite removes orphans from the previous snapshot.
    let files_dir = paths.profile_files_dir(name);
    if files_dir.exists() {
        std::fs::remove_dir_all(&files_dir)
            .with_context(|| format!("clearing old files dir: {}", files_dir.display()))?;
    }
    std::fs::create_dir_all(&files_dir)
        .with_context(|| format!("creating profile files dir: {}", files_dir.display()))?;

    // Scan trackable files.
    let trackable = scan_trackable_files(&claude_dir)?;

    progress.set_total(trackable.len() as u64);

    // Copy files, compute checksums.
    let mut entries: HashMap<String, FileEntry> = HashMap::new();

    for (i, rel) in trackable.iter().enumerate() {
        let src = claude_dir.join(rel);
        let dst = files_dir.join(rel);

        let rel_str = rel.to_string_lossy().to_string();
        progress.tick(i as u64 + 1, &rel_str);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir: {}", parent.display()))?;
        }

        std::fs::copy(&src, &dst).with_context(|| format!("copying file: {}", rel.display()))?;

        let hash = checksum::sha256_file(&dst)?;
        let meta = std::fs::metadata(&dst)
            .with_context(|| format!("reading metadata: {}", dst.display()))?;

        let content = std::fs::read(&dst)
            .with_context(|| format!("reading file for classification: {}", dst.display()))?;

        let source = classify_source(&rel_str, &content);

        entries.insert(
            rel_str,
            FileEntry {
                checksum: hash,
                size: meta.len(),
                source,
            },
        );
    }

    // When overwriting, preserve historical metadata and any description/tags
    // the caller left empty. This makes `save` behave like a "save game":
    // re-snapshotting the active profile keeps its identity.
    let (created_at, load_count, last_loaded, final_description, final_tags) = match existing {
        Some(old) => (
            old.created_at,
            old.load_count,
            old.last_loaded,
            if description.is_empty() {
                old.description
            } else {
                description.to_string()
            },
            if tags.is_empty() {
                old.tags
            } else {
                tags.to_vec()
            },
        ),
        None => (Utc::now(), 0, None, description.to_string(), tags.to_vec()),
    };

    let manifest = ProfileManifest {
        version: 1,
        name: name.to_string(),
        created_at,
        last_loaded,
        load_count,
        description: final_description.clone(),
        tags: final_tags.clone(),
        files: entries,
        excluded_patterns: EXCLUDED_PATTERNS.iter().map(|s| (*s).to_string()).collect(),
    };

    // Write manifest.
    manifest::write(&paths.profile_manifest(name), &manifest)?;

    // Extract and write plugin blueprint.
    let blueprint = plugins::extract_blueprint(&claude_dir).unwrap_or_default();
    plugins_manifest::write(&paths.profile_plugins(name), &blueprint)?;

    // Write metadata.
    let profile_meta = ProfileMeta {
        description: final_description,
        tags: final_tags,
        notes: None,
        created_by: whoami(),
    };
    meta::write(&paths.profile_meta(name), &profile_meta)?;

    Ok(manifest)
}

/// Best-effort username detection.
fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
