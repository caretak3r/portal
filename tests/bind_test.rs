#![allow(clippy::unwrap_used, clippy::expect_used)]

use assert_cmd::Command;
use portal::core::{bind, skeleton, snapshot};
use portal::storage::paths::PortalPaths;
use predicates::prelude::*;

/// Build a sandbox rooted at a temp home with portal dirs created.
fn sandbox() -> (tempfile::TempDir, PortalPaths) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");
    (tmp, paths)
}

/// Seed `~/.claude` with a skeleton plus the given (rel, content) files, then
/// save it as `name`.
fn save_profile(paths: &PortalPaths, name: &str, files: &[(&str, &str)]) {
    let claude = paths.claude_root();
    skeleton::create(&claude).expect("skeleton");
    for (rel, content) in files {
        let p = claude.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&p, content).expect("write seed file");
    }
    snapshot::save(paths, name, name, &[]).expect("save profile");
}

#[test]
fn materialize_lays_down_tracked_files() {
    let (_tmp, paths) = sandbox();
    save_profile(&paths, "alpha", &[("CLAUDE.md", "alpha config"), ("rules/r.md", "# rule")]);

    let target = bind::materialize(&paths, "alpha", false).expect("materialize");
    assert!(target.refreshed, "first materialize must place files");
    assert_eq!(target.dir, paths.live_dir("alpha"));

    assert_eq!(
        std::fs::read_to_string(target.dir.join("CLAUDE.md")).expect("read CLAUDE.md"),
        "alpha config"
    );
    assert_eq!(
        std::fs::read_to_string(target.dir.join("rules/r.md")).expect("read rule"),
        "# rule"
    );
}

#[test]
fn materialize_preserves_runtime_data_across_refresh() {
    let (_tmp, paths) = sandbox();
    save_profile(&paths, "beta", &[("CLAUDE.md", "v1")]);

    let dir = bind::materialize(&paths, "beta", false).expect("materialize").dir;

    // Simulate session runtime data landing in the live dir (never tracked).
    let runtime = dir.join("projects/x.json");
    std::fs::create_dir_all(runtime.parent().unwrap()).expect("mkdir projects");
    std::fs::write(&runtime, "runtime state").expect("write runtime");

    // Change the profile and re-save under the same name.
    std::fs::write(paths.claude_root().join("CLAUDE.md"), "v2").expect("edit");
    snapshot::save(&paths, "beta", "beta", &[]).expect("re-save");

    let target = bind::materialize(&paths, "beta", false).expect("re-materialize");
    assert!(target.refreshed, "changed manifest must refresh");

    // Tracked file updated; runtime data survived.
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md")).expect("read"),
        "v2"
    );
    assert!(runtime.exists(), "runtime projects/ must survive refresh");
    assert_eq!(
        std::fs::read_to_string(&runtime).expect("read runtime"),
        "runtime state"
    );
}

#[test]
fn stamp_skips_noop_refresh() {
    let (_tmp, paths) = sandbox();
    save_profile(&paths, "gamma", &[("CLAUDE.md", "stable")]);

    let dir = bind::materialize(&paths, "gamma", false).expect("materialize").dir;
    let tracked = dir.join("CLAUDE.md");
    let mtime_before = std::fs::metadata(&tracked).unwrap().modified().unwrap();

    // Second materialize with an unchanged manifest is a no-op.
    let second = bind::materialize(&paths, "gamma", false).expect("second materialize");
    assert!(!second.refreshed, "unchanged manifest must not refresh");

    let mtime_after = std::fs::metadata(&tracked).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after, "no-op refresh must not rewrite files");
}

#[test]
fn refresh_removes_deleted_tracked_file() {
    let (_tmp, paths) = sandbox();
    save_profile(&paths, "delta", &[("CLAUDE.md", "cfg"), ("rules/drop.md", "bye")]);

    let dir = bind::materialize(&paths, "delta", false).expect("materialize").dir;
    assert!(dir.join("rules/drop.md").exists(), "file present after first materialize");

    // Drop the file from the profile and re-save.
    std::fs::remove_file(paths.claude_root().join("rules/drop.md")).expect("remove");
    snapshot::save(&paths, "delta", "delta", &[]).expect("re-save");

    let target = bind::materialize(&paths, "delta", false).expect("re-materialize");
    assert!(target.refreshed);
    assert!(
        !dir.join("rules/drop.md").exists(),
        "tracked file dropped from manifest must be unlinked on refresh"
    );
    assert!(dir.join("CLAUDE.md").exists(), "surviving tracked file remains");
}

#[test]
fn corrupt_live_manifest_recovers_on_refresh() {
    let (_tmp, paths) = sandbox();
    save_profile(&paths, "epsilon", &[("CLAUDE.md", "cfg"), ("rules/r.md", "# rule")]);

    let dir = bind::materialize(&paths, "epsilon", false).expect("materialize").dir;
    assert!(dir.join("CLAUDE.md").exists(), "tracked file present after first materialize");

    // Simulate an interrupted `manifest::write`: truncated/garbage manifest plus a
    // stale stamp, while the tracked files it laid down still exist on disk.
    std::fs::write(dir.join(".portal-manifest.json"), "{ truncated").expect("corrupt manifest");
    std::fs::write(dir.join(".portal-stamp"), "stale-hash").expect("stale stamp");

    // Next refresh must recover (not fail with EEXIST) and re-lay the tracked files.
    let target = bind::materialize(&paths, "epsilon", false).expect("recovering re-materialize");
    assert!(target.refreshed, "stale stamp must trigger a refresh");
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md")).expect("read CLAUDE.md"),
        "cfg"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("rules/r.md")).expect("read rule"),
        "# rule"
    );
}

#[test]
fn two_live_dirs_are_independent() {
    let (_tmp, paths) = sandbox();
    save_profile(&paths, "one", &[("CLAUDE.md", "one")]);
    // Re-seed claude root for the second profile.
    save_profile(&paths, "two", &[("CLAUDE.md", "two")]);

    let dir1 = bind::materialize(&paths, "one", false).expect("materialize one").dir;
    let dir2 = bind::materialize(&paths, "two", false).expect("materialize two").dir;
    assert_ne!(dir1, dir2);

    // Runtime write into one is invisible to the other.
    std::fs::create_dir_all(dir1.join("projects")).expect("mkdir");
    std::fs::write(dir1.join("projects/a.json"), "a").expect("write");
    assert!(!dir2.join("projects/a.json").exists(), "live dirs must be isolated");

    // And their tracked content is distinct.
    assert_eq!(std::fs::read_to_string(dir1.join("CLAUDE.md")).unwrap(), "one");
    assert_eq!(std::fs::read_to_string(dir2.join("CLAUDE.md")).unwrap(), "two");
}

#[test]
fn missing_profile_bails() {
    let (_tmp, paths) = sandbox();
    let err = bind::materialize(&paths, "ghost", false).unwrap_err();
    assert!(
        err.to_string().contains("not found"),
        "missing profile must bail with 'not found', got: {err}"
    );
}

// ── CLI ──────────────────────────────────────────────────────────────

fn portal_cmd() -> Command {
    Command::cargo_bin("portal").expect("binary exists")
}

#[test]
fn cli_use_print_env_prints_live_dir() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let home = tmp.path();

    portal_cmd().env("HOME", home).args(["reset", "--force"]).assert().success();
    portal_cmd().env("HOME", home).args(["save", "wip", "--force"]).assert().success();

    portal_cmd()
        .env("HOME", home)
        .args(["use", "wip", "--print-env"])
        .assert()
        .success()
        .stdout(predicate::str::contains("export CLAUDE_CONFIG_DIR="))
        .stdout(predicate::str::contains(".config/portal/live/wip"));
}

#[test]
fn cli_use_missing_profile_fails() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["use", "does-not-exist", "--print-env"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}
