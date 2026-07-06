#![allow(clippy::unwrap_used, clippy::expect_used)]

use portal::core::profile::PortalState;
use portal::core::{remove, skeleton, snapshot};
use portal::storage::paths::PortalPaths;
use portal::storage::state;

/// Build a tempdir-rooted portal with one saved profile and return its paths.
fn setup_profile(name: &str) -> (tempfile::TempDir, PortalPaths) {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "# Config").unwrap();
    snapshot::save(&paths, name, "", &[]).unwrap();

    (tmp, paths)
}

/// Drop a fake backup archive into the backups dir; returns its path.
fn seed_backup(paths: &PortalPaths) -> std::path::PathBuf {
    let backup = paths
        .backups_dir()
        .join("pre-load-2026-06-16T09-00-00.tar.zst");
    std::fs::write(&backup, b"fake-compressed-bytes").unwrap();
    backup
}

#[test]
fn delete_removes_profile_dir_but_keeps_backups() {
    let (_tmp, paths) = setup_profile("work");
    let backup = seed_backup(&paths);

    assert!(paths.profile_dir("work").exists());
    assert!(backup.exists());

    remove::delete_profile(&paths, "work").unwrap();

    // Profile reference is gone...
    assert!(!paths.profile_dir("work").exists());
    // ...but the compressed backup survives — this is the whole point.
    assert!(
        backup.exists(),
        "backup must NOT be deleted with the profile"
    );
    assert!(paths.backups_dir().exists());
}

#[test]
fn delete_clears_active_and_previous_pointers() {
    let (_tmp, paths) = setup_profile("work");
    seed_backup(&paths);

    let st = PortalState {
        active_profile: Some("work".to_string()),
        previous_profile: Some("work".to_string()),
        ..PortalState::default()
    };
    state::write(&paths.state_file(), &st).unwrap();

    let outcome = remove::delete_profile(&paths, "work").unwrap();
    assert!(outcome.cleared_active);
    assert!(outcome.cleared_previous);

    let after = state::read(&paths.state_file()).unwrap();
    assert!(after.active_profile.is_none());
    assert!(after.previous_profile.is_none());
}

#[test]
fn delete_leaves_unrelated_pointers_intact() {
    let (_tmp, paths) = setup_profile("work");
    snapshot::save(&paths, "play", "", &[]).unwrap();

    let st = PortalState {
        active_profile: Some("play".to_string()),
        previous_profile: Some("play".to_string()),
        ..PortalState::default()
    };
    state::write(&paths.state_file(), &st).unwrap();

    let outcome = remove::delete_profile(&paths, "work").unwrap();
    assert!(!outcome.cleared_active);
    assert!(!outcome.cleared_previous);

    let after = state::read(&paths.state_file()).unwrap();
    assert_eq!(after.active_profile.as_deref(), Some("play"));
    assert!(paths.profile_dir("play").exists());
}

#[test]
fn delete_nonexistent_profile_errors() {
    let (_tmp, paths) = setup_profile("work");
    let err = remove::delete_profile(&paths, "ghost").unwrap_err();
    assert!(err.to_string().contains("not found"));
}
