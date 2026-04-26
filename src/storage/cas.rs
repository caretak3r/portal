//! Content-addressed storage for profile file contents.
//!
//! Files are stored once at `~/.config/portal/objects/<aa>/<rest>` keyed by
//! SHA-256 of their content. Every profile manifest then references files by
//! hash, so two profiles that share content share bytes on disk.
//!
//! Loading copies (or reflinks) from the object pool into the build dir,
//! which is metadata-only on filesystems that support copy-on-write
//! (APFS clonefile, btrfs/xfs FICLONE).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::core::checksum;
use crate::storage::paths::PortalPaths;

/// Returns true if an object with this hash is already in the pool.
#[must_use]
pub fn exists(paths: &PortalPaths, hash: &str) -> bool {
    paths.object_path(hash).exists()
}

/// Write `bytes` to the CAS pool keyed by its SHA-256 hash.
///
/// Returns the `sha256:<hex>` hash. If the object already exists, the existing
/// content is left untouched (CAS is content-keyed, so it would be identical).
/// Writes go to a tempfile in the same directory then atomically rename so
/// concurrent writers cannot observe a torn object.
///
/// # Errors
///
/// Returns an error if the shard directory cannot be created or the temp/rename
/// dance fails.
pub fn write(paths: &PortalPaths, bytes: &[u8]) -> Result<String> {
    let hash = checksum::sha256_bytes(bytes);
    let dest = paths.object_path(&hash);

    if dest.exists() {
        return Ok(hash);
    }

    let shard = dest.parent().context("object path missing shard parent")?;
    std::fs::create_dir_all(shard)
        .with_context(|| format!("creating CAS shard dir: {}", shard.display()))?;

    let tmp = tempfile::NamedTempFile::new_in(shard)
        .with_context(|| format!("creating CAS tempfile in {}", shard.display()))?;
    std::fs::write(tmp.path(), bytes)
        .with_context(|| format!("writing CAS tempfile: {}", tmp.path().display()))?;

    // persist() does atomic rename; if rename fails because dest now exists
    // (another writer beat us), discard our copy and accept the existing one.
    match tmp.persist_noclobber(&dest) {
        Ok(_) => Ok(hash),
        Err(e) if dest.exists() => {
            drop(e);
            Ok(hash)
        }
        Err(e) => Err(anyhow::anyhow!(
            "persisting CAS object {}: {}",
            dest.display(),
            e.error
        )),
    }
}

/// Place an object from the CAS pool at `dest`.
///
/// Tries reflink (`CoW`) first via `reflink-copy`; falls back to a regular copy
/// if reflink is unsupported (different filesystem, ext4 without reflink, etc.).
/// Reflink is metadata-only on APFS / btrfs / xfs, so this is constant-time per
/// file regardless of size. Writes to the working copy never propagate back
/// into the pool because the kernel performs `CoW` on the next write.
///
/// # Errors
///
/// Returns an error if neither reflink nor copy succeeds, or if the parent
/// directory cannot be created.
pub fn place(paths: &PortalPaths, hash: &str, dest: &Path) -> Result<()> {
    let src = paths.object_path(hash);

    // clonefile(2) on macOS and FICLONE on Linux refuse to overwrite an
    // existing destination — the skeleton overlay step lays down some files
    // before profile content covers them. Try the unlink unconditionally and
    // ignore NotFound, saving a stat for the common cold-build case.
    match std::fs::remove_file(dest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).with_context(|| format!("removing stale dest: {}", dest.display()));
        }
    }

    match reflink_copy::reflink_or_copy(&src, dest) {
        Ok(_) => Ok(()),
        Err(e) => Err(e)
            .with_context(|| format!("placing CAS object {} -> {}", src.display(), dest.display())),
    }
}

/// Migrate a profile's legacy `files/<rel>` layout into the CAS pool.
///
/// For each file already in the legacy directory, write its bytes to the CAS
/// (keyed by manifest's recorded hash, which we re-verify) and then delete the
/// legacy copy. Idempotent: missing legacy files are skipped, already-in-CAS
/// objects are not rewritten.
///
/// Returns the number of files migrated (objects newly written).
///
/// # Errors
///
/// Returns an error if a legacy file's actual content doesn't match its
/// recorded checksum (data corruption), or if any I/O fails.
pub fn migrate_profile_files<S: std::hash::BuildHasher>(
    paths: &PortalPaths,
    files_dir: &Path,
    files: &std::collections::HashMap<String, crate::core::profile::FileEntry, S>,
) -> Result<usize> {
    if !files_dir.is_dir() {
        return Ok(0);
    }

    let mut migrated = 0;
    for (rel, entry) in files {
        let src = files_dir.join(rel);
        if !src.is_file() {
            continue;
        }

        let bytes = std::fs::read(&src)
            .with_context(|| format!("reading legacy file: {}", src.display()))?;
        let actual = checksum::sha256_bytes(&bytes);
        if actual != entry.checksum {
            anyhow::bail!(
                "legacy file {} checksum mismatch (expected {}, got {}) — refusing migration",
                rel,
                entry.checksum,
                actual
            );
        }

        if !exists(paths, &entry.checksum) {
            write(paths, &bytes)?;
            migrated += 1;
        }
    }

    // Now that every object is in CAS, blow away the legacy tree.
    std::fs::remove_dir_all(files_dir)
        .with_context(|| format!("removing legacy files dir: {}", files_dir.display()))?;

    Ok(migrated)
}

/// Garbage-collect CAS objects that no profile manifest references.
///
/// Walks every profile manifest under `profiles_root()`, collects the set of
/// referenced hashes, then removes any object outside that set. Returns
/// `(removed, bytes_freed)`.
///
/// # Errors
///
/// Returns an error on directory walk or unlink failures.
pub fn gc(paths: &PortalPaths) -> Result<(usize, u64)> {
    use std::collections::HashSet;

    let mut referenced: HashSet<String> = HashSet::new();
    let profiles_root = paths.profiles_root();
    if profiles_root.is_dir() {
        for entry in std::fs::read_dir(&profiles_root)
            .with_context(|| format!("reading profiles dir: {}", profiles_root.display()))?
        {
            let entry = entry?;
            let manifest_path = entry.path().join("portal.json");
            if !manifest_path.is_file() {
                continue;
            }
            let manifest = crate::storage::manifest::read(&manifest_path)?;
            for file_entry in manifest.files.values() {
                referenced.insert(file_entry.checksum.clone());
            }
        }
    }

    let objects_root = paths.objects_root();
    if !objects_root.is_dir() {
        return Ok((0, 0));
    }

    let mut removed = 0;
    let mut bytes_freed: u64 = 0;
    for shard_entry in std::fs::read_dir(&objects_root)? {
        let shard = shard_entry?.path();
        if !shard.is_dir() {
            continue;
        }
        let shard_name = shard
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        for obj_entry in std::fs::read_dir(&shard)? {
            let obj_path = obj_entry?.path();
            let rest = obj_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let hash = format!("sha256:{shard_name}{rest}");
            if !referenced.contains(&hash) {
                let size = std::fs::metadata(&obj_path).map_or(0, |m| m.len());
                std::fs::remove_file(&obj_path).with_context(|| {
                    format!("removing unreferenced object: {}", obj_path.display())
                })?;
                bytes_freed += size;
                removed += 1;
            }
        }
    }

    Ok((removed, bytes_freed))
}

/// True if the portal directory has any CAS objects (used to decide whether to
/// prefer the CAS path during load when `files/` also exists).
#[must_use]
pub fn has_objects(paths: &PortalPaths) -> bool {
    paths.objects_root().is_dir()
        && std::fs::read_dir(paths.objects_root()).is_ok_and(|mut it| it.next().is_some())
}

/// Helper used by tests to compute the path that an object would live at.
#[must_use]
pub fn object_dir(paths: &PortalPaths, hash: &str) -> PathBuf {
    paths.object_path(hash)
}
