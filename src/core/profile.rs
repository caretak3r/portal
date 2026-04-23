use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Profile manifest — stored as `portal.json` in each profile directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileManifest {
    pub version: u32,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_loaded: Option<DateTime<Utc>>,
    pub load_count: u64,
    pub description: String,
    pub tags: Vec<String>,
    pub files: HashMap<String, FileEntry>,
    pub excluded_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub checksum: String,
    pub size: u64,
    pub source: FileSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileSource {
    User,
    Skeleton,
}

/// Profile metadata — stored as `meta.json`, human-editable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub description: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub created_by: String,
}

/// Global state — stored as `portal.state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalState {
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_operation: Option<LastOperation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skeleton_checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastOperation {
    #[serde(rename = "type")]
    pub op_type: OperationType,
    pub profile: String,
    pub timestamp: DateTime<Utc>,
    pub backup_path: String,
    pub plugins_installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OperationType {
    Load,
    Reset,
    Undo,
}

/// Plugin blueprint — stored as `plugins.json` in each profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginBlueprint {
    pub version: u32,
    pub plugins: Vec<PluginEntry>,
    #[serde(default)]
    pub extra_known_marketplaces: HashMap<String, MarketplaceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub id: String,
    pub enabled: bool,
    pub source: PluginSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginSource {
    Marketplace { marketplace: String, repo: String },
    Local { path: String },
    Github { repo: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntry {
    pub source: MarketplaceSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum MarketplaceSource {
    Github { repo: String },
    Directory { path: String },
}

impl Default for PortalState {
    fn default() -> Self {
        Self {
            version: 1,
            active_profile: None,
            last_operation: None,
            skeleton_checksum: None,
        }
    }
}

impl Default for PluginBlueprint {
    fn default() -> Self {
        Self {
            version: 1,
            plugins: Vec::new(),
            extra_known_marketplaces: HashMap::new(),
        }
    }
}
