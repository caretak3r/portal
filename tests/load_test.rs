#![allow(clippy::unwrap_used, clippy::expect_used)]

use portal::core::progress::NoProgress;

#[test]
fn test_load_profile() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = portal::storage::paths::PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).expect("create skeleton");
    std::fs::write(claude.join("CLAUDE.md"), "original config").expect("write CLAUDE.md");
    std::fs::create_dir_all(claude.join("rules")).expect("create rules dir");
    std::fs::write(claude.join("rules/test.md"), "# Rule").expect("write rule");

    portal::core::snapshot::save(&paths, "profile-a", "Profile A", &[]).expect("save profile-a");

    // Modify .claude/ to simulate different state.
    std::fs::write(claude.join("CLAUDE.md"), "modified config").expect("modify CLAUDE.md");
    std::fs::remove_file(claude.join("rules/test.md")).expect("remove rule");

    // Load profile-a back (skip Claude process check in test).
    portal::core::loader::load(&paths, "profile-a", true, true).expect("load profile-a");

    // Verify .claude/ matches profile-a.
    let content = std::fs::read_to_string(claude.join("CLAUDE.md")).expect("read CLAUDE.md");
    assert_eq!(content, "original config");
    assert!(claude.join("rules/test.md").exists());
}

/// `no_backup=true` must skip the tar.zst archive entirely. Default (`load`)
/// keeps writing one. This pins down the Phase 5 wiring of the
/// `load_with_progress` `no_backup` parameter so a future caller-side bug
/// can't silently disable backups again.
#[test]
fn no_backup_flag_skips_archive_creation() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = portal::storage::paths::PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).expect("create skeleton");
    std::fs::write(claude.join("CLAUDE.md"), "stamp").expect("write CLAUDE.md");
    portal::core::snapshot::save(&paths, "alpha", "alpha", &[]).expect("save alpha");

    let count_archives = || -> usize {
        std::fs::read_dir(paths.backups_dir())
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".tar.zst"))
                    .count()
            })
            .unwrap_or(0)
    };

    assert_eq!(count_archives(), 0, "no archives before any load");

    // Default load: backup IS created.
    portal::core::loader::load_with_progress(
        &paths,
        "alpha",
        true,  // no_plugins
        false, // no_backup
        true,  // skip_claude_check
        &NoProgress,
    )
    .expect("default load");
    assert_eq!(count_archives(), 1, "default load writes one archive");

    // no_backup=true: archive count must NOT increase.
    portal::core::loader::load_with_progress(&paths, "alpha", true, true, true, &NoProgress)
        .expect("no-backup load");
    assert_eq!(
        count_archives(),
        1,
        "no_backup=true must not create an archive"
    );

    // The returned backup_path is the sentinel `no-backup-skipped`, not a
    // real archive — verifies the LoadResult shape stays sane.
    let result =
        portal::core::loader::load_with_progress(&paths, "alpha", true, true, true, &NoProgress)
            .expect("no-backup load 2");
    assert!(
        result
            .backup_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "no-backup-skipped"),
        "expected sentinel backup path, got {}",
        result.backup_path.display()
    );
}
