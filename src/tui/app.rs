use std::collections::{BTreeMap, HashSet};

use ratatui::widgets::ListState;

use crate::core::clone::Category;
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
    CloneDialog,
    Help,
}

/// Whether the new-profile dialog creates an empty profile or clones from selected.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NewProfileMode {
    /// Clone categories from the selected profile.
    #[default]
    CloneFrom,
    /// Start with a blank skeleton (empty CLAUDE.md only).
    Empty,
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

// ── File tree for the detail pane ──────────────────────────────────────

/// A node in the file tree: either a directory (with children) or a leaf file.
#[derive(Debug, Clone)]
pub enum TreeNode {
    Dir {
        name: String,
        children: Vec<Self>,
        total_size: u64,
        file_count: usize,
    },
    File {
        name: String,
        size: u64,
    },
}

/// A flattened, visible row in the tree (after applying expand/collapse).
pub struct TreeRow {
    pub depth: usize,
    pub label: String,
    pub is_dir: bool,
    pub dir_path: Option<String>, // full dir path for expand/collapse key
    pub size_label: String,
}

/// Build a tree from a flat map of `relative_path -> size`.
pub fn build_file_tree(files: &BTreeMap<String, u64>) -> Vec<TreeNode> {
    // Intermediate: nested BTreeMap of path segments
    let mut root: BTreeMap<String, DirBuilder> = BTreeMap::new();

    for (path, &size) in files {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() == 1 {
            root.entry(parts[0].to_string())
                .or_insert_with(|| DirBuilder::Leaf(size));
        } else {
            insert_into(&mut root, &parts, size);
        }
    }

    flatten_builders(root)
}

enum DirBuilder {
    Leaf(u64),
    Dir(BTreeMap<String, Self>),
}

fn insert_into(map: &mut BTreeMap<String, DirBuilder>, parts: &[&str], size: u64) {
    let key = parts[0].to_string();
    if parts.len() == 1 {
        map.insert(key, DirBuilder::Leaf(size));
        return;
    }
    let entry = map
        .entry(key)
        .or_insert_with(|| DirBuilder::Dir(BTreeMap::new()));
    if let DirBuilder::Dir(children) = entry {
        insert_into(children, &parts[1..], size);
    }
}

fn flatten_builders(map: BTreeMap<String, DirBuilder>) -> Vec<TreeNode> {
    let mut nodes = Vec::new();
    for (name, builder) in map {
        match builder {
            DirBuilder::Leaf(size) => nodes.push(TreeNode::File { name, size }),
            DirBuilder::Dir(children) => {
                let child_nodes = flatten_builders(children);
                let total_size = sum_tree_size(&child_nodes);
                let file_count = count_tree_files(&child_nodes);
                nodes.push(TreeNode::Dir {
                    name,
                    children: child_nodes,
                    total_size,
                    file_count,
                });
            }
        }
    }
    // Sort: directories first, then files, both alphabetical
    nodes.sort_by(|a, b| {
        let a_is_dir = matches!(a, TreeNode::Dir { .. });
        let b_is_dir = matches!(b, TreeNode::Dir { .. });
        b_is_dir.cmp(&a_is_dir).then_with(|| {
            let a_name = match a {
                TreeNode::Dir { name, .. } | TreeNode::File { name, .. } => name,
            };
            let b_name = match b {
                TreeNode::Dir { name, .. } | TreeNode::File { name, .. } => name,
            };
            a_name.cmp(b_name)
        })
    });
    nodes
}

fn sum_tree_size(nodes: &[TreeNode]) -> u64 {
    nodes.iter().map(|n| match n {
        TreeNode::File { size, .. } => *size,
        TreeNode::Dir { total_size, .. } => *total_size,
    }).sum()
}

fn count_tree_files(nodes: &[TreeNode]) -> usize {
    nodes.iter().map(|n| match n {
        TreeNode::File { .. } => 1,
        TreeNode::Dir { file_count, .. } => *file_count,
    }).sum()
}

/// Flatten the tree into visible rows, respecting which directories are expanded.
pub fn visible_rows(nodes: &[TreeNode], expanded: &HashSet<String>, prefix: &str) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    let depth = if prefix.is_empty() { 0 } else { prefix.matches('/').count() + 1 };

    for node in nodes {
        match node {
            TreeNode::Dir { name, children, total_size, file_count } => {
                let dir_path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let is_expanded = expanded.contains(&dir_path);
                let arrow = if is_expanded { "▾" } else { "▸" };
                rows.push(TreeRow {
                    depth,
                    label: format!("{arrow} {name}/"),
                    is_dir: true,
                    dir_path: Some(dir_path.clone()),
                    size_label: format!("{} files, {}", file_count, fmt_size(*total_size)),
                });
                if is_expanded {
                    rows.extend(visible_rows(children, expanded, &dir_path));
                }
            }
            TreeNode::File { name, size } => {
                rows.push(TreeRow {
                    depth,
                    label: name.clone(),
                    is_dir: false,
                    dir_path: None,
                    size_label: fmt_size(*size),
                });
            }
        }
    }
    rows
}

#[allow(clippy::cast_precision_loss)]
fn fmt_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ── App state ──────────────────────────────────────────────────────────

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

    // Clone / new-profile dialog fields
    pub clone_name: String,
    pub clone_mode: NewProfileMode,
    pub clone_categories: Vec<(Category, bool)>,
    pub clone_fresh_md: bool,
    /// 0 = name, 1 = mode toggle, 2..=10 = category toggles, 11 = fresh CLAUDE.md
    pub clone_field_index: usize,

    // Detail pane tree state
    pub expanded_dirs: HashSet<String>,
    pub detail_cursor: usize,
    /// Cached tree for the currently selected profile.
    pub file_tree: Vec<TreeNode>,
    /// Cached visible rows after applying expand/collapse.
    pub tree_rows: Vec<TreeRow>,
    /// Which profile the cached tree belongs to.
    tree_profile: Option<String>,
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
            clone_name: String::new(),
            clone_mode: NewProfileMode::CloneFrom,
            clone_categories: Category::all().into_iter().map(|c| (c, true)).collect(),
            clone_fresh_md: false,
            clone_field_index: 0,
            expanded_dirs: HashSet::new(),
            detail_cursor: 0,
            file_tree: Vec::new(),
            tree_rows: Vec::new(),
            tree_profile: None,
        };
        app.refresh()?;
        if !app.profiles.is_empty() {
            app.list_state.select(Some(0));
        }
        app.rebuild_tree();
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

    /// Rebuild the file tree for the currently selected profile (if it changed).
    pub fn rebuild_tree(&mut self) {
        let current_name = self.selected_profile().map(|p| p.name.clone());
        if current_name == self.tree_profile {
            // Just refresh visible rows (expand/collapse may have changed).
            self.tree_rows = visible_rows(&self.file_tree, &self.expanded_dirs, "");
            return;
        }

        self.tree_profile = current_name;
        self.expanded_dirs.clear();
        self.detail_cursor = 0;

        let Some(profile) = self.selected_profile() else {
            self.file_tree.clear();
            self.tree_rows.clear();
            return;
        };

        let file_map: BTreeMap<String, u64> = profile
            .manifest
            .files
            .iter()
            .map(|(k, v)| (k.clone(), v.size))
            .collect();

        self.file_tree = build_file_tree(&file_map);
        self.tree_rows = visible_rows(&self.file_tree, &self.expanded_dirs, "");
    }

    /// Toggle expand/collapse of the directory at the current cursor.
    pub fn toggle_expand(&mut self) {
        let Some(row) = self.tree_rows.get(self.detail_cursor) else {
            return;
        };
        if let Some(ref dir_path) = row.dir_path {
            let path = dir_path.clone();
            if self.expanded_dirs.contains(&path) {
                self.expanded_dirs.remove(&path);
            } else {
                self.expanded_dirs.insert(path);
            }
            self.tree_rows = visible_rows(&self.file_tree, &self.expanded_dirs, "");
        }
    }

    /// Move the detail cursor within the visible tree rows.
    pub fn move_detail_cursor(&mut self, delta: isize) {
        if self.tree_rows.is_empty() {
            return;
        }
        let len = self.tree_rows.len();
        if delta < 0 {
            self.detail_cursor = self.detail_cursor.saturating_sub(delta.unsigned_abs());
        } else {
            self.detail_cursor = (self.detail_cursor + delta.unsigned_abs()).min(len - 1);
        }
    }
}
