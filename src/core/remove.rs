use crate::storage::{paths::PortalPaths, state};
use anyhow::{Result, bail};

/// What state pointers were cleared as a side effect of a deletion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeleteOutcome {
    /// The deleted profile was the active one; `active_profile` was cleared.
    pub cleared_active: bool,
    /// The deleted profile was the previous one; `previous_profile` was cleared.
    pub cleared_previous: bool,
}

/// Delete a profile's *reference* — its `profiles/<name>/` directory (manifest,
/// metadata, plugin blueprint, and any materialized files) — and clear any
/// active/previous state pointers to it.
///
/// **Backups are deliberately left untouched.** The compressed `.tar.zst`
/// archives under `backups/` are independent of profiles: they capture the
/// state of `~/.claude` at load/reset time and back `portal undo`. Removing a
/// profile must never destroy that recovery path. CAS objects are likewise
/// left in place — they are shared across profiles and reclaimed separately.
///
/// # Errors
///
/// Returns an error if the profile does not exist, the directory cannot be
/// removed, or the state file cannot be read/written.
pub fn delete_profile(paths: &PortalPaths, name: &str) -> Result<DeleteOutcome> {
    let profile_dir = paths.profile_dir(name);
    if !profile_dir.exists() {
        bail!("Profile \"{name}\" not found.");
    }

    std::fs::remove_dir_all(&profile_dir)?;

    // Clear active/previous pointers so `portal toggle` (and the TUI) don't
    // reference a profile we just removed.
    let state_path = paths.state_file();
    let mut portal_state = state::read(&state_path)?;
    let cleared_active = portal_state.active_profile.as_deref() == Some(name);
    let cleared_previous = portal_state.previous_profile.as_deref() == Some(name);
    if cleared_active {
        portal_state.active_profile = None;
    }
    if cleared_previous {
        portal_state.previous_profile = None;
    }
    if cleared_active || cleared_previous {
        state::write(&state_path, &portal_state)?;
    }

    Ok(DeleteOutcome {
        cleared_active,
        cleared_previous,
    })
}
