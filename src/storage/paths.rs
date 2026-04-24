use std::path::PathBuf;

/// Resolved paths for all Portal storage locations.
#[derive(Debug, Clone)]
pub struct PortalPaths {
    home: PathBuf,
}

impl PortalPaths {
    /// Detect home directory from the environment.
    ///
    /// # Panics
    ///
    /// Panics if the home directory cannot be determined.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn detect() -> Self {
        let home = dirs::home_dir().expect("cannot detect home directory");
        Self { home }
    }

    /// Create paths rooted at a specific home directory (useful for testing).
    #[must_use]
    pub const fn with_home(home: PathBuf) -> Self {
        Self { home }
    }

    #[must_use]
    pub fn portal_root(&self) -> PathBuf {
        self.home.join(".config/portal")
    }

    #[must_use]
    pub fn profiles_root(&self) -> PathBuf {
        self.portal_root().join("profiles")
    }

    #[must_use]
    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_root().join(name)
    }

    #[must_use]
    pub fn profile_files_dir(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("files")
    }

    #[must_use]
    pub fn profile_manifest(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("portal.json")
    }

    #[must_use]
    pub fn profile_plugins(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("plugins.json")
    }

    #[must_use]
    pub fn profile_meta(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("meta.json")
    }

    #[must_use]
    pub fn skeleton_dir(&self) -> PathBuf {
        self.portal_root().join("skeleton")
    }

    #[must_use]
    pub fn skeleton_files_dir(&self) -> PathBuf {
        self.skeleton_dir().join("files")
    }

    #[must_use]
    pub fn skeleton_manifest(&self) -> PathBuf {
        self.skeleton_dir().join("skeleton.json")
    }

    #[must_use]
    pub fn backups_dir(&self) -> PathBuf {
        self.portal_root().join("backups")
    }

    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.portal_root().join("portal.state.json")
    }

    #[must_use]
    pub fn lock_file(&self) -> PathBuf {
        self.portal_root().join(".portal.lock")
    }

    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.portal_root().join("portal.config.toml")
    }

    #[must_use]
    pub fn exclude_file(&self) -> PathBuf {
        self.portal_root().join("portal.exclude")
    }

    #[must_use]
    pub fn claude_root(&self) -> PathBuf {
        self.home.join(".claude")
    }

    #[must_use]
    pub fn claude_old(&self) -> PathBuf {
        self.home.join(".claude.portal-old")
    }

    /// Create all required Portal directories.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails (permissions, disk full, etc.).
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.profiles_root())?;
        std::fs::create_dir_all(self.skeleton_dir())?;
        std::fs::create_dir_all(self.backups_dir())?;
        Ok(())
    }
}
