#![allow(clippy::unwrap_used, clippy::expect_used)]

use portal::core::profile::FileSource;
use portal::core::skeleton;
use portal::core::snapshot;
use portal::storage::cas;
use portal::storage::manifest;
use portal::storage::paths::PortalPaths;

/// CAS-aware: confirm the manifest carries this path AND the object exists.
fn profile_has(paths: &PortalPaths, profile: &str, rel: &str) -> bool {
    manifest::read(&paths.profile_manifest(profile))
        .map(|mf| {
            mf.files
                .get(rel)
                .is_some_and(|e| cas::exists(paths, &e.checksum))
        })
        .unwrap_or(false)
}

fn read_profile_file(paths: &PortalPaths, profile: &str, rel: &str) -> Option<Vec<u8>> {
    let mf = manifest::read(&paths.profile_manifest(profile)).ok()?;
    let entry = mf.files.get(rel)?;
    std::fs::read(paths.object_path(&entry.checksum)).ok()
}

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
    assert!(profile_has(&paths, "test-profile", "CLAUDE.md"));
    assert!(profile_has(&paths, "test-profile", "rules/test.md"));

    let read_manifest = manifest::read(&paths.profile_manifest("test-profile")).unwrap();

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
fn test_save_overwrite_preserves_metadata() {
    // Re-saving a profile by name should keep created_at, load_count,
    // and last_loaded so it behaves like "save game" (same identity).
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "v1").unwrap();

    let first = snapshot::save(&paths, "wip", "first description", &["a".into()]).unwrap();
    let original_created = first.created_at;

    // Hand-edit the manifest to simulate the profile having been loaded once.
    let manifest_path = paths.profile_manifest("wip");
    let mut m = manifest::read(&manifest_path).unwrap();
    m.load_count = 5;
    m.last_loaded = Some(chrono::Utc::now());
    manifest::write(&manifest_path, &m).unwrap();
    let load_marker = m.last_loaded;

    // Mutate the working copy and re-save with empty description/tags —
    // should preserve everything from the existing manifest.
    std::fs::write(claude.join("CLAUDE.md"), "v2").unwrap();
    let second = snapshot::save(&paths, "wip", "", &[]).unwrap();

    assert_eq!(second.created_at, original_created, "created_at preserved");
    assert_eq!(second.load_count, 5, "load_count preserved");
    assert_eq!(second.last_loaded, load_marker, "last_loaded preserved");
    assert_eq!(
        second.description, "first description",
        "description preserved when empty"
    );
    assert_eq!(
        second.tags,
        vec!["a".to_string()],
        "tags preserved when empty"
    );

    // Content actually updated though.
    let bytes = read_profile_file(&paths, "wip", "CLAUDE.md").unwrap();
    assert_eq!(String::from_utf8_lossy(&bytes), "v2");
}

#[test]
fn test_save_overwrite_replaces_description_when_provided() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();

    snapshot::save(&paths, "p", "old desc", &["t1".into()]).unwrap();
    let updated = snapshot::save(&paths, "p", "new desc", &["t2".into()]).unwrap();

    assert_eq!(updated.description, "new desc");
    assert_eq!(updated.tags, vec!["t2".to_string()]);
}

#[test]
fn test_save_overwrite_removes_orphan_files() {
    // Files that existed in the previous snapshot but aren't in the current
    // .claude/ should not linger in the profile's files/ directory.
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();
    std::fs::create_dir_all(claude.join("rules")).unwrap();
    std::fs::write(claude.join("rules/keep.md"), "keep me").unwrap();
    std::fs::write(claude.join("rules/orphan.md"), "delete me").unwrap();

    let first = snapshot::save(&paths, "p", "", &[]).unwrap();
    assert!(first.files.contains_key("rules/orphan.md"));

    // Remove the file from .claude/ and re-save.
    std::fs::remove_file(claude.join("rules/orphan.md")).unwrap();
    let updated = snapshot::save(&paths, "p", "", &[]).unwrap();

    assert!(
        updated.files.contains_key("rules/keep.md"),
        "kept file remains in manifest"
    );
    assert!(
        !updated.files.contains_key("rules/orphan.md"),
        "orphan file removed from manifest"
    );
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

    assert_eq!(result.files["settings.json"].source, FileSource::Skeleton);
    assert_eq!(
        result.files[".claude/settings.local.json"].source,
        FileSource::Skeleton
    );
    assert_eq!(result.files["CLAUDE.md"].source, FileSource::Skeleton);
}
