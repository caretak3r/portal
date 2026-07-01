use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::Instant;

use walkdir::WalkDir;

use ratatui::widgets::ListState;

use crate::config::{self, Theme};
use crate::core::clone::Category;
use crate::core::profile::{PluginBlueprint, ProfileManifest, ProfileMeta};
use crate::core::progress::LoadEvent;
use crate::storage::{manifest, meta, paths::PortalPaths, plugins_manifest, state};

/// Which pane / overlay the TUI is currently showing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum View {
    #[default]
    Detail,
    Diff,
    /// Inline unified diff for a single file (entered from Diff view).
    ContentDiff,
    SaveDialog,
    LoadConfirm,
    /// Confirmation modal before deleting the selected profile. Deleting only
    /// removes the profile reference — the compressed backups are kept.
    DeleteConfirm,
    CloneDialog,
    /// Theme picker overlay (`T`).
    ThemePicker,
    Help,
    /// Modal shown while a load is running on a worker thread. The
    /// associated state lives in `App.load_in_flight`.
    LoadInProgress,
    /// Fuzzy-search overlay for fast profile selection (`/` from Detail).
    QuickSwitch,
    /// Per-file picker for a specific clone category (Skills/Rules/Commands/Agents).
    /// Entered from `CloneDialog` via the Right arrow on a pickable category row.
    FilePicker,
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
        /// True when every file under this dir is runtime-only (not in the profile manifest).
        runtime: bool,
    },
    File {
        name: String,
        size: u64,
        /// True when this file is present on disk but not tracked in the profile manifest.
        runtime: bool,
    },
}

/// A flattened, visible row in the tree (after applying expand/collapse).
pub struct TreeRow {
    pub depth: usize,
    pub label: String,
    pub is_dir: bool,
    pub dir_path: Option<String>, // full dir path for expand/collapse key
    pub size_label: String,
    /// True when the row represents a runtime-only item (not in the profile manifest).
    pub runtime: bool,
}

/// Build a tree from a flat map of `relative_path -> (size, is_runtime)`.
///
/// `is_runtime` marks files that are present on disk but not tracked in the profile manifest.
pub fn build_file_tree(files: &BTreeMap<String, (u64, bool)>) -> Vec<TreeNode> {
    let mut root: BTreeMap<String, DirBuilder> = BTreeMap::new();

    for (path, &(size, runtime)) in files {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() == 1 {
            root.entry(parts[0].to_string())
                .or_insert_with(|| DirBuilder::Leaf(size, runtime));
        } else {
            insert_into(&mut root, &parts, size, runtime);
        }
    }

    flatten_builders(root)
}

enum DirBuilder {
    Leaf(u64, bool), // (size, runtime)
    Dir(BTreeMap<String, Self>),
}

fn insert_into(map: &mut BTreeMap<String, DirBuilder>, parts: &[&str], size: u64, runtime: bool) {
    let key = parts[0].to_string();
    if parts.len() == 1 {
        map.insert(key, DirBuilder::Leaf(size, runtime));
        return;
    }
    let entry = map
        .entry(key)
        .or_insert_with(|| DirBuilder::Dir(BTreeMap::new()));
    if let DirBuilder::Dir(children) = entry {
        insert_into(children, &parts[1..], size, runtime);
    }
}

fn flatten_builders(map: BTreeMap<String, DirBuilder>) -> Vec<TreeNode> {
    let mut nodes = Vec::new();
    for (name, builder) in map {
        match builder {
            DirBuilder::Leaf(size, runtime) => nodes.push(TreeNode::File {
                name,
                size,
                runtime,
            }),
            DirBuilder::Dir(children) => {
                let child_nodes = flatten_builders(children);
                let total_size = sum_tree_size(&child_nodes);
                let file_count = count_tree_files(&child_nodes);
                // A dir is runtime-only when every file under it is runtime.
                let runtime = child_nodes.iter().all(|n| match n {
                    TreeNode::File { runtime, .. } | TreeNode::Dir { runtime, .. } => *runtime,
                });
                nodes.push(TreeNode::Dir {
                    name,
                    children: child_nodes,
                    total_size,
                    file_count,
                    runtime,
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
    nodes
        .iter()
        .map(|n| match n {
            TreeNode::File { size, .. } => *size,
            TreeNode::Dir { total_size, .. } => *total_size,
        })
        .sum()
}

fn count_tree_files(nodes: &[TreeNode]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            TreeNode::File { .. } => 1,
            TreeNode::Dir { file_count, .. } => *file_count,
        })
        .sum()
}

/// Flatten the tree into visible rows, respecting which directories are expanded.
pub fn visible_rows(nodes: &[TreeNode], expanded: &HashSet<String>, prefix: &str) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    let depth = if prefix.is_empty() {
        0
    } else {
        prefix.matches('/').count() + 1
    };

    for node in nodes {
        match node {
            TreeNode::Dir {
                name,
                children,
                total_size,
                file_count,
                runtime,
            } => {
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
                    runtime: *runtime,
                });
                if is_expanded {
                    rows.extend(visible_rows(children, expanded, &dir_path));
                }
            }
            TreeNode::File {
                name,
                size,
                runtime,
            } => {
                rows.push(TreeRow {
                    depth,
                    label: name.clone(),
                    is_dir: false,
                    dir_path: None,
                    size_label: fmt_size(*size),
                    runtime: *runtime,
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

/// Walk a live directory into a `path -> (size, runtime)` map for the file tree.
///
/// The map reflects what is *on disk* — files deleted since the last save are
/// absent, freshly added files appear. `runtime` marks rows rendered dimmed:
/// files present on disk but not tracked by the manifest (newly added, or
/// excluded runtime infrastructure). Excluded paths (`.git`, `plugins/cache`,
/// `projects`, …) are omitted unless `show_all` is set, mirroring the
/// exclusion rules the snapshot itself uses so the default view stays clean.
fn scan_live_map(
    dir: &std::path::Path,
    manifest: &ProfileManifest,
    show_all: bool,
) -> BTreeMap<String, (u64, bool)> {
    let mut map = BTreeMap::new();
    for entry in WalkDir::new(dir).min_depth(1) {
        let Ok(e) = entry else { continue };
        if !e.file_type().is_file() {
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(dir) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().to_string();
        let excluded = crate::core::snapshot::is_excluded(&rel_str);
        if excluded && !show_all {
            continue;
        }
        let size = e.metadata().map_or(0, |m| m.len());
        // Dim anything the manifest doesn't track: untracked-new or excluded infra.
        let runtime = excluded || !manifest.files.contains_key(&rel_str);
        map.insert(rel_str, (size, runtime));
    }
    map
}

// ── App state ──────────────────────────────────────────────────────────

/// Root application state for the TUI.
pub struct App {
    pub paths: PortalPaths,
    pub profiles: Vec<ProfileInfo>,
    pub active_profile: Option<String>,
    /// Last profile that was active before the current one — populated from
    /// `portal.state.json` on every refresh. Drives the `Backspace` instant
    /// toggle and the hint shown in the status bar.
    pub previous_profile: Option<String>,
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

    // Diff view state
    /// Navigable file list in diff view (modified files that can be drilled into).
    pub diff_files: Vec<String>,
    /// Cursor position in the diff file list.
    pub diff_cursor: usize,
    /// Cached content diff lines for the currently viewed file.
    pub content_diff_text: String,
    /// Scroll position in content diff view.
    pub content_diff_scroll: u16,

    // Detail pane tree state
    pub expanded_dirs: HashSet<String>,
    pub detail_cursor: usize,
    /// Cached tree for the currently selected profile.
    pub file_tree: Vec<TreeNode>,
    /// Cached visible rows after applying expand/collapse.
    pub tree_rows: Vec<TreeRow>,
    /// Which profile the cached tree belongs to.
    tree_profile: Option<String>,
    /// When true, the file tree includes runtime-only items from `~/.claude`
    /// (excluded from snapshots: .git, plugins/cache, etc.) shown dimmed.
    pub tree_show_all: bool,

    /// Active TUI color theme.
    pub theme: Theme,
    /// Cursor position inside the theme picker overlay.
    pub theme_cursor: usize,

    /// State for an in-flight async load. `None` means no load is running.
    pub load_in_flight: Option<LoadInFlight>,

    /// Toggleable per-load options surfaced in the `LoadConfirm` modal.
    /// Reset to `LoadOptions::default()` every time the modal opens.
    pub load_options: LoadOptions,

    // ── QuickSwitch overlay (`/`) ──
    /// Live fuzzy-search query.
    pub quick_query: String,
    /// Profile indices into `App.profiles`, ranked by score (or recency
    /// when the query is empty). Recomputed on every keystroke.
    pub quick_matches: Vec<usize>,
    /// Cursor position within `quick_matches` — drives the highlighted row
    /// and the target of an `Enter` keypress.
    pub quick_cursor: usize,

    // ── FilePicker overlay (→ from CloneDialog category rows) ──
    /// Which category is being picked in the `FilePicker` view.
    pub file_picker_category: Category,
    /// Items in the file picker as `(label, checked)`. For directory-based
    /// categories the label is a collapsed unit (e.g. `skills/<name>`) rather
    /// than an individual file — see [`file_picker_members`](Self::file_picker_members).
    pub file_picker_items: Vec<(String, bool)>,
    /// Maps each picker row label to the concrete relative paths it covers, so
    /// toggling one `skills/<name>` row selects every file under that skill.
    pub file_picker_members: HashMap<String, Vec<String>>,
    /// Cursor row in the file picker list.
    pub file_picker_cursor: usize,
    /// Accumulated per-category file selections for the pending clone.
    /// Cleared when the `CloneDialog` is first opened.
    pub clone_file_picks: HashMap<Category, HashSet<String>>,
}

/// Per-load options toggled inside the `LoadConfirm` dialog. Mirrors the CLI
/// flags (`--no-backup`, `--no-plugins`, `--dry-run`) so power users get the
/// same controls without having to drop to a terminal.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    pub backup: bool,
    pub plugins: bool,
    pub dry_run: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            backup: true,
            plugins: true,
            dry_run: false,
        }
    }
}

/// State carried by a running async load. Owned by the main thread; the
/// worker thread holds the matching `Sender` end of `rx` and posts events
/// as the loader makes progress, then a final `LoadEvent::Done`.
pub struct LoadInFlight {
    /// Profile name being loaded — shown in the modal title.
    pub target: String,
    /// When the load started — used to drive the spinner animation frame.
    pub started_at: Instant,
    /// Most recent phase label. "" until the worker emits the first phase.
    pub phase: String,
    /// File-level progress within the current phase; both 0 between phases.
    pub current: u64,
    pub total: u64,
    /// Last per-file label ticked. "" until a tick arrives.
    pub item: String,
    /// Receiver end of the progress channel. Drained each event-loop tick.
    pub rx: Receiver<LoadEvent>,
    /// Worker join handle. Kept alive so the thread isn't detached; we
    /// don't actively `join()` because the `Done` event already carries
    /// the result.
    pub _handle: JoinHandle<()>,
}

impl App {
    /// Build initial app state by scanning profiles on disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the state file or profile directories
    /// cannot be read.
    pub fn new(paths: PortalPaths) -> anyhow::Result<Self> {
        let theme = load_theme_from_config(&paths);
        let theme_cursor = Theme::all().iter().position(|t| *t == theme).unwrap_or(0);
        let mut app = Self {
            paths,
            profiles: Vec::new(),
            active_profile: None,
            previous_profile: None,
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
            diff_files: Vec::new(),
            diff_cursor: 0,
            content_diff_text: String::new(),
            content_diff_scroll: 0,
            expanded_dirs: HashSet::new(),
            detail_cursor: 0,
            file_tree: Vec::new(),
            tree_rows: Vec::new(),
            tree_profile: None,
            tree_show_all: false,
            theme,
            theme_cursor,
            load_in_flight: None,
            load_options: LoadOptions::default(),
            quick_query: String::new(),
            quick_matches: Vec::new(),
            quick_cursor: 0,
            file_picker_category: Category::Skills,
            file_picker_items: Vec::new(),
            file_picker_members: HashMap::new(),
            file_picker_cursor: 0,
            clone_file_picks: HashMap::new(),
        };
        app.refresh()?;
        if !app.profiles.is_empty() {
            app.list_state.select(Some(0));
        }
        app.rebuild_tree();
        Ok(app)
    }

    /// Persist the current theme back to `portal.config.toml`. Idempotent — if
    /// the file already records this theme, the write is a no-op.
    pub fn save_theme(&self) -> anyhow::Result<()> {
        save_theme_to_config(&self.paths, self.theme)
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
        self.previous_profile = portal_state.previous_profile;

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

    /// Non-blockingly drain pending events from an in-flight async load.
    ///
    /// Returns `true` if the load just finished (i.e. a `Done` event was
    /// observed); the caller is then responsible for the post-load UI
    /// transition (refresh, status message, view back to Detail). When no
    /// load is in flight, this is a no-op.
    pub fn drain_load_events(&mut self) -> bool {
        let Some(flight) = self.load_in_flight.as_mut() else {
            return false;
        };

        let mut finished = false;
        let mut summary: Option<Result<crate::core::loader::LoadResult, String>> = None;

        loop {
            match flight.rx.try_recv() {
                Ok(LoadEvent::Phase(label)) => {
                    flight.phase = label;
                    // Reset the per-phase counters so a phase that doesn't
                    // emit ticks (backup, swap) doesn't keep stale numbers
                    // on screen from the previous phase.
                    flight.current = 0;
                    flight.total = 0;
                    flight.item.clear();
                }
                Ok(LoadEvent::Progress {
                    current,
                    total,
                    item,
                }) => {
                    flight.current = current;
                    flight.total = total;
                    flight.item = item;
                }
                Ok(LoadEvent::Done(result)) => {
                    summary = Some(result);
                    finished = true;
                    // Keep draining: there may be late progress events
                    // queued before the worker thread sent Done.
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Worker dropped its sender without a Done event — treat
                    // as a silent failure rather than spinning forever.
                    if !finished {
                        summary = Some(Err("loader thread terminated unexpectedly".into()));
                        finished = true;
                    }
                    break;
                }
            }
        }

        if finished {
            // Drop the in-flight handle and apply the result to status.
            self.load_in_flight = None;
            self.view = View::Detail;
            match summary {
                Some(Ok(r)) => {
                    self.status_message = Some(format!(
                        "Loaded \"{}\" ({} files).",
                        r.profile, r.files_loaded
                    ));
                }
                Some(Err(e)) => {
                    self.status_message = Some(format!("Load failed: {e}"));
                }
                None => {}
            }
            let _ = self.refresh();
            self.rebuild_tree();
        }

        finished
    }

    /// Open the quick-switch overlay with an empty query (recency-ordered list).
    pub fn quick_switch_open(&mut self) {
        self.quick_query.clear();
        self.quick_cursor = 0;
        self.recompute_quick_matches();
        self.view = View::QuickSwitch;
    }

    /// Re-rank `quick_matches` against the current `quick_query`. Cheap enough
    /// to call on every keystroke — fuzzy-matcher is microseconds per profile.
    pub fn recompute_quick_matches(&mut self) {
        let inputs: Vec<super::quick_switch::RankInput<'_>> = self
            .profiles
            .iter()
            .map(|p| super::quick_switch::RankInput {
                name: p.name.as_str(),
                last_loaded: p.manifest.last_loaded,
            })
            .collect();
        self.quick_matches = super::quick_switch::rank_profiles(&self.quick_query, &inputs);
        if self.quick_cursor >= self.quick_matches.len() {
            self.quick_cursor = self.quick_matches.len().saturating_sub(1);
        }
    }

    /// Profile currently highlighted in the quick switcher, if any.
    pub fn quick_switch_selected(&self) -> Option<&ProfileInfo> {
        self.quick_matches
            .get(self.quick_cursor)
            .and_then(|&i| self.profiles.get(i))
    }

    /// Move the quick-switch cursor with wrap-around.
    pub fn quick_switch_move(&mut self, delta: isize) {
        let len = self.quick_matches.len();
        if len == 0 {
            return;
        }
        let cur = isize::try_from(self.quick_cursor).unwrap_or(0);
        let next = ((cur + delta).rem_euclid(isize::try_from(len).unwrap_or(1))).max(0);
        self.quick_cursor = usize::try_from(next).unwrap_or(0);
    }

    /// Currently selected profile, if any.
    pub fn selected_profile(&self) -> Option<&ProfileInfo> {
        self.list_state
            .selected()
            .and_then(|i| self.profiles.get(i))
    }

    /// Move the list cursor to the profile with the given name, if present.
    ///
    /// `refresh()` rebuilds `profiles` in alphabetical order, so a `list_state`
    /// index captured before the refresh no longer points at the same profile.
    /// Call this after create/clone so the freshly made profile — not whatever
    /// happens to land at the old index — becomes the cursor (and thus the
    /// load target).
    pub fn select_by_name(&mut self, name: &str) {
        if let Some(idx) = self.profiles.iter().position(|p| p.name == name) {
            self.list_state.select(Some(idx));
        }
    }

    /// Whether the named profile is the currently active one.
    pub fn is_active(&self, name: &str) -> bool {
        self.active_profile.as_deref().is_some_and(|a| a == name)
    }

    /// Delete the currently selected profile's reference. Backups are kept.
    ///
    /// Refreshes the profile list, clamps the selection, and rebuilds the
    /// file tree. Sets a status message describing the outcome.
    pub fn delete_selected_profile(&mut self) {
        let Some(name) = self.selected_profile().map(|p| p.name.clone()) else {
            self.view = View::Detail;
            return;
        };

        match crate::core::remove::delete_profile(&self.paths, &name) {
            Ok(_) => {
                self.status_message = Some(format!("Deleted \"{name}\" (backups kept)."));
                let _ = self.refresh();
                // The list shrank — keep the cursor on a valid row.
                if self.profiles.is_empty() {
                    self.list_state.select(None);
                } else {
                    let idx = self
                        .list_state
                        .selected()
                        .unwrap_or(0)
                        .min(self.profiles.len() - 1);
                    self.list_state.select(Some(idx));
                }
                // Force a rebuild even if the new selection happens to share a
                // name slot — the deleted profile must not linger in the tree.
                self.tree_profile = None;
                self.rebuild_tree();
            }
            Err(e) => {
                self.status_message = Some(format!("Delete failed: {e}"));
            }
        }
        self.view = View::Detail;
    }

    /// Rebuild the file tree for the currently selected profile.
    ///
    /// This is a **live** listing, not the saved manifest snapshot. When the
    /// selected profile is the active one, we walk `~/.claude` on disk every
    /// call, so any drift since the last save (added, deleted, or resized
    /// files) is reflected immediately. Non-active profiles have no live
    /// backing directory — their content is content-addressed in the CAS pool
    /// — so they fall back to the immutable manifest.
    pub fn rebuild_tree(&mut self) {
        let current_name = self.selected_profile().map(|p| p.name.clone());
        let profile_changed = current_name != self.tree_profile;

        // Switching profiles resets navigation. A live re-scan of the *same*
        // profile must preserve the user's expand/collapse state and cursor.
        if profile_changed {
            self.tree_profile = current_name;
            self.expanded_dirs.clear();
            self.detail_cursor = 0;
        }

        let Some(profile) = self.selected_profile() else {
            self.file_tree.clear();
            self.tree_rows.clear();
            return;
        };

        let is_active_sel = self
            .active_profile
            .as_deref()
            .is_some_and(|a| a == profile.name);
        let claude_dir = self.paths.claude_root();

        let file_map = if is_active_sel && claude_dir.is_dir() {
            // LIVE: list what's actually in ~/.claude right now.
            scan_live_map(&claude_dir, &profile.manifest, self.tree_show_all)
        } else {
            // Immutable CAS snapshot — the manifest is the source of truth.
            profile
                .manifest
                .files
                .iter()
                .map(|(k, v)| (k.clone(), (v.size, false)))
                .collect()
        };

        self.file_tree = build_file_tree(&file_map);
        self.tree_rows = visible_rows(&self.file_tree, &self.expanded_dirs, "");
        // A live directory can shrink between scans — keep the cursor in range.
        if self.detail_cursor >= self.tree_rows.len() {
            self.detail_cursor = self.tree_rows.len().saturating_sub(1);
        }
    }

    /// Toggle the "show all files" mode and force a full tree rebuild.
    pub fn toggle_tree_all(&mut self) {
        self.tree_show_all = !self.tree_show_all;
        self.tree_profile = None; // force rebuild even if profile hasn't changed
        self.rebuild_tree();
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

fn load_theme_from_config(paths: &PortalPaths) -> Theme {
    config::load(&paths.config_file())
        .map(|c| c.ui.theme)
        .unwrap_or_default()
}

fn save_theme_to_config(paths: &PortalPaths, theme: Theme) -> anyhow::Result<()> {
    let path = paths.config_file();
    let mut cfg = config::load(&path).unwrap_or_default();
    if cfg.ui.theme == theme {
        return Ok(());
    }
    cfg.ui.theme = theme;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = toml::to_string_pretty(&cfg)?;
    std::fs::write(&path, serialized)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::core::profile::{FileEntry, FileSource};

    fn manifest_with(tracked: &[&str]) -> ProfileManifest {
        let files = tracked
            .iter()
            .map(|p| {
                (
                    (*p).to_string(),
                    FileEntry {
                        checksum: "sha256:x".to_string(),
                        size: 1,
                        source: FileSource::User,
                        mode: None,
                    },
                )
            })
            .collect();
        ProfileManifest {
            version: 1,
            name: "t".to_string(),
            created_at: chrono::Utc::now(),
            last_loaded: None,
            load_count: 0,
            description: String::new(),
            tags: Vec::new(),
            files,
            excluded_patterns: Vec::new(),
        }
    }

    #[test]
    fn live_map_marks_untracked_and_drops_deleted() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("CLAUDE.md"), "x").unwrap(); // tracked
        std::fs::write(dir.join("new.md"), "y").unwrap(); // on disk, untracked
        // "gone.md" is tracked in the manifest but absent on disk.
        let mf = manifest_with(&["CLAUDE.md", "gone.md"]);

        let map = scan_live_map(dir, &mf, false);

        assert_eq!(map.get("CLAUDE.md").map(|v| v.1), Some(false));
        assert_eq!(map.get("new.md").map(|v| v.1), Some(true));
        assert!(
            !map.contains_key("gone.md"),
            "a file deleted on disk must not appear — this is live, not a snapshot"
        );
    }

    #[test]
    fn live_map_hides_excluded_unless_show_all() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("projects")).unwrap();
        std::fs::write(dir.join("projects/log.json"), "{}").unwrap(); // excluded infra
        std::fs::write(dir.join("CLAUDE.md"), "x").unwrap();
        let mf = manifest_with(&["CLAUDE.md"]);

        let hidden = scan_live_map(dir, &mf, false);
        assert!(!hidden.contains_key("projects/log.json"));
        assert!(hidden.contains_key("CLAUDE.md"));

        let shown = scan_live_map(dir, &mf, true);
        assert_eq!(shown.get("projects/log.json").map(|v| v.1), Some(true));
    }
}
