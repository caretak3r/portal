use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level portal configuration, loaded from `portal.config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortalConfig {
    #[serde(default)]
    pub backup: BackupConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub history: HistoryConfig,
    /// The `.claude` directory this portal instance manages.
    /// Confirmed on first run and persisted here; overrides `$HOME/.claude`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_dir: Option<PathBuf>,
}

/// Color theme for the TUI. Themes only affect rendering — keybindings,
/// layout, and behaviour are identical across themes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Default ratatui colours (terminal palette).
    #[default]
    Default,
    /// Catppuccin Mocha — dark, warm purple-pink accents.
    CatppuccinMocha,
    /// Tokyo Night — dark, blue + magenta highlights.
    TokyoNight,
    /// Solarized Dark — desaturated cyan/yellow on slate.
    SolarizedDark,
    /// Gruvbox Dark — earthy browns + orange.
    GruvboxDark,
}

impl Theme {
    /// All available themes, ordered for the TUI picker.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Default,
            Self::CatppuccinMocha,
            Self::TokyoNight,
            Self::SolarizedDark,
            Self::GruvboxDark,
        ]
    }

    /// Display label for the picker.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::TokyoNight => "Tokyo Night",
            Self::SolarizedDark => "Solarized Dark",
            Self::GruvboxDark => "Gruvbox Dark",
        }
    }
}

/// UI / TUI configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub theme: Theme,
}

/// Backup-related configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    #[serde(default = "default_max_count")]
    pub max_count: usize,
    #[serde(default = "default_max_age_days")]
    pub max_age_days: u32,
    #[serde(default = "default_compression")]
    pub compression: String,
    #[serde(default = "default_compression_level")]
    pub compression_level: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            max_count: default_max_count(),
            max_age_days: default_max_age_days(),
            compression: default_compression(),
            compression_level: default_compression_level(),
        }
    }
}

/// Plugin-related configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default = "default_reinstall_timeout")]
    pub reinstall_timeout_secs: u64,
    #[serde(default)]
    pub retry_failed_on_status: bool,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            reinstall_timeout_secs: default_reinstall_timeout(),
            retry_failed_on_status: false,
        }
    }
}

/// Per-profile git history configuration.
///
/// When enabled (the default), every save/clone records a commit on the
/// profile's orphan branch in the history repo. Purely additive — git here
/// never drives the live `~/.claude`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "default_history_enabled")]
    pub enabled: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_history_enabled(),
        }
    }
}

const fn default_history_enabled() -> bool {
    true
}

const fn default_max_count() -> usize {
    10
}
const fn default_max_age_days() -> u32 {
    90
}
fn default_compression() -> String {
    "zstd".to_string()
}
const fn default_compression_level() -> u32 {
    3
}
const fn default_reinstall_timeout() -> u64 {
    30
}

/// Persist configuration to a TOML file, creating parent directories as needed.
///
/// # Errors
///
/// Returns an error if serialization fails or the file cannot be written.
pub fn save(config: &PortalConfig, path: &Path) -> Result<()> {
    let content = toml::to_string_pretty(config)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

/// Load configuration from a TOML file, returning defaults if the file does not exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load(path: &Path) -> Result<PortalConfig> {
    if !path.exists() {
        return Ok(PortalConfig::default());
    }
    let content = std::fs::read_to_string(path)?;
    let config: PortalConfig = toml::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = PortalConfig::default();
        assert_eq!(cfg.backup.max_count, 10);
        assert_eq!(cfg.backup.max_age_days, 90);
        assert_eq!(cfg.backup.compression, "zstd");
        assert_eq!(cfg.backup.compression_level, 3);
        assert_eq!(cfg.plugins.reinstall_timeout_secs, 30);
        assert!(!cfg.plugins.retry_failed_on_status);
        assert!(cfg.history.enabled, "history is on by default");
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let cfg = load(Path::new("/tmp/nonexistent-portal-config.toml"));
        assert!(cfg.is_ok());
    }

    #[test]
    fn load_partial_toml() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), "[backup]\nmax_count = 5\n").expect("write");
        let cfg = load(tmp.path()).expect("load");
        assert_eq!(cfg.backup.max_count, 5);
        assert_eq!(cfg.backup.max_age_days, 90); // default
        assert_eq!(cfg.plugins.reinstall_timeout_secs, 30); // default
        assert!(cfg.history.enabled); // default
    }

    #[test]
    fn history_can_be_disabled_via_toml() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), "[history]\nenabled = false\n").expect("write");
        let cfg = load(tmp.path()).expect("load");
        assert!(!cfg.history.enabled);
    }
}
