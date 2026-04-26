use anyhow::{Context, Result, bail};
use std::fs::File;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::storage::paths::PortalPaths;

/// Export a profile to a portable `.tar.zst` archive.
///
/// The archive contains the full profile directory (manifest, files, plugins
/// blueprint, metadata) under a `portal-profile/<name>/` prefix so that
/// import can identify and extract it.
///
/// # Errors
///
/// Returns an error if the profile does not exist, or if tar/zstd encoding
/// fails.
pub fn export(paths: &PortalPaths, name: &str, output: &Path) -> Result<PathBuf> {
    let profile_dir = paths.profile_dir(name);
    if !profile_dir.exists() {
        bail!("Profile \"{name}\" not found.");
    }

    let archive_path = if output.is_dir() {
        output.join(format!("{name}.portal.tar.zst"))
    } else {
        output.to_path_buf()
    };

    info!("exporting profile \"{name}\" to {}", archive_path.display());

    let file = File::create(&archive_path)
        .with_context(|| format!("creating export file: {}", archive_path.display()))?;
    let encoder = zstd::Encoder::new(file, 3)?;
    let mut tar = tar::Builder::new(encoder);

    tar.append_dir_all(format!("portal-profile/{name}"), &profile_dir)
        .context("archiving profile directory")?;

    let encoder = tar.into_inner()?;
    encoder.finish()?;

    info!("export complete: {}", archive_path.display());
    Ok(archive_path)
}

/// Import a profile from a `.tar.zst` archive.
///
/// The archive must contain a `portal-profile/<name>/` directory with at
/// least a `portal.json` manifest. If a profile with the same name already
/// exists, `overwrite` must be true or the operation will fail.
///
/// # Errors
///
/// Returns an error if the archive is invalid, the profile already exists
/// (and `overwrite` is false), or if extraction fails.
pub fn import(paths: &PortalPaths, archive_path: &Path, overwrite: bool) -> Result<String> {
    if !archive_path.exists() {
        bail!("Archive not found: {}", archive_path.display());
    }

    info!("importing from {}", archive_path.display());

    // Extract to a temp dir first to validate.
    let tmp = tempfile::tempdir_in(paths.portal_root()).context("creating temp dir for import")?;

    let file = File::open(archive_path)
        .with_context(|| format!("opening archive: {}", archive_path.display()))?;
    let decoder = zstd::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(tmp.path())?;

    // Find the profile name from the extracted structure.
    let prefix_dir = tmp.path().join("portal-profile");
    if !prefix_dir.exists() {
        bail!(
            "Invalid portal archive: missing 'portal-profile/' directory. \
             This doesn't look like a portal export."
        );
    }

    let entries: Vec<_> = std::fs::read_dir(&prefix_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.is_empty() {
        bail!("Invalid portal archive: no profile directory found inside 'portal-profile/'.");
    }
    if entries.len() > 1 {
        bail!(
            "Invalid portal archive: multiple profiles found. \
             Portal archives should contain exactly one profile."
        );
    }

    let profile_entry = &entries[0];
    let name = profile_entry.file_name().to_string_lossy().to_string();
    let extracted_dir = profile_entry.path();

    // Validate: must have portal.json
    if !extracted_dir.join("portal.json").exists() {
        bail!("Invalid portal archive: profile \"{name}\" is missing portal.json manifest.");
    }

    // Check for existing profile.
    let target_dir = paths.profile_dir(&name);
    if target_dir.exists() {
        if !overwrite {
            bail!("Profile \"{name}\" already exists. Use --force to overwrite.");
        }
        std::fs::remove_dir_all(&target_dir)
            .with_context(|| format!("removing existing profile \"{name}\""))?;
    }

    // Move extracted profile into place.
    paths.ensure_dirs()?;
    std::fs::rename(&extracted_dir, &target_dir)
        .or_else(|_| copy_dir_recursive(&extracted_dir, &target_dir))
        .with_context(|| format!("installing profile \"{name}\""))?;

    info!("imported profile \"{name}\"");
    Ok(name)
}

/// Recursive directory copy fallback (for cross-filesystem moves).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
