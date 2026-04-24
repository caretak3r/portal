use ratatui::widgets::ListState;

use crate::core::profile::{PluginBlueprint, ProfileManifest, ProfileMeta};
use crate::storage::{manifest, meta, paths::PortalPaths, plugins_manifest, state};

/// Which pane / overlay the TUI is currently showing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum View {
    #[default]
    Detail,
    Diff,
    SaveDialog,
    LoadConfirm,
    Help,
}

/// Aggregated info for a single profile.
pub struct ProfileInfo {
    pub name: String,
    pub manifest: ProfileManifest,
    /// Stored for future profile editing UI.
    #[expect(dead_code)]
    pub meta: Option<ProfileMeta>,
    pub blueprint: Option<PluginBlueprint>,
}

/// Root application state for the TUI.
pub struct App {
    pub paths: PortalPaths,
    pub profiles: Vec<ProfileInfo>,
    pub active_profile: Option<String>,
    pub list_state: ListState,
    pub view: View,
    pub should_quit: bool,

    // Save dialog fields
    pub save_name: String,
    pub save_description: String,
    pub save_tags: String,
    pub save_field_index: usize,

    pub status_message: Option<String>,
    pub file_scroll: u16,
}

impl App {
    /// Build initial app state by scanning profiles on disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the state file or profile directories
    /// cannot be read.
    pub fn new(paths: PortalPaths) -> anyhow::Result<Self> {
        let mut app = Self {
            paths,
            profiles: Vec::new(),
            active_profile: None,
            list_state: ListState::default(),
            view: View::default(),
            should_quit: false,
            save_name: String::new(),
            save_description: String::new(),
            save_tags: String::new(),
            save_field_index: 0,
            status_message: None,
            file_scroll: 0,
        };
        app.refresh()?;
        if !app.profiles.is_empty() {
            app.list_state.select(Some(0));
        }
        Ok(app)
    }

    /// Re-scan profiles directory and reload state.
    ///
    /// # Errors
    ///
    /// Returns an error if the state file cannot be read or a profile
    /// manifest is corrupt.
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let portal_state = state::read(&self.paths.state_file())?;
        self.active_profile = portal_state.active_profile;

        let profiles_root = self.paths.profiles_root();
        let mut profiles = Vec::new();

        if profiles_root.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(&profiles_root)?
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .collect();
            entries.sort_by_key(std::fs::DirEntry::file_name);

            for entry in entries {
                let name = entry.file_name().to_string_lossy().to_string();
                let manifest_path = self.paths.profile_manifest(&name);
                if !manifest_path.exists() {
                    continue;
                }
                let m = manifest::read(&manifest_path)?;
                let meta_result = meta::read(&self.paths.profile_meta(&name)).ok();
                let blueprint = plugins_manifest::read(&self.paths.profile_plugins(&name)).ok();
                profiles.push(ProfileInfo {
                    name,
                    manifest: m,
                    meta: meta_result,
                    blueprint,
                });
            }
        }

        self.profiles = profiles;
        Ok(())
    }

    /// Currently selected profile, if any.
    pub fn selected_profile(&self) -> Option<&ProfileInfo> {
        self.list_state
            .selected()
            .and_then(|i| self.profiles.get(i))
    }

    /// Whether the named profile is the currently active one.
    pub fn is_active(&self, name: &str) -> bool {
        self.active_profile
            .as_deref()
            .is_some_and(|a| a == name)
    }
}
