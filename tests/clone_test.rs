#![allow(clippy::unwrap_used)]

use portal::core::clone::{self, Category, CloneOptions};
use portal::storage::paths::PortalPaths;
use tempfile::TempDir;

fn setup_source_profile(paths: &PortalPaths) {
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "# My Config\nCustom content").unwrap();
    std::fs::write(claude.join("settings.json"), r#"{"permissions":{"allow":true}}"#).unwrap();
    std::fs::create_dir_all(claude.join("skills/red-team")).unwrap();
    std::fs::write(claude.join("skills/red-team/SKILL.md"), "# Red Team").unwrap();
    std::fs::create_dir_all(claude.join("rules")).unwrap();
    std::fs::write(claude.join("rules/security.md"), "# Security rules").unwrap();
    std::fs::create_dir_all(claude.join("memory")).unwrap();
    std::fs::write(claude.join("memory/notes.md"), "# Notes").unwrap();
    std::fs::create_dir_all(claude.join("commands")).unwrap();
    std::fs::write(claude.join("commands/deploy.md"), "# Deploy").unwrap();
    std::fs::create_dir_all(claude.join("agents")).unwrap();
    std::fs::write(claude.join("agents/reviewer.md"), "# Reviewer").unwrap();
    std::fs::create_dir_all(claude.join("hooks")).unwrap();
    std::fs::write(claude.join("hooks/pre-commit.sh"), "#!/bin/sh").unwrap();

    portal::core::snapshot::save(paths, "source", "Source profile", &[]).unwrap();
}

#[test]
fn test_clone_all() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    setup_source_profile(&paths);

    let opts = CloneOptions {
        source: "source",
        target: "full-clone",
        description: "Full clone",
        only: None,
        without: None,
        fresh_claude_md: false,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    assert!(result.files_cloned > 0);
    assert_eq!(result.files_skipped, 0);
    assert!(paths.profile_dir("full-clone").exists());
    assert!(paths.profile_manifest("full-clone").exists());

    // Verify CLAUDE.md was copied.
    let content = std::fs::read_to_string(
        paths.profile_files_dir("full-clone").join("CLAUDE.md"),
    )
    .unwrap();
    assert!(content.contains("Custom content"));
}

#[test]
fn test_clone_only_skills() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    setup_source_profile(&paths);

    let opts = CloneOptions {
        source: "source",
        target: "skills-only",
        description: "Skills only",
        only: Some(vec![Category::Skills]),
        without: None,
        fresh_claude_md: false,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    assert!(result.files_cloned > 0);
    assert!(result.files_skipped > 0);

    // Skills should exist.
    assert!(paths
        .profile_files_dir("skills-only")
        .join("skills/red-team/SKILL.md")
        .exists());

    // Rules should NOT exist.
    assert!(!paths
        .profile_files_dir("skills-only")
        .join("rules/security.md")
        .exists());

    // CLAUDE.md should NOT exist (not in --only).
    assert!(!paths
        .profile_files_dir("skills-only")
        .join("CLAUDE.md")
        .exists());
}

#[test]
fn test_clone_without_memory() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    setup_source_profile(&paths);

    let opts = CloneOptions {
        source: "source",
        target: "no-memory",
        description: "Without memory",
        only: None,
        without: Some(vec![Category::Memory]),
        fresh_claude_md: false,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    // Memory should NOT exist.
    assert!(!paths
        .profile_files_dir("no-memory")
        .join("memory/notes.md")
        .exists());

    // Everything else should.
    assert!(paths
        .profile_files_dir("no-memory")
        .join("skills/red-team/SKILL.md")
        .exists());
    assert!(paths
        .profile_files_dir("no-memory")
        .join("rules/security.md")
        .exists());
    assert!(paths
        .profile_files_dir("no-memory")
        .join("CLAUDE.md")
        .exists());

    assert!(result.files_skipped > 0);
}

#[test]
fn test_clone_fresh_claude_md() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    setup_source_profile(&paths);

    let opts = CloneOptions {
        source: "source",
        target: "fresh-md",
        description: "Fresh CLAUDE.md",
        only: None,
        without: None,
        fresh_claude_md: true,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    // CLAUDE.md should exist but be empty.
    let content = std::fs::read_to_string(
        paths.profile_files_dir("fresh-md").join("CLAUDE.md"),
    )
    .unwrap();
    assert!(content.is_empty());

    // Skills should still be there.
    assert!(paths
        .profile_files_dir("fresh-md")
        .join("skills/red-team/SKILL.md")
        .exists());

    assert!(result.files_cloned > 0);
}

#[test]
fn test_clone_target_already_exists() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    setup_source_profile(&paths);

    // Clone once.
    let opts = CloneOptions {
        source: "source",
        target: "dupe",
        description: "",
        only: None,
        without: None,
        fresh_claude_md: false,
    };
    clone::clone_profile(&paths, &opts).unwrap();

    // Clone again — should fail.
    let result = clone::clone_profile(&paths, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));
}

#[test]
fn test_parse_categories() {
    let cats = clone::parse_categories("skills,rules,memory").unwrap();
    assert_eq!(cats.len(), 3);
    assert!(cats.contains(&Category::Skills));
    assert!(cats.contains(&Category::Rules));
    assert!(cats.contains(&Category::Memory));
}

#[test]
fn test_parse_categories_invalid() {
    let result = clone::parse_categories("skills,bogus");
    assert!(result.is_err());
}
