use portal::storage::paths::PortalPaths;
use tempfile::TempDir;

#[test]
fn test_export_and_import() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create a .claude/ and save a profile.
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "export test config").unwrap();
    std::fs::create_dir_all(claude.join("rules")).unwrap();
    std::fs::write(claude.join("rules/test.md"), "# Rule").unwrap();
    portal::core::snapshot::save(&paths, "export-test", "Export test", &[]).unwrap();

    // Export it.
    let export_dir = tmp.path().join("exports");
    std::fs::create_dir_all(&export_dir).unwrap();
    let archive = portal::core::transport::export(&paths, "export-test", &export_dir).unwrap();
    assert!(archive.exists());
    assert!(archive.to_string_lossy().ends_with(".portal.tar.zst"));

    // Import into a different portal instance.
    let tmp2 = TempDir::new().unwrap();
    let paths2 = PortalPaths::with_home(tmp2.path().to_path_buf());
    paths2.ensure_dirs().unwrap();

    let imported_name = portal::core::transport::import(&paths2, &archive, false).unwrap();
    assert_eq!(imported_name, "export-test");

    // Verify the imported profile has the right files.
    let manifest_path = paths2.profile_manifest("export-test");
    assert!(manifest_path.exists());
    let m = portal::storage::manifest::read(&manifest_path).unwrap();
    assert!(m.files.contains_key("CLAUDE.md"));
    assert!(m.files.contains_key("rules/test.md"));

    // Verify file content was preserved.
    let claude_md =
        std::fs::read_to_string(paths2.profile_files_dir("export-test").join("CLAUDE.md")).unwrap();
    assert_eq!(claude_md, "export test config");
}

#[test]
fn test_import_refuses_existing_without_force() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create and save a profile.
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    portal::core::snapshot::save(&paths, "dupe-test", "Test", &[]).unwrap();

    // Export it.
    let archive = portal::core::transport::export(&paths, "dupe-test", tmp.path()).unwrap();

    // Import should fail because profile already exists.
    let result = portal::core::transport::import(&paths, &archive, false);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));

    // With overwrite=true it should succeed.
    let result = portal::core::transport::import(&paths, &archive, true);
    assert!(result.is_ok());
}

#[test]
fn test_export_nonexistent_profile() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let result = portal::core::transport::export(&paths, "nope", tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn test_import_invalid_archive() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create a file that isn't a valid archive.
    let bad_archive = tmp.path().join("bad.tar.zst");
    std::fs::write(&bad_archive, "not a real archive").unwrap();

    let result = portal::core::transport::import(&paths, &bad_archive, false);
    assert!(result.is_err());
}
