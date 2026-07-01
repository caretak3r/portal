use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::app::{App, LoadInFlight, LoadOptions, NewProfileMode, View};
use crate::core::clone::{Category, categorize_file};
use crate::core::progress::{ChannelProgress, LoadEvent};

/// Rows moved per `PageUp` / `PageDown` across all navigable lists.
const PAGE_JUMP: usize = 10;
/// Same jump as a signed delta for cursor-move helpers that take `isize`.
const PAGE_JUMP_I: isize = 10;

/// Poll for input and dispatch to the appropriate handler.
///
/// Returns `Ok(true)` when the application should exit.
///
/// # Errors
///
/// Returns an error on terminal I/O failure.
pub fn handle(app: &mut App) -> io::Result<bool> {
    // Drain any queued progress events from an in-flight load before we
    // block on input. Keeps the spinner / phase indicator current and
    // performs the post-load transition the moment Done arrives.
    app.drain_load_events();

    if !event::poll(Duration::from_millis(100))? {
        // Drain again after the poll: the loader thread may have made
        // progress while we slept on input. Cheap when nothing's queued.
        app.drain_load_events();
        return Ok(app.should_quit);
    }

    let Event::Key(key) = event::read()? else {
        return Ok(false);
    };

    // Only handle key press events (ignore release/repeat on supported terminals).
    if key.kind != KeyEventKind::Press {
        return Ok(false);
    }

    // Ctrl+C always quits.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    match app.view {
        View::Detail | View::Diff => handle_main(app, key.code),
        View::ContentDiff => handle_content_diff(app, key.code),
        View::SaveDialog => handle_save_dialog(app, key.code),
        View::LoadConfirm => handle_load_confirm(app, key.code),
        View::DeleteConfirm => handle_delete_confirm(app, key.code),
        View::CloneDialog => handle_clone_dialog(app, key.code),
        View::ThemePicker => handle_theme_picker(app, key.code),
        View::Help => handle_help(app, key.code),
        View::QuickSwitch => handle_quick_switch(app, key.code),
        // While a load runs we ignore all keys except the Ctrl+C above —
        // there's no in-flight cancellation in the loader, and accidental
        // input could swap views and lose the progress modal.
        View::LoadInProgress => {}
        View::FilePicker => handle_file_picker(app, key.code),
    }

    Ok(app.should_quit)
}

fn handle_theme_picker(app: &mut App, code: KeyCode) {
    use crate::config::Theme;
    let len = Theme::all().len();
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Reset cursor to whatever is currently active.
            app.theme_cursor = Theme::all()
                .iter()
                .position(|t| *t == app.theme)
                .unwrap_or(0);
            app.view = View::Detail;
        }
        KeyCode::Down | KeyCode::Char('j') if len > 0 => {
            app.theme_cursor = (app.theme_cursor + 1) % len;
        }
        KeyCode::Up | KeyCode::Char('k') if len > 0 => {
            app.theme_cursor = if app.theme_cursor == 0 {
                len - 1
            } else {
                app.theme_cursor - 1
            };
        }
        KeyCode::Enter => {
            if let Some(theme) = Theme::all().get(app.theme_cursor).copied() {
                app.theme = theme;
                match app.save_theme() {
                    Ok(()) => {
                        app.status_message = Some(format!("Theme: {}", theme.label()));
                    }
                    Err(e) => {
                        app.status_message = Some(format!("Theme set, save failed: {e}"));
                    }
                }
            }
            app.view = View::Detail;
        }
        _ => {}
    }
}

#[allow(clippy::too_many_lines)] // dispatch table for the main view's keybinds
fn handle_main(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.view = View::Help,
        KeyCode::Char('T') => app.view = View::ThemePicker,

        // Diff view: j/k navigates the diff file list
        KeyCode::Down | KeyCode::Char('j')
            if app.view == View::Diff && !app.diff_files.is_empty() =>
        {
            app.diff_cursor = (app.diff_cursor + 1).min(app.diff_files.len() - 1);
        }
        KeyCode::Up | KeyCode::Char('k') if app.view == View::Diff => {
            app.diff_cursor = app.diff_cursor.saturating_sub(1);
        }

        // Detail view: j/k moves tree cursor, Tab switches to profile nav
        KeyCode::Down | KeyCode::Char('j') if app.view == View::Detail => {
            app.move_detail_cursor(1);
        }
        KeyCode::Up | KeyCode::Char('k') if app.view == View::Detail => {
            app.move_detail_cursor(-1);
        }

        // Tab cycles between profile list nav and detail tree nav
        KeyCode::Tab => {
            // Move to next profile (like old j/k behavior)
            move_selection(app, 1);
        }
        KeyCode::BackTab => {
            move_selection(app, -1);
        }

        // Enter in detail view: toggle folder expand/collapse
        // Enter in other views: load confirmation
        KeyCode::Enter if app.view == View::Detail => {
            // If cursor is on a directory row, toggle it.
            // If cursor is on a file or above the tree, open load confirm.
            let on_dir = app
                .tree_rows
                .get(app.detail_cursor)
                .is_some_and(|r| r.is_dir);
            if on_dir {
                app.toggle_expand();
            } else if app.selected_profile().is_some() {
                // Enter is a real load, identical to `l`. Reset options so a
                // stale dry-run flag from a prior Shift+L preview can't make
                // the load silently no-op (no swap, no backup).
                app.load_options = LoadOptions::default();
                app.view = View::LoadConfirm;
            }
        }
        KeyCode::Enter if app.view == View::Diff && !app.diff_files.is_empty() => {
            open_content_diff(app);
        }
        KeyCode::Enter if app.selected_profile().is_some() => {
            app.load_options = LoadOptions::default();
            app.view = View::LoadConfirm;
        }

        KeyCode::Char('*') if app.view == View::Detail => {
            app.toggle_tree_all();
        }
        // `x` deletes the selected profile after a confirmation modal. The
        // delete only drops the profile reference; backups are preserved.
        KeyCode::Char('x') if app.view == View::Detail && app.selected_profile().is_some() => {
            app.view = View::DeleteConfirm;
        }
        // `r` forces a live re-scan of the directory (no-op for non-active
        // profiles, which are immutable snapshots).
        KeyCode::Char('r') if app.view == View::Detail => {
            app.rebuild_tree();
            app.status_message = Some("Re-scanned directory.".to_string());
        }
        KeyCode::Char('l') if app.view == View::Detail && app.selected_profile().is_some() => {
            app.load_options = LoadOptions::default();
            app.view = View::LoadConfirm;
        }
        // Shift+L = "load with dry-run" preset. Same modal, but the
        // dry-run flag is on by default — handy for previewing the
        // file/plugin diff before you actually swap.
        KeyCode::Char('L') if app.view == View::Detail && app.selected_profile().is_some() => {
            app.load_options = LoadOptions {
                dry_run: true,
                ..LoadOptions::default()
            };
            app.view = View::LoadConfirm;
        }
        // Backspace = instant toggle to the previously active profile. Only
        // active in Detail view (other views use Backspace for text editing).
        KeyCode::Backspace if app.view == View::Detail && app.previous_profile.is_some() => {
            execute_toggle(app);
        }
        // `/` opens the fuzzy quick-switch overlay. Type-to-find beats
        // Tab-cycling once the profile list grows past ~10 entries.
        KeyCode::Char('/') if app.view == View::Detail && !app.profiles.is_empty() => {
            app.quick_switch_open();
        }
        KeyCode::Char('d') => {
            app.file_scroll = 0;
            app.view = if app.view == View::Diff {
                View::Detail
            } else {
                View::Diff
            };
        }
        KeyCode::Char('s') => {
            // Pre-fill name with the active profile so pressing `s` and Enter
            // saves over the loaded profile (game-save semantics). User can
            // backspace and type a different name to fork instead.
            app.save_name = app.active_profile.clone().unwrap_or_default();
            app.save_description.clear();
            app.save_tags.clear();
            app.save_field_index = 0;
            app.view = View::SaveDialog;
        }
        KeyCode::Char('c') if app.selected_profile().is_some() => {
            app.clone_name.clear();
            app.clone_mode = NewProfileMode::CloneFrom;
            app.clone_categories = crate::core::clone::Category::all()
                .into_iter()
                .map(|c| (c, true))
                .collect();
            app.clone_fresh_md = false;
            app.clone_field_index = 0;
            app.clone_file_picks.clear();
            app.view = View::CloneDialog;
        }
        KeyCode::Char('n') => {
            let mode = if app.selected_profile().is_some() {
                NewProfileMode::CloneFrom
            } else {
                NewProfileMode::Empty
            };
            app.clone_name.clear();
            app.clone_mode = mode;
            app.clone_categories = crate::core::clone::Category::all()
                .into_iter()
                .map(|c| (c, true))
                .collect();
            app.clone_fresh_md = false;
            app.clone_field_index = 0;
            app.clone_file_picks.clear();
            app.view = View::CloneDialog;
        }
        KeyCode::Esc => {
            app.file_scroll = 0;
            app.view = View::Detail;
            app.status_message = None;
        }
        // Page jumps for the detail tree and the diff file list.
        KeyCode::PageDown if app.view == View::Detail => {
            app.move_detail_cursor(PAGE_JUMP_I);
        }
        KeyCode::PageUp if app.view == View::Detail => {
            app.move_detail_cursor(-PAGE_JUMP_I);
        }
        KeyCode::PageDown if app.view == View::Diff && !app.diff_files.is_empty() => {
            app.diff_cursor = (app.diff_cursor + PAGE_JUMP).min(app.diff_files.len() - 1);
        }
        KeyCode::PageUp if app.view == View::Diff => {
            app.diff_cursor = app.diff_cursor.saturating_sub(PAGE_JUMP);
        }
        _ => {}
    }
}

fn handle_save_dialog(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.view = View::Detail,
        KeyCode::Tab => app.save_field_index = (app.save_field_index + 1) % 3,
        KeyCode::BackTab => {
            app.save_field_index = if app.save_field_index == 0 {
                2
            } else {
                app.save_field_index - 1
            };
        }
        KeyCode::Backspace => {
            active_field_mut(app).pop();
        }
        KeyCode::Enter => execute_save(app),
        KeyCode::Char(c) => {
            active_field_mut(app).push(c);
        }
        _ => {}
    }
}

fn handle_quick_switch(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.view = View::Detail;
            app.quick_query.clear();
            app.quick_cursor = 0;
        }
        // Up/Down arrows move the highlight; vim-style j/k would conflict
        // with literal letters in the fuzzy query so they're intentionally
        // not bound here.
        KeyCode::Down => app.quick_switch_move(1),
        KeyCode::Up => app.quick_switch_move(-1),
        KeyCode::PageDown => app.quick_switch_move(PAGE_JUMP_I),
        KeyCode::PageUp => app.quick_switch_move(-PAGE_JUMP_I),
        KeyCode::Backspace => {
            app.quick_query.pop();
            app.recompute_quick_matches();
        }
        KeyCode::Enter => {
            if let Some(name) = app.quick_switch_selected().map(|p| p.name.clone()) {
                // Sync the underlying list cursor too so the Detail pane
                // shows the right profile when the load modal dismisses.
                if let Some(idx) = app.quick_matches.get(app.quick_cursor).copied() {
                    app.list_state.select(Some(idx));
                    app.rebuild_tree();
                }
                // Quick-switch loads use safe defaults — there's no
                // LoadConfirm modal where the user could toggle flags.
                spawn_load(app, name, LoadOptions::default());
            } else {
                app.status_message = Some("No profile matches that query.".to_string());
                app.view = View::Detail;
            }
            app.quick_query.clear();
            app.quick_cursor = 0;
        }
        KeyCode::Char(c) => {
            app.quick_query.push(c);
            app.recompute_quick_matches();
        }
        _ => {}
    }
}

fn handle_load_confirm(app: &mut App, code: KeyCode) {
    match code {
        // 'y' is reserved for the legacy "yes" confirmation; 'n' was the
        // legacy "no". Both still work. The flag toggles use the same
        // letters as the CLI long-flags (b/p/d) so the muscle memory
        // transfers cleanly.
        KeyCode::Char('b') => {
            app.load_options.backup = !app.load_options.backup;
        }
        KeyCode::Char('p') => {
            app.load_options.plugins = !app.load_options.plugins;
        }
        KeyCode::Char('d') => {
            app.load_options.dry_run = !app.load_options.dry_run;
        }
        KeyCode::Char('y') | KeyCode::Enter => execute_load(app),
        KeyCode::Esc | KeyCode::Char('n') => app.view = View::Detail,
        _ => {}
    }
}

fn handle_delete_confirm(app: &mut App, code: KeyCode) {
    match code {
        // Default is "no" — deletion requires an explicit y / Enter.
        KeyCode::Char('y') | KeyCode::Enter => app.delete_selected_profile(),
        KeyCode::Esc | KeyCode::Char('n' | 'q') => app.view = View::Detail,
        _ => {}
    }
}

#[allow(clippy::missing_const_for_fn)] // KeyCode match is not const-compatible
fn handle_help(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('?' | 'q') => app.view = View::Detail,
        _ => {}
    }
}

fn handle_file_picker(app: &mut App, code: KeyCode) {
    let len = app.file_picker_items.len();
    match code {
        // Confirm and persist picks back to the clone dialog. Each checked row
        // expands to the concrete files it covers so the clone (which matches
        // exact paths) includes every file under a selected skill.
        KeyCode::Esc | KeyCode::Enter => {
            let mut selected: HashSet<String> = HashSet::new();
            for (group, on) in &app.file_picker_items {
                if *on {
                    match app.file_picker_members.get(group) {
                        Some(files) => selected.extend(files.iter().cloned()),
                        None => {
                            selected.insert(group.clone());
                        }
                    }
                }
            }
            app.clone_file_picks
                .insert(app.file_picker_category, selected);
            app.view = View::CloneDialog;
        }
        KeyCode::Down | KeyCode::Char('j') if len > 0 => {
            app.file_picker_cursor = (app.file_picker_cursor + 1) % len;
        }
        KeyCode::Up | KeyCode::Char('k') if len > 0 => {
            app.file_picker_cursor = if app.file_picker_cursor == 0 {
                len - 1
            } else {
                app.file_picker_cursor - 1
            };
        }
        // Page jumps clamp at the ends (no wrap) for predictable fast scroll.
        KeyCode::PageDown if len > 0 => {
            app.file_picker_cursor = (app.file_picker_cursor + PAGE_JUMP).min(len - 1);
        }
        KeyCode::PageUp => {
            app.file_picker_cursor = app.file_picker_cursor.saturating_sub(PAGE_JUMP);
        }
        KeyCode::Home if len > 0 => app.file_picker_cursor = 0,
        KeyCode::End if len > 0 => app.file_picker_cursor = len - 1,
        // Space toggles the highlighted file.
        KeyCode::Char(' ') => {
            if let Some(item) = app.file_picker_items.get_mut(app.file_picker_cursor) {
                item.1 = !item.1;
            }
        }
        // 'a' toggles all files at once.
        KeyCode::Char('a') => {
            let all_on = app.file_picker_items.iter().all(|(_, s)| *s);
            for item in &mut app.file_picker_items {
                item.1 = !all_on;
            }
        }
        _ => {}
    }
}

/// Returns true for categories that hold many individual files and therefore
/// benefit from a per-file picker (directory-based categories).
fn is_pickable(cat: Category) -> bool {
    matches!(
        cat,
        Category::Skills | Category::Rules | Category::Commands | Category::Agents
    )
}

#[allow(clippy::missing_const_for_fn)] // KeyCode match is not const-compatible
fn handle_content_diff(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.view = View::Diff;
            app.content_diff_scroll = 0;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.content_diff_scroll = app.content_diff_scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.content_diff_scroll = app.content_diff_scroll.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.content_diff_scroll = app.content_diff_scroll.saturating_add(20);
        }
        KeyCode::PageUp => {
            app.content_diff_scroll = app.content_diff_scroll.saturating_sub(20);
        }
        _ => {}
    }
}

fn open_content_diff(app: &mut App) {
    let Some(file_path) = app.diff_files.get(app.diff_cursor).cloned() else {
        return;
    };
    let Some(ref active_name) = app.active_profile.clone() else {
        return;
    };
    let Some(profile) = app.selected_profile() else {
        return;
    };
    let selected_name = profile.name.clone();

    let left = crate::core::diff::DiffSide::Profile(active_name);
    let right = crate::core::diff::DiffSide::Profile(&selected_name);

    match crate::core::diff::content_diff(&app.paths, &left, &right, &file_path) {
        Ok(text) => {
            app.content_diff_text = text;
            app.content_diff_scroll = 0;
            app.view = View::ContentDiff;
        }
        Err(e) => {
            app.status_message = Some(format!("Diff error: {e}"));
        }
    }
}

#[allow(clippy::too_many_lines)] // dispatch table for the clone dialog's fields
fn handle_clone_dialog(app: &mut App, code: KeyCode) {
    // Field layout:
    //   0 = name
    //   1 = mode toggle (Empty / CloneFrom)
    //   2..=10 = category toggles (only in CloneFrom mode)
    //   11 = fresh CLAUDE.md toggle (only in CloneFrom mode)
    let num_cats = app.clone_categories.len(); // 9
    let max_field = if app.clone_mode == NewProfileMode::CloneFrom {
        num_cats + 2 // 0=name, 1=mode, 2..10=cats, 11=fresh
    } else {
        1 // 0=name, 1=mode (no categories in Empty mode)
    };

    match code {
        KeyCode::Esc => app.view = View::Detail,
        KeyCode::Tab | KeyCode::Down => {
            app.clone_field_index = (app.clone_field_index + 1) % (max_field + 1);
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.clone_field_index = if app.clone_field_index == 0 {
                max_field
            } else {
                app.clone_field_index - 1
            };
        }
        // Mode toggle (field 1)
        KeyCode::Char(' ') if app.clone_field_index == 1 => {
            app.clone_mode = match app.clone_mode {
                NewProfileMode::CloneFrom => NewProfileMode::Empty,
                NewProfileMode::Empty => {
                    if app.selected_profile().is_some() {
                        NewProfileMode::CloneFrom
                    } else {
                        NewProfileMode::Empty // can't clone without a source
                    }
                }
            };
            // Clamp field index if we just switched to Empty mode
            if app.clone_mode == NewProfileMode::Empty && app.clone_field_index > 1 {
                app.clone_field_index = 1;
            }
        }
        // Category toggles (fields 2..=10, CloneFrom only)
        KeyCode::Char(' ')
            if app.clone_mode == NewProfileMode::CloneFrom
                && app.clone_field_index >= 2
                && app.clone_field_index <= num_cats + 1 =>
        {
            let idx = app.clone_field_index - 2;
            let new_val = !app.clone_categories[idx].1;
            app.clone_categories[idx].1 = new_val;
            // Mutual exclusivity: enabling CLAUDE.md category disables fresh_md
            if idx == 0 && new_val {
                app.clone_fresh_md = false;
            }
        }
        // Fresh CLAUDE.md toggle (field 11, CloneFrom only)
        KeyCode::Char(' ')
            if app.clone_mode == NewProfileMode::CloneFrom
                && app.clone_field_index == num_cats + 2 =>
        {
            app.clone_fresh_md = !app.clone_fresh_md;
            // Mutual exclusivity: enabling fresh_md disables CLAUDE.md category
            if app.clone_fresh_md {
                app.clone_categories[0].1 = false;
            }
        }
        // Right arrow on a pickable category opens the per-file picker.
        KeyCode::Right
            if app.clone_mode == NewProfileMode::CloneFrom
                && app.clone_field_index >= 2
                && app.clone_field_index <= num_cats + 1 =>
        {
            let cat_idx = app.clone_field_index - 2;
            let (cat, enabled) = app.clone_categories[cat_idx];
            if enabled
                && is_pickable(cat)
                && let Some(source) = app.selected_profile()
            {
                let existing_picks = app.clone_file_picks.get(&cat);

                // Collapse category files into pickable units (one row per
                // skill dir, etc.) and remember which files each row covers.
                let mut members: HashMap<String, Vec<String>> = HashMap::new();
                for p in source
                    .manifest
                    .files
                    .keys()
                    .filter(|p| categorize_file(p) == cat)
                {
                    members
                        .entry(crate::core::clone::picker_group_key(p))
                        .or_default()
                        .push(p.clone());
                }
                for files in members.values_mut() {
                    files.sort();
                }

                // A row is pre-checked when there's no prior pick set, or every
                // file it covers is already in that set.
                let mut items: Vec<(String, bool)> = members
                    .iter()
                    .map(|(group, files)| {
                        let selected = existing_picks.is_none_or(|s| {
                            s.is_empty() || files.iter().all(|f| s.contains(f.as_str()))
                        });
                        (group.clone(), selected)
                    })
                    .collect();
                items.sort_by(|a, b| a.0.cmp(&b.0));

                if items.is_empty() {
                    app.status_message = Some(format!("No files in {cat:?} category."));
                } else {
                    app.file_picker_category = cat;
                    app.file_picker_items = items;
                    app.file_picker_members = members;
                    app.file_picker_cursor = 0;
                    app.view = View::FilePicker;
                }
            }
        }
        KeyCode::Backspace if app.clone_field_index == 0 => {
            app.clone_name.pop();
        }
        KeyCode::Char(c) if app.clone_field_index == 0 => {
            app.clone_name.push(c);
        }
        KeyCode::Enter => match app.clone_mode {
            NewProfileMode::Empty => execute_new_empty(app),
            NewProfileMode::CloneFrom => execute_clone(app),
        },
        _ => {}
    }
}

fn execute_clone(app: &mut App) {
    let target = app.clone_name.trim().to_string();
    if target.is_empty() {
        app.status_message = Some("Clone name cannot be empty.".to_string());
        return;
    }

    let Some(source_name) = app.selected_profile().map(|p| p.name.clone()) else {
        return;
    };

    let only: Vec<Category> = app
        .clone_categories
        .iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(cat, _)| *cat)
        .collect();

    // Build per-file picks: only carry forward picks for enabled categories
    // that actually have a non-empty selection set.
    let file_picks: HashMap<Category, HashSet<String>> = app
        .clone_file_picks
        .iter()
        .filter(|(cat, files)| only.contains(cat) && !files.is_empty())
        .map(|(cat, files)| (*cat, files.clone()))
        .collect();

    let opts = crate::core::clone::CloneOptions {
        source: &source_name,
        target: &target,
        description: "",
        only: Some(only),
        without: None,
        fresh_claude_md: app.clone_fresh_md,
        file_picks: if file_picks.is_empty() {
            None
        } else {
            Some(file_picks)
        },
    };

    match crate::core::clone::clone_profile(&app.paths, &opts) {
        Ok(result) => {
            app.status_message = Some(format!(
                "Cloned \"{}\" -> \"{}\" ({} files)",
                source_name, target, result.files_cloned
            ));
            let _ = app.refresh();
            // Select the clone target so the cursor follows it for an
            // immediate load, instead of staying on the source profile.
            app.select_by_name(&target);
            app.rebuild_tree();
        }
        Err(e) => {
            app.status_message = Some(format!("Clone failed: {e}"));
        }
    }
    app.view = View::Detail;
}

fn execute_new_empty(app: &mut App) {
    let name = app.clone_name.trim().to_string();
    if name.is_empty() {
        app.status_message = Some("Profile name cannot be empty.".to_string());
        return;
    }

    let profile_dir = app.paths.profile_dir(&name);
    if profile_dir.exists() {
        app.status_message = Some(format!("Profile \"{name}\" already exists."));
        return;
    }

    let files_dir = app.paths.profile_files_dir(&name);
    if let Err(e) = std::fs::create_dir_all(&files_dir) {
        app.status_message = Some(format!("Failed to create profile: {e}"));
        app.view = View::Detail;
        return;
    }

    // Write an empty CLAUDE.md
    let claude_md = files_dir.join("CLAUDE.md");
    if let Err(e) = std::fs::write(&claude_md, "") {
        app.status_message = Some(format!("Failed to write CLAUDE.md: {e}"));
        app.view = View::Detail;
        return;
    }

    let hash = crate::core::checksum::sha256_file(&claude_md).unwrap_or_default();
    let mut files = std::collections::HashMap::new();
    files.insert(
        "CLAUDE.md".to_string(),
        crate::core::profile::FileEntry {
            checksum: hash,
            size: 0,
            source: crate::core::profile::FileSource::Skeleton,
            mode: None,
        },
    );

    let manifest = crate::core::profile::ProfileManifest {
        version: 1,
        name: name.clone(),
        created_at: chrono::Utc::now(),
        last_loaded: None,
        load_count: 0,
        description: String::new(),
        tags: Vec::new(),
        files,
        excluded_patterns: Vec::new(),
    };

    if let Err(e) = crate::storage::manifest::write(&app.paths.profile_manifest(&name), &manifest) {
        app.status_message = Some(format!("Failed to write manifest: {e}"));
        app.view = View::Detail;
        return;
    }

    let meta = crate::core::profile::ProfileMeta {
        description: String::new(),
        tags: Vec::new(),
        notes: Some("Created as empty profile".to_string()),
        created_by: std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string()),
    };
    let _ = crate::storage::meta::write(&app.paths.profile_meta(&name), &meta);

    app.status_message = Some(format!("Created empty profile \"{name}\"."));
    let _ = app.refresh();
    // Move the cursor onto the profile we just made so the obvious next step
    // (`l` to load it) targets it — not whatever sits at the stale index.
    app.select_by_name(&name);
    app.rebuild_tree();
    app.view = View::Detail;
}

fn move_selection(app: &mut App, delta: isize) {
    let len = app.profiles.len();
    if len == 0 {
        return;
    }
    let current = app.list_state.selected().unwrap_or(0);
    let next = if delta < 0 {
        (current + len - delta.unsigned_abs() % len) % len
    } else {
        (current + delta.unsigned_abs()) % len
    };
    app.list_state.select(Some(next));
    app.file_scroll = 0;
    // Switching profiles rebuilds the file tree
    app.rebuild_tree();
}

#[allow(clippy::missing_const_for_fn)] // match on runtime index
fn active_field_mut(app: &mut App) -> &mut String {
    match app.save_field_index {
        0 => &mut app.save_name,
        1 => &mut app.save_description,
        _ => &mut app.save_tags,
    }
}

fn execute_save(app: &mut App) {
    let name = app.save_name.trim().to_string();
    if name.is_empty() {
        app.status_message = Some("Name cannot be empty.".to_string());
        return;
    }

    let tags: Vec<String> = app
        .save_tags
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    match crate::core::snapshot::save(&app.paths, &name, app.save_description.trim(), &tags) {
        Ok(_) => {
            app.status_message = Some(format!("Saved profile \"{name}\"."));
            let _ = app.refresh();
            app.view = View::Detail;
        }
        Err(e) => {
            app.status_message = Some(format!("Save failed: {e}"));
            app.view = View::Detail;
        }
    }
}

fn execute_load(app: &mut App) {
    let Some(name) = app.selected_profile().map(|p| p.name.clone()) else {
        return;
    };
    let opts = app.load_options;
    if opts.dry_run {
        // Don't actually swap — give the user a status-bar summary of what
        // *would* have happened. The LoadConfirm modal already shows the
        // file/plugin diff against the active profile.
        app.status_message = Some(format!(
            "[dry-run] Would load \"{name}\" (backup={}, plugins={}). No changes made.",
            opts.backup, opts.plugins
        ));
        app.view = View::Detail;
        return;
    }
    spawn_load(app, name, opts);
}

fn execute_toggle(app: &mut App) {
    let Some(target) = app.previous_profile.clone() else {
        app.status_message = Some("No previous profile to toggle to.".to_string());
        return;
    };
    // Toggle uses safe defaults — there's no LoadConfirm modal in front of
    // it where the user could opt out, so we keep the auto-backup and
    // plugin reinstall behaviour matching the pre-flag baseline.
    spawn_load(app, target, LoadOptions::default());
}

/// Kick off a load on a worker thread, transitioning the UI to
/// `LoadInProgress`. The main loop polls the returned channel each tick to
/// drive the spinner and to detect completion.
fn spawn_load(app: &mut App, target: String, opts: LoadOptions) {
    // Refuse to start a second load if one is already in flight — the
    // loader takes a file lock anyway, but failing fast in the UI is
    // cleaner than letting the worker block on lock acquisition.
    if app.load_in_flight.is_some() {
        app.status_message = Some("A load is already in progress.".to_string());
        return;
    }

    let (tx, rx) = mpsc::channel::<LoadEvent>();
    let paths = app.paths.clone();
    let target_for_thread = target.clone();
    let tx_done = tx.clone();

    let handle = std::thread::spawn(move || {
        let reporter = ChannelProgress::new(tx);
        let outcome = crate::core::loader::load_with_progress(
            &paths,
            &target_for_thread,
            !opts.plugins, // no_plugins
            !opts.backup,  // no_backup
            true,          // skip_claude_check (TUI assumes interactive context)
            &reporter,
        );
        // Forward the terminal result regardless of which event branch
        // the loader exited through.
        let event = match outcome {
            Ok(r) => LoadEvent::Done(Ok(r)),
            Err(e) => LoadEvent::Done(Err(format!("{e:#}"))),
        };
        let _ = tx_done.send(event);
    });

    app.load_in_flight = Some(LoadInFlight {
        target,
        started_at: Instant::now(),
        phase: String::new(),
        current: 0,
        total: 0,
        item: String::new(),
        rx,
        _handle: handle,
    });
    app.view = View::LoadInProgress;
    app.status_message = None;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::core::profile::PortalState;
    use crate::storage::{paths::PortalPaths, state};

    /// Build an App rooted in a tempdir with one saved + active "default"
    /// profile. The list cursor starts on index 0 = "default".
    fn app_with_default() -> (tempfile::TempDir, App) {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = PortalPaths::with_home(tmp.path().to_path_buf());
        paths.ensure_dirs().unwrap();

        let claude = paths.claude_root();
        crate::core::skeleton::create(&claude).unwrap();
        std::fs::write(claude.join("CLAUDE.md"), "# default").unwrap();
        crate::core::snapshot::save(&paths, "default", "", &[]).unwrap();
        state::write(
            &paths.state_file(),
            &PortalState {
                active_profile: Some("default".to_string()),
                ..PortalState::default()
            },
        )
        .unwrap();

        let app = App::new(paths).unwrap();
        (tmp, app)
    }

    /// Regression for "create empty profile + load does nothing, still on
    /// default": a new profile whose name sorts AFTER the active one must end
    /// up under the cursor, so the obvious next action (`l` to load) targets
    /// it instead of the stale index that still pointed at "default".
    #[test]
    fn new_empty_selects_the_created_profile() {
        let (_tmp, mut app) = app_with_default();
        assert_eq!(app.selected_profile().unwrap().name, "default");

        app.clone_name = "zzz".to_string(); // sorts after "default"
        execute_new_empty(&mut app);

        assert_eq!(
            app.selected_profile().map(|p| p.name.as_str()),
            Some("zzz"),
            "cursor must follow the freshly created profile, not stay on the old index"
        );
    }

    /// Same guarantee for clone: after cloning, the cursor moves to the new
    /// clone, not the source it was copied from.
    #[test]
    fn clone_selects_the_target_profile() {
        let (_tmp, mut app) = app_with_default();

        app.clone_name = "zztarget".to_string(); // sorts after the source
        app.clone_categories = crate::core::clone::Category::all()
            .into_iter()
            .map(|c| (c, true))
            .collect();
        execute_clone(&mut app);

        assert_eq!(
            app.selected_profile().map(|p| p.name.as_str()),
            Some("zztarget"),
            "after clone the cursor must move to the new clone, not the source"
        );
    }

    /// Regression for "load does nothing and the previous profile isn't backed
    /// up": `Shift+L` arms a dry-run preview by setting `load_options.dry_run`.
    /// That flag must not leak into the next real load. Opening the confirm
    /// modal via `Enter` (or `l`) has to reset to a real load — otherwise the
    /// stale dry-run flag makes `execute_load` no-op (no swap, no backup),
    /// which the user sees as the profile simply refusing to load.
    #[test]
    fn enter_clears_stale_dry_run_from_shift_l_preview() {
        let (_tmp, mut app) = app_with_default();

        // Shift+L arms a dry-run preview, then the user backs out.
        handle_main(&mut app, KeyCode::Char('L'));
        assert!(app.load_options.dry_run, "Shift+L should arm dry-run");
        assert_eq!(app.view, View::LoadConfirm);
        handle_load_confirm(&mut app, KeyCode::Esc);
        assert_eq!(app.view, View::Detail);

        // Now open the modal via Enter for a *real* load. Park the cursor past
        // the tree so Enter opens the modal rather than toggling a folder row.
        app.detail_cursor = usize::MAX;
        handle_main(&mut app, KeyCode::Enter);

        assert_eq!(app.view, View::LoadConfirm);
        assert!(
            !app.load_options.dry_run,
            "Enter-to-load must clear a stale dry-run flag, else the load silently no-ops"
        );
    }

    /// End-to-end through the real TUI path: create a profile, select it,
    /// `l` then `y` to load, then pump the worker to completion. The whole
    /// point of the tool — the active profile must switch AND a backup of the
    /// outgoing config must land in `backups_dir()`.
    #[test]
    fn tui_load_switches_active_and_writes_backup() {
        let (_tmp, mut app) = app_with_default();

        app.clone_name = "work".to_string();
        execute_new_empty(&mut app);
        assert_eq!(app.selected_profile().unwrap().name, "work");

        handle_main(&mut app, KeyCode::Char('l'));
        assert_eq!(app.view, View::LoadConfirm);
        handle_load_confirm(&mut app, KeyCode::Char('y'));

        // Drive the async load to completion (worker thread + channel drain).
        // `drain_load_events` is non-blocking (`try_recv`), so bound the wait by
        // wall-clock and sleep between polls — a fixed spin count would race the
        // worker thread and flake on machines fast enough to exhaust it before
        // the real filesystem work (backup, swap, post-swap verify) completes.
        let start = std::time::Instant::now();
        while app.load_in_flight.is_some() {
            app.drain_load_events();
            if app.load_in_flight.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(30),
                "load never completed"
            );
        }

        let state = crate::storage::state::read(&app.paths.state_file()).unwrap();
        assert_eq!(
            state.active_profile.as_deref(),
            Some("work"),
            "active profile must switch to the loaded one"
        );

        let backups: Vec<_> = std::fs::read_dir(app.paths.backups_dir())
            .map(|rd| rd.filter_map(Result::ok).collect())
            .unwrap_or_default();
        assert!(
            !backups.is_empty(),
            "loading must back up the outgoing config"
        );
    }
}
