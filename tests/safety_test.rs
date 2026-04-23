use portal::core::{safety, skeleton};
use portal::storage::paths::PortalPaths;

#[test]
fn test_preflight_no_claude_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    let result = safety::preflight_load(&paths, "test");
    assert!(result.is_err());
}

#[test]
fn test_preflight_missing_profile() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    skeleton::create(&paths.claude_root()).unwrap();

    let result = safety::preflight_load(&paths, "nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_file_lock() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let lock = safety::acquire_lock(&paths).unwrap();
    assert!(paths.lock_file().exists());
    drop(lock);
    assert!(!paths.lock_file().exists());
}
