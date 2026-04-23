use portal::core::{backup, skeleton};
use portal::storage::paths::PortalPaths;

#[test]
fn test_backup_and_restore() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "backup test content").unwrap();

    let backup_path = backup::create(&paths, "load", "test-profile").unwrap();
    assert!(backup_path.exists());

    // Modify after backup.
    std::fs::write(claude.join("CLAUDE.md"), "modified after backup").unwrap();

    // Restore should bring back the original content.
    backup::restore(&paths, &backup_path).unwrap();
    let content = std::fs::read_to_string(claude.join("CLAUDE.md")).unwrap();
    assert_eq!(content, "backup test content");
}

#[test]
fn test_backup_prune() {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    skeleton::create(&claude).unwrap();

    // Create 3 backups with distinct operation names to guarantee unique filenames.
    backup::create(&paths, "load-1", "a").unwrap();
    backup::create(&paths, "load-2", "b").unwrap();
    backup::create(&paths, "load-3", "c").unwrap();

    let before = backup::list(&paths).unwrap();
    assert_eq!(before.len(), 3, "should have 3 backups before prune");

    let pruned = backup::prune(&paths, 1).unwrap();
    assert_eq!(pruned.len(), 2);

    let remaining = backup::list(&paths).unwrap();
    assert_eq!(remaining.len(), 1);
}
