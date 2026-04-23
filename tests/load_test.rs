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
