use crate::core::profile::FileEntry;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::path::Path;

const PREFIX: &str = "sha256:";

/// Compute SHA-256 hash of a file, returned as `sha256:<hex>`.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn sha256_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path)
        .with_context(|| format!("reading file for checksum: {}", path.display()))?;
    let hash = Sha256::digest(&data);
    Ok(format!("{PREFIX}{hash:x}"))
}

/// Compute SHA-256 hash of raw bytes, returned as `sha256:<hex>`.
#[must_use]
pub fn sha256_bytes(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    format!("{PREFIX}{hash:x}")
}

/// Verify a file's checksum matches an expected value.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn verify_file(path: &Path, expected: &str) -> Result<bool> {
    let actual = sha256_file(path)?;
    Ok(actual == expected)
}

/// A single checksum mismatch found during manifest verification.
#[derive(Debug)]
pub struct ChecksumMismatch {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

/// Verify multiple files against a manifest's file entries.
///
/// Returns a list of mismatches (empty if everything checks out).
///
/// # Errors
///
/// Returns an error if any existing file cannot be read for hashing.
pub fn verify_manifest<S: BuildHasher>(
    base_dir: &Path,
    files: &HashMap<String, FileEntry, S>,
) -> Result<Vec<ChecksumMismatch>> {
    let mut mismatches = Vec::new();
    for (rel_path, entry) in files {
        let full_path = base_dir.join(rel_path);
        if !full_path.exists() {
            mismatches.push(ChecksumMismatch {
                path: rel_path.clone(),
                expected: entry.checksum.clone(),
                actual: "<missing>".into(),
            });
            continue;
        }
        let actual = sha256_file(&full_path)?;
        if actual != entry.checksum {
            mismatches.push(ChecksumMismatch {
                path: rel_path.clone(),
                expected: entry.checksum.clone(),
                actual,
            });
        }
    }
    Ok(mismatches)
}
