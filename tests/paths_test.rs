#[test]
fn test_portal_paths_resolve() {
    let paths = portal::storage::paths::PortalPaths::with_home("/tmp/test-home".into());
    assert_eq!(
        paths.portal_root().to_str().expect("portal_root"),
        "/tmp/test-home/.portal"
    );
    assert_eq!(
        paths.claude_root().to_str().expect("claude_root"),
        "/tmp/test-home/.claude"
    );
    assert_eq!(
        paths.profile_dir("work").to_str().expect("profile_dir"),
        "/tmp/test-home/.portal/profiles/work"
    );
    assert_eq!(
        paths
            .profile_files_dir("work")
            .to_str()
            .expect("profile_files_dir"),
        "/tmp/test-home/.portal/profiles/work/files"
    );
}
