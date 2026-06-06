use portal::core::profile::{FileEntry, FileSource, ProfileManifest};
use portal::storage::manifest;
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn test_manifest_write_and_read() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("portal.json");

    let m = ProfileManifest {
        version: 1,
        name: "test".into(),
        created_at: chrono::Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: "test profile".into(),
        tags: vec![],
        files: HashMap::from([(
            "CLAUDE.md".into(),
            FileEntry {
                checksum: "sha256:abc".into(),
                size: 100,
                source: FileSource::User,
                mode: None,
            },
        )]),
        excluded_patterns: vec!["sessions/**".into()],
    };

    manifest::write(&path, &m).unwrap();
    let loaded = manifest::read(&path).unwrap();
    assert_eq!(loaded.name, "test");
    assert_eq!(loaded.files.len(), 1);
    assert_eq!(loaded.excluded_patterns, vec!["sessions/**"]);
}
