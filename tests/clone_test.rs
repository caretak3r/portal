#![allow(clippy::unwrap_used)]

use portal::core::clone::{self, Category, CloneOptions};
use portal::storage::paths::PortalPaths;
use portal::storage::{cas, manifest};
use tempfile::TempDir;

/// Helper: read a file's content out of the CAS by looking up its hash in the
/// cloned profile's manifest. Tests use this instead of reading from the
/// legacy `profile_files_dir`, which no longer exists in CAS-mode profiles.
fn read_cloned(paths: &PortalPaths, profile: &str, rel: &str) -> Option<Vec<u8>> {
    let mf = manifest::read(&paths.profile_manifest(profile)).ok()?;
    let entry = mf.files.get(rel)?;
    std::fs::read(paths.object_path(&entry.checksum)).ok()
}

fn cloned_has(paths: &PortalPaths, profile: &str, rel: &str) -> bool {
    manifest::read(&paths.profile_manifest(profile))
        .map(|mf| {
            mf.files
                .get(rel)
                .is_some_and(|e| cas::exists(paths, &e.checksum))
        })
        .unwrap_or(false)
}

fn setup_source_profile(paths: &PortalPaths) {
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "# My Config\nCustom content").unwrap();
    std::fs::write(
        claude.join("settings.json"),
        r#"{"permissions":{"allow":true}}"#,
    )
    .unwrap();
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
        file_picks: None,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    assert!(result.files_cloned > 0);
    assert_eq!(result.files_skipped, 0);
    assert!(paths.profile_dir("full-clone").exists());
    assert!(paths.profile_manifest("full-clone").exists());

    // Verify CLAUDE.md was copied (content lives in the CAS pool, not files/).
    let content = read_cloned(&paths, "full-clone", "CLAUDE.md")
        .map(|b| String::from_utf8_lossy(&b).to_string())
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
        file_picks: None,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    assert!(result.files_cloned > 0);
    assert!(result.files_skipped > 0);

    assert!(cloned_has(
        &paths,
        "skills-only",
        "skills/red-team/SKILL.md"
    ));
    assert!(!cloned_has(&paths, "skills-only", "rules/security.md"));
    assert!(!cloned_has(&paths, "skills-only", "CLAUDE.md"));
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
        file_picks: None,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    assert!(!cloned_has(&paths, "no-memory", "memory/notes.md"));
    assert!(cloned_has(&paths, "no-memory", "skills/red-team/SKILL.md"));
    assert!(cloned_has(&paths, "no-memory", "rules/security.md"));
    assert!(cloned_has(&paths, "no-memory", "CLAUDE.md"));

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
        file_picks: None,
    };
    let result = clone::clone_profile(&paths, &opts).unwrap();

    let content = read_cloned(&paths, "fresh-md", "CLAUDE.md").unwrap();
    assert!(content.is_empty());

    assert!(cloned_has(&paths, "fresh-md", "skills/red-team/SKILL.md"));

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
        file_picks: None,
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

#[test]
fn picker_group_key_collapses_nested_dirs() {
    use clone::picker_group_key;
    // Nested skill files collapse to the skill directory.
    assert_eq!(picker_group_key("skills/foo/SKILL.md"), "skills/foo");
    assert_eq!(
        picker_group_key("skills/foo/references/bar.md"),
        "skills/foo"
    );
    assert_eq!(picker_group_key("agents/team/lead.md"), "agents/team");
    // Flat entries are unchanged.
    assert_eq!(picker_group_key("rules/security.md"), "rules/security.md");
    assert_eq!(picker_group_key("commands/deploy.md"), "commands/deploy.md");
    assert_eq!(picker_group_key("CLAUDE.md"), "CLAUDE.md");
}

#[test]
fn clone_includes_all_files_under_a_picked_skill() {
    use std::collections::{HashMap, HashSet};

    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    std::fs::create_dir_all(claude.join("skills/multi/references")).unwrap();
    std::fs::write(claude.join("skills/multi/SKILL.md"), "# multi").unwrap();
    std::fs::write(claude.join("skills/multi/references/extra.md"), "# extra").unwrap();
    std::fs::create_dir_all(claude.join("skills/solo")).unwrap();
    std::fs::write(claude.join("skills/solo/SKILL.md"), "# solo").unwrap();
    portal::core::snapshot::save(&paths, "src", "", &[]).unwrap();

    // Simulate the picker selecting only the "skills/multi" row, which the TUI
    // expands to every file that skill covers.
    let mut picks: HashMap<Category, HashSet<String>> = HashMap::new();
    picks.insert(
        Category::Skills,
        HashSet::from([
            "skills/multi/SKILL.md".to_string(),
            "skills/multi/references/extra.md".to_string(),
        ]),
    );

    let opts = CloneOptions {
        source: "src",
        target: "dst",
        description: "",
        only: Some(vec![Category::Skills]),
        without: None,
        fresh_claude_md: false,
        file_picks: Some(picks),
    };
    clone::clone_profile(&paths, &opts).unwrap();

    let mf = manifest::read(&paths.profile_manifest("dst")).unwrap();
    assert!(mf.files.contains_key("skills/multi/SKILL.md"));
    assert!(mf.files.contains_key("skills/multi/references/extra.md"));
    assert!(
        !mf.files.contains_key("skills/solo/SKILL.md"),
        "a deselected skill's files must be excluded"
    );
}
