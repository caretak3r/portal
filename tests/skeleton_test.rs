use portal::core::skeleton;

#[test]
fn test_create_skeleton() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path().join(".claude");
    skeleton::create(&claude_dir).unwrap();

    assert!(claude_dir.join("settings.json").exists());
    assert!(claude_dir.join("CLAUDE.md").exists());
    assert!(claude_dir.join(".claude/settings.local.json").exists());
    assert!(claude_dir.join(".claude/hooks").is_dir());
    assert!(claude_dir.join("skills").is_dir());
    assert!(claude_dir.join("memory").is_dir());
    assert!(claude_dir.join("commands").is_dir());
    assert!(claude_dir.join("agents").is_dir());
    assert!(claude_dir.join("rules").is_dir());
    assert!(claude_dir.join("hooks").is_dir());

    let settings: serde_json::Value =
        serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
    assert!(settings.is_object());

    let claude_md = std::fs::read_to_string(claude_dir.join("CLAUDE.md")).unwrap();
    assert!(claude_md.is_empty());
}

#[test]
fn test_verify_skeleton() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path().join(".claude");
    skeleton::create(&claude_dir).unwrap();

    let issues = skeleton::verify(&claude_dir).unwrap();
    assert!(issues.is_empty());
}

#[test]
fn test_verify_detects_missing_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path().join(".claude");
    skeleton::create(&claude_dir).unwrap();

    // Remove a directory.
    std::fs::remove_dir_all(claude_dir.join("skills")).unwrap();

    let issues = skeleton::verify(&claude_dir).unwrap();
    assert!(!issues.is_empty());
    assert!(issues.contains(&skeleton::SkeletonIssue::MissingDir("skills".to_string())));
}

#[test]
fn test_verify_detects_missing_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path().join(".claude");
    skeleton::create(&claude_dir).unwrap();

    // Remove a file.
    std::fs::remove_file(claude_dir.join("settings.json")).unwrap();

    let issues = skeleton::verify(&claude_dir).unwrap();
    assert!(!issues.is_empty());
    assert!(issues.contains(&skeleton::SkeletonIssue::MissingFile("settings.json".to_string())));
}
