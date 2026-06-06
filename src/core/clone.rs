use anyhow::{Result, bail};
use chrono::Utc;
use std::collections::{HashMap, HashSet};

use crate::core::profile::{FileEntry, FileSource, ProfileManifest, ProfileMeta};
use crate::core::progress::ProgressReporter;
use crate::storage::{cas, manifest, meta, paths::PortalPaths, plugins_manifest};

/// Categories of files that can be selectively included in a clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    ClaudeMd,
    Settings,
    Skills,
    Rules,
    Memory,
    Commands,
    Agents,
    Hooks,
    Plugins,
}

impl Category {
    /// Parse a category name from user input.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "claude-md" | "claudemd" | "claude_md" => Some(Self::ClaudeMd),
            "settings" => Some(Self::Settings),
            "skills" => Some(Self::Skills),
            "rules" => Some(Self::Rules),
            "memory" => Some(Self::Memory),
            "commands" | "cmds" => Some(Self::Commands),
            "agents" => Some(Self::Agents),
            "hooks" => Some(Self::Hooks),
            "plugins" => Some(Self::Plugins),
            _ => None,
        }
    }

    /// All available categories.
    #[must_use]
    pub fn all() -> Vec<Self> {
        vec![
            Self::ClaudeMd,
            Self::Settings,
            Self::Skills,
            Self::Rules,
            Self::Memory,
            Self::Commands,
            Self::Agents,
            Self::Hooks,
            Self::Plugins,
        ]
    }
}

/// Parse a comma-separated list of category names.
///
/// # Errors
///
/// Returns an error if any category name is unrecognized.
pub fn parse_categories(input: &str) -> Result<Vec<Category>> {
    let mut cats = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match Category::parse(part) {
            Some(c) => cats.push(c),
            None => bail!(
                "Unknown category \"{part}\". Valid: claude-md, settings, skills, rules, memory, commands, agents, hooks, plugins"
            ),
        }
    }
    Ok(cats)
}

/// Determine which category a file path belongs to.
pub fn categorize_file(rel_path: &str) -> Category {
    if rel_path == "CLAUDE.md" {
        Category::ClaudeMd
    } else if rel_path == "settings.json" || rel_path.starts_with(".claude/settings") {
        Category::Settings
    } else if rel_path.starts_with("skills/") {
        Category::Skills
    } else if rel_path.starts_with("rules/") {
        Category::Rules
    } else if rel_path.starts_with("memory/") {
        Category::Memory
    } else if rel_path.starts_with("commands/") {
        Category::Commands
    } else if rel_path.starts_with("agents/") {
        Category::Agents
    } else if rel_path.starts_with("hooks/") || rel_path.starts_with(".claude/hooks/") {
        Category::Hooks
    } else {
        // Uncategorized files go with settings as a catch-all.
        Category::Settings
    }
}

/// Options for a clone operation.
pub struct CloneOptions<'a> {
    pub source: &'a str,
    pub target: &'a str,
    pub description: &'a str,
    pub only: Option<Vec<Category>>,
    pub without: Option<Vec<Category>>,
    pub fresh_claude_md: bool,
    /// Per-category file filter for fine-grained selection.
    /// If a category key is present with a non-empty set, only those specific
    /// relative paths are copied. Missing key or empty set → all files in category.
    pub file_picks: Option<HashMap<Category, HashSet<String>>>,
}

/// Result of a clone operation.
#[derive(Debug)]
pub struct CloneResult {
    pub source: String,
    pub target: String,
    pub files_cloned: usize,
    pub files_skipped: usize,
    pub plugins_included: bool,
    pub categories_included: Vec<String>,
}

/// Clone a profile, selectively copying file categories.
///
/// Convenience wrapper around [`clone_profile_with_progress`] with a no-op reporter.
///
/// # Errors
///
/// Returns an error if the source profile doesn't exist, the target
/// already exists, or file operations fail.
pub fn clone_profile(paths: &PortalPaths, opts: &CloneOptions<'_>) -> Result<CloneResult> {
    clone_profile_with_progress(paths, opts, &super::progress::NoProgress)
}

/// Clone a profile with progress reporting.
///
/// # Errors
///
/// Returns an error if the source profile doesn't exist, the target
/// already exists, or file operations fail.
#[allow(clippy::too_many_lines)]
pub fn clone_profile_with_progress(
    paths: &PortalPaths,
    opts: &CloneOptions<'_>,
    progress: &dyn ProgressReporter,
) -> Result<CloneResult> {
    let source_dir = paths.profile_dir(opts.source);
    if !source_dir.exists() {
        bail!("Source profile \"{}\" not found.", opts.source);
    }

    let target_dir = paths.profile_dir(opts.target);
    if target_dir.exists() {
        bail!(
            "Target profile \"{}\" already exists. Delete it first or choose a different name.",
            opts.target
        );
    }

    // Read source manifest.
    let source_manifest = manifest::read(&paths.profile_manifest(opts.source))?;
    let source_files_dir = paths.profile_files_dir(opts.source);

    // Determine which categories to include.
    let included: Vec<Category> = match (&opts.only, &opts.without) {
        (Some(only), _) => only.clone(),
        (_, Some(without)) => Category::all()
            .into_iter()
            .filter(|c| !without.contains(c))
            .collect(),
        _ => Category::all(),
    };

    let include_plugins = included.contains(&Category::Plugins);

    // CAS-aware clone: filter the source manifest by category and reuse the
    // same content hashes in the target manifest. No bytes are copied — both
    // profiles point at the same CAS objects.
    std::fs::create_dir_all(paths.profile_dir(opts.target))?;

    let mut cloned_entries: HashMap<String, FileEntry> = HashMap::new();
    let mut skipped = 0usize;
    let mut processed: u64 = 0;
    let _ = source_files_dir; // legacy path kept above for migration; CAS clone doesn't read it

    progress.set_total(source_manifest.files.len() as u64);

    for (rel_path, entry) in &source_manifest.files {
        processed += 1;
        progress.tick(processed, rel_path);
        let cat = categorize_file(rel_path);

        if opts.fresh_claude_md && cat == Category::ClaudeMd {
            skipped += 1;
            continue;
        }

        if !included.contains(&cat) {
            skipped += 1;
            continue;
        }

        // Per-file filter: if the caller specified picks for this category,
        // only include files in the allowed set (empty set = include all).
        if opts
            .file_picks
            .as_ref()
            .and_then(|p| p.get(&cat))
            .is_some_and(|allowed| {
                !allowed.is_empty() && !allowed.contains(rel_path.as_str())
            })
        {
            skipped += 1;
            continue;
        }

        // The source manifest already records the content hash. As long as the
        // CAS object exists, the target profile can reference it directly.
        if cas::exists(paths, &entry.checksum) {
            cloned_entries.insert(rel_path.clone(), entry.clone());
        } else {
            // Source still uses the legacy `files/` layout. Migrate the byte
            // stream into CAS so the cloned profile can reference it too.
            let src = source_files_dir.join(rel_path);
            if src.is_file() {
                let bytes = std::fs::read(&src)?;
                let hash = cas::write(paths, &bytes)?;
                let mut e = entry.clone();
                e.checksum = hash;
                cloned_entries.insert(rel_path.clone(), e);
            } else {
                skipped += 1;
            }
        }
    }

    // If fresh-claude-md, write an empty file into the CAS pool and reference it.
    if opts.fresh_claude_md {
        let hash = cas::write(paths, b"")?;
        cloned_entries.insert(
            "CLAUDE.md".to_string(),
            FileEntry {
                checksum: hash,
                size: 0,
                source: FileSource::Skeleton,
                mode: None,
            },
        );
    }

    // Write manifest.
    let target_manifest = ProfileManifest {
        version: 1,
        name: opts.target.to_string(),
        created_at: Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: opts.description.to_string(),
        tags: Vec::new(),
        files: cloned_entries.clone(),
        excluded_patterns: source_manifest.excluded_patterns,
    };
    manifest::write(&paths.profile_manifest(opts.target), &target_manifest)?;

    // Handle plugins.
    if include_plugins {
        let source_plugins = paths.profile_plugins(opts.source);
        if source_plugins.exists() {
            let bp = plugins_manifest::read(&source_plugins)?;
            plugins_manifest::write(&paths.profile_plugins(opts.target), &bp)?;
        }
    }

    // Write metadata.
    let profile_meta = ProfileMeta {
        description: opts.description.to_string(),
        tags: Vec::new(),
        notes: Some(format!("Cloned from \"{}\"", opts.source)),
        created_by: std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string()),
    };
    meta::write(&paths.profile_meta(opts.target), &profile_meta)?;

    let category_names: Vec<String> = included
        .iter()
        .filter(|c| **c != Category::Plugins)
        .map(|c| format!("{c:?}").to_lowercase())
        .collect();

    Ok(CloneResult {
        source: opts.source.to_string(),
        target: opts.target.to_string(),
        files_cloned: cloned_entries.len(),
        files_skipped: skipped,
        plugins_included: include_plugins,
        categories_included: category_names,
    })
}
