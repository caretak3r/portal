use crate::core::checksum;
use crate::core::plugins;
use crate::core::profile::{FileEntry, FileSource, ProfileManifest, ProfileMeta};
use crate::storage::{manifest, meta, paths::PortalPaths, plugins_manifest};
use anyhow::{bail, Context, Result};
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

/// Check whether a relative path matches any exclusion pattern.
#[must_use]
pub fn is_excluded(rel_path: &str) -> bool {
    EXCLUDED_PATTERNS
        .iter()
        .any(|pat| rel_path == *pat || rel_path.starts_with(&format!("{pat}/")))
}

/// Walk `claude_dir`, filter out excluded paths, return sorted trackable files.
///
/// # Errors
///
/// Returns an error if the directory walk encounters I/O errors.
pub fn scan_trackable_files(claude_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(claude_dir).min_depth(1) {
        let entry = entry.with_context(|| {
            format!("walking directory: {}", claude_dir.display())
        })?;

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
/// Scans trackable files, copies them into the profile directory,
/// computes checksums, writes the manifest, plugin blueprint, and metadata.
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
    let claude_dir = paths.claude_root();
    if !claude_dir.is_dir() {
        bail!(
            ".claude/ directory not found at {}",
            claude_dir.display()
        );
    }

    // Create profile directories.
    let files_dir = paths.profile_files_dir(name);
    std::fs::create_dir_all(&files_dir)
        .with_context(|| format!("creating profile files dir: {}", files_dir.display()))?;

    // Scan trackable files.
    let trackable = scan_trackable_files(&claude_dir)?;

    // Copy files, compute checksums.
    let mut entries: HashMap<String, FileEntry> = HashMap::new();

    for rel in &trackable {
        let src = claude_dir.join(rel);
        let dst = files_dir.join(rel);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir: {}", parent.display()))?;
        }

        std::fs::copy(&src, &dst)
            .with_context(|| format!("copying file: {}", rel.display()))?;

        let hash = checksum::sha256_file(&dst)?;
        let meta = std::fs::metadata(&dst)
            .with_context(|| format!("reading metadata: {}", dst.display()))?;

        let content = std::fs::read(&dst)
            .with_context(|| format!("reading file for classification: {}", dst.display()))?;

        let rel_str = rel.to_string_lossy().to_string();
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

    let manifest = ProfileManifest {
        version: 1,
        name: name.to_string(),
        created_at: Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: description.to_string(),
        tags: tags.to_vec(),
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
        description: description.to_string(),
        tags: tags.to_vec(),
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
