use portal::core::profile::FileSource;
use portal::core::skeleton;
use portal::core::snapshot;
use portal::storage::manifest;
use portal::storage::paths::PortalPaths;

#[test]
fn test_save_profile() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "# My Config\nHello world").unwrap();
    std::fs::create_dir_all(claude.join("rules")).unwrap();
    std::fs::write(claude.join("rules/test.md"), "# Test Rule").unwrap();

    let result = snapshot::save(&paths, "test-profile", "Test profile", &[]).unwrap();

    assert!(paths.profile_dir("test-profile").exists());
    assert!(paths.profile_manifest("test-profile").exists());
    assert!(paths.profile_plugins("test-profile").exists());
    assert!(paths.profile_meta("test-profile").exists());
    assert!(
        paths
            .profile_files_dir("test-profile")
            .join("CLAUDE.md")
            .exists()
    );
    assert!(
        paths
            .profile_files_dir("test-profile")
            .join("rules/test.md")
            .exists()
    );

    let read_manifest =
        manifest::read(&paths.profile_manifest("test-profile")).unwrap();

    assert!(read_manifest.files.contains_key("CLAUDE.md"));
    assert!(read_manifest.files.contains_key("rules/test.md"));
    assert_eq!(read_manifest.files["CLAUDE.md"].source, FileSource::User);

    // Verify the returned manifest matches.
    assert_eq!(result.name, "test-profile");
    assert_eq!(result.description, "Test profile");
}

#[test]
fn test_save_excludes_sessions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();

    // Create excluded paths.
    std::fs::create_dir_all(claude.join("sessions")).unwrap();
    std::fs::write(claude.join("sessions/abc.json"), "{}").unwrap();
    std::fs::write(claude.join("history.jsonl"), "{}").unwrap();
    std::fs::create_dir_all(claude.join("telemetry")).unwrap();
    std::fs::write(claude.join("telemetry/data.json"), "{}").unwrap();

    let result = snapshot::save(&paths, "excl-test", "Exclusion test", &[]).unwrap();

    assert!(!result.files.contains_key("sessions/abc.json"));
    assert!(!result.files.contains_key("history.jsonl"));
    assert!(!result.files.contains_key("telemetry/data.json"));
}

#[test]
fn test_is_excluded() {
    assert!(snapshot::is_excluded("sessions"));
    assert!(snapshot::is_excluded("sessions/abc.json"));
    assert!(snapshot::is_excluded("history.jsonl"));
    assert!(snapshot::is_excluded("plugins/cache"));
    assert!(snapshot::is_excluded("plugins/cache/foo.json"));
    assert!(snapshot::is_excluded(".DS_Store"));

    assert!(!snapshot::is_excluded("CLAUDE.md"));
    assert!(!snapshot::is_excluded("settings.json"));
    assert!(!snapshot::is_excluded("rules/test.md"));
    assert!(!snapshot::is_excluded("plugins/installed.json"));
}

#[test]
fn test_skeleton_files_classified_correctly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();

    // Default skeleton files should be FileSource::Skeleton.
    let result = snapshot::save(&paths, "skel-class", "Classify test", &[]).unwrap();

    assert_eq!(
        result.files["settings.json"].source,
        FileSource::Skeleton
    );
    assert_eq!(
        result.files[".claude/settings.local.json"].source,
        FileSource::Skeleton
    );
    assert_eq!(result.files["CLAUDE.md"].source, FileSource::Skeleton);
}
