use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io;
use std::time::Duration;

use super::app::{App, NewProfileMode, View};

/// Poll for input and dispatch to the appropriate handler.
///
/// Returns `Ok(true)` when the application should exit.
///
/// # Errors
///
/// Returns an error on terminal I/O failure.
pub fn handle(app: &mut App) -> io::Result<bool> {
    if !event::poll(Duration::from_millis(100))? {
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
        View::CloneDialog => handle_clone_dialog(app, key.code),
        View::Help => handle_help(app, key.code),
    }

    Ok(app.should_quit)
}

fn handle_main(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.view = View::Help,

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
                app.view = View::LoadConfirm;
            }
        }
        KeyCode::Enter if app.view == View::Diff && !app.diff_files.is_empty() => {
            open_content_diff(app);
        }
        KeyCode::Enter if app.selected_profile().is_some() => {
            app.view = View::LoadConfirm;
        }

        KeyCode::Char('l') if app.view == View::Detail && app.selected_profile().is_some() => {
            app.view = View::LoadConfirm;
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
            app.save_name.clear();
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
            app.view = View::CloneDialog;
        }
        KeyCode::Esc => {
            app.file_scroll = 0;
            app.view = View::Detail;
            app.status_message = None;
        }
        KeyCode::PageDown => app.file_scroll = app.file_scroll.saturating_add(10),
        KeyCode::PageUp => app.file_scroll = app.file_scroll.saturating_sub(10),
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

fn handle_load_confirm(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('y') | KeyCode::Enter => execute_load(app),
        KeyCode::Esc | KeyCode::Char('n') => app.view = View::Detail,
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
        KeyCode::Backspace if app.clone_field_index == 0 => {
            app.clone_name.pop();
        }
        KeyCode::Char(c) if app.clone_field_index == 0 => {
            app.clone_name.push(c);
        }
        KeyCode::Enter => {
            match app.clone_mode {
                NewProfileMode::Empty => execute_new_empty(app),
                NewProfileMode::CloneFrom => execute_clone(app),
            }
        }
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

    let only: Vec<crate::core::clone::Category> = app
        .clone_categories
        .iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(cat, _)| *cat)
        .collect();

    let opts = crate::core::clone::CloneOptions {
        source: &source_name,
        target: &target,
        description: "",
        only: Some(only),
        without: None,
        fresh_claude_md: app.clone_fresh_md,
    };

    match crate::core::clone::clone_profile(&app.paths, &opts) {
        Ok(result) => {
            app.status_message = Some(format!(
                "Cloned \"{}\" -> \"{}\" ({} files)",
                source_name, target, result.files_cloned
            ));
            let _ = app.refresh();
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

    match crate::core::loader::load(&app.paths, &name, false, true) {
        Ok(result) => {
            app.status_message = Some(format!(
                "Loaded \"{}\" ({} files).",
                result.profile, result.files_loaded
            ));
            let _ = app.refresh();
        }
        Err(e) => {
            app.status_message = Some(format!("Load failed: {e}"));
        }
    }
    app.view = View::Detail;
}
