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

/// Verify that Unix exec bits set on a hook script survive a save → load cycle.
/// Regression test for the "hooks broke, permissions changed" bug where CAS
/// placement and std::fs::copy both silently dropped mode bits.
#[test]
fn test_load_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = portal::storage::paths::PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).expect("create skeleton");

    std::fs::create_dir_all(claude.join("hooks")).expect("create hooks dir");
    let hook = claude.join("hooks/my-hook.sh");
    std::fs::write(&hook, "#!/bin/sh\necho hello\n").expect("write hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod +x");

    portal::core::snapshot::save(&paths, "perm-profile", "test", &[]).expect("save");

    // Corrupt the exec bit to prove load restores it.
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o644))
        .expect("remove exec bit");

    portal::core::loader::load(&paths, "perm-profile", true, true).expect("load");

    let mode = std::fs::metadata(&hook)
        .expect("hook metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755, "exec bit must survive save/load cycle");
}

/// Verify that runtime infrastructure dirs present in the old ~/.claude
/// (excluded from profile snapshots: .git, plugins/cache, projects, etc.)
/// survive a profile load unchanged.
#[test]
fn test_load_preserves_runtime_dirs() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = portal::storage::paths::PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).expect("create skeleton");

    // Plant a fake .git and a plugins/cache entry to represent runtime infra.
    let git_dir = claude.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create .git");
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("write HEAD");

    let cache_dir = claude.join("plugins/cache/some-plugin");
    std::fs::create_dir_all(&cache_dir).expect("create plugins/cache");
    std::fs::write(cache_dir.join("plugin.bin"), b"binary").expect("write plugin binary");

    // Save a profile (snapshot excludes .git and plugins/cache per EXCLUDED_PATTERNS).
    portal::core::snapshot::save(&paths, "rt-profile", "runtime test", &[]).expect("save");

    // Wipe state as portal's atomic swap would do, then load.
    portal::core::loader::load(&paths, "rt-profile", true, true).expect("load");

    // .git must survive the profile swap.
    assert!(
        claude.join(".git/HEAD").exists(),
        ".git/HEAD must be preserved across profile load"
    );
    // plugins/cache must survive too.
    assert!(
        claude.join("plugins/cache/some-plugin/plugin.bin").exists(),
        "plugins/cache must be preserved across profile load"
    );
}
