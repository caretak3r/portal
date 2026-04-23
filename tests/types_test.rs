use std::collections::HashMap;

#[test]
fn test_profile_manifest_roundtrip() {
    let manifest = portal::core::profile::ProfileManifest {
        version: 1,
        name: "test-profile".into(),
        created_at: chrono::Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: "Test profile".into(),
        tags: vec!["test".into()],
        files: HashMap::from([(
            "CLAUDE.md".into(),
            portal::core::profile::FileEntry {
                checksum: "sha256:abc123".into(),
                size: 1024,
                source: portal::core::profile::FileSource::User,
            },
        )]),
        excluded_patterns: vec!["sessions/**".into()],
    };

    let json = serde_json::to_string_pretty(&manifest).expect("serialize");
    let parsed: portal::core::profile::ProfileManifest =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.name, "test-profile");
    assert_eq!(parsed.files.len(), 1);
    assert_eq!(parsed.version, 1);
}
