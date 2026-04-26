use crate::storage::{cas, manifest, paths::PortalPaths};
use anyhow::{Result, bail};
use std::collections::HashMap;

/// Which side of a diff comparison to resolve.
#[derive(Debug, Clone)]
pub enum DiffSide<'a> {
    /// A named saved profile.
    Profile(&'a str),
    /// The bare skeleton (no profile-specific files).
    Skeleton,
}

/// Summary of differences between two profile snapshots.
#[derive(Debug)]
pub struct DiffResult {
    /// Display name for the left side.
    pub left_name: String,
    /// Display name for the right side.
    pub right_name: String,
    /// Relative paths present in both sides with identical checksums.
    pub shared_same: Vec<String>,
    /// Relative paths present in both sides with differing checksums.
    pub different_content: Vec<FileDiff>,
    /// Relative paths only in the left side.
    pub only_left: Vec<String>,
    /// Relative paths only in the right side.
    pub only_right: Vec<String>,
}

/// A single file that exists in both sides but differs.
#[derive(Debug)]
pub struct FileDiff {
    /// Relative path within the profile.
    pub path: String,
    /// Size in bytes on the left side.
    pub left_size: u64,
    /// Size in bytes on the right side.
    pub right_size: u64,
}

/// Level 4 placeholder — plugin-level diff (future).
#[derive(Debug)]
pub struct PluginDiff {
    /// Plugins only in the left side.
    pub only_left: Vec<String>,
    /// Plugins only in the right side.
    pub only_right: Vec<String>,
    /// Plugins present in both but with different configuration.
    pub changed: Vec<String>,
}

/// Compare two profiles at manifest level (Level 1) and directory level (Level 2).
///
/// Produces a `DiffResult` showing which files are shared-same,
/// different-content, left-only, or right-only.
///
/// For `DiffSide::Skeleton`, an empty file map is used (the skeleton
/// has no profile-specific files to compare).
///
/// # Errors
///
/// Returns an error if a referenced profile does not exist or its
/// manifest cannot be read.
pub fn diff_profiles(
    paths: &PortalPaths,
    left: &DiffSide<'_>,
    right: &DiffSide<'_>,
) -> Result<DiffResult> {
    let (left_name, left_files) = resolve_side(paths, left)?;
    let (right_name, right_files) = resolve_side(paths, right)?;

    let mut shared_same = Vec::new();
    let mut different_content = Vec::new();
    let mut only_left = Vec::new();

    for (path, left_entry) in &left_files {
        if let Some(right_entry) = right_files.get(path) {
            if left_entry.0 == right_entry.0 {
                shared_same.push(path.clone());
            } else {
                different_content.push(FileDiff {
                    path: path.clone(),
                    left_size: left_entry.1,
                    right_size: right_entry.1,
                });
            }
        } else {
            only_left.push(path.clone());
        }
    }

    let only_right: Vec<String> = right_files
        .keys()
        .filter(|k| !left_files.contains_key(*k))
        .cloned()
        .collect();

    // Sort everything for deterministic output.
    shared_same.sort();
    different_content.sort_by(|a, b| a.path.cmp(&b.path));
    only_left.sort();
    let mut only_right = only_right;
    only_right.sort();

    Ok(DiffResult {
        left_name,
        right_name,
        shared_same,
        different_content,
        only_left,
        only_right,
    })
}

/// Generate a unified text diff for a specific file between two profiles (Level 3).
///
/// Uses `similar::TextDiff` to produce a unified diff string. Both
/// files must be valid UTF-8.
///
/// # Errors
///
/// Returns an error if a referenced profile does not exist, the file
/// is not present in either side, or the file content is not valid UTF-8.
pub fn content_diff(
    paths: &PortalPaths,
    left: &DiffSide<'_>,
    right: &DiffSide<'_>,
    file_path: &str,
) -> Result<String> {
    let left_content = read_file_from_side(paths, left, file_path)?;
    let right_content = read_file_from_side(paths, right, file_path)?;

    let left_label = side_name(left);
    let right_label = side_name(right);

    let diff = similar::TextDiff::from_lines(&left_content, &right_content);
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("{left_label}/{file_path}"),
            &format!("{right_label}/{file_path}"),
        )
        .to_string();

    Ok(unified)
}

/// Checksum + size pair used internally for comparison.
type FileInfo = (String, u64);

/// Resolve a `DiffSide` into a display name and a map of relative paths
/// to `(checksum, size)`.
fn resolve_side(
    paths: &PortalPaths,
    side: &DiffSide<'_>,
) -> Result<(String, HashMap<String, FileInfo>)> {
    match side {
        DiffSide::Profile(name) => {
            let manifest_path = paths.profile_manifest(name);
            if !manifest_path.exists() {
                bail!("Profile \"{name}\" not found.");
            }
            let manifest = manifest::read(&manifest_path)?;
            let map: HashMap<String, FileInfo> = manifest
                .files
                .into_iter()
                .map(|(k, v)| (k, (v.checksum, v.size)))
                .collect();
            Ok(((*name).to_string(), map))
        }
        DiffSide::Skeleton => Ok(("skeleton".to_string(), HashMap::new())),
    }
}

/// Read a file's content from a profile, preferring the CAS pool and falling
/// back to the legacy `files/` directory for unmigrated profiles.
fn read_file_from_side(
    paths: &PortalPaths,
    side: &DiffSide<'_>,
    file_path: &str,
) -> Result<String> {
    match side {
        DiffSide::Profile(name) => {
            let manifest_path = paths.profile_manifest(name);
            if let Ok(mf) = manifest::read(&manifest_path)
                && let Some(entry) = mf.files.get(file_path)
                && cas::exists(paths, &entry.checksum)
            {
                return std::fs::read_to_string(paths.object_path(&entry.checksum)).map_err(|e| {
                    anyhow::anyhow!("reading CAS object for {file_path} in \"{name}\": {e}")
                });
            }

            let full = paths.profile_files_dir(name).join(file_path);
            if !full.exists() {
                bail!("File \"{file_path}\" not found in profile \"{name}\".");
            }
            std::fs::read_to_string(&full)
                .map_err(|e| anyhow::anyhow!("reading {file_path} from profile \"{name}\": {e}"))
        }
        DiffSide::Skeleton => {
            // Skeleton has no profile-specific files — return empty string
            // so the diff shows everything as added/removed.
            Ok(String::new())
        }
    }
}

/// Human-readable name for a diff side.
fn side_name(side: &DiffSide<'_>) -> String {
    match side {
        DiffSide::Profile(name) => (*name).to_string(),
        DiffSide::Skeleton => "skeleton".to_string(),
    }
}
