//! Bind-mode: materialize a profile into an isolated `CLAUDE_CONFIG_DIR` (no swap).
//!
//! Unlike `loader::load`, which atomically swaps `~/.claude`, bind-mode projects a
//! profile into a private `live/<name>` directory that a single `claude` session
//! binds to via `CLAUDE_CONFIG_DIR`. Switching another profile never disturbs a
//! running bound session.
//!
//! **Why runtime data is safe:** only manifest-tracked paths are written. Session
//! runtime (`projects/`, `todos/`, `plugins/` cache, `.git`, everything in
//! `snapshot::EXCLUDED_PATTERNS`) is never in the manifest, so it is never touched —
//! the inverse of `loader::preserve_runtime_data`, achieved by omission.

use crate::core::profile::ProfileManifest;
use crate::core::{checksum, loader, skeleton};
use crate::storage::{manifest, paths::PortalPaths};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// File in `live/<name>` recording the manifest hash of the last materialize, so
/// an unchanged refresh is a cheap no-op.
const STAMP_FILE: &str = ".portal-stamp";
/// File in `live/<name>` holding a copy of the last-materialized manifest, used to
/// reconcile deletions (and clear the previous tracked set) on refresh.
const LIVE_MANIFEST_FILE: &str = ".portal-manifest.json";

/// Result of a bind-mode materialization.
#[derive(Debug)]
pub struct BindTarget {
    /// The isolated config dir the session should bind to.
    pub dir: PathBuf,
    /// True when files were (re)placed; false when the stamp matched and nothing ran.
    pub refreshed: bool,
}

/// A deterministic hash of the manifest's tracked-file set (path, checksum, mode).
///
/// `serde_json` serializes the `files` `HashMap` in nondeterministic order, so we
/// cannot hash the raw JSON. Instead we hash a sorted, canonical rendering of the
/// entries that actually drive materialization.
fn manifest_hash(manifest: &ProfileManifest) -> String {
    let mut entries: Vec<(&String, &str, u32)> = manifest
        .files
        .iter()
        .map(|(rel, entry)| (rel, entry.checksum.as_str(), entry.mode.unwrap_or(0)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut canonical = String::new();
    for (rel, checksum, mode) in entries {
        canonical.push_str(rel);
        canonical.push('\0');
        canonical.push_str(checksum);
        canonical.push('\0');
        canonical.push_str(&mode.to_string());
        canonical.push('\n');
    }
    checksum::sha256_bytes(canonical.as_bytes())
}

/// Materialize/refresh `live/<name>` from the profile manifest. Idempotent: skips
/// when the stamp matches the current manifest hash (unless `force`).
///
/// # Errors
///
/// Returns an error if the profile is missing, a CAS object is absent, or any
/// filesystem operation fails.
pub fn materialize(paths: &PortalPaths, name: &str, force: bool) -> Result<BindTarget> {
    let manifest_path = paths.profile_manifest(name);
    if !manifest_path.is_file() {
        bail!("profile \"{name}\" not found");
    }
    let manifest = manifest::read(&manifest_path)?;
    let hash = manifest_hash(&manifest);

    let dir = paths.live_dir(name);
    let stamp_path = dir.join(STAMP_FILE);

    // Fast path: an unchanged manifest means the live dir is already current.
    if !force && std::fs::read_to_string(&stamp_path).is_ok_and(|existing| existing.trim() == hash)
    {
        return Ok(BindTarget {
            dir,
            refreshed: false,
        });
    }

    // Idempotent — creates missing skeleton dirs/files only.
    skeleton::create(&dir).with_context(|| format!("creating skeleton in {}", dir.display()))?;

    // Clear the previously-tracked set. This both reconciles deletions (paths no
    // longer in the manifest) and frees intersection paths so CAS `place_fresh`
    // (which refuses to overwrite) can re-place them. Runtime paths are untouched
    // because they were never in the previous manifest.
    let live_manifest_path = dir.join(LIVE_MANIFEST_FILE);
    match manifest::read(&live_manifest_path) {
        Ok(prev) => {
            for rel in prev.files.keys() {
                remove_tracked(&dir, rel)?;
            }
        }
        // Genuinely missing prior manifest: first materialize, nothing to reconcile.
        Err(_) if !live_manifest_path.exists() => {}
        // Corrupt/unparseable prior manifest (e.g. an interrupted `manifest::write`
        // left a truncated file while the stamp is stale). We can't reconcile
        // deletions, but we must still free any NEW tracked path that already exists
        // so `place_fresh` won't fail with EEXIST and wedge live/<name>. Only the new
        // manifest's paths are touched, so runtime data stays intact.
        Err(_) => {
            for rel in manifest.files.keys() {
                remove_tracked(&dir, rel)?;
            }
        }
    }

    loader::materialize_tracked(paths, &manifest, &dir)?;

    // Record what we placed so the next refresh can reconcile against it, and stamp
    // the hash so an unchanged refresh short-circuits.
    manifest::write(&live_manifest_path, &manifest)?;
    write_stamp(&stamp_path, &hash)?;

    Ok(BindTarget {
        dir,
        refreshed: true,
    })
}

/// True when `live/<name>` has been materialized at least once (its stamp exists).
#[must_use]
pub fn is_materialized(paths: &PortalPaths, name: &str) -> bool {
    paths.live_dir(name).join(STAMP_FILE).is_file()
}

/// Remove a tracked file under `dir`, treating an already-absent path as success.
fn remove_tracked(dir: &Path, rel: &str) -> Result<()> {
    let p = dir.join(rel);
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing stale tracked file: {}", p.display())),
    }
}

fn write_stamp(path: &Path, hash: &str) -> Result<()> {
    std::fs::write(path, hash).with_context(|| format!("writing stamp: {}", path.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::core::snapshot;
    use crate::storage::paths::PortalPaths;

    /// Build a sandbox with a saved profile and return its paths + claude root.
    fn sandbox() -> (tempfile::TempDir, PortalPaths) {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let paths = PortalPaths::with_home(tmp.path().to_path_buf());
        paths.ensure_dirs().expect("ensure_dirs");
        (tmp, paths)
    }

    #[test]
    fn manifest_hash_is_deterministic() {
        let (_tmp, paths) = sandbox();
        let claude = paths.claude_root();
        skeleton::create(&claude).expect("skeleton");
        std::fs::write(claude.join("CLAUDE.md"), "hello").expect("write");
        snapshot::save(&paths, "p", "p", &[]).expect("save");

        let m = manifest::read(&paths.profile_manifest("p")).expect("manifest");
        assert_eq!(manifest_hash(&m), manifest_hash(&m));
    }

    #[test]
    fn is_materialized_tracks_stamp() {
        let (_tmp, paths) = sandbox();
        let claude = paths.claude_root();
        skeleton::create(&claude).expect("skeleton");
        std::fs::write(claude.join("CLAUDE.md"), "hello").expect("write");
        snapshot::save(&paths, "p", "p", &[]).expect("save");

        assert!(!is_materialized(&paths, "p"));
        materialize(&paths, "p", false).expect("materialize");
        assert!(is_materialized(&paths, "p"));
    }
}
