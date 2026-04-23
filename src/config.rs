use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level portal configuration, loaded from `portal.config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortalConfig {
    #[serde(default)]
    pub backup: BackupConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
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
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let cfg = load(Path::new("/tmp/nonexistent-portal-config.toml"));
        assert!(cfg.is_ok());
    }

    #[test]
    fn load_partial_toml() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(
            tmp.path(),
            "[backup]\nmax_count = 5\n",
        )
        .expect("write");
        let cfg = load(tmp.path()).expect("load");
        assert_eq!(cfg.backup.max_count, 5);
        assert_eq!(cfg.backup.max_age_days, 90); // default
        assert_eq!(cfg.plugins.reinstall_timeout_secs, 30); // default
    }
}
