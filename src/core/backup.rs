use anyhow::{Context, Result, bail};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::info;

use crate::storage::paths::PortalPaths;

/// Metadata for a single backup archive.
#[derive(Debug)]
pub struct BackupInfo {
    /// Path to the `.tar.zst` file.
    pub path: PathBuf,
    /// Size in bytes.
    pub size: u64,
    /// Last-modified timestamp (used as creation proxy).
    pub created: SystemTime,
}

/// Create a zstd-compressed tar backup of `~/.claude/`.
///
/// The archive is stored under `<portal_root>/backups/` with a filename
/// encoding the operation type and a UTC timestamp.
///
/// # Errors
///
/// Returns an error if directory creation, file I/O, or tar/zstd
/// encoding fails.
pub fn create(paths: &PortalPaths, op_type: &str, _profile_name: &str) -> Result<PathBuf> {
    let claude_dir = paths.claude_root();
    anyhow::ensure!(
        claude_dir.exists(),
        ".claude directory not found at {}; run `portal` without arguments to reconfigure",
        claude_dir.display()
    );
    let backups_dir = paths.backups_dir();
    std::fs::create_dir_all(&backups_dir)?;

    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S%.3f");
    let filename = format!("pre-{op_type}-{timestamp}.tar.zst");
    let backup_path = backups_dir.join(&filename);

    info!("creating backup: {}", backup_path.display());

    let file = File::create(&backup_path)
        .with_context(|| format!("creating backup file: {}", backup_path.display()))?;
    let encoder = zstd::Encoder::new(file, 3)?;
    let mut tar = tar::Builder::new(encoder);
    // Store symlinks as-is rather than dereferencing — avoids ENOENT on
    // broken symlinks (e.g. ~/.claude/debug/latest -> deleted file).
    tar.follow_symlinks(false);

    tar.append_dir_all("claude", &claude_dir)
        .context("archiving .claude/ directory")?;

    let encoder = tar.into_inner()?;
    encoder.finish()?;

    // A backup is worthless if it's empty or unreadable. Validate before
    // returning the path — otherwise a silent encode failure leaves a 0-byte
    // archive that `portal undo` would later try (and fail) to restore from.
    if let Err(e) = verify_archive(&backup_path) {
        let _ = std::fs::remove_file(&backup_path);
        return Err(e).context("backup verification failed");
    }

    info!("backup created: {filename}");
    Ok(backup_path)
}

/// Verify a freshly-written archive is non-empty and contains the `claude/`
/// root entry. Cheap: reads only the first tar header, not the whole archive.
///
/// # Errors
///
/// Returns an error if the file is empty, cannot be decoded, or does not start
/// with a `claude/` entry.
pub fn verify_archive(path: &Path) -> Result<()> {
    let len = std::fs::metadata(path)
        .with_context(|| format!("stat backup: {}", path.display()))?
        .len();
    if len == 0 {
        bail!("backup archive is empty: {}", path.display());
    }

    let file = File::open(path).with_context(|| format!("opening backup: {}", path.display()))?;
    let decoder = zstd::Decoder::new(file).context("decoding backup")?;
    let mut archive = tar::Archive::new(decoder);
    let mut entries = archive.entries().context("reading backup entries")?;
    match entries.next() {
        Some(Ok(entry)) => {
            let entry_path = entry.path().context("reading backup entry path")?;
            if !entry_path.starts_with("claude") {
                bail!(
                    "backup archive does not start with a claude/ entry: {}",
                    path.display()
                );
            }
            Ok(())
        }
        _ => bail!("backup archive has no entries: {}", path.display()),
    }
}

/// Restore from a zstd-compressed tar backup, replacing `~/.claude/`.
///
/// The current `~/.claude/` is moved to `~/.claude.portal-old` during the
/// swap and removed on success. If `~/.claude.portal-old` already exists
/// it is deleted first.
///
/// # Errors
///
/// Returns an error if the archive cannot be read, does not contain a
/// `claude/` directory, or if filesystem operations (rename, remove) fail.
pub fn restore(paths: &PortalPaths, backup_path: &Path) -> Result<()> {
    let claude_dir = paths.claude_root();

    info!("restoring from backup: {}", backup_path.display());

    // Extract into a temp dir inside .portal so rename is same-filesystem.
    let tmp = tempfile::tempdir_in(paths.portal_root())?;
    let file = File::open(backup_path)?;
    let decoder = zstd::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(tmp.path())?;

    let extracted_claude = tmp.path().join("claude");
    if !extracted_claude.exists() {
        anyhow::bail!("backup archive does not contain 'claude/' directory");
    }

    // Atomic-ish swap: old -> .portal-old, extracted -> .claude
    let old_path = paths.claude_old();
    if old_path.exists() {
        std::fs::remove_dir_all(&old_path)?;
    }
    if claude_dir.exists() {
        std::fs::rename(&claude_dir, &old_path)?;
    }
    std::fs::rename(&extracted_claude, &claude_dir)?;

    // Clean up the old copy.
    if old_path.exists() {
        std::fs::remove_dir_all(&old_path)?;
    }

    info!("restore complete");
    Ok(())
}

/// Prune old backups, keeping only the most recent `keep_count`.
///
/// Backups are sorted by filesystem modification time, newest first.
/// Returns the list of paths that were deleted.
///
/// # Errors
///
/// Returns an error if the backups directory cannot be read or a file
/// cannot be removed.
pub fn prune(paths: &PortalPaths, keep_count: usize) -> Result<Vec<PathBuf>> {
    let backups_dir = paths.backups_dir();
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups: Vec<_> = std::fs::read_dir(&backups_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
        .collect();

    backups.sort_by(|a, b| {
        let a_time = a
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let b_time = b
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        b_time.cmp(&a_time)
    });

    let mut pruned = Vec::new();
    for entry in backups.iter().skip(keep_count) {
        let path = entry.path();
        std::fs::remove_file(&path)?;
        pruned.push(path);
    }

    Ok(pruned)
}

/// List available backups, newest first.
///
/// # Errors
///
/// Returns an error if the backups directory cannot be read.
pub fn list(paths: &PortalPaths) -> Result<Vec<BackupInfo>> {
    let backups_dir = paths.backups_dir();
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }

    let mut infos: Vec<_> = std::fs::read_dir(&backups_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(BackupInfo {
                path: e.path(),
                size: meta.len(),
                created: meta.modified().ok()?,
            })
        })
        .collect();

    infos.sort_by_key(|info| std::cmp::Reverse(info.created));
    Ok(infos)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn verify_archive_rejects_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.tar.zst");
        std::fs::write(&path, b"").unwrap();
        assert!(verify_archive(&path).is_err(), "0-byte archive must fail");
    }

    #[test]
    fn verify_archive_rejects_non_archive_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("garbage.tar.zst");
        std::fs::write(&path, b"not a zstd stream").unwrap();
        assert!(
            verify_archive(&path).is_err(),
            "garbage must fail to decode"
        );
    }

    #[test]
    fn verify_archive_accepts_a_real_claude_archive() {
        let tmp = tempfile::tempdir().unwrap();
        // Build a minimal claude/ tree and archive it the same way create() does.
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("CLAUDE.md"), b"hi").unwrap();

        let path = tmp.path().join("good.tar.zst");
        let file = File::create(&path).unwrap();
        let encoder = zstd::Encoder::new(file, 3).unwrap();
        let mut tar = tar::Builder::new(encoder);
        tar.append_dir_all("claude", &claude).unwrap();
        tar.into_inner().unwrap().finish().unwrap();

        verify_archive(&path).expect("a real claude archive must verify");
    }
}
