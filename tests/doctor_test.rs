#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Tests for `core::doctor` — diagnostics + guided fixes, fully sandboxed.

use portal::core::doctor::{self, FixAction, Severity};
use portal::storage::paths::PortalPaths;

fn sandbox() -> (tempfile::TempDir, PortalPaths) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");
    portal::core::skeleton::create(&paths.claude_root()).expect("skeleton");
    (tmp, paths)
}

fn check<'a>(report: &'a doctor::DoctorReport, id: &str) -> &'a doctor::Check {
    report
        .checks
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("no check with id {id}"))
}

#[test]
fn clean_sandbox_reports_no_errors() {
    let (_tmp, paths) = sandbox();
    let report = doctor::diagnose(&paths).expect("diagnose");
    assert!(!report.has_errors(), "fresh skeleton should be healthy");
    assert_eq!(check(&report, "skeleton").severity, Severity::Ok);
}

#[test]
fn diagnose_flags_and_fixes_missing_skeleton_dir() {
    let (_tmp, paths) = sandbox();
    std::fs::remove_dir_all(paths.claude_root().join("rules")).expect("rm rules");

    let report = doctor::diagnose(&paths).expect("diagnose");
    let skel = check(&report, "skeleton");
    assert_eq!(skel.severity, Severity::Warning);
    let fix = skel.fix.clone().expect("skeleton fix");
    assert!(matches!(fix, FixAction::RecreateSkeleton));

    doctor::apply_fix(&paths, &fix).expect("apply skeleton fix");
    assert!(paths.claude_root().join("rules").is_dir());

    let after = doctor::diagnose(&paths).expect("re-diagnose");
    assert_eq!(check(&after, "skeleton").severity, Severity::Ok);
}

#[test]
fn diagnose_flags_zero_byte_backup_and_spares_healthy_ones() {
    let (_tmp, paths) = sandbox();
    let backups = paths.backups_dir();
    let bad = backups.join("pre-load-empty.tar.zst");
    let good = backups.join("pre-load-good.tar.zst");
    std::fs::write(&bad, b"").expect("write empty");
    std::fs::write(&good, b"not empty").expect("write good");

    let report = doctor::diagnose(&paths).expect("diagnose");
    let b = check(&report, "backups");
    assert_eq!(b.severity, Severity::Warning);
    let fix = b.fix.clone().expect("backup fix");

    doctor::apply_fix(&paths, &fix).expect("prune");
    assert!(!bad.exists(), "zero-byte backup removed");
    assert!(good.exists(), "healthy backup preserved");
}

#[test]
fn diagnose_detects_legacy_root() {
    let (_tmp, paths) = sandbox();
    let legacy = paths.legacy_root().join("profiles").join("god");
    std::fs::create_dir_all(legacy.join("files")).expect("legacy dirs");
    // A minimal but valid manifest with one file present in the legacy tree.
    let body = b"# legacy";
    let sum = portal::core::checksum::sha256_bytes(body);
    std::fs::write(legacy.join("files/CLAUDE.md"), body).expect("legacy file");
    let manifest = serde_json::json!({
        "version": 1, "name": "god", "created_at": "2026-04-23T00:00:00Z",
        "load_count": 0, "description": "", "tags": [],
        "files": { "CLAUDE.md": { "checksum": sum, "size": body.len(), "source": "user" } },
        "excluded_patterns": []
    });
    std::fs::write(
        legacy.join("portal.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .expect("legacy manifest");

    let report = doctor::diagnose(&paths).expect("diagnose");
    let legacy_check = report
        .checks
        .iter()
        .find(|c| c.id == "legacy-root")
        .expect("legacy check present");
    assert_eq!(legacy_check.severity, Severity::Warning);
    assert!(matches!(
        legacy_check.fix,
        Some(FixAction::MigrateLegacyRoot { .. })
    ));
}

#[test]
fn migrate_legacy_root_imports_into_cas() {
    let (_tmp, paths) = sandbox();
    let legacy = paths.legacy_root().join("profiles").join("god");
    std::fs::create_dir_all(legacy.join("files")).expect("legacy dirs");
    let body = b"# legacy config";
    let sum = portal::core::checksum::sha256_bytes(body);
    std::fs::write(legacy.join("files/CLAUDE.md"), body).expect("legacy file");
    let manifest = serde_json::json!({
        "version": 1, "name": "god", "created_at": "2026-04-23T00:00:00Z",
        "load_count": 0, "description": "", "tags": [],
        "files": { "CLAUDE.md": { "checksum": sum, "size": body.len(), "source": "user" } },
        "excluded_patterns": []
    });
    std::fs::write(
        legacy.join("portal.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .expect("legacy manifest");

    let fix = FixAction::MigrateLegacyRoot {
        name: "god".into(),
        dir: legacy.clone(),
    };
    doctor::apply_fix(&paths, &fix).expect("migrate");

    assert!(paths.profile_manifest("god").exists(), "manifest imported");
    assert!(portal::storage::cas::exists(&paths, &sum), "object in CAS");
    assert!(!legacy.join("files").exists(), "legacy files tree consumed");
}

#[test]
fn delete_legacy_root_removes_dir() {
    let (_tmp, paths) = sandbox();
    let dir = paths.legacy_root();
    std::fs::create_dir_all(dir.join("profiles/old")).expect("legacy");
    doctor::apply_fix(&paths, &FixAction::DeleteLegacyRoot { dir: dir.clone() }).expect("delete");
    assert!(!dir.exists());
}

#[test]
fn managed_dirs_table_reflects_disk() {
    let (_tmp, paths) = sandbox();
    std::fs::create_dir_all(paths.claude_root().join("skills/foo")).expect("skill dir");
    std::fs::write(paths.claude_root().join("skills/foo/skill.md"), "x").expect("skill file");

    let report = doctor::diagnose(&paths).expect("diagnose");
    let skills = report
        .managed_dirs
        .iter()
        .find(|r| r.dir == "skills")
        .expect("skills row");
    assert_eq!(skills.category, "skills");
    assert!(skills.exists);
    assert!(skills.file_count >= 1);
}
