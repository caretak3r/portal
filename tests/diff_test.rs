#[test]
fn test_diff_profiles() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = portal::storage::paths::PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).expect("create skeleton");

    // Save profile A.
    std::fs::write(claude.join("CLAUDE.md"), "Profile A content").expect("write CLAUDE.md");
    std::fs::create_dir_all(claude.join("rules")).expect("create rules dir");
    std::fs::write(claude.join("rules/a-only.md"), "only in A").expect("write a-only");
    portal::core::snapshot::save(&paths, "a", "Profile A", &[]).expect("save a");

    // Save profile B.
    std::fs::write(claude.join("CLAUDE.md"), "Profile B content").expect("write CLAUDE.md");
    std::fs::remove_file(claude.join("rules/a-only.md")).expect("remove a-only");
    std::fs::write(claude.join("rules/b-only.md"), "only in B").expect("write b-only");
    portal::core::snapshot::save(&paths, "b", "Profile B", &[]).expect("save b");

    let result = portal::core::diff::diff_profiles(
        &paths,
        &portal::core::diff::DiffSide::Profile("a"),
        &portal::core::diff::DiffSide::Profile("b"),
    )
    .expect("diff_profiles");

    assert!(!result.only_left.is_empty(), "should have a-only files");
    assert!(!result.only_right.is_empty(), "should have b-only files");
    assert!(
        !result.different_content.is_empty(),
        "CLAUDE.md should differ"
    );
}

#[test]
fn test_content_diff() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = portal::storage::paths::PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).expect("create skeleton");

    std::fs::write(claude.join("CLAUDE.md"), "line one\nline two\n").expect("write x");
    portal::core::snapshot::save(&paths, "x", "X", &[]).expect("save x");

    std::fs::write(claude.join("CLAUDE.md"), "line one\nline THREE\n").expect("write y");
    portal::core::snapshot::save(&paths, "y", "Y", &[]).expect("save y");

    let diff = portal::core::diff::content_diff(
        &paths,
        &portal::core::diff::DiffSide::Profile("x"),
        &portal::core::diff::DiffSide::Profile("y"),
        "CLAUDE.md",
    )
    .expect("content_diff");

    assert!(diff.contains("line two"), "should show old line");
    assert!(diff.contains("line THREE"), "should show new line");
}
