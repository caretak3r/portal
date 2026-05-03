#![allow(clippy::unwrap_used, clippy::expect_used)]

use portal::storage::paths::PortalPaths;
use portal::storage::state;

/// Helper: build a tempdir-rooted Portal home with a `~/.claude/` skeleton ready
/// to snapshot. Returns the paths plus the temp dir guard.
fn fresh_home() -> (tempfile::TempDir, PortalPaths) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().expect("ensure_dirs");
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).expect("create skeleton");
    (tmp, paths)
}

/// Stamp `~/.claude/CLAUDE.md` so the next snapshot has unique content per profile.
fn stamp_claude_md(paths: &PortalPaths, body: &str) {
    std::fs::write(paths.claude_root().join("CLAUDE.md"), body).expect("write CLAUDE.md");
}

/// Loading B after A records A as `previous_profile`; toggling back to A then
/// records B. The pair flips on every load.
#[test]
fn toggle_swaps_active_and_previous() {
    let (_tmp, paths) = fresh_home();

    stamp_claude_md(&paths, "alpha config");
    portal::core::snapshot::save(&paths, "alpha", "alpha", &[]).expect("save alpha");

    stamp_claude_md(&paths, "beta config");
    portal::core::snapshot::save(&paths, "beta", "beta", &[]).expect("save beta");

    // Load alpha first so the state machine has an active profile to swap from.
    portal::core::loader::load(&paths, "alpha", true, true).expect("load alpha");
    let s = state::read(&paths.state_file()).expect("read state");
    assert_eq!(s.active_profile.as_deref(), Some("alpha"));
    assert!(
        s.previous_profile.is_none(),
        "no previous profile until a second load happens"
    );

    // Loading beta makes alpha the previous.
    portal::core::loader::load(&paths, "beta", true, true).expect("load beta");
    let s = state::read(&paths.state_file()).expect("read state");
    assert_eq!(s.active_profile.as_deref(), Some("beta"));
    assert_eq!(s.previous_profile.as_deref(), Some("alpha"));

    // Loading the previous_profile (the toggle action) flips them.
    let target = s.previous_profile.expect("previous set");
    portal::core::loader::load(&paths, &target, true, true).expect("toggle back");
    let s = state::read(&paths.state_file()).expect("read state");
    assert_eq!(s.active_profile.as_deref(), Some("alpha"));
    assert_eq!(s.previous_profile.as_deref(), Some("beta"));
}

/// Loading the already-active profile must not clobber `previous_profile` —
/// otherwise the user loses their toggle target by re-saving over the same one.
#[test]
fn loading_active_profile_preserves_previous() {
    let (_tmp, paths) = fresh_home();

    stamp_claude_md(&paths, "alpha config");
    portal::core::snapshot::save(&paths, "alpha", "alpha", &[]).expect("save alpha");

    stamp_claude_md(&paths, "beta config");
    portal::core::snapshot::save(&paths, "beta", "beta", &[]).expect("save beta");

    portal::core::loader::load(&paths, "alpha", true, true).expect("load alpha");
    portal::core::loader::load(&paths, "beta", true, true).expect("load beta");

    // Re-load the active profile (beta) — toggle history must survive.
    portal::core::loader::load(&paths, "beta", true, true).expect("re-load beta");
    let s = state::read(&paths.state_file()).expect("read state");
    assert_eq!(s.active_profile.as_deref(), Some("beta"));
    assert_eq!(
        s.previous_profile.as_deref(),
        Some("alpha"),
        "re-loading the active profile must not overwrite previous_profile"
    );
}

/// Deleting a profile that was being held as `previous_profile` must clear the
/// pointer — otherwise `portal toggle` would later try to load a ghost.
#[test]
fn deleting_previous_profile_clears_pointer() {
    let (_tmp, paths) = fresh_home();

    stamp_claude_md(&paths, "alpha config");
    portal::core::snapshot::save(&paths, "alpha", "alpha", &[]).expect("save alpha");
    stamp_claude_md(&paths, "beta config");
    portal::core::snapshot::save(&paths, "beta", "beta", &[]).expect("save beta");

    portal::core::loader::load(&paths, "alpha", true, true).expect("load alpha");
    portal::core::loader::load(&paths, "beta", true, true).expect("load beta");

    // Manually mimic the cli `rm` cleanup: drop the dir, then sweep state.
    std::fs::remove_dir_all(paths.profile_dir("alpha")).expect("rm alpha dir");
    let mut s = state::read(&paths.state_file()).expect("read state");
    if s.previous_profile.as_deref() == Some("alpha") {
        s.previous_profile = None;
        state::write(&paths.state_file(), &s).expect("write state");
    }

    let s = state::read(&paths.state_file()).expect("read state");
    assert_eq!(s.active_profile.as_deref(), Some("beta"));
    assert!(s.previous_profile.is_none());
}
