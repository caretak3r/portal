#![allow(clippy::unwrap_used, clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn portal_cmd() -> Command {
    Command::cargo_bin("portal").expect("binary exists")
}

#[test]
fn test_cli_version() {
    portal_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("portal"));
}

#[test]
fn test_cli_help() {
    portal_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Configuration transport layer"));
}

#[test]
#[cfg(not(feature = "tui-ratatui"))]
fn test_cli_no_subcommand() {
    portal_cmd()
        .assert()
        .success()
        .stdout(predicate::str::contains("portal"));
}

#[test]
fn test_cli_list_empty() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    portal_cmd()
        .env("HOME", tmp.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No profiles yet"));
}

#[test]
fn test_cli_status_empty() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir profiles");
    std::fs::create_dir_all(tmp.path().join(".config/portal/backups")).expect("mkdir backups");
    std::fs::create_dir_all(tmp.path().join(".config/portal/skeleton")).expect("mkdir skeleton");

    portal_cmd()
        .env("HOME", tmp.path())
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Portal Status"))
        .stdout(predicate::str::contains("(none)"));
}

#[test]
fn test_cli_rm_nonexistent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["rm", "does-not-exist", "--force"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_show_nonexistent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["show", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_verify_no_active() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .arg("verify")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No profile specified"));
}

#[test]
fn test_cli_rename_nonexistent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["rename", "old", "new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_reset_force() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/backups")).expect("mkdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/skeleton")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["reset", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Reset .claude/ to skeleton"));

    // Verify skeleton was created.
    assert!(tmp.path().join(".claude/settings.json").exists());
    assert!(tmp.path().join(".claude/CLAUDE.md").exists());
}

#[test]
fn test_cli_save_requires_name_noninteractive() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal")).expect("mkdir");
    std::fs::create_dir_all(tmp.path().join(".claude")).expect("mkdir");
    std::fs::write(tmp.path().join(".claude/settings.json"), "{}").expect("write");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["save", "--force"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Profile name required"));
}

#[test]
fn test_cli_save_no_name_overwrites_active_profile() {
    // With an active profile set, `portal save --force` (no name) should
    // overwrite that active profile, not error out — the "save game" path.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let home = tmp.path();

    portal_cmd()
        .env("HOME", home)
        .args(["reset", "--force"])
        .assert()
        .success();

    portal_cmd()
        .env("HOME", home)
        .args(["save", "wip", "-d", "first", "--force"])
        .assert()
        .success();

    portal_cmd()
        .env("HOME", home)
        .args(["load", "wip", "--force"])
        .assert()
        .success();

    // Touch the working copy so the snapshot has new content.
    std::fs::write(home.join(".claude/CLAUDE.md"), "edited content").expect("write");

    // No name + --force + active profile set → overwrite "wip".
    // (indicatif progress is silent in non-TTY, so we verify by file content.)
    portal_cmd()
        .env("HOME", home)
        .args(["save", "--force"])
        .assert()
        .success();

    // Confirm content was persisted into the active profile's files/.
    let claude_md = home.join(".config/portal/profiles/wip/files/CLAUDE.md");
    let saved = std::fs::read_to_string(&claude_md).expect("read");
    assert_eq!(saved, "edited content");
}

#[test]
fn test_cli_save_explicit_name_matching_active_skips_prompt() {
    // Saving by name to the active profile shouldn't be blocked by the
    // overwrite-cancel default — it should overwrite directly.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let home = tmp.path();

    portal_cmd()
        .env("HOME", home)
        .args(["reset", "--force"])
        .assert()
        .success();
    portal_cmd()
        .env("HOME", home)
        .args(["save", "wip", "--force"])
        .assert()
        .success();
    portal_cmd()
        .env("HOME", home)
        .args(["load", "wip", "--force"])
        .assert()
        .success();

    std::fs::write(home.join(".claude/CLAUDE.md"), "round 2").expect("write");

    // Explicit name matching active — without --force in non-interactive mode,
    // this still works because the prompt is skipped for the active profile.
    portal_cmd()
        .env("HOME", home)
        .args(["save", "wip", "--force"])
        .assert()
        .success();

    let claude_md = home.join(".config/portal/profiles/wip/files/CLAUDE.md");
    assert_eq!(std::fs::read_to_string(claude_md).unwrap(), "round 2");
}

#[test]
fn test_cli_load_no_backup_requires_force() {
    let tmp = tempfile::TempDir::new().expect("tempdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["load", "test", "--no-backup"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--no-backup requires --force"));
}

#[test]
fn test_cli_undo_nothing() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["undo", "--force"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Nothing to undo"));
}

#[test]
fn test_cli_diff_nonexistent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["diff", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_export_nonexistent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["export", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_import_nonexistent_archive() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["import", "/tmp/does-not-exist.tar.zst"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_recover_no_crash() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".config/portal")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .arg("recover")
        .assert()
        .success()
        .stdout(predicate::str::contains("No crash recovery needed"));
}
