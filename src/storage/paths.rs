use std::path::PathBuf;

/// Resolved paths for all Portal storage locations.
#[derive(Debug, Clone)]
pub struct PortalPaths {
    home: PathBuf,
    /// Override for the managed `.claude` directory. When set, `claude_root()`
    /// returns this path instead of `$HOME/.claude`. Persisted in config.
    claude_override: Option<PathBuf>,
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
        Self {
            home,
            claude_override: None,
        }
    }

    /// Create paths rooted at a specific home directory (useful for testing).
    #[must_use]
    pub const fn with_home(home: PathBuf) -> Self {
        Self {
            home,
            claude_override: None,
        }
    }

    /// Override which `.claude` directory this instance manages.
    #[must_use]
    pub fn with_claude_override(mut self, dir: PathBuf) -> Self {
        self.claude_override = Some(dir);
        self
    }

    /// The home directory this instance is rooted at.
    #[must_use]
    pub fn home(&self) -> &std::path::Path {
        &self.home
    }

    #[must_use]
    pub fn portal_root(&self) -> PathBuf {
        self.home.join(".config/portal")
    }

    /// Legacy pre-XDG storage root (`~/.portal`). Older portal versions kept
    /// profiles here; the current binary never reads it. `portal doctor`
    /// detects leftovers so they can be migrated or removed.
    #[must_use]
    pub fn legacy_root(&self) -> PathBuf {
        self.home.join(".portal")
    }

    /// Git working tree used as a per-profile history store (Phase 3). One
    /// repo, one orphan branch per profile. Distinct from `~/.claude` — git
    /// here records history and never drives the live config.
    #[must_use]
    pub fn history_dir(&self) -> PathBuf {
        self.portal_root().join("history")
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
    pub fn objects_root(&self) -> PathBuf {
        self.portal_root().join("objects")
    }

    /// Path to a content-addressed object given its `sha256:<hex>` hash.
    /// Splits on the first two hex chars to keep directory entries bounded.
    #[must_use]
    pub fn object_path(&self, hash: &str) -> PathBuf {
        let hex = hash.strip_prefix("sha256:").unwrap_or(hash);
        let (prefix, rest) = hex.split_at(hex.len().min(2));
        self.objects_root().join(prefix).join(rest)
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
        self.claude_override
            .clone()
            .unwrap_or_else(|| self.home.join(".claude"))
    }

    #[must_use]
    pub fn claude_old(&self) -> PathBuf {
        let claude = self.claude_root();
        let parent = claude.parent().unwrap_or_else(|| std::path::Path::new("/"));
        let stem = claude.file_name().unwrap_or_default().to_string_lossy();
        parent.join(format!("{stem}.portal-old"))
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
        std::fs::create_dir_all(self.objects_root())?;
        Ok(())
    }
}
