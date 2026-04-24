use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io;
use std::time::Duration;

use super::app::{App, View};

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
        View::SaveDialog => handle_save_dialog(app, key.code),
        View::LoadConfirm => handle_load_confirm(app, key.code),
        View::Help => handle_help(app, key.code),
    }

    Ok(app.should_quit)
}

fn handle_main(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.view = View::Help,

        // Profile list navigation (left pane)
        KeyCode::Down | KeyCode::Char('j') if app.view == View::Diff => {
            app.file_scroll = app.file_scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') if app.view == View::Diff => {
            app.file_scroll = app.file_scroll.saturating_sub(1);
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
