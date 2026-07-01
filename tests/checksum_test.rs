use portal::core::checksum;
use portal::core::profile::{FileEntry, FileSource};
use std::collections::HashMap;
use tempfile::TempDir;

const HELLO_HASH: &str = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

#[test]
fn test_sha256_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"hello world").unwrap();

    let hash = checksum::sha256_file(&path).unwrap();
    assert_eq!(hash, HELLO_HASH);
}

#[test]
fn test_sha256_bytes() {
    let hash = checksum::sha256_bytes(b"hello world");
    assert_eq!(hash, HELLO_HASH);
}

#[test]
fn test_verify_file_ok() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"hello world").unwrap();

    assert!(checksum::verify_file(&path, HELLO_HASH).unwrap());
}

#[test]
fn test_verify_file_mismatch() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"hello world").unwrap();

    assert!(!checksum::verify_file(&path, "sha256:deadbeef").unwrap());
}

#[test]
fn test_verify_manifest_all_ok() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"hello world").unwrap();

    let files = HashMap::from([(
        "test.txt".into(),
        FileEntry {
            checksum: HELLO_HASH.into(),
            size: 11,
            source: FileSource::User,
            mode: None,
        },
    )]);

    let mismatches = checksum::verify_manifest(tmp.path(), &files).unwrap();
    assert!(mismatches.is_empty());
}

#[test]
fn test_verify_manifest_missing_file() {
    let tmp = TempDir::new().unwrap();
    let files = HashMap::from([(
        "gone.txt".into(),
        FileEntry {
            checksum: HELLO_HASH.into(),
            size: 11,
            source: FileSource::User,
            mode: None,
        },
    )]);

    let mismatches = checksum::verify_manifest(tmp.path(), &files).unwrap();
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].actual, "<missing>");
}

#[test]
fn test_verify_manifest_wrong_checksum() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"different content").unwrap();

    let files = HashMap::from([(
        "test.txt".into(),
        FileEntry {
            checksum: HELLO_HASH.into(),
            size: 11,
            source: FileSource::User,
            mode: None,
        },
    )]);

    let mismatches = checksum::verify_manifest(tmp.path(), &files).unwrap();
    assert_eq!(mismatches.len(), 1);
    assert_ne!(mismatches[0].actual, HELLO_HASH);
}
