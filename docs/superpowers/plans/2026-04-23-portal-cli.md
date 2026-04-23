# Portal CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI+TUI tool for saving, switching, and diffing `.claude` configuration profiles with atomic swap safety, producing two TUI mockup branches for comparison.

**Architecture:** Single crate with `core/` (engine), `storage/` (persistence), `cli.rs` (clap commands), and `tui/` (UI). Core + CLI built on `main`, TUI variants on separate git worktrees (`tui/ratatui` and `tui/ftui`). The `tui/` module is feature-gated so the binary compiles without either TUI.

**Tech Stack:** Rust 1.95+, clap 4 (derive), ratatui 0.30 / ftui 0.3.1 (git), serde/serde_json, sha2, tempfile, walkdir, tar, zstd, chrono, similar 2, dialoguer, indicatif, tracing

**Worktree Strategy:**
- `main` → core engine + CLI commands (no TUI)
- `tui/ratatui` worktree → `src/tui/` with ratatui 0.30 + crossterm
- `tui/ftui` worktree → `src/tui/` with ftui (Elm-style Model trait)

---

## File Structure

```
portal/
├── Cargo.toml                    # Single crate, feature-gated TUI
├── deny.toml                     # cargo-deny config (security)
├── clippy.toml                   # Strict lints
├── rustfmt.toml                  # Formatting
├── .gitignore
├── src/
│   ├── main.rs                   # Entry point: dispatch CLI or TUI
│   ├── cli.rs                    # Clap command definitions + handlers
│   ├── config.rs                 # portal.config.toml parsing
│   ├── core/
│   │   ├── mod.rs                # Re-exports
│   │   ├── profile.rs            # Profile type + CRUD operations
│   │   ├── skeleton.rs           # Skeleton definition + creation
│   │   ├── snapshot.rs           # Save engine (scan, copy, hash, blueprint)
│   │   ├── loader.rs             # Load engine (atomic swap)
│   │   ├── diff.rs               # Diff engine (4 levels)
│   │   ├── checksum.rs           # SHA-256 computation + verification
│   │   ├── backup.rs             # tar.zst backup + restore + pruning
│   │   ├── plugins.rs            # Plugin blueprint extraction + reinstall
│   │   └── safety.rs             # Pre-flight checks + file lock
│   ├── storage/
│   │   ├── mod.rs                # Re-exports
│   │   ├── paths.rs              # Path resolution (~/.portal/, ~/.claude/)
│   │   ├── manifest.rs           # portal.json read/write
│   │   ├── plugins_manifest.rs   # plugins.json read/write
│   │   ├── state.rs              # portal.state.json read/write
│   │   └── meta.rs               # meta.json read/write
│   └── tui/                      # DIFFERS PER WORKTREE
│       ├── mod.rs
│       ├── app.rs                # TUI application state
│       ├── ui.rs                 # Rendering (split-pane layout)
│       └── event.rs              # Input handling
└── tests/
    ├── integration/
    │   ├── save_test.rs
    │   ├── load_test.rs
    │   ├── diff_test.rs
    │   └── safety_test.rs
    └── fixtures/
        └── skeleton/             # Test skeleton reference
```

---

## Task 1: Project Scaffold + Security Config

**Files:**
- Create: `Cargo.toml`
- Create: `deny.toml`
- Create: `clippy.toml`
- Create: `rustfmt.toml`
- Create: `.gitignore`
- Create: `src/main.rs`

- [ ] **Step 1: Initialize git repo**

```bash
cd /Users/rohit/Documents/portal
git init
```

- [ ] **Step 2: Create Cargo.toml**

```toml
[package]
name = "portal"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
description = "Configuration transport layer for Claude Code"
license = "MIT"

[features]
default = []
tui-ratatui = ["dep:ratatui", "dep:crossterm"]
tui-ftui = ["dep:ftui"]

[dependencies]
# CLI
clap = { version = "4", features = ["derive", "env"] }
dialoguer = "0.11"
console = "0.15"
indicatif = "0.17"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Core
sha2 = "0.10"
tempfile = "3"
walkdir = "2"
tar = "0.4"
zstd = "0.13"
chrono = { version = "0.4", features = ["serde"] }
similar = "2"
glob = "0.3"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Error handling
anyhow = "1"
thiserror = "2"

# TUI (feature-gated)
ratatui = { version = "0.30", optional = true }
crossterm = { version = "0.28", optional = true }
ftui = { git = "https://github.com/Dicklesworthstone/frankentui", tag = "v0.3.1", optional = true }

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
```

- [ ] **Step 3: Create deny.toml**

```toml
[advisories]
vulnerability = "deny"
unmaintained = "warn"
yanked = "deny"
notice = "warn"

[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "0BSD",
    "Unicode-3.0",
    "Unicode-DFS-2016",
]
unlicensed = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "allow"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = ["https://github.com/Dicklesworthstone/frankentui"]
```

- [ ] **Step 4: Create clippy.toml**

```toml
too-many-arguments-threshold = 8
cognitive-complexity-threshold = 30
```

- [ ] **Step 5: Create rustfmt.toml**

```toml
edition = "2024"
max_width = 100
use_field_init_shorthand = true
```

- [ ] **Step 6: Create .gitignore**

```gitignore
/target
Cargo.lock
*.swp
*.swo
.DS_Store
```

- [ ] **Step 7: Create minimal main.rs**

```rust
use anyhow::Result;

mod cli;
mod config;
mod core;
mod storage;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    cli::run()
}
```

- [ ] **Step 8: Create stub modules so it compiles**

`src/cli.rs`:
```rust
use anyhow::Result;

pub fn run() -> Result<()> {
    println!("portal v{}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
```

`src/config.rs`:
```rust
// portal.config.toml parsing — placeholder
```

`src/core/mod.rs`:
```rust
pub mod backup;
pub mod checksum;
pub mod diff;
pub mod loader;
pub mod plugins;
pub mod profile;
pub mod safety;
pub mod skeleton;
pub mod snapshot;
```

Create empty files for each core submodule and storage submodules.

`src/storage/mod.rs`:
```rust
pub mod manifest;
pub mod meta;
pub mod paths;
pub mod plugins_manifest;
pub mod state;
```

Create empty files for each storage submodule.

- [ ] **Step 9: Verify it compiles**

```bash
cargo build 2>&1
```
Expected: Successful build with possible warnings for empty modules.

- [ ] **Step 10: Run clippy**

```bash
cargo clippy -- -D warnings 2>&1
```
Expected: Clean pass (empty modules).

- [ ] **Step 11: Commit scaffold**

```bash
git add -A
git commit -m "feat: project scaffold with security-hardened Cargo config"
```

---

## Task 2: Core Types — Data Model

**Files:**
- Create: `src/core/profile.rs`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Write profile types test**

Create `tests/integration/types_test.rs`:
```rust
use std::collections::HashMap;

#[test]
fn test_profile_manifest_roundtrip() {
    let manifest = portal::core::profile::ProfileManifest {
        version: 1,
        name: "test-profile".into(),
        created_at: chrono::Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: "Test profile".into(),
        tags: vec!["test".into()],
        files: HashMap::from([(
            "CLAUDE.md".into(),
            portal::core::profile::FileEntry {
                checksum: "sha256:abc123".into(),
                size: 1024,
                source: portal::core::profile::FileSource::User,
            },
        )]),
        excluded_patterns: vec!["sessions/**".into()],
    };

    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let parsed: portal::core::profile::ProfileManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "test-profile");
    assert_eq!(parsed.files.len(), 1);
    assert_eq!(parsed.version, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test types_test 2>&1
```
Expected: FAIL — types don't exist yet.

- [ ] **Step 3: Implement profile types**

`src/core/profile.rs`:
```rust
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
    Marketplace {
        marketplace: String,
        repo: String,
    },
    Local {
        path: String,
    },
    Github {
        repo: String,
    },
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
```

- [ ] **Step 4: Make types public from lib root**

Add `src/lib.rs`:
```rust
pub mod core;
pub mod storage;
pub mod config;
```

Update `src/main.rs` to also use the binary-local modules:
```rust
use anyhow::Result;

mod cli;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    cli::run()
}
```

- [ ] **Step 5: Run test to verify it passes**

```bash
cargo test --test types_test 2>&1
```
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: core data model types with serde roundtrip"
```

---

## Task 3: Storage — Path Resolution

**Files:**
- Create: `src/storage/paths.rs`

- [ ] **Step 1: Write path resolution tests**

Add to `tests/integration/paths_test.rs`:
```rust
#[test]
fn test_portal_paths_resolve() {
    let paths = portal::storage::paths::PortalPaths::with_home("/tmp/test-home".into());
    assert_eq!(paths.portal_root().to_str().unwrap(), "/tmp/test-home/.portal");
    assert_eq!(paths.claude_root().to_str().unwrap(), "/tmp/test-home/.claude");
    assert_eq!(
        paths.profile_dir("work").to_str().unwrap(),
        "/tmp/test-home/.portal/profiles/work"
    );
    assert_eq!(
        paths.profile_files_dir("work").to_str().unwrap(),
        "/tmp/test-home/.portal/profiles/work/files"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test paths_test 2>&1
```

- [ ] **Step 3: Implement paths**

`src/storage/paths.rs`:
```rust
use std::path::{Path, PathBuf};

/// All path resolution for Portal's storage layout.
#[derive(Debug, Clone)]
pub struct PortalPaths {
    home: PathBuf,
}

impl PortalPaths {
    /// Create from detected home directory.
    pub fn detect() -> Self {
        let home = dirs::home_dir().expect("cannot detect home directory");
        Self { home }
    }

    /// Create with explicit home (for testing).
    pub fn with_home(home: PathBuf) -> Self {
        Self { home }
    }

    // ── Portal storage ──

    pub fn portal_root(&self) -> PathBuf {
        self.home.join(".portal")
    }

    pub fn profiles_root(&self) -> PathBuf {
        self.portal_root().join("profiles")
    }

    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_root().join(name)
    }

    pub fn profile_files_dir(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("files")
    }

    pub fn profile_manifest(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("portal.json")
    }

    pub fn profile_plugins(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("plugins.json")
    }

    pub fn profile_meta(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("meta.json")
    }

    pub fn skeleton_dir(&self) -> PathBuf {
        self.portal_root().join("skeleton")
    }

    pub fn skeleton_files_dir(&self) -> PathBuf {
        self.skeleton_dir().join("files")
    }

    pub fn skeleton_manifest(&self) -> PathBuf {
        self.skeleton_dir().join("skeleton.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.portal_root().join("backups")
    }

    pub fn state_file(&self) -> PathBuf {
        self.portal_root().join("portal.state.json")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.portal_root().join(".portal.lock")
    }

    pub fn config_file(&self) -> PathBuf {
        self.portal_root().join("portal.config.toml")
    }

    pub fn exclude_file(&self) -> PathBuf {
        self.portal_root().join("portal.exclude")
    }

    // ── Claude directory ──

    pub fn claude_root(&self) -> PathBuf {
        self.home.join(".claude")
    }

    pub fn claude_old(&self) -> PathBuf {
        self.home.join(".claude.portal-old")
    }

    /// Ensure all required Portal directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.profiles_root())?;
        std::fs::create_dir_all(self.skeleton_dir())?;
        std::fs::create_dir_all(self.backups_dir())?;
        Ok(())
    }
}
```

Add `dirs` to `Cargo.toml` dependencies:
```toml
dirs = "6"
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --test paths_test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: path resolution for portal and claude directories"
```

---

## Task 4: Storage — Manifest + State Read/Write

**Files:**
- Create: `src/storage/manifest.rs`
- Create: `src/storage/plugins_manifest.rs`
- Create: `src/storage/state.rs`
- Create: `src/storage/meta.rs`

- [ ] **Step 1: Write manifest roundtrip test**

`tests/integration/manifest_test.rs`:
```rust
use portal::core::profile::{FileEntry, FileSource, ProfileManifest};
use portal::storage::manifest;
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn test_manifest_write_and_read() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("portal.json");

    let manifest = ProfileManifest {
        version: 1,
        name: "test".into(),
        created_at: chrono::Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: "test profile".into(),
        tags: vec![],
        files: HashMap::from([(
            "CLAUDE.md".into(),
            FileEntry {
                checksum: "sha256:abc".into(),
                size: 100,
                source: FileSource::User,
            },
        )]),
        excluded_patterns: vec!["sessions/**".into()],
    };

    manifest::write(&path, &manifest).unwrap();
    let loaded = manifest::read(&path).unwrap();
    assert_eq!(loaded.name, "test");
    assert_eq!(loaded.files.len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test manifest_test 2>&1
```

- [ ] **Step 3: Implement all storage modules**

`src/storage/manifest.rs`:
```rust
use crate::core::profile::ProfileManifest;
use anyhow::{Context, Result};
use std::path::Path;

pub fn read(path: &Path) -> Result<ProfileManifest> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading manifest: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing manifest: {}", path.display()))
}

pub fn write(path: &Path, manifest: &ProfileManifest) -> Result<()> {
    let content = serde_json::to_string_pretty(manifest)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing manifest: {}", path.display()))
}
```

`src/storage/plugins_manifest.rs`:
```rust
use crate::core::profile::PluginBlueprint;
use anyhow::{Context, Result};
use std::path::Path;

pub fn read(path: &Path) -> Result<PluginBlueprint> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading plugin blueprint: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing plugin blueprint: {}", path.display()))
}

pub fn write(path: &Path, blueprint: &PluginBlueprint) -> Result<()> {
    let content = serde_json::to_string_pretty(blueprint)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing plugin blueprint: {}", path.display()))
}
```

`src/storage/state.rs`:
```rust
use crate::core::profile::PortalState;
use anyhow::{Context, Result};
use std::path::Path;

pub fn read(path: &Path) -> Result<PortalState> {
    if !path.exists() {
        return Ok(PortalState::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading state: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing state: {}", path.display()))
}

pub fn write(path: &Path, state: &PortalState) -> Result<()> {
    let content = serde_json::to_string_pretty(state)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing state: {}", path.display()))
}
```

`src/storage/meta.rs`:
```rust
use crate::core::profile::ProfileMeta;
use anyhow::{Context, Result};
use std::path::Path;

pub fn read(path: &Path) -> Result<ProfileMeta> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading metadata: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing metadata: {}", path.display()))
}

pub fn write(path: &Path, meta: &ProfileMeta) -> Result<()> {
    let content = serde_json::to_string_pretty(meta)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing metadata: {}", path.display()))
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test manifest_test 2>&1
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: storage layer for manifest, state, meta, and plugin blueprint"
```

---

## Task 5: Core — Checksum Engine

**Files:**
- Create: `src/core/checksum.rs`

- [ ] **Step 1: Write checksum tests**

`tests/integration/checksum_test.rs`:
```rust
use tempfile::TempDir;
use std::io::Write;

#[test]
fn test_sha256_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"hello world").unwrap();

    let hash = portal::core::checksum::sha256_file(&path).unwrap();
    // Known SHA-256 of "hello world"
    assert_eq!(
        hash,
        "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn test_verify_file_ok() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"hello world").unwrap();

    let expected = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    assert!(portal::core::checksum::verify_file(&path, expected).unwrap());
}

#[test]
fn test_verify_file_mismatch() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.txt");
    std::fs::write(&path, b"hello world").unwrap();

    assert!(!portal::core::checksum::verify_file(&path, "sha256:deadbeef").unwrap());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test checksum_test 2>&1
```

- [ ] **Step 3: Implement checksum module**

`src/core/checksum.rs`:
```rust
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

const PREFIX: &str = "sha256:";

/// Compute SHA-256 hash of a file, returned as `sha256:<hex>`.
pub fn sha256_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path)
        .with_context(|| format!("reading file for checksum: {}", path.display()))?;
    let hash = Sha256::digest(&data);
    Ok(format!("{PREFIX}{hash:x}"))
}

/// Compute SHA-256 hash of raw bytes.
pub fn sha256_bytes(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    format!("{PREFIX}{hash:x}")
}

/// Verify a file's checksum matches expected value.
pub fn verify_file(path: &Path, expected: &str) -> Result<bool> {
    let actual = sha256_file(path)?;
    Ok(actual == expected)
}

/// Verify multiple files against a manifest.
/// Returns list of (relative_path, expected, actual) for mismatches.
pub fn verify_manifest(
    base_dir: &Path,
    files: &std::collections::HashMap<String, crate::core::profile::FileEntry>,
) -> Result<Vec<ChecksumMismatch>> {
    let mut mismatches = Vec::new();
    for (rel_path, entry) in files {
        let full_path = base_dir.join(rel_path);
        if !full_path.exists() {
            mismatches.push(ChecksumMismatch {
                path: rel_path.clone(),
                expected: entry.checksum.clone(),
                actual: "<missing>".into(),
            });
            continue;
        }
        let actual = sha256_file(&full_path)?;
        if actual != entry.checksum {
            mismatches.push(ChecksumMismatch {
                path: rel_path.clone(),
                expected: entry.checksum.clone(),
                actual,
            });
        }
    }
    Ok(mismatches)
}

#[derive(Debug)]
pub struct ChecksumMismatch {
    pub path: String,
    pub expected: String,
    pub actual: String,
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test checksum_test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: SHA-256 checksum engine with file verification"
```

---

## Task 6: Core — Skeleton Management

**Files:**
- Create: `src/core/skeleton.rs`

- [ ] **Step 1: Write skeleton tests**

`tests/integration/skeleton_test.rs`:
```rust
use tempfile::TempDir;

#[test]
fn test_create_skeleton() {
    let tmp = TempDir::new().unwrap();
    let claude_dir = tmp.path().join(".claude");

    portal::core::skeleton::create(&claude_dir).unwrap();

    // Verify required files exist
    assert!(claude_dir.join("settings.json").exists());
    assert!(claude_dir.join("CLAUDE.md").exists());
    assert!(claude_dir.join(".claude/settings.local.json").exists());

    // Verify required directories exist
    assert!(claude_dir.join(".claude/hooks").is_dir());
    assert!(claude_dir.join("skills").is_dir());
    assert!(claude_dir.join("memory").is_dir());
    assert!(claude_dir.join("commands").is_dir());
    assert!(claude_dir.join("agents").is_dir());
    assert!(claude_dir.join("rules").is_dir());
    assert!(claude_dir.join("hooks").is_dir());

    // Verify settings.json has default content
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(claude_dir.join("settings.json")).unwrap())
            .unwrap();
    assert!(settings.is_object());

    // Verify CLAUDE.md is empty
    let claude_md = std::fs::read_to_string(claude_dir.join("CLAUDE.md")).unwrap();
    assert!(claude_md.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test skeleton_test 2>&1
```

- [ ] **Step 3: Implement skeleton**

`src/core/skeleton.rs`:
```rust
use anyhow::{Context, Result};
use std::path::Path;

/// The default settings.json content for a skeleton.
const DEFAULT_SETTINGS: &str = r#"{
  "permissions": {},
  "env": {},
  "hooks": {}
}"#;

/// Directories that must exist in a skeleton .claude/.
const SKELETON_DIRS: &[&str] = &[
    ".claude/hooks",
    "skills",
    "memory",
    "commands",
    "agents",
    "rules",
    "hooks",
];

/// Create a skeleton .claude/ directory at the given path.
pub fn create(claude_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(claude_dir)
        .with_context(|| format!("creating claude dir: {}", claude_dir.display()))?;

    // Create required directories
    for dir in SKELETON_DIRS {
        std::fs::create_dir_all(claude_dir.join(dir))?;
    }

    // Create settings.json with defaults
    std::fs::write(claude_dir.join("settings.json"), DEFAULT_SETTINGS)?;

    // Create empty CLAUDE.md
    std::fs::write(claude_dir.join("CLAUDE.md"), "")?;

    // Create .claude/settings.local.json with empty object
    std::fs::write(claude_dir.join(".claude/settings.local.json"), "{}")?;

    Ok(())
}

/// Verify a directory matches the skeleton structure.
pub fn verify(claude_dir: &Path) -> Result<Vec<SkeletonIssue>> {
    let mut issues = Vec::new();

    if !claude_dir.join("settings.json").exists() {
        issues.push(SkeletonIssue::MissingFile("settings.json".into()));
    }
    if !claude_dir.join("CLAUDE.md").exists() {
        issues.push(SkeletonIssue::MissingFile("CLAUDE.md".into()));
    }

    for dir in SKELETON_DIRS {
        let dir_path = claude_dir.join(dir);
        if !dir_path.is_dir() {
            issues.push(SkeletonIssue::MissingDir(dir.to_string()));
        }
    }

    Ok(issues)
}

#[derive(Debug)]
pub enum SkeletonIssue {
    MissingFile(String),
    MissingDir(String),
}

impl std::fmt::Display for SkeletonIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFile(p) => write!(f, "missing file: {p}"),
            Self::MissingDir(p) => write!(f, "missing directory: {p}"),
        }
    }
}
```

- [ ] **Step 4: Run test**

```bash
cargo test --test skeleton_test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: skeleton creation and verification"
```

---

## Task 7: Core — Snapshot Engine (Save)

**Files:**
- Create: `src/core/snapshot.rs`
- Create: `src/core/plugins.rs`

- [ ] **Step 1: Write snapshot test**

`tests/integration/save_test.rs`:
```rust
use portal::core::profile::FileSource;
use portal::storage::paths::PortalPaths;
use tempfile::TempDir;

#[test]
fn test_save_profile() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create a fake .claude/ with some files
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "# My Config\nHello world").unwrap();
    std::fs::create_dir_all(claude.join("rules")).unwrap();
    std::fs::write(claude.join("rules/test.md"), "# Test Rule").unwrap();

    // Save it
    let result = portal::core::snapshot::save(&paths, "test-profile", "Test profile", &[]).unwrap();

    // Verify profile was created
    assert!(paths.profile_dir("test-profile").exists());
    assert!(paths.profile_manifest("test-profile").exists());
    assert!(paths.profile_plugins("test-profile").exists());
    assert!(paths.profile_meta("test-profile").exists());

    // Verify files were copied
    assert!(paths.profile_files_dir("test-profile").join("CLAUDE.md").exists());
    assert!(paths.profile_files_dir("test-profile").join("rules/test.md").exists());

    // Verify manifest has correct file count
    let manifest = portal::storage::manifest::read(&paths.profile_manifest("test-profile")).unwrap();
    assert!(manifest.files.contains_key("CLAUDE.md"));
    assert!(manifest.files.contains_key("rules/test.md"));
    assert_eq!(manifest.files["CLAUDE.md"].source, FileSource::User);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test save_test 2>&1
```

- [ ] **Step 3: Implement exclusion patterns**

Add to top of `src/core/snapshot.rs`:
```rust
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

use crate::core::checksum;
use crate::core::profile::*;
use crate::storage::paths::PortalPaths;

/// Patterns to always exclude from profiles.
const EXCLUDED_PATTERNS: &[&str] = &[
    "session-env",
    "sessions",
    "shell-snapshots",
    "history.jsonl",
    "todos",
    "file-history",
    "telemetry",
    "statsig",
    "paste-cache",
    "debug",
    "stats-cache.json",
    "mcp-needs-auth-cache.json",
    "plans",
    "projects",
    "repositories",
    "plugins/cache",
    "plugins/marketplaces",
    "plugins/data",
    "plugins/blocklist.json",
    "plugins/install-counts-cache.json",
    "plugins/known_marketplaces.json",
    ".DS_Store",
];

/// Check if a relative path should be excluded.
fn is_excluded(rel_path: &str) -> bool {
    EXCLUDED_PATTERNS.iter().any(|pat| {
        rel_path == *pat
            || rel_path.starts_with(&format!("{pat}/"))
    })
}

/// Scan a .claude/ directory and return trackable files (relative paths).
pub fn scan_trackable_files(claude_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(claude_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(claude_dir)
            .context("stripping prefix")?;
        let rel_str = rel.to_string_lossy();
        if !is_excluded(&rel_str) {
            files.push(rel.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

/// Save current .claude/ as a named profile.
pub fn save(
    paths: &PortalPaths,
    name: &str,
    description: &str,
    tags: &[String],
) -> Result<ProfileManifest> {
    let claude_dir = paths.claude_root();
    if !claude_dir.exists() {
        bail!("~/.claude/ does not exist");
    }

    let profile_dir = paths.profile_dir(name);
    let files_dir = paths.profile_files_dir(name);
    std::fs::create_dir_all(&files_dir)?;

    // Scan trackable files
    let trackable = scan_trackable_files(&claude_dir)?;
    info!("{} trackable files found", trackable.len());

    // Copy files and compute checksums
    let mut file_entries = HashMap::new();
    for rel_path in &trackable {
        let src = claude_dir.join(rel_path);
        let dst = files_dir.join(rel_path);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dst)?;

        // Verify copy by checksumming the destination
        let hash = checksum::sha256_file(&dst)?;
        let size = std::fs::metadata(&dst)?.len();

        file_entries.insert(
            rel_path.to_string_lossy().to_string(),
            FileEntry {
                checksum: hash,
                size,
                source: FileSource::User,
            },
        );
    }

    // Build manifest
    let manifest = ProfileManifest {
        version: 1,
        name: name.to_string(),
        created_at: chrono::Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: description.to_string(),
        tags: tags.to_vec(),
        files: file_entries,
        excluded_patterns: EXCLUDED_PATTERNS.iter().map(|s| format!("{s}/**")).collect(),
    };

    // Write manifest
    crate::storage::manifest::write(&paths.profile_manifest(name), &manifest)?;

    // Extract and write plugin blueprint
    let blueprint = crate::core::plugins::extract_blueprint(&claude_dir)?;
    crate::storage::plugins_manifest::write(&paths.profile_plugins(name), &blueprint)?;

    // Write metadata
    let meta = ProfileMeta {
        description: description.to_string(),
        tags: tags.to_vec(),
        notes: None,
        created_by: format!("portal v{}", env!("CARGO_PKG_VERSION")),
    };
    crate::storage::meta::write(&paths.profile_meta(name), &meta)?;

    Ok(manifest)
}
```

- [ ] **Step 4: Implement plugin blueprint extraction**

`src/core/plugins.rs`:
```rust
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::core::profile::*;

/// Extract a plugin blueprint from a .claude/ directory.
/// Reads settings.json for enabledPlugins and extraKnownMarketplaces,
/// and plugins/installed_plugins.json for details.
pub fn extract_blueprint(claude_dir: &Path) -> Result<PluginBlueprint> {
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.exists() {
        return Ok(PluginBlueprint::default());
    }

    let settings_str = std::fs::read_to_string(&settings_path)?;
    let settings: serde_json::Value = serde_json::from_str(&settings_str)?;

    let mut plugins = Vec::new();

    // Extract enabledPlugins
    if let Some(enabled) = settings.get("enabledPlugins").and_then(|v| v.as_object()) {
        for (id, val) in enabled {
            let is_enabled = val.as_bool().unwrap_or(false);

            // Determine source from extraKnownMarketplaces
            let source = determine_source(&settings, id);

            plugins.push(PluginEntry {
                id: id.clone(),
                enabled: is_enabled,
                source,
            });
        }
    }

    // Extract extraKnownMarketplaces
    let mut extra = HashMap::new();
    if let Some(mkts) = settings.get("extraKnownMarketplaces").and_then(|v| v.as_object()) {
        for (name, val) in mkts {
            if let Ok(entry) = serde_json::from_value::<MarketplaceEntry>(val.clone()) {
                extra.insert(name.clone(), entry);
            }
        }
    }

    Ok(PluginBlueprint {
        version: 1,
        plugins,
        extra_known_marketplaces: extra,
    })
}

/// Determine plugin source from settings.json extraKnownMarketplaces.
fn determine_source(settings: &serde_json::Value, plugin_id: &str) -> PluginSource {
    // Plugin ID format: "name@marketplace"
    let marketplace_name = plugin_id.split('@').nth(1).unwrap_or("");

    if let Some(mkts) = settings.get("extraKnownMarketplaces").and_then(|v| v.as_object()) {
        if let Some(mkt) = mkts.get(marketplace_name) {
            if let Some(source) = mkt.get("source") {
                if let Some(src_type) = source.get("source").and_then(|s| s.as_str()) {
                    match src_type {
                        "github" => {
                            let repo = source
                                .get("repo")
                                .and_then(|r| r.as_str())
                                .unwrap_or("")
                                .to_string();
                            return PluginSource::Marketplace {
                                marketplace: marketplace_name.to_string(),
                                repo,
                            };
                        }
                        "directory" => {
                            let path = source
                                .get("path")
                                .and_then(|p| p.as_str())
                                .unwrap_or("")
                                .to_string();
                            return PluginSource::Local { path };
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Fallback: marketplace with unknown repo
    PluginSource::Marketplace {
        marketplace: marketplace_name.to_string(),
        repo: String::new(),
    }
}

/// Reinstall plugins from a blueprint.
/// Runs after atomic swap. Failures are non-fatal.
pub fn reinstall(blueprint: &PluginBlueprint) -> Vec<PluginInstallResult> {
    let mut results = Vec::new();

    for plugin in &blueprint.plugins {
        if !plugin.enabled {
            continue;
        }

        let result = match &plugin.source {
            PluginSource::Marketplace { marketplace: _, repo: _ } => {
                install_marketplace_plugin(&plugin.id)
            }
            PluginSource::Local { path } => install_local_plugin(&plugin.id, path),
            PluginSource::Github { repo } => install_github_plugin(&plugin.id, repo),
        };

        results.push(PluginInstallResult {
            id: plugin.id.clone(),
            success: result.is_ok(),
            message: result.unwrap_or_else(|e| format!("FAILED: {e}")),
        });
    }

    results
}

fn install_marketplace_plugin(id: &str) -> Result<String, String> {
    let output = std::process::Command::new("claude")
        .args(["plugin", "install", id])
        .output()
        .map_err(|e| format!("failed to run claude: {e}"))?;

    if output.status.success() {
        Ok(format!("installed (marketplace)"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn install_local_plugin(id: &str, path: &str) -> Result<String, String> {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return Err(format!("local source not found: {}", path.display()));
    }

    let output = std::process::Command::new("claude")
        .args(["plugin", "install", &path.to_string_lossy()])
        .output()
        .map_err(|e| format!("failed to run claude: {e}"))?;

    if output.status.success() {
        Ok(format!("installed (local)"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn install_github_plugin(_id: &str, repo: &str) -> Result<String, String> {
    // Clone to tempdir, install from there
    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let clone_output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", &format!("https://github.com/{repo}"), &tmp.path().to_string_lossy()])
        .output()
        .map_err(|e| format!("git clone: {e}"))?;

    if !clone_output.status.success() {
        return Err(format!("git clone failed: {}", String::from_utf8_lossy(&clone_output.stderr)));
    }

    let output = std::process::Command::new("claude")
        .args(["plugin", "install", &tmp.path().to_string_lossy()])
        .output()
        .map_err(|e| format!("claude install: {e}"))?;

    if output.status.success() {
        Ok(format!("installed (github)"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[derive(Debug)]
pub struct PluginInstallResult {
    pub id: String,
    pub success: bool,
    pub message: String,
}
```

- [ ] **Step 5: Run test**

```bash
cargo test --test save_test 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: snapshot engine (save) with plugin blueprint extraction"
```

---

## Task 8: Core — Backup Engine

**Files:**
- Create: `src/core/backup.rs`

- [ ] **Step 1: Write backup tests**

`tests/integration/backup_test.rs`:
```rust
use portal::storage::paths::PortalPaths;
use tempfile::TempDir;

#[test]
fn test_backup_and_restore() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create a .claude/ with content
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "backup test content").unwrap();

    // Create backup
    let backup_path = portal::core::backup::create(&paths, "load", "test-profile").unwrap();
    assert!(backup_path.exists());

    // Modify .claude/
    std::fs::write(claude.join("CLAUDE.md"), "modified after backup").unwrap();

    // Restore
    portal::core::backup::restore(&paths, &backup_path).unwrap();

    // Verify restored content
    let content = std::fs::read_to_string(claude.join("CLAUDE.md")).unwrap();
    assert_eq!(content, "backup test content");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test backup_test 2>&1
```

- [ ] **Step 3: Implement backup engine**

`src/core/backup.rs`:
```rust
use anyhow::{Context, Result};
use std::fs::File;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::storage::paths::PortalPaths;

/// Create a zstd-compressed tar backup of ~/.claude/.
/// Returns the path to the created backup file.
pub fn create(paths: &PortalPaths, op_type: &str, profile_name: &str) -> Result<PathBuf> {
    let claude_dir = paths.claude_root();
    let backups_dir = paths.backups_dir();
    std::fs::create_dir_all(&backups_dir)?;

    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S");
    let filename = format!("pre-{op_type}-{timestamp}.tar.zst");
    let backup_path = backups_dir.join(&filename);

    info!("creating backup: {}", backup_path.display());

    let file = File::create(&backup_path)
        .with_context(|| format!("creating backup file: {}", backup_path.display()))?;
    let encoder = zstd::Encoder::new(file, 3)?;
    let mut tar = tar::Builder::new(encoder);

    // Add all files from .claude/ (excluding ephemeral)
    tar.append_dir_all("claude", &claude_dir)
        .with_context(|| "archiving .claude/ directory")?;

    let encoder = tar.into_inner()?;
    encoder.finish()?;

    info!("backup created: {} ({} bytes)", filename, std::fs::metadata(&backup_path)?.len());
    Ok(backup_path)
}

/// Restore from a zstd-compressed tar backup, replacing ~/.claude/.
pub fn restore(paths: &PortalPaths, backup_path: &Path) -> Result<()> {
    let claude_dir = paths.claude_root();

    info!("restoring from backup: {}", backup_path.display());

    // Extract to tempdir first (safety)
    let tmp = tempfile::tempdir_in(paths.portal_root())?;
    let file = File::open(backup_path)?;
    let decoder = zstd::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(tmp.path())?;

    let extracted_claude = tmp.path().join("claude");
    if !extracted_claude.exists() {
        anyhow::bail!("backup archive does not contain 'claude/' directory");
    }

    // Atomic swap: rename old out, rename extracted in
    let old_path = paths.claude_old();
    if old_path.exists() {
        std::fs::remove_dir_all(&old_path)?;
    }

    if claude_dir.exists() {
        std::fs::rename(&claude_dir, &old_path)?;
    }

    std::fs::rename(&extracted_claude, &claude_dir)?;

    // Cleanup old
    if old_path.exists() {
        std::fs::remove_dir_all(&old_path)?;
    }

    info!("restore complete");
    Ok(())
}

/// Prune old backups, keeping only the most recent `keep_count`.
pub fn prune(paths: &PortalPaths, keep_count: usize) -> Result<Vec<PathBuf>> {
    let backups_dir = paths.backups_dir();
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups: Vec<_> = std::fs::read_dir(&backups_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "zst")
        })
        .collect();

    // Sort by modification time, newest first
    backups.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let b_time = b.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        b_time.cmp(&a_time)
    });

    let mut pruned = Vec::new();
    for entry in backups.iter().skip(keep_count) {
        let path = entry.path();
        std::fs::remove_file(&path)?;
        pruned.push(path);
    }

    Ok(pruned)
}

/// List available backups, newest first.
pub fn list(paths: &PortalPaths) -> Result<Vec<BackupInfo>> {
    let backups_dir = paths.backups_dir();
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }

    let mut infos: Vec<_> = std::fs::read_dir(&backups_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(BackupInfo {
                path: e.path(),
                size: meta.len(),
                created: meta.modified().ok()?,
            })
        })
        .collect();

    infos.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(infos)
}

#[derive(Debug)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub size: u64,
    pub created: std::time::SystemTime,
}
```

- [ ] **Step 4: Run test**

```bash
cargo test --test backup_test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: tar.zst backup engine with create, restore, and pruning"
```

---

## Task 9: Core — Safety Checks

**Files:**
- Create: `src/core/safety.rs`

- [ ] **Step 1: Write safety tests**

`tests/integration/safety_test.rs`:
```rust
use portal::storage::paths::PortalPaths;
use tempfile::TempDir;

#[test]
fn test_preflight_no_claude_dir() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    let result = portal::core::safety::preflight_load(&paths, "test");
    assert!(result.is_err());
}

#[test]
fn test_preflight_missing_profile() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    portal::core::skeleton::create(&paths.claude_root()).unwrap();

    let result = portal::core::safety::preflight_load(&paths, "nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_file_lock() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let lock = portal::core::safety::acquire_lock(&paths).unwrap();
    // Lock should exist
    assert!(paths.lock_file().exists());
    drop(lock);
    // Lock should be cleaned up
    assert!(!paths.lock_file().exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test safety_test 2>&1
```

- [ ] **Step 3: Implement safety module**

`src/core/safety.rs`:
```rust
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use tracing::warn;

use crate::storage::paths::PortalPaths;

/// Pre-flight checks before a load operation.
pub fn preflight_load(paths: &PortalPaths, profile_name: &str) -> Result<PreflightReport> {
    let mut report = PreflightReport::default();

    // Check 1: Is Claude running?
    if is_claude_running() {
        bail!("Claude is running. Close all Claude Code sessions first.");
    }
    report.claude_not_running = true;

    // Check 2: Does .claude/ exist?
    let claude_dir = paths.claude_root();
    if !claude_dir.exists() {
        bail!(
            "~/.claude/ does not exist. Run `portal reset` to create a skeleton first."
        );
    }
    report.claude_dir_exists = true;

    // Check 3: Does the profile exist?
    let profile_dir = paths.profile_dir(profile_name);
    if !profile_dir.exists() {
        bail!(
            "Profile \"{profile_name}\" not found. Run `portal list` to see available profiles."
        );
    }
    let manifest_path = paths.profile_manifest(profile_name);
    if !manifest_path.exists() {
        bail!("Profile \"{profile_name}\" is missing portal.json manifest.");
    }
    report.profile_exists = true;

    // Check 4: Crash recovery — is .claude.portal-old lingering?
    if paths.claude_old().exists() {
        warn!("Found ~/.claude.portal-old — previous operation may have crashed");
        report.crash_recovery_needed = true;
    }

    Ok(report)
}

/// Pre-flight checks before a save operation.
pub fn preflight_save(paths: &PortalPaths) -> Result<()> {
    let claude_dir = paths.claude_root();
    if !claude_dir.exists() {
        bail!("~/.claude/ does not exist. Nothing to save.");
    }
    if !claude_dir.join("settings.json").exists() {
        bail!("~/.claude/settings.json not found. Is this a valid Claude configuration?");
    }
    Ok(())
}

/// Check if Claude Code is running.
fn is_claude_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", "claude"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// File-based lock to prevent concurrent operations.
pub struct PortalLock {
    path: PathBuf,
}

impl Drop for PortalLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Acquire an exclusive lock for portal operations.
pub fn acquire_lock(paths: &PortalPaths) -> Result<PortalLock> {
    let lock_path = paths.lock_file();

    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if lock_path.exists() {
        // Check if the lock is stale (older than 5 minutes)
        if let Ok(meta) = std::fs::metadata(&lock_path) {
            if let Ok(modified) = meta.modified() {
                let age = std::time::SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                if age.as_secs() > 300 {
                    warn!("removing stale lock file ({}s old)", age.as_secs());
                    std::fs::remove_file(&lock_path)?;
                } else {
                    bail!(
                        "Another portal operation is in progress (lock file exists). \
                         If this is stale, delete: {}",
                        lock_path.display()
                    );
                }
            }
        }
    }

    std::fs::write(&lock_path, format!("{}", std::process::id()))
        .with_context(|| "creating lock file")?;

    Ok(PortalLock { path: lock_path })
}

#[derive(Debug, Default)]
pub struct PreflightReport {
    pub claude_not_running: bool,
    pub claude_dir_exists: bool,
    pub profile_exists: bool,
    pub crash_recovery_needed: bool,
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test safety_test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: safety pre-flight checks and file locking"
```

---

## Task 10: Core — Loader Engine (Atomic Swap)

**Files:**
- Create: `src/core/loader.rs`

- [ ] **Step 1: Write loader test**

`tests/integration/load_test.rs`:
```rust
use portal::storage::paths::PortalPaths;
use tempfile::TempDir;

#[test]
fn test_load_profile() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create a .claude/ and save a profile
    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "original config").unwrap();
    std::fs::create_dir_all(claude.join("rules")).unwrap();
    std::fs::write(claude.join("rules/test.md"), "# Rule").unwrap();

    portal::core::snapshot::save(&paths, "profile-a", "Profile A", &[]).unwrap();

    // Modify .claude/ to simulate different state
    std::fs::write(claude.join("CLAUDE.md"), "modified config").unwrap();
    std::fs::remove_file(claude.join("rules/test.md")).unwrap();

    // Load profile-a back (skip preflight Claude check in test)
    portal::core::loader::load(&paths, "profile-a", false, true).unwrap();

    // Verify .claude/ matches profile-a
    let content = std::fs::read_to_string(claude.join("CLAUDE.md")).unwrap();
    assert_eq!(content, "original config");
    assert!(claude.join("rules/test.md").exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test load_test 2>&1
```

- [ ] **Step 3: Implement loader**

`src/core/loader.rs`:
```rust
use anyhow::{bail, Context, Result};
use tracing::info;

use crate::core::{backup, checksum, plugins, safety, skeleton};
use crate::core::profile::*;
use crate::storage::{manifest, paths::PortalPaths, plugins_manifest, state};

/// Load a profile, replacing ~/.claude/ via atomic swap.
///
/// - `no_backup`: skip backup creation (dangerous)
/// - `no_plugins`: skip plugin reinstallation
pub fn load(
    paths: &PortalPaths,
    profile_name: &str,
    no_plugins: bool,
    skip_claude_check: bool,
) -> Result<LoadResult> {
    // 1. Pre-flight checks
    if !skip_claude_check {
        safety::preflight_load(paths, profile_name)?;
    }

    let _lock = safety::acquire_lock(paths)?;

    // 2. Read profile manifest
    let manifest = manifest::read(&paths.profile_manifest(profile_name))?;
    info!("loading profile '{}' ({} files)", profile_name, manifest.files.len());

    // 3. Verify profile integrity
    let mismatches = checksum::verify_manifest(
        &paths.profile_files_dir(profile_name),
        &manifest.files,
    )?;
    if !mismatches.is_empty() {
        let details: Vec<_> = mismatches
            .iter()
            .map(|m| format!("  {}: expected {}, got {}", m.path, m.expected, m.actual))
            .collect();
        bail!(
            "Profile integrity check failed:\n{}",
            details.join("\n")
        );
    }

    // 4. Create backup
    let backup_path = backup::create(paths, "load", profile_name)?;
    info!("backup created: {}", backup_path.display());

    // 5. Build target in tempdir
    let tmp = tempfile::tempdir_in(
        paths.portal_root()
    ).context("creating tempdir for build")?;
    let build_dir = tmp.path().join("claude-build");

    // Build skeleton
    skeleton::create(&build_dir)?;

    // Overlay profile files
    let profile_files = paths.profile_files_dir(profile_name);
    copy_dir_recursive(&profile_files, &build_dir)?;

    // 6. Verify build checksums
    let build_mismatches = checksum::verify_manifest(&build_dir, &manifest.files)?;
    if !build_mismatches.is_empty() {
        bail!("Build verification failed — aborting swap");
    }

    // 7. Atomic swap
    let claude_dir = paths.claude_root();
    let old_dir = paths.claude_old();

    // Clean up any leftover .portal-old
    if old_dir.exists() {
        std::fs::remove_dir_all(&old_dir)?;
    }

    // Rename current out of the way
    std::fs::rename(&claude_dir, &old_dir)
        .context("renaming ~/.claude/ to ~/.claude.portal-old")?;

    // Rename build into place
    if let Err(e) = std::fs::rename(&build_dir, &claude_dir) {
        // CRITICAL: Restore old if rename fails
        let _ = std::fs::rename(&old_dir, &claude_dir);
        return Err(e).context("atomic swap failed — restored original");
    }

    // Remove old
    std::fs::remove_dir_all(&old_dir).ok();

    // 8. Reinstall plugins (non-fatal)
    let plugin_results = if !no_plugins {
        let blueprint_path = paths.profile_plugins(profile_name);
        if blueprint_path.exists() {
            let blueprint = plugins_manifest::read(&blueprint_path)?;
            plugins::reinstall(&blueprint)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // 9. Update state
    let portal_state = PortalState {
        version: 1,
        active_profile: Some(profile_name.to_string()),
        last_operation: Some(LastOperation {
            op_type: OperationType::Load,
            profile: profile_name.to_string(),
            timestamp: chrono::Utc::now(),
            backup_path: backup_path.to_string_lossy().to_string(),
            plugins_installed: plugin_results.iter().all(|r| r.success),
        }),
        skeleton_checksum: None,
    };
    state::write(&paths.state_file(), &portal_state)?;

    // Update manifest load count
    let mut updated_manifest = manifest;
    updated_manifest.last_loaded = Some(chrono::Utc::now());
    updated_manifest.load_count += 1;
    crate::storage::manifest::write(
        &paths.profile_manifest(profile_name),
        &updated_manifest,
    )?;

    Ok(LoadResult {
        profile: profile_name.to_string(),
        files_loaded: updated_manifest.files.len(),
        backup_path,
        plugin_results,
    })
}

/// Recursively copy directory contents, merging into destination.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct LoadResult {
    pub profile: String,
    pub files_loaded: usize,
    pub backup_path: std::path::PathBuf,
    pub plugin_results: Vec<plugins::PluginInstallResult>,
}
```

- [ ] **Step 4: Run test**

```bash
cargo test --test load_test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: atomic swap loader with backup and plugin reinstall"
```

---

## Task 11: Core — Diff Engine

**Files:**
- Create: `src/core/diff.rs`

- [ ] **Step 1: Write diff tests**

`tests/integration/diff_test.rs`:
```rust
use portal::core::diff::{diff_profiles, DiffSide};
use portal::storage::paths::PortalPaths;
use tempfile::TempDir;

#[test]
fn test_diff_profiles() {
    let tmp = TempDir::new().unwrap();
    let paths = PortalPaths::with_home(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let claude = paths.claude_root();
    portal::core::skeleton::create(&claude).unwrap();

    // Save profile A
    std::fs::write(claude.join("CLAUDE.md"), "Profile A content").unwrap();
    std::fs::create_dir_all(claude.join("rules")).unwrap();
    std::fs::write(claude.join("rules/a-only.md"), "only in A").unwrap();
    portal::core::snapshot::save(&paths, "a", "Profile A", &[]).unwrap();

    // Save profile B
    std::fs::write(claude.join("CLAUDE.md"), "Profile B content").unwrap();
    std::fs::remove_file(claude.join("rules/a-only.md")).unwrap();
    std::fs::write(claude.join("rules/b-only.md"), "only in B").unwrap();
    portal::core::snapshot::save(&paths, "b", "Profile B", &[]).unwrap();

    let result = diff_profiles(&paths, DiffSide::Profile("a"), DiffSide::Profile("b")).unwrap();

    assert!(!result.only_left.is_empty(), "should have a-only files");
    assert!(!result.only_right.is_empty(), "should have b-only files");
    assert!(!result.different_content.is_empty(), "CLAUDE.md should differ");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test diff_test 2>&1
```

- [ ] **Step 3: Implement diff engine**

`src/core/diff.rs`:
```rust
use anyhow::{bail, Result};
use similar::TextDiff;
use std::collections::{BTreeMap, BTreeSet};

use crate::core::profile::ProfileManifest;
use crate::storage::{manifest, paths::PortalPaths};

/// What to diff against.
pub enum DiffSide<'a> {
    Profile(&'a str),
    Skeleton,
}

/// Result of comparing two profiles at the manifest level.
#[derive(Debug)]
pub struct DiffResult {
    pub left_name: String,
    pub right_name: String,
    pub shared_same: Vec<String>,
    pub different_content: Vec<FileDiff>,
    pub only_left: Vec<String>,
    pub only_right: Vec<String>,
}

#[derive(Debug)]
pub struct FileDiff {
    pub path: String,
    pub left_size: u64,
    pub right_size: u64,
}

/// Compare two profiles (or profile vs skeleton).
pub fn diff_profiles(
    paths: &PortalPaths,
    left: DiffSide<'_>,
    right: DiffSide<'_>,
) -> Result<DiffResult> {
    let (left_name, left_files) = load_side(paths, &left)?;
    let (right_name, right_files) = load_side(paths, &right)?;

    let left_keys: BTreeSet<_> = left_files.keys().cloned().collect();
    let right_keys: BTreeSet<_> = right_files.keys().cloned().collect();

    let mut shared_same = Vec::new();
    let mut different_content = Vec::new();

    for key in left_keys.intersection(&right_keys) {
        let l = &left_files[key];
        let r = &right_files[key];
        if l.0 == r.0 {
            shared_same.push(key.clone());
        } else {
            different_content.push(FileDiff {
                path: key.clone(),
                left_size: l.1,
                right_size: r.1,
            });
        }
    }

    let only_left: Vec<_> = left_keys.difference(&right_keys).cloned().collect();
    let only_right: Vec<_> = right_keys.difference(&left_keys).cloned().collect();

    Ok(DiffResult {
        left_name,
        right_name,
        shared_same,
        different_content,
        only_left,
        only_right,
    })
}

/// Load file checksums for a diff side.
/// Returns (name, map of relative_path -> (checksum, size)).
fn load_side(
    paths: &PortalPaths,
    side: &DiffSide<'_>,
) -> Result<(String, BTreeMap<String, (String, u64)>)> {
    match side {
        DiffSide::Profile(name) => {
            let manifest_path = paths.profile_manifest(name);
            if !manifest_path.exists() {
                bail!("Profile \"{name}\" not found");
            }
            let m = manifest::read(&manifest_path)?;
            let files = m
                .files
                .into_iter()
                .map(|(k, v)| (k, (v.checksum, v.size)))
                .collect();
            Ok((name.to_string(), files))
        }
        DiffSide::Skeleton => {
            // Skeleton has minimal files — empty map for comparison
            Ok(("skeleton".into(), BTreeMap::new()))
        }
    }
}

/// Generate a unified text diff for a specific file between two profiles.
pub fn content_diff(
    paths: &PortalPaths,
    left: DiffSide<'_>,
    right: DiffSide<'_>,
    file_path: &str,
) -> Result<String> {
    let left_content = read_file_from_side(paths, &left, file_path)?;
    let right_content = read_file_from_side(paths, &right, file_path)?;

    let (left_name, _) = load_side(paths, &left)?;
    let (right_name, _) = load_side(paths, &right)?;

    let diff = TextDiff::from_lines(&left_content, &right_content);
    Ok(diff
        .unified_diff()
        .header(
            &format!("{left_name}/{file_path}"),
            &format!("{right_name}/{file_path}"),
        )
        .to_string())
}

fn read_file_from_side(paths: &PortalPaths, side: &DiffSide<'_>, file_path: &str) -> Result<String> {
    match side {
        DiffSide::Profile(name) => {
            let path = paths.profile_files_dir(name).join(file_path);
            if !path.exists() {
                return Ok(String::new());
            }
            Ok(std::fs::read_to_string(path)?)
        }
        DiffSide::Skeleton => Ok(String::new()),
    }
}
```

- [ ] **Step 4: Run test**

```bash
cargo test --test diff_test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: four-level diff engine with unified content diff"
```

---

## Task 12: CLI Commands

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement full CLI with clap derive**

`src/cli.rs`:
```rust
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};

use portal::core::{backup, diff, loader, profile, safety, skeleton, snapshot};
use portal::storage::{manifest, meta, paths::PortalPaths, plugins_manifest, state};

#[derive(Parser)]
#[command(
    name = "portal",
    about = "Configuration transport layer for Claude Code",
    version,
    after_help = "Run `portal` without a subcommand to launch the TUI (if compiled with TUI support)."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Show what would happen without executing
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Skip auto-backup (requires --force)
    #[arg(long, global = true)]
    pub no_backup: bool,

    /// Skip plugin reinstallation on load
    #[arg(long, global = true)]
    pub no_plugins: bool,

    /// Override safety checks
    #[arg(long, global = true)]
    pub force: bool,

    /// Verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Quiet mode (errors only)
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Save current .claude/ as a named profile
    Save {
        /// Profile name
        name: Option<String>,
        /// Profile description
        #[arg(short, long)]
        description: Option<String>,
        /// Tags (comma-separated)
        #[arg(short, long)]
        tags: Option<String>,
    },
    /// Load a profile (atomic swap + auto-backup)
    Load {
        /// Profile name
        name: String,
    },
    /// List all profiles
    List,
    /// Show profile details
    Show {
        /// Profile name
        name: String,
    },
    /// Diff two profiles (B defaults to skeleton)
    Diff {
        /// First profile
        a: String,
        /// Second profile (defaults to skeleton)
        b: Option<String>,
        /// Show diff for a specific file
        #[arg(long)]
        file: Option<String>,
        /// Compare plugins only
        #[arg(long)]
        plugins: bool,
        /// Compare against active profile
        #[arg(long)]
        active: bool,
    },
    /// Delete a profile
    Rm {
        /// Profile name
        name: String,
    },
    /// Reset .claude/ to skeleton
    Reset,
    /// Undo last load/reset (restore from backup)
    Undo,
    /// Show current status
    Status,
    /// Rename a profile
    Rename {
        /// Old name
        old: String,
        /// New name
        new: String,
    },
    /// Verify profile integrity
    Verify {
        /// Profile name (defaults to active)
        name: Option<String>,
        /// Attempt to fix failed plugins
        #[arg(long)]
        fix_plugins: bool,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = PortalPaths::detect();
    paths.ensure_dirs()?;

    match cli.command {
        None => {
            #[cfg(feature = "tui-ratatui")]
            {
                return portal::tui::run(&paths);
            }
            #[cfg(feature = "tui-ftui")]
            {
                return portal::tui::run(&paths);
            }
            #[cfg(not(any(feature = "tui-ratatui", feature = "tui-ftui")))]
            {
                println!("Portal v{}", env!("CARGO_PKG_VERSION"));
                println!("TUI not compiled. Use a subcommand or rebuild with --features tui-ratatui");
                println!();
                println!("Commands: save, load, list, show, diff, rm, reset, undo, status");
                Ok(())
            }
        }
        Some(Commands::Save { name, description, tags }) => {
            cmd_save(&paths, name, description, tags, &cli)
        }
        Some(Commands::Load { name }) => cmd_load(&paths, &name, &cli),
        Some(Commands::List) => cmd_list(&paths),
        Some(Commands::Show { name }) => cmd_show(&paths, &name),
        Some(Commands::Diff { a, b, file, plugins, active }) => {
            cmd_diff(&paths, &a, b.as_deref(), file.as_deref(), plugins, active)
        }
        Some(Commands::Rm { name }) => cmd_rm(&paths, &name),
        Some(Commands::Reset) => cmd_reset(&paths, &cli),
        Some(Commands::Undo) => cmd_undo(&paths),
        Some(Commands::Status) => cmd_status(&paths),
        Some(Commands::Rename { old, new }) => cmd_rename(&paths, &old, &new),
        Some(Commands::Verify { name, fix_plugins }) => cmd_verify(&paths, name.as_deref(), fix_plugins),
    }
}

fn cmd_save(
    paths: &PortalPaths,
    name: Option<String>,
    description: Option<String>,
    tags: Option<String>,
    cli: &Cli,
) -> Result<()> {
    safety::preflight_save(paths)?;

    let name = match name {
        Some(n) => n,
        None => {
            let input: String = dialoguer::Input::new()
                .with_prompt("Profile name")
                .interact_text()?;
            input
        }
    };

    // Check if profile already exists
    if paths.profile_dir(&name).exists() && !cli.force {
        let overwrite = dialoguer::Confirm::new()
            .with_prompt(format!("Profile \"{name}\" already exists. Overwrite?"))
            .default(false)
            .interact()?;
        if !overwrite {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let desc = description.unwrap_or_default();
    let tag_list: Vec<String> = tags
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    if cli.dry_run {
        println!("{}", style("DRY RUN — no changes will be made").yellow());
        let trackable = snapshot::scan_trackable_files(&paths.claude_root())?;
        println!("Would save {} files as profile \"{}\"", trackable.len(), name);
        return Ok(());
    }

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}")?);
    pb.set_message(format!("Saving profile \"{name}\"..."));

    let result = snapshot::save(paths, &name, &desc, &tag_list)?;

    pb.finish_and_clear();
    println!(
        "{} Profile \"{}\" saved successfully.",
        style("✓").green().bold(),
        name
    );
    println!(
        "  {} files tracked | {}",
        result.files.len(),
        format_bytes(result.files.values().map(|f| f.size).sum()),
    );

    Ok(())
}

fn cmd_load(paths: &PortalPaths, name: &str, cli: &Cli) -> Result<()> {
    if cli.dry_run {
        println!("{}", style("DRY RUN — no changes will be made").yellow());
        let manifest = manifest::read(&paths.profile_manifest(name))?;
        println!("Would load profile \"{}\" ({} files)", name, manifest.files.len());
        return Ok(());
    }

    let result = loader::load(paths, name, cli.no_plugins, false)?;

    println!(
        "{} Profile \"{}\" loaded successfully.",
        style("✓").green().bold(),
        result.profile
    );
    println!("  {} files loaded", result.files_loaded);

    for pr in &result.plugin_results {
        let icon = if pr.success {
            style("✓").green()
        } else {
            style("✗").red()
        };
        println!("  {icon} Plugin: {}", pr.message);
    }

    Ok(())
}

fn cmd_list(paths: &PortalPaths) -> Result<()> {
    let profiles_dir = paths.profiles_root();
    if !profiles_dir.exists() {
        println!("No profiles found. Run `portal save <name>` to create one.");
        return Ok(());
    }

    let current_state = state::read(&paths.state_file())?;

    println!(
        "  {:<20} {:>5}  {:>6}  {:>7}  {:<20}  {}",
        "Profile", "Files", "Size", "Plugins", "Tags", "Active"
    );
    println!("  {}", "─".repeat(78));

    let mut entries: Vec<_> = std::fs::read_dir(&profiles_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let manifest_path = paths.profile_manifest(&name);
        if !manifest_path.exists() {
            continue;
        }

        let m = manifest::read(&manifest_path)?;
        let total_size: u64 = m.files.values().map(|f| f.size).sum();
        let plugin_count = plugins_manifest::read(&paths.profile_plugins(&name))
            .map(|b| b.plugins.len())
            .unwrap_or(0);

        let is_active = current_state
            .active_profile
            .as_deref()
            .is_some_and(|a| a == name);
        let active_marker = if is_active { "●" } else { "○" };
        let tags = m.tags.join(", ");

        println!(
            "  {:<20} {:>5}  {:>6}  {:>7}  {:<20}  {}",
            name,
            m.files.len(),
            format_bytes(total_size),
            plugin_count,
            if tags.len() > 20 { format!("{}…", &tags[..19]) } else { tags },
            active_marker
        );
    }

    Ok(())
}

fn cmd_show(paths: &PortalPaths, name: &str) -> Result<()> {
    let m = manifest::read(&paths.profile_manifest(name))?;
    let meta_data = meta::read(&paths.profile_meta(name)).ok();
    let blueprint = plugins_manifest::read(&paths.profile_plugins(name)).ok();

    println!("Profile: {}", style(name).bold());
    if let Some(md) = &meta_data {
        println!("  Description: {}", md.description);
        println!("  Tags: {}", md.tags.join(", "));
    }
    println!("  Created: {}", m.created_at.format("%Y-%m-%d %H:%M"));
    if let Some(ll) = m.last_loaded {
        println!("  Last loaded: {}", ll.format("%Y-%m-%d %H:%M"));
    }
    println!("  Load count: {}", m.load_count);
    println!();

    println!("Files ({} total):", m.files.len());
    let mut sorted_files: Vec<_> = m.files.iter().collect();
    sorted_files.sort_by_key(|(k, _)| k.clone());
    for (path, entry) in &sorted_files {
        println!(
            "  {} {:<40} {:>6}  {}",
            if entry.source == profile::FileSource::Skeleton { "○" } else { "●" },
            path,
            format_bytes(entry.size),
            &entry.checksum[..20],
        );
    }

    if let Some(bp) = blueprint {
        println!();
        println!("Plugins ({}):", bp.plugins.len());
        for p in &bp.plugins {
            let source_str = match &p.source {
                profile::PluginSource::Marketplace { marketplace, .. } => {
                    format!("marketplace ({marketplace})")
                }
                profile::PluginSource::Local { path } => format!("local ({path})"),
                profile::PluginSource::Github { repo } => format!("github ({repo})"),
            };
            let enabled = if p.enabled { "✓" } else { "✗" };
            println!("  {enabled} {:<30} {source_str}", p.id);
        }
    }

    Ok(())
}

fn cmd_diff(
    paths: &PortalPaths,
    a: &str,
    b: Option<&str>,
    file: Option<&str>,
    _plugins: bool,
    _active: bool,
) -> Result<()> {
    let left = diff::DiffSide::Profile(a);
    let right = match b {
        Some(name) => diff::DiffSide::Profile(name),
        None => diff::DiffSide::Skeleton,
    };

    if let Some(file_path) = file {
        let content = diff::content_diff(paths, left, right, file_path)?;
        println!("{content}");
        return Ok(());
    }

    let result = diff::diff_profiles(paths, left, right)?;

    println!(
        "Diff: {} vs {}",
        style(&result.left_name).cyan(),
        style(&result.right_name).cyan()
    );
    println!(
        "  Shared (same content):    {} files",
        result.shared_same.len()
    );
    println!(
        "  Shared (different):       {} files",
        result.different_content.len()
    );
    println!(
        "  Only in {}:  {} files",
        result.left_name,
        result.only_left.len()
    );
    println!(
        "  Only in {}:  {} files",
        result.right_name,
        result.only_right.len()
    );

    if !result.different_content.is_empty() {
        println!();
        println!("  Different content:");
        for f in &result.different_content {
            println!(
                "    ~ {:<40} {} → {}",
                f.path,
                format_bytes(f.left_size),
                format_bytes(f.right_size)
            );
        }
    }

    if !result.only_left.is_empty() {
        println!();
        println!("  Only in {}:", result.left_name);
        for f in &result.only_left {
            println!("    + {f}");
        }
    }

    if !result.only_right.is_empty() {
        println!();
        println!("  Only in {}:", result.right_name);
        for f in &result.only_right {
            println!("    + {f}");
        }
    }

    Ok(())
}

fn cmd_rm(paths: &PortalPaths, name: &str) -> Result<()> {
    let profile_dir = paths.profile_dir(name);
    if !profile_dir.exists() {
        bail!("Profile \"{name}\" not found.");
    }

    let confirm = dialoguer::Confirm::new()
        .with_prompt(format!("Delete profile \"{name}\"? This cannot be undone"))
        .default(false)
        .interact()?;

    if !confirm {
        println!("Cancelled.");
        return Ok(());
    }

    std::fs::remove_dir_all(&profile_dir)?;
    println!("{} Profile \"{name}\" deleted.", style("✓").green().bold());
    Ok(())
}

fn cmd_reset(paths: &PortalPaths, cli: &Cli) -> Result<()> {
    if cli.dry_run {
        println!("{}", style("DRY RUN — no changes will be made").yellow());
        println!("Would reset ~/.claude/ to skeleton");
        return Ok(());
    }

    let confirm = dialoguer::Confirm::new()
        .with_prompt("Reset ~/.claude/ to skeleton? Current config will be backed up")
        .default(false)
        .interact()?;

    if !confirm {
        println!("Cancelled.");
        return Ok(());
    }

    let _lock = safety::acquire_lock(paths)?;
    let backup_path = backup::create(paths, "reset", "skeleton")?;

    let claude_dir = paths.claude_root();
    if claude_dir.exists() {
        std::fs::remove_dir_all(&claude_dir)?;
    }
    skeleton::create(&claude_dir)?;

    let portal_state = profile::PortalState {
        version: 1,
        active_profile: None,
        last_operation: Some(profile::LastOperation {
            op_type: profile::OperationType::Reset,
            profile: "skeleton".into(),
            timestamp: chrono::Utc::now(),
            backup_path: backup_path.to_string_lossy().to_string(),
            plugins_installed: false,
        }),
        skeleton_checksum: None,
    };
    state::write(&paths.state_file(), &portal_state)?;

    println!("{} Reset to skeleton.", style("✓").green().bold());
    println!("  Backup: {}", backup_path.display());
    Ok(())
}

fn cmd_undo(paths: &PortalPaths) -> Result<()> {
    let current_state = state::read(&paths.state_file())?;
    let last_op = current_state
        .last_operation
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No previous operation to undo"))?;

    let backup_path = std::path::Path::new(&last_op.backup_path);
    if !backup_path.exists() {
        bail!("Backup not found: {}", backup_path.display());
    }

    println!(
        "Last operation: {} \"{}\" at {}",
        match last_op.op_type {
            profile::OperationType::Load => "load",
            profile::OperationType::Reset => "reset",
            profile::OperationType::Undo => "undo",
        },
        last_op.profile,
        last_op.timestamp.format("%Y-%m-%d %H:%M")
    );

    let confirm = dialoguer::Confirm::new()
        .with_prompt("Restore from backup?")
        .default(true)
        .interact()?;

    if !confirm {
        println!("Cancelled.");
        return Ok(());
    }

    let _lock = safety::acquire_lock(paths)?;
    backup::restore(paths, backup_path)?;

    println!(
        "{} Restored from backup.",
        style("✓").green().bold()
    );
    Ok(())
}

fn cmd_status(paths: &PortalPaths) -> Result<()> {
    let current_state = state::read(&paths.state_file())?;

    println!("Portal Status");
    println!("─────────────");

    match &current_state.active_profile {
        Some(name) => println!("Active profile: {}", style(name).green().bold()),
        None => println!("Active profile: {}", style("none (skeleton)").dim()),
    }

    if let Some(op) = &current_state.last_operation {
        println!(
            "Last operation: {} \"{}\" ({})",
            match op.op_type {
                profile::OperationType::Load => "load",
                profile::OperationType::Reset => "reset",
                profile::OperationType::Undo => "undo",
            },
            op.profile,
            op.timestamp.format("%Y-%m-%d %H:%M")
        );
    }

    // Count profiles and backups
    let profile_count = std::fs::read_dir(paths.profiles_root())
        .map(|rd| rd.filter_map(|e| e.ok()).count())
        .unwrap_or(0);
    let backup_list = backup::list(paths)?;

    println!();
    println!(
        "{} profiles, {} backups",
        profile_count,
        backup_list.len()
    );

    // Check for crash recovery
    if paths.claude_old().exists() {
        println!();
        println!(
            "{} ~/.claude.portal-old exists — previous swap may have crashed",
            style("WARNING:").red().bold()
        );
    }

    Ok(())
}

fn cmd_rename(paths: &PortalPaths, old: &str, new: &str) -> Result<()> {
    let old_dir = paths.profile_dir(old);
    let new_dir = paths.profile_dir(new);

    if !old_dir.exists() {
        bail!("Profile \"{old}\" not found.");
    }
    if new_dir.exists() {
        bail!("Profile \"{new}\" already exists.");
    }

    std::fs::rename(&old_dir, &new_dir)?;

    // Update manifest name
    let manifest_path = paths.profile_manifest(new);
    if manifest_path.exists() {
        let mut m = manifest::read(&manifest_path)?;
        m.name = new.to_string();
        manifest::write(&manifest_path, &m)?;
    }

    println!(
        "{} Renamed \"{}\" → \"{}\"",
        style("✓").green().bold(),
        old,
        new
    );
    Ok(())
}

fn cmd_verify(paths: &PortalPaths, name: Option<&str>, _fix_plugins: bool) -> Result<()> {
    let profile_name = match name {
        Some(n) => n.to_string(),
        None => {
            let s = state::read(&paths.state_file())?;
            s.active_profile
                .ok_or_else(|| anyhow::anyhow!("No active profile. Specify a profile name."))?
        }
    };

    let m = manifest::read(&paths.profile_manifest(&profile_name))?;
    let mismatches = portal::core::checksum::verify_manifest(
        &paths.profile_files_dir(&profile_name),
        &m.files,
    )?;

    if mismatches.is_empty() {
        println!(
            "{} Profile \"{}\" — {}/{} files verified",
            style("✓").green().bold(),
            profile_name,
            m.files.len(),
            m.files.len()
        );
    } else {
        println!(
            "{} Profile \"{}\" — {} integrity failures:",
            style("✗").red().bold(),
            profile_name,
            mismatches.len()
        );
        for mm in &mismatches {
            println!("  {} {}", style("✗").red(), mm.path);
            println!("    expected: {}", mm.expected);
            println!("    actual:   {}", mm.actual);
        }
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
```

- [ ] **Step 2: Update main.rs**

`src/main.rs`:
```rust
use anyhow::Result;

mod cli;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    cli::run()
}
```

- [ ] **Step 3: Verify build**

```bash
cargo build 2>&1
cargo clippy -- -D warnings 2>&1
```

- [ ] **Step 4: Run all tests**

```bash
cargo test 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: complete CLI command suite with clap derive"
```

---

## Task 13: Config File Support

**Files:**
- Create: `src/config.rs`

- [ ] **Step 1: Implement config**

`src/config.rs`:
```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalConfig {
    #[serde(default)]
    pub backup: BackupConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default = "default_reinstall_timeout")]
    pub reinstall_timeout_secs: u64,
    #[serde(default)]
    pub retry_failed_on_status: bool,
}

fn default_max_count() -> usize { 10 }
fn default_max_age_days() -> u32 { 90 }
fn default_compression() -> String { "zstd".into() }
fn default_compression_level() -> u32 { 3 }
fn default_reinstall_timeout() -> u64 { 30 }

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

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            reinstall_timeout_secs: default_reinstall_timeout(),
            retry_failed_on_status: false,
        }
    }
}

impl Default for PortalConfig {
    fn default() -> Self {
        Self {
            backup: BackupConfig::default(),
            plugins: PluginsConfig::default(),
        }
    }
}

pub fn load(path: &Path) -> Result<PortalConfig> {
    if !path.exists() {
        return Ok(PortalConfig::default());
    }
    let content = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}
```

- [ ] **Step 2: Verify build**

```bash
cargo build 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: portal.config.toml support with defaults"
```

---

## Task 14: Integration Test Suite

**Files:**
- Modify: existing test files
- Create: `tests/integration/cli_test.rs`

- [ ] **Step 1: Write CLI integration tests**

`tests/integration/cli_test.rs`:
```rust
use assert_cmd::Command;

#[test]
fn test_cli_version() {
    Command::cargo_bin("portal")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains("portal"));
}

#[test]
fn test_cli_list_empty() {
    let tmp = tempfile::TempDir::new().unwrap();
    Command::cargo_bin("portal")
        .unwrap()
        .env("HOME", tmp.path())
        .arg("list")
        .assert()
        .success();
}

#[test]
fn test_cli_status() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create .portal dir
    std::fs::create_dir_all(tmp.path().join(".portal/profiles")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".portal/backups")).unwrap();

    Command::cargo_bin("portal")
        .unwrap()
        .env("HOME", tmp.path())
        .arg("status")
        .assert()
        .success();
}
```

- [ ] **Step 2: Run all tests**

```bash
cargo test 2>&1
```

- [ ] **Step 3: Run clippy and audit**

```bash
cargo clippy -- -D warnings 2>&1
cargo audit 2>&1 || echo "cargo-audit not installed — install with: cargo install cargo-audit"
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test: integration tests for CLI commands"
```

---

## Task 15: Create Worktree — Ratatui TUI

**Files:**
- Create: `src/tui/mod.rs`
- Create: `src/tui/app.rs`
- Create: `src/tui/ui.rs`
- Create: `src/tui/event.rs`

- [ ] **Step 1: Create the worktree branch**

```bash
cd /Users/rohit/Documents/portal
git checkout -b tui/ratatui
```

- [ ] **Step 2: Create TUI module entry**

`src/tui/mod.rs`:
```rust
mod app;
mod event;
mod ui;

use anyhow::Result;
use crate::storage::paths::PortalPaths;

pub fn run(paths: &PortalPaths) -> Result<()> {
    let mut app = app::App::new(paths.clone())?;
    ratatui::run(|mut terminal| {
        loop {
            terminal.draw(|frame| ui::render(frame, &mut app))?;
            if event::handle(&mut app)? {
                break Ok(());
            }
        }
    })
}
```

- [ ] **Step 3: Implement TUI app state**

`src/tui/app.rs`:
```rust
use ratatui::widgets::ListState;

use crate::core::profile::{PluginBlueprint, ProfileManifest, ProfileMeta};
use crate::storage::{manifest, meta, paths::PortalPaths, plugins_manifest, state};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Detail,
    Diff,
    ContentDiff,
    SaveDialog,
    LoadConfirm,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
}

pub struct ProfileInfo {
    pub name: String,
    pub manifest: ProfileManifest,
    pub meta: Option<ProfileMeta>,
    pub blueprint: Option<PluginBlueprint>,
}

pub struct App {
    pub paths: PortalPaths,
    pub profiles: Vec<ProfileInfo>,
    pub active_profile: Option<String>,
    pub list_state: ListState,
    pub view: View,
    pub active_pane: Pane,
    pub should_quit: bool,
    pub diff_target: Option<usize>,
    pub file_scroll: u16,
    pub status_message: Option<String>,

    // Save dialog state
    pub save_name: String,
    pub save_description: String,
    pub save_tags: String,
    pub save_field_index: usize,
}

impl App {
    pub fn new(paths: PortalPaths) -> anyhow::Result<Self> {
        let mut app = Self {
            paths,
            profiles: Vec::new(),
            active_profile: None,
            list_state: ListState::default(),
            view: View::Detail,
            active_pane: Pane::Left,
            should_quit: false,
            diff_target: None,
            file_scroll: 0,
            status_message: None,
            save_name: String::new(),
            save_description: String::new(),
            save_tags: String::new(),
            save_field_index: 0,
        };
        app.refresh()?;
        if !app.profiles.is_empty() {
            app.list_state.select(Some(0));
        }
        Ok(app)
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        self.profiles.clear();
        let profiles_dir = self.paths.profiles_root();
        if !profiles_dir.exists() {
            return Ok(());
        }

        let current_state = state::read(&self.paths.state_file())?;
        self.active_profile = current_state.active_profile;

        let mut entries: Vec<_> = std::fs::read_dir(&profiles_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let manifest_path = self.paths.profile_manifest(&name);
            if !manifest_path.exists() {
                continue;
            }
            let man = manifest::read(&manifest_path)?;
            let met = meta::read(&self.paths.profile_meta(&name)).ok();
            let bp = plugins_manifest::read(&self.paths.profile_plugins(&name)).ok();

            self.profiles.push(ProfileInfo {
                name,
                manifest: man,
                meta: met,
                blueprint: bp,
            });
        }

        Ok(())
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn selected_profile(&self) -> Option<&ProfileInfo> {
        self.selected_index().and_then(|i| self.profiles.get(i))
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.active_profile.as_deref() == Some(name)
    }
}
```

- [ ] **Step 4: Implement TUI rendering**

`src/tui/ui.rs`:
```rust
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, View};

pub fn render(frame: &mut Frame, app: &mut App) {
    let [main_area, status_bar] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
    ]).areas(frame.area());

    let [left_pane, right_pane] = Layout::horizontal([
        Constraint::Length(28),
        Constraint::Min(40),
    ]).areas(main_area);

    render_profile_list(frame, app, left_pane);

    match app.view {
        View::Detail => render_detail(frame, app, right_pane),
        View::Diff => render_diff(frame, app, right_pane),
        View::ContentDiff => render_content_diff(frame, app, right_pane),
        View::SaveDialog => render_save_dialog(frame, app, right_pane),
        View::LoadConfirm => render_load_confirm(frame, app, right_pane),
        View::Help => render_help(frame, right_pane),
    }

    render_status_bar(frame, app, status_bar);
}

fn render_profile_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .profiles
        .iter()
        .map(|p| {
            let marker = if app.is_active(&p.name) { "● " } else { "○ " };
            let suffix = if app.is_active(&p.name) { " *" } else { "" };
            let style = if app.is_active(&p.name) {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(format!("{}{suffix}", p.name), style),
            ]))
        })
        .collect();

    let title = format!(" Profiles ({}) ", items.len());
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_bottom(" * = active "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(profile) = app.selected_profile() else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Detail ");
        let para = Paragraph::new("No profile selected")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, area);
        return;
    };

    let active_marker = if app.is_active(&profile.name) { " ● active" } else { "" };
    let title = format!(" {}{active_marker} ", profile.name);

    let mut lines: Vec<Line> = Vec::new();

    // Metadata
    if let Some(meta) = &profile.meta {
        lines.push(Line::from(vec![
            Span::styled("Description: ", Style::default().fg(Color::Cyan)),
            Span::raw(&meta.description),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Tags: ", Style::default().fg(Color::Cyan)),
            Span::raw(meta.tags.join(", ")),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("Created: ", Style::default().fg(Color::Cyan)),
        Span::raw(profile.manifest.created_at.format("%Y-%m-%d").to_string()),
    ]));
    if let Some(ll) = profile.manifest.last_loaded {
        lines.push(Line::from(vec![
            Span::styled("Last loaded: ", Style::default().fg(Color::Cyan)),
            Span::raw(ll.format("%Y-%m-%d %H:%M").to_string()),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("Load count: ", Style::default().fg(Color::Cyan)),
        Span::raw(profile.manifest.load_count.to_string()),
    ]));
    lines.push(Line::raw(""));

    // Files
    lines.push(Line::from(Span::styled(
        format!("Tracked Files ({})", profile.manifest.files.len()),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));

    let mut sorted: Vec<_> = profile.manifest.files.iter().collect();
    sorted.sort_by_key(|(k, _)| k.clone());
    for (path, entry) in &sorted {
        let size = format_bytes(entry.size);
        lines.push(Line::from(vec![
            Span::styled("  ● ", Style::default().fg(Color::Green)),
            Span::raw(format!("{path:<35} {size:>6}")),
        ]));
    }

    // Plugins
    if let Some(bp) = &profile.blueprint {
        if !bp.plugins.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                format!("Plugins ({})", bp.plugins.len()),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
            for p in &bp.plugins {
                let source = match &p.source {
                    crate::core::profile::PluginSource::Marketplace { .. } => "marketplace",
                    crate::core::profile::PluginSource::Local { .. } => "local",
                    crate::core::profile::PluginSource::Github { .. } => "github",
                };
                lines.push(Line::from(vec![
                    Span::styled("  ● ", Style::default().fg(Color::Cyan)),
                    Span::raw(format!("{:<25} {source}", p.id)),
                ]));
            }
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(Color::Yellow)),
        Span::raw(" Load  "),
        Span::styled("[d]", Style::default().fg(Color::Yellow)),
        Span::raw(" Diff  "),
        Span::styled("[x]", Style::default().fg(Color::Yellow)),
        Span::raw(" Delete  "),
        Span::styled("[s]", Style::default().fg(Color::Yellow)),
        Span::raw(" Save current"),
    ]));

    let block = Block::default().borders(Borders::ALL).title(title);
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn render_diff(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Diff Mode ");
    let para = Paragraph::new("Diff view — select a profile and press [d] to compare")
        .block(block);
    frame.render_widget(para, area);
}

fn render_content_diff(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" File Diff ");
    let para = Paragraph::new("Content diff view").block(block);
    frame.render_widget(para, area);
}

fn render_save_dialog(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Save Profile ");

    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Profile name: ", Style::default().fg(Color::Cyan)),
            Span::raw(&app.save_name),
            Span::styled("_", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Description:  ", Style::default().fg(Color::Cyan)),
            Span::raw(&app.save_description),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Tags:         ", Style::default().fg(Color::Cyan)),
            Span::raw(&app.save_tags),
        ]),
        Line::raw(""),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  [Enter]", Style::default().fg(Color::Yellow)),
            Span::raw(" Save   "),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::raw(" Cancel"),
        ]),
    ];

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn render_load_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm Load ");

    let name = app
        .selected_profile()
        .map(|p| p.name.as_str())
        .unwrap_or("?");

    let lines = vec![
        Line::raw(""),
        Line::from(Span::styled(
            format!("  Load profile \"{name}\"?"),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::raw("  Backup will be created before swap."),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  [y]", Style::default().fg(Color::Green)),
            Span::raw(" Load   "),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::raw(" Cancel"),
        ]),
    ];

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ");

    let lines = vec![
        Line::raw(""),
        Line::from(Span::styled("  Key Bindings", Style::default().add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::raw("  ↑/↓, j/k     Navigate profiles"),
        Line::raw("  Enter         Load selected / confirm"),
        Line::raw("  d             Diff selected vs active"),
        Line::raw("  s             Save current .claude/"),
        Line::raw("  x             Delete selected profile"),
        Line::raw("  u             Undo last operation"),
        Line::raw("  r             Reset to skeleton"),
        Line::raw("  ?             Toggle this help"),
        Line::raw("  q             Quit"),
        Line::raw(""),
        Line::raw("  Esc           Back / Cancel"),
    ];

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let active = app
        .active_profile
        .as_deref()
        .unwrap_or("none");
    let profile_count = app.profiles.len();

    let status = Line::from(vec![
        Span::styled(" Active: ", Style::default().fg(Color::Cyan)),
        Span::raw(active),
        Span::raw(" │ "),
        Span::styled("Profiles: ", Style::default().fg(Color::Cyan)),
        Span::raw(profile_count.to_string()),
        Span::raw(" │ "),
        Span::styled("[?]", Style::default().fg(Color::DarkGray)),
        Span::styled(" help  ", Style::default().fg(Color::DarkGray)),
        Span::styled("[q]", Style::default().fg(Color::DarkGray)),
        Span::styled(" quit", Style::default().fg(Color::DarkGray)),
    ]);

    let para = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(para, area);
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
```

- [ ] **Step 5: Implement TUI event handling**

`src/tui/event.rs`:
```rust
use std::time::Duration;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use super::app::{App, Pane, View};
use crate::core::{loader, snapshot};

/// Handle terminal events. Returns true if the app should quit.
pub fn handle(app: &mut App) -> std::io::Result<bool> {
    if !event::poll(Duration::from_millis(50))? {
        return Ok(false);
    }

    let Event::Key(key) = event::read()? else {
        return Ok(false);
    };

    if key.kind != KeyEventKind::Press {
        return Ok(false);
    }

    // Global: Ctrl+C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    match app.view {
        View::Detail | View::Diff => handle_main(app, key.code),
        View::SaveDialog => handle_save_dialog(app, key.code),
        View::LoadConfirm => handle_load_confirm(app, key.code),
        View::Help => handle_help(app, key.code),
        View::ContentDiff => handle_content_diff(app, key.code),
    }

    Ok(app.should_quit)
}

fn handle_main(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.view = View::Help,

        // Navigation
        KeyCode::Up | KeyCode::Char('k') => {
            app.list_state.select_previous();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.list_state.select_next();
        }

        // Actions
        KeyCode::Enter => {
            if let Some(profile) = app.selected_profile() {
                if !app.is_active(&profile.name) {
                    app.view = View::LoadConfirm;
                }
            }
        }
        KeyCode::Char('d') => {
            app.view = if app.view == View::Diff {
                View::Detail
            } else {
                View::Diff
            };
        }
        KeyCode::Char('s') => {
            app.save_name.clear();
            app.save_description.clear();
            app.save_tags.clear();
            app.save_field_index = 0;
            app.view = View::SaveDialog;
        }
        KeyCode::Char('x') => {
            if let Some(profile) = app.selected_profile() {
                let name = profile.name.clone();
                if !app.is_active(&name) {
                    let dir = app.paths.profile_dir(&name);
                    let _ = std::fs::remove_dir_all(&dir);
                    let _ = app.refresh();
                }
            }
        }
        KeyCode::Char('u') => {
            // Undo — delegate to backup restore
            app.status_message = Some("Use CLI: portal undo".into());
        }

        KeyCode::Esc => {
            if app.view != View::Detail {
                app.view = View::Detail;
            }
        }
        _ => {}
    }
}

fn handle_save_dialog(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.view = View::Detail,
        KeyCode::Tab => {
            app.save_field_index = (app.save_field_index + 1) % 3;
        }
        KeyCode::Enter => {
            if !app.save_name.is_empty() {
                let tags: Vec<String> = app
                    .save_tags
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let _ = snapshot::save(
                    &app.paths,
                    &app.save_name,
                    &app.save_description,
                    &tags,
                );
                let _ = app.refresh();
                app.view = View::Detail;
            }
        }
        KeyCode::Backspace => {
            let field = match app.save_field_index {
                0 => &mut app.save_name,
                1 => &mut app.save_description,
                _ => &mut app.save_tags,
            };
            field.pop();
        }
        KeyCode::Char(c) => {
            let field = match app.save_field_index {
                0 => &mut app.save_name,
                1 => &mut app.save_description,
                _ => &mut app.save_tags,
            };
            field.push(c);
        }
        _ => {}
    }
}

fn handle_load_confirm(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('y') | KeyCode::Enter => {
            if let Some(profile) = app.selected_profile() {
                let name = profile.name.clone();
                match loader::load(&app.paths, &name, false, true) {
                    Ok(_) => {
                        app.status_message = Some(format!("Loaded: {name}"));
                        let _ = app.refresh();
                    }
                    Err(e) => {
                        app.status_message = Some(format!("Error: {e}"));
                    }
                }
            }
            app.view = View::Detail;
        }
        KeyCode::Esc | KeyCode::Char('n') => app.view = View::Detail,
        _ => {}
    }
}

fn handle_help(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
            app.view = View::Detail;
        }
        _ => {}
    }
}

fn handle_content_diff(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.view = View::Diff,
        KeyCode::Char('j') | KeyCode::Down => app.file_scroll = app.file_scroll.saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => app.file_scroll = app.file_scroll.saturating_sub(1),
        _ => {}
    }
}
```

- [ ] **Step 6: Add TUI module to lib.rs behind feature gate**

Add to `src/lib.rs`:
```rust
pub mod core;
pub mod storage;
pub mod config;

#[cfg(feature = "tui-ratatui")]
pub mod tui;
```

- [ ] **Step 7: Verify build with feature**

```bash
cargo build --features tui-ratatui 2>&1
cargo clippy --features tui-ratatui -- -D warnings 2>&1
```

- [ ] **Step 8: Commit on tui/ratatui branch**

```bash
git add -A
git commit -m "feat: ratatui TUI with split-pane browser, detail, diff, save, load"
```

---

## Task 16: Create Worktree — FrankenTUI (ftui)

**Files:**
- Create: `src/tui/mod.rs`
- Create: `src/tui/app.rs`
- Create: `src/tui/ui.rs`

- [ ] **Step 1: Switch back to main and create ftui branch**

```bash
cd /Users/rohit/Documents/portal
git checkout main
git checkout -b tui/ftui
```

- [ ] **Step 2: Create TUI module with Elm-style Model**

`src/tui/mod.rs`:
```rust
mod app;
mod ui;

use anyhow::Result;
use crate::storage::paths::PortalPaths;

pub fn run(paths: &PortalPaths) -> Result<()> {
    let model = app::PortalModel::new(paths.clone())?;
    ftui::App::new(model)
        .screen_mode(ftui::ScreenMode::Fullscreen)
        .run()
        .map_err(|e| anyhow::anyhow!("TUI error: {e}"))
}
```

- [ ] **Step 3: Implement Elm-style Model**

`src/tui/app.rs`:
```rust
use ftui_core::event::Event;
use ftui_core::geometry::Rect;
use ftui_render::frame::Frame;
use ftui_runtime::{Cmd, Model};

use crate::core::profile::{PluginBlueprint, ProfileManifest, ProfileMeta};
use crate::core::{loader, snapshot};
use crate::storage::{manifest, meta, paths::PortalPaths, plugins_manifest, state};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Detail,
    Diff,
    SaveDialog,
    LoadConfirm,
    Help,
}

pub struct ProfileInfo {
    pub name: String,
    pub manifest: ProfileManifest,
    pub meta: Option<ProfileMeta>,
    pub blueprint: Option<PluginBlueprint>,
}

pub enum Msg {
    Quit,
    NavigateUp,
    NavigateDown,
    Select,
    ToggleDiff,
    OpenSave,
    DeleteSelected,
    ToggleHelp,
    ConfirmLoad,
    CancelModal,
    TypeChar(char),
    Backspace,
    TabField,
    Noop,
    Refresh,
}

pub struct PortalModel {
    pub paths: PortalPaths,
    pub profiles: Vec<ProfileInfo>,
    pub active_profile: Option<String>,
    pub selected: usize,
    pub view: View,
    pub status_message: Option<String>,

    // Save dialog
    pub save_name: String,
    pub save_description: String,
    pub save_tags: String,
    pub save_field: usize,
}

impl From<Event> for Msg {
    fn from(event: Event) -> Self {
        match event {
            Event::Key(k) if k.is_char('q') => Msg::Quit,
            Event::Key(k) if k.is_char('?') => Msg::ToggleHelp,
            Event::Key(k) if k.is_char('k') || k.is_up() => Msg::NavigateUp,
            Event::Key(k) if k.is_char('j') || k.is_down() => Msg::NavigateDown,
            Event::Key(k) if k.is_enter() => Msg::Select,
            Event::Key(k) if k.is_char('d') => Msg::ToggleDiff,
            Event::Key(k) if k.is_char('s') => Msg::OpenSave,
            Event::Key(k) if k.is_char('x') => Msg::DeleteSelected,
            Event::Key(k) if k.is_char('y') => Msg::ConfirmLoad,
            Event::Key(k) if k.is_esc() => Msg::CancelModal,
            Event::Key(k) if k.is_tab() => Msg::TabField,
            Event::Key(k) if k.is_backspace() => Msg::Backspace,
            Event::Key(k) => {
                if let Some(c) = k.char() {
                    Msg::TypeChar(c)
                } else {
                    Msg::Noop
                }
            }
            _ => Msg::Noop,
        }
    }
}

impl Model for PortalModel {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        Cmd::Msg(Msg::Refresh)
    }

    fn update(&mut self, msg: Msg) -> Cmd<Msg> {
        match msg {
            Msg::Quit => return Cmd::quit(),
            Msg::Refresh => {
                let _ = self.refresh();
            }
            Msg::NavigateUp => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            Msg::NavigateDown => {
                if self.selected + 1 < self.profiles.len() {
                    self.selected += 1;
                }
            }
            Msg::Select => match self.view {
                View::Detail => {
                    if let Some(p) = self.profiles.get(self.selected) {
                        if !self.is_active(&p.name) {
                            self.view = View::LoadConfirm;
                        }
                    }
                }
                View::SaveDialog => {
                    if !self.save_name.is_empty() {
                        let tags: Vec<String> = self.save_tags
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let _ = snapshot::save(
                            &self.paths,
                            &self.save_name,
                            &self.save_description,
                            &tags,
                        );
                        let _ = self.refresh();
                        self.view = View::Detail;
                    }
                }
                _ => {}
            },
            Msg::ToggleDiff => {
                self.view = if self.view == View::Diff {
                    View::Detail
                } else {
                    View::Diff
                };
            }
            Msg::OpenSave => {
                self.save_name.clear();
                self.save_description.clear();
                self.save_tags.clear();
                self.save_field = 0;
                self.view = View::SaveDialog;
            }
            Msg::DeleteSelected => {
                if let Some(p) = self.profiles.get(self.selected) {
                    let name = p.name.clone();
                    if !self.is_active(&name) {
                        let dir = self.paths.profile_dir(&name);
                        let _ = std::fs::remove_dir_all(&dir);
                        let _ = self.refresh();
                    }
                }
            }
            Msg::ToggleHelp => {
                self.view = if self.view == View::Help {
                    View::Detail
                } else {
                    View::Help
                };
            }
            Msg::ConfirmLoad => {
                if self.view == View::LoadConfirm {
                    if let Some(p) = self.profiles.get(self.selected) {
                        let name = p.name.clone();
                        match loader::load(&self.paths, &name, false, true) {
                            Ok(_) => {
                                self.status_message = Some(format!("Loaded: {name}"));
                                let _ = self.refresh();
                            }
                            Err(e) => {
                                self.status_message = Some(format!("Error: {e}"));
                            }
                        }
                    }
                    self.view = View::Detail;
                }
            }
            Msg::CancelModal => {
                self.view = View::Detail;
            }
            Msg::TypeChar(c) => {
                if self.view == View::SaveDialog {
                    let field = match self.save_field {
                        0 => &mut self.save_name,
                        1 => &mut self.save_description,
                        _ => &mut self.save_tags,
                    };
                    field.push(c);
                }
            }
            Msg::Backspace => {
                if self.view == View::SaveDialog {
                    let field = match self.save_field {
                        0 => &mut self.save_name,
                        1 => &mut self.save_description,
                        _ => &mut self.save_tags,
                    };
                    field.pop();
                }
            }
            Msg::TabField => {
                if self.view == View::SaveDialog {
                    self.save_field = (self.save_field + 1) % 3;
                }
            }
            Msg::Noop => {}
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame) {
        super::ui::render(self, frame);
    }
}

impl PortalModel {
    pub fn new(paths: PortalPaths) -> anyhow::Result<Self> {
        let mut model = Self {
            paths,
            profiles: Vec::new(),
            active_profile: None,
            selected: 0,
            view: View::Detail,
            status_message: None,
            save_name: String::new(),
            save_description: String::new(),
            save_tags: String::new(),
            save_field: 0,
        };
        model.refresh()?;
        Ok(model)
    }

    fn refresh(&mut self) -> anyhow::Result<()> {
        self.profiles.clear();
        let profiles_dir = self.paths.profiles_root();
        if !profiles_dir.exists() {
            return Ok(());
        }

        let current_state = state::read(&self.paths.state_file())?;
        self.active_profile = current_state.active_profile;

        let mut entries: Vec<_> = std::fs::read_dir(&profiles_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let manifest_path = self.paths.profile_manifest(&name);
            if !manifest_path.exists() { continue; }
            let man = manifest::read(&manifest_path)?;
            let met = meta::read(&self.paths.profile_meta(&name)).ok();
            let bp = plugins_manifest::read(&self.paths.profile_plugins(&name)).ok();
            self.profiles.push(ProfileInfo { name, manifest: man, meta: met, blueprint: bp });
        }
        Ok(())
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.active_profile.as_deref() == Some(name)
    }

    pub fn selected_profile(&self) -> Option<&ProfileInfo> {
        self.profiles.get(self.selected)
    }
}
```

- [ ] **Step 4: Implement ftui rendering**

`src/tui/ui.rs`:
```rust
use ftui_core::geometry::Rect;
use ftui_render::frame::Frame;
use ftui_layout::{Flex, Constraint};
use ftui_widgets::{block::Block, paragraph::Paragraph, list::List};

use super::app::{PortalModel, View};

pub fn render(model: &PortalModel, frame: &mut Frame) {
    let area = Rect::new(0, 0, frame.width(), frame.height());

    let chunks = Flex::vertical()
        .constraints([Constraint::Min(3), Constraint::Fixed(1)])
        .split(area);

    let main_chunks = Flex::horizontal()
        .constraints([Constraint::Fixed(28), Constraint::Fill])
        .split(chunks[0]);

    render_profile_list(model, frame, main_chunks[0]);

    match model.view {
        View::Detail => render_detail(model, frame, main_chunks[1]),
        View::Diff => render_diff(model, frame, main_chunks[1]),
        View::SaveDialog => render_save(model, frame, main_chunks[1]),
        View::LoadConfirm => render_load_confirm(model, frame, main_chunks[1]),
        View::Help => render_help(frame, main_chunks[1]),
    }

    render_status_bar(model, frame, chunks[1]);
}

fn render_profile_list(model: &PortalModel, frame: &mut Frame, area: Rect) {
    let items: Vec<String> = model.profiles.iter().enumerate().map(|(i, p)| {
        let marker = if model.is_active(&p.name) { "● " } else { "○ " };
        let selected = if i == model.selected { "▸ " } else { "  " };
        let suffix = if model.is_active(&p.name) { " *" } else { "" };
        format!("{selected}{marker}{}{suffix}", p.name)
    }).collect();

    let block = Block::new()
        .title(format!(" Profiles ({}) ", items.len()))
        .borders(true);

    let list = List::new(items).block(block);
    list.render(area, frame);
}

fn render_detail(model: &PortalModel, frame: &mut Frame, area: Rect) {
    let Some(profile) = model.selected_profile() else {
        Paragraph::new("No profile selected")
            .block(Block::new().title(" Detail ").borders(true))
            .render(area, frame);
        return;
    };

    let active_tag = if model.is_active(&profile.name) { " ● active" } else { "" };
    let mut text = String::new();

    if let Some(meta) = &profile.meta {
        text.push_str(&format!("Description: {}\n", meta.description));
        text.push_str(&format!("Tags: {}\n", meta.tags.join(", ")));
    }
    text.push_str(&format!("Created: {}\n", profile.manifest.created_at.format("%Y-%m-%d")));
    if let Some(ll) = profile.manifest.last_loaded {
        text.push_str(&format!("Last loaded: {}\n", ll.format("%Y-%m-%d %H:%M")));
    }
    text.push_str(&format!("Load count: {}\n\n", profile.manifest.load_count));

    text.push_str(&format!("Tracked Files ({})\n", profile.manifest.files.len()));
    let mut sorted: Vec<_> = profile.manifest.files.iter().collect();
    sorted.sort_by_key(|(k, _)| k.clone());
    for (path, entry) in &sorted {
        text.push_str(&format!("  ● {:<35} {:>6}\n", path, format_bytes(entry.size)));
    }

    if let Some(bp) = &profile.blueprint {
        if !bp.plugins.is_empty() {
            text.push_str(&format!("\nPlugins ({})\n", bp.plugins.len()));
            for p in &bp.plugins {
                let source = match &p.source {
                    crate::core::profile::PluginSource::Marketplace { .. } => "marketplace",
                    crate::core::profile::PluginSource::Local { .. } => "local",
                    crate::core::profile::PluginSource::Github { .. } => "github",
                };
                text.push_str(&format!("  ● {:<25} {}\n", p.id, source));
            }
        }
    }

    text.push_str("\n[Enter] Load  [d] Diff  [x] Delete  [s] Save current\n");

    Paragraph::new(text)
        .block(Block::new().title(format!(" {}{active_tag} ", profile.name)).borders(true))
        .render(area, frame);
}

fn render_diff(model: &PortalModel, frame: &mut Frame, area: Rect) {
    Paragraph::new("Diff view — press [d] on a profile to compare")
        .block(Block::new().title(" Diff Mode ").borders(true))
        .render(area, frame);
}

fn render_save(model: &PortalModel, frame: &mut Frame, area: Rect) {
    let text = format!(
        "\n  Profile name: {}_\n\n  Description:  {}\n\n  Tags:         {}\n\n\n  [Enter] Save   [Esc] Cancel",
        model.save_name, model.save_description, model.save_tags
    );
    Paragraph::new(text)
        .block(Block::new().title(" Save Profile ").borders(true))
        .render(area, frame);
}

fn render_load_confirm(model: &PortalModel, frame: &mut Frame, area: Rect) {
    let name = model.selected_profile().map(|p| p.name.as_str()).unwrap_or("?");
    let text = format!(
        "\n  Load profile \"{name}\"?\n\n  Backup will be created before swap.\n\n  [y] Load   [Esc] Cancel"
    );
    Paragraph::new(text)
        .block(Block::new().title(" Confirm Load ").borders(true))
        .render(area, frame);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let text = "\n  Key Bindings\n\n  ↑/↓, j/k     Navigate profiles\n  Enter         Load / confirm\n  d             Diff mode\n  s             Save current\n  x             Delete profile\n  ?             Toggle help\n  q             Quit\n\n  Esc           Back / Cancel";
    Paragraph::new(text)
        .block(Block::new().title(" Help ").borders(true))
        .render(area, frame);
}

fn render_status_bar(model: &PortalModel, frame: &mut Frame, area: Rect) {
    let active = model.active_profile.as_deref().unwrap_or("none");
    let text = format!(
        " Active: {} │ Profiles: {} │ [?] help  [q] quit",
        active, model.profiles.len()
    );
    Paragraph::new(text).render(area, frame);
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
```

- [ ] **Step 5: Add TUI module to lib.rs behind ftui feature gate**

In `src/lib.rs`, the ftui feature gate:
```rust
#[cfg(feature = "tui-ftui")]
pub mod tui;
```

- [ ] **Step 6: Verify build with ftui feature**

```bash
cargo build --features tui-ftui 2>&1
```

Note: This may fail if ftui git deps aren't fully available. Document any compilation issues for comparison.

- [ ] **Step 7: Commit on tui/ftui branch**

```bash
git add -A
git commit -m "feat: ftui (FrankenTUI) TUI with Elm-style Model architecture"
```

---

## Task 17: Final Verification + Branch Summary

- [ ] **Step 1: Verify main branch builds clean**

```bash
git checkout main
cargo build 2>&1
cargo test 2>&1
cargo clippy -- -D warnings 2>&1
```

- [ ] **Step 2: Verify ratatui branch builds**

```bash
git checkout tui/ratatui
cargo build --features tui-ratatui 2>&1
cargo clippy --features tui-ratatui -- -D warnings 2>&1
```

- [ ] **Step 3: Verify ftui branch builds**

```bash
git checkout tui/ftui
cargo build --features tui-ftui 2>&1
```

- [ ] **Step 4: Document comparison**

Create `docs/TUI_COMPARISON.md` on main branch:

```markdown
# TUI Framework Comparison

## Ratatui (branch: tui/ratatui)
- **Maturity**: Stable, widely used, large ecosystem
- **Architecture**: Manual event loop (imperative)
- **Rendering**: Immediate-mode via Frame + render_widget calls
- **Layout**: `Layout::horizontal/vertical` with Constraint arrays
- **State**: Manual — you own ListState, scroll offsets, etc.
- **Widgets**: List, Table, Paragraph, Block, Tabs, Scrollbar
- **Dependencies**: ratatui 0.30 + crossterm 0.28
- **Build**: `cargo build --features tui-ratatui`

## FrankenTUI / ftui (branch: tui/ftui)
- **Maturity**: Experimental (0.3.1), most crates only on git
- **Architecture**: Elm/Bubbletea Model trait (functional)
- **Rendering**: Model.view() called by runtime
- **Layout**: Flex/Grid with richer constraint types (FitContent, etc.)
- **State**: Implicit — Model owns all state, update() returns Cmd
- **Widgets**: 80+ widgets including CommandPalette, FilePicker, DragPreview
- **Dependencies**: ftui 0.3.1 (git dep)
- **Build**: `cargo build --features tui-ftui`

## Recommendation
- **Ship with ratatui** for stability and ecosystem support
- **Watch ftui** — the Elm architecture is cleaner for complex state
- **Core is identical** on both branches — TUI is the only difference
```

- [ ] **Step 5: Commit comparison doc**

```bash
git checkout main
git add docs/TUI_COMPARISON.md
git commit -m "docs: TUI framework comparison (ratatui vs ftui)"
```
