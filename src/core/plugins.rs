use crate::core::profile::{
    MarketplaceEntry, MarketplaceSource, PluginBlueprint, PluginEntry, PluginSource,
};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

/// Result of attempting to install a single plugin.
///
/// `skipped == true` means the plugin was already current (same id and source
/// as the previously active profile) and reinstall was skipped as an
/// optimization. `success` is still `true` for skipped plugins because the
/// desired end state (plugin code on disk) is satisfied.
#[derive(Debug)]
pub struct PluginInstallResult {
    pub id: String,
    pub success: bool,
    pub skipped: bool,
    pub message: String,
}

/// Extract a plugin blueprint from `settings.json` in the given Claude directory.
///
/// Reads `enabledPlugins` and `extraKnownMarketplaces`, then correlates
/// plugin IDs with marketplace sources.
///
/// # Errors
///
/// Returns an error if `settings.json` cannot be read or parsed.
pub fn extract_blueprint(claude_dir: &Path) -> Result<PluginBlueprint> {
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.is_file() {
        return Ok(PluginBlueprint::default());
    }

    let raw = std::fs::read_to_string(&settings_path)
        .with_context(|| "reading settings.json for plugin extraction")?;
    let settings: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| "parsing settings.json")?;

    // Parse extraKnownMarketplaces first — needed for source resolution.
    let mut marketplaces: HashMap<String, MarketplaceEntry> = HashMap::new();

    if let Some(mkts) = settings
        .get("extraKnownMarketplaces")
        .and_then(|v| v.as_object())
    {
        for (name, val) in mkts {
            if let Some(src) = val.get("source").and_then(|v| v.as_object()) {
                let source_type = src.get("source").and_then(|v| v.as_str()).unwrap_or("");

                let ms = match source_type {
                    "github" => {
                        let repo = src
                            .get("repo")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        MarketplaceSource::Github { repo }
                    }
                    "directory" => {
                        let path = src
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        MarketplaceSource::Directory { path }
                    }
                    _ => continue,
                };

                marketplaces.insert(name.clone(), MarketplaceEntry { source: ms });
            }
        }
    }

    // Parse enabledPlugins.
    let mut plugins = Vec::new();

    if let Some(enabled) = settings.get("enabledPlugins").and_then(|v| v.as_object()) {
        for (id, val) in enabled {
            let is_enabled = val.as_bool().unwrap_or(false);
            let source = determine_source(&settings, id);
            plugins.push(PluginEntry {
                id: id.clone(),
                enabled: is_enabled,
                source,
            });
        }
    }

    // Sort plugins by ID for deterministic output.
    plugins.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(PluginBlueprint {
        version: 1,
        plugins,
        extra_known_marketplaces: marketplaces,
    })
}

/// Determine the source type for a plugin by inspecting marketplace info.
///
/// Plugin IDs have the form `name@marketplace`. The marketplace suffix
/// is looked up in `extraKnownMarketplaces`.
#[must_use]
pub fn determine_source(settings: &serde_json::Value, plugin_id: &str) -> PluginSource {
    // Split on '@' — last segment is the marketplace name.
    let parts: Vec<&str> = plugin_id.splitn(2, '@').collect();
    if parts.len() < 2 {
        // No '@' — treat as marketplace with unknown source.
        return PluginSource::Marketplace {
            marketplace: String::new(),
            repo: String::new(),
        };
    }

    let marketplace_name = parts[1];

    // Look up in extraKnownMarketplaces.
    if let Some(mkts) = settings
        .get("extraKnownMarketplaces")
        .and_then(|v| v.as_object())
        && let Some(mkt) = mkts.get(marketplace_name)
        && let Some(src) = mkt.get("source").and_then(|v| v.as_object())
    {
        let source_type = src.get("source").and_then(|v| v.as_str()).unwrap_or("");

        return match source_type {
            "github" => {
                let repo = src
                    .get("repo")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                PluginSource::Github { repo }
            }
            "directory" => {
                let path = src
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                PluginSource::Local { path }
            }
            _ => PluginSource::Marketplace {
                marketplace: marketplace_name.to_string(),
                repo: String::new(),
            },
        };
    }

    // Fallback: standard marketplace plugin.
    PluginSource::Marketplace {
        marketplace: marketplace_name.to_string(),
        repo: String::new(),
    }
}

/// Stable fingerprint that identifies "this plugin from this source" — the
/// pair we treat as install-equivalent. Two plugins with the same fingerprint
/// have the same code on disk; only the `enabled` flag may differ, and that
/// is carried in the swapped-in `settings.json` so it needs no reinstall.
fn plugin_fingerprint(entry: &PluginEntry) -> String {
    let source = match &entry.source {
        PluginSource::Marketplace { marketplace, repo } => {
            format!("marketplace:{marketplace}:{repo}")
        }
        PluginSource::Local { path } => format!("local:{path}"),
        PluginSource::Github { repo } => format!("github:{repo}"),
    };
    format!("{}|{source}", entry.id)
}

/// Partition the target blueprint into "must install" vs "already current"
/// against an optional active blueprint. With no active blueprint, everything
/// must install. With an active blueprint, any plugin whose fingerprint is
/// already present is skipped.
fn diff_plugins<'a>(
    target: &'a PluginBlueprint,
    active: Option<&PluginBlueprint>,
) -> (Vec<&'a PluginEntry>, Vec<PluginInstallResult>) {
    let active_set: HashSet<String> = active
        .map(|bp| bp.plugins.iter().map(plugin_fingerprint).collect())
        .unwrap_or_default();

    let mut to_install = Vec::new();
    let mut skipped = Vec::new();
    for entry in &target.plugins {
        if active_set.contains(&plugin_fingerprint(entry)) {
            skipped.push(PluginInstallResult {
                id: entry.id.clone(),
                success: true,
                skipped: true,
                message: "already installed".to_string(),
            });
        } else {
            to_install.push(entry);
        }
    }
    (to_install, skipped)
}

/// Attempt to reinstall every plugin in the blueprint via `claude plugin install`.
///
/// All failures are non-fatal and captured in the returned results.
#[must_use]
pub fn reinstall(blueprint: &PluginBlueprint) -> Vec<PluginInstallResult> {
    reinstall_with_diff(blueprint, None)
}

/// Reinstall only the plugins that differ between `target` and `active`.
///
/// When `active` is `None` (no prior state to compare against) this behaves
/// identically to [`reinstall`] — every plugin is freshly installed.
///
/// When `active` is provided, plugins whose `(id, source)` fingerprint matches
/// an entry in `active` are skipped: their code is already on disk from the
/// previous load, and the `enabled` flag is carried by `settings.json` which
/// the atomic swap has already replaced. For the toggle-back case where the
/// two profiles share most plugins, this drops a O(N) network/git wait to ~0.
///
/// Results are sorted by id and combine both the "skipped" and "freshly
/// installed" sets so callers always see the full target plugin list.
#[must_use]
pub fn reinstall_with_diff(
    target: &PluginBlueprint,
    active: Option<&PluginBlueprint>,
) -> Vec<PluginInstallResult> {
    let (to_install, mut results) = diff_plugins(target, active);
    results.extend(to_install.iter().copied().map(install_single));
    results.sort_by(|a, b| a.id.cmp(&b.id));
    results
}

/// Install a single plugin, returning the result.
fn install_single(entry: &PluginEntry) -> PluginInstallResult {
    let result = match &entry.source {
        PluginSource::Marketplace { .. } => run_claude_install(&entry.id),
        PluginSource::Local { path } => {
            let p = Path::new(path);
            if p.exists() {
                run_claude_install(path)
            } else {
                Err(anyhow::anyhow!("local plugin path does not exist: {path}"))
            }
        }
        PluginSource::Github { repo } => install_from_github(repo),
    };

    match result {
        Ok(msg) => PluginInstallResult {
            id: entry.id.clone(),
            success: true,
            skipped: false,
            message: msg,
        },
        Err(e) => PluginInstallResult {
            id: entry.id.clone(),
            success: false,
            skipped: false,
            message: format!("{e:#}"),
        },
    }
}

/// Run `claude plugin install <target>` with stdio fully captured so subprocess
/// output never leaks into a parent TUI's alternate screen.
fn run_claude_install(target: &str) -> Result<String> {
    let output = Command::new("claude")
        .args(["plugin", "install", target])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running claude plugin install {target}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("claude plugin install failed: {stderr}");
    }
}

/// Clone a GitHub repo to a temp dir, then install from there. Stdio is fully
/// captured for the same reason as `run_claude_install`.
fn install_from_github(repo: &str) -> Result<String> {
    let tmp = tempfile::tempdir().context("creating temp dir for github clone")?;
    let clone_url = if repo.starts_with("https://") || repo.starts_with("git@") {
        repo.to_string()
    } else {
        format!("https://github.com/{repo}.git")
    };

    let output = Command::new("git")
        .args(["clone", "--depth", "1", &clone_url])
        .arg(tmp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("cloning {clone_url}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed for {clone_url}: {stderr}");
    }

    run_claude_install(&tmp.path().to_string_lossy())
}
