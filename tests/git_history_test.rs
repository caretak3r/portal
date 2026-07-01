#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Tests for `core::git_history` — per-profile orphan-branch history.
//! Skipped gracefully when `git` is not on PATH.

use portal::storage::paths::PortalPaths;

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn sandbox() -> (tempfile::TempDir, PortalPaths) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");
    portal::core::skeleton::create(&paths.claude_root()).expect("skeleton");
    (tmp, paths)
}

/// Save a profile and return its manifest.
fn save(paths: &PortalPaths, name: &str) -> portal::core::profile::ProfileManifest {
    portal::core::snapshot::save(paths, name, name, &[]).expect("save")
}

#[test]
fn save_creates_orphan_branch_with_commit() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let (_tmp, paths) = sandbox();
    std::fs::write(paths.claude_root().join("CLAUDE.md"), "v1").unwrap();
    save(&paths, "alpha");

    let commits = portal::core::git_history::log(&paths, "alpha").expect("log");
    assert_eq!(commits.len(), 1, "one save → one commit");
}

#[test]
fn second_save_adds_second_commit() {
    if !git_available() {
        return;
    }
    let (_tmp, paths) = sandbox();
    let claude = paths.claude_root();
    std::fs::write(claude.join("CLAUDE.md"), "v1").unwrap();
    save(&paths, "alpha");
    // Change the live config and save again.
    std::fs::write(claude.join("CLAUDE.md"), "v2 — changed").unwrap();
    save(&paths, "alpha");

    let commits = portal::core::git_history::log(&paths, "alpha").expect("log");
    assert_eq!(commits.len(), 2, "two distinct saves → two commits");
}

#[test]
fn unchanged_resave_does_not_add_commit() {
    if !git_available() {
        return;
    }
    let (_tmp, paths) = sandbox();
    std::fs::write(paths.claude_root().join("CLAUDE.md"), "v1").unwrap();
    save(&paths, "alpha");
    save(&paths, "alpha"); // identical content

    let commits = portal::core::git_history::log(&paths, "alpha").expect("log");
    assert_eq!(commits.len(), 1, "no-op resave must not commit");
}

#[test]
fn two_profiles_have_independent_orphan_histories() {
    if !git_available() {
        return;
    }
    let (_tmp, paths) = sandbox();
    let claude = paths.claude_root();
    std::fs::write(claude.join("CLAUDE.md"), "alpha config").unwrap();
    save(&paths, "alpha");
    std::fs::write(claude.join("CLAUDE.md"), "beta config").unwrap();
    save(&paths, "beta");

    let a = portal::core::git_history::log(&paths, "alpha").expect("log alpha");
    let b = portal::core::git_history::log(&paths, "beta").expect("log beta");
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);

    // Orphan branches share no commit — distinct root commits.
    assert_ne!(
        a[0].hash, b[0].hash,
        "orphan branches must have unrelated histories"
    );

    // And git agrees they have no common ancestor.
    let merge_base = std::process::Command::new("git")
        .arg("-C")
        .arg(paths.history_dir())
        .args(["merge-base", "profile/alpha", "profile/beta"])
        .output()
        .expect("merge-base");
    assert!(
        !merge_base.status.success(),
        "orphan branches must have no merge-base"
    );
}

#[test]
fn git_failure_does_not_break_save() {
    if !git_available() {
        return;
    }
    let (_tmp, paths) = sandbox();
    // Sabotage the history location: plant a regular file where the repo dir
    // must be, so every git invocation fails.
    std::fs::write(paths.history_dir(), b"not a directory").unwrap();

    std::fs::write(paths.claude_root().join("CLAUDE.md"), "v1").unwrap();
    // Save must still succeed despite git being unable to record history.
    let result = portal::core::snapshot::save(&paths, "alpha", "alpha", &[]);
    assert!(result.is_ok(), "save must survive a git history failure");
}

#[test]
fn history_disabled_skips_recording() {
    if !git_available() {
        return;
    }
    let (_tmp, paths) = sandbox();
    portal::config::save(
        &portal::config::PortalConfig {
            history: portal::config::HistoryConfig { enabled: false },
            ..Default::default()
        },
        &paths.config_file(),
    )
    .expect("write config");

    std::fs::write(paths.claude_root().join("CLAUDE.md"), "v1").unwrap();
    save(&paths, "alpha");

    let commits = portal::core::git_history::log(&paths, "alpha").expect("log");
    assert!(commits.is_empty(), "disabled history records nothing");
}
