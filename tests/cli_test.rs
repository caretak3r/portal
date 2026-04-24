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
#[cfg(not(any(feature = "tui-ratatui", feature = "tui-ftui")))]
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
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).expect("mkdir profiles");
    std::fs::create_dir_all(tmp.path().join(".portal/backups")).expect("mkdir backups");
    std::fs::create_dir_all(tmp.path().join(".portal/skeleton")).expect("mkdir skeleton");

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
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).expect("mkdir");

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
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).expect("mkdir");

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
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).expect("mkdir");

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
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).expect("mkdir");

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
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).expect("mkdir");
    std::fs::create_dir_all(tmp.path().join(".portal/backups")).expect("mkdir");
    std::fs::create_dir_all(tmp.path().join(".portal/skeleton")).expect("mkdir");

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
    std::fs::create_dir_all(tmp.path().join(".portal")).expect("mkdir");
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
    std::fs::create_dir_all(tmp.path().join(".portal")).expect("mkdir");

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
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).expect("mkdir");

    portal_cmd()
        .env("HOME", tmp.path())
        .args(["diff", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}
