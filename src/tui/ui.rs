use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::config::Theme;
use crate::core::diff::{DiffSide, diff_profiles};

use super::app::{App, NewProfileMode, View};
use super::palette::Palette;

/// Render the entire TUI frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    let [left_pane, right_pane] =
        Layout::horizontal([Constraint::Length(28), Constraint::Min(0)]).areas(main_area);

    render_profile_list(frame, app, left_pane);

    match app.view {
        View::Detail => render_detail(frame, app, right_pane),
        View::Diff => render_diff(frame, app, right_pane),
        View::ContentDiff => render_content_diff(frame, app, right_pane),
        View::SaveDialog => render_save_dialog(frame, app, right_pane),
        View::LoadConfirm => render_load_confirm(frame, app, right_pane),
        View::CloneDialog => render_clone_dialog(frame, app, right_pane),
        View::ThemePicker => {
            render_detail(frame, app, right_pane);
            render_theme_picker(frame, app, frame.area());
        }
        View::Help => render_help(frame, right_pane),
    }

    render_status_bar(frame, app, status_area);
}

fn render_theme_picker(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let palette = Palette::for_theme(app.theme);
    let themes = Theme::all();
    let height = u16::try_from(themes.len() + 6)
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(2));
    let width = 36u16.min(area.width.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = ratatui::layout::Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    let items: Vec<ListItem<'_>> = themes
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let marker = if *t == app.theme { "● " } else { "  " };
            let label = format!("{marker}{}", t.label());
            let style = if i == app.theme_cursor {
                Style::default()
                    .bg(palette.selection_bg)
                    .fg(palette.selection_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Theme ")
        .style(Style::default().fg(palette.header));

    let list = List::new(items).block(block);
    frame.render_widget(list, popup);

    let hint_y = popup.y + popup.height.saturating_sub(2);
    let hint_area =
        ratatui::layout::Rect::new(popup.x + 2, hint_y, popup.width.saturating_sub(4), 1);
    let hint = Paragraph::new(Line::styled(
        "j/k move  Enter apply  Esc cancel",
        Style::default().fg(palette.hint),
    ));
    frame.render_widget(hint, hint_area);
}

fn render_profile_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem<'_>> = app
        .profiles
        .iter()
        .map(|p| {
            let marker = if app.is_active(&p.name) {
                Span::styled("● ", Style::default().fg(Color::Green).bold())
            } else {
                Span::styled("○ ", Style::default().fg(Color::DarkGray))
            };
            let name = Span::raw(&p.name);
            ListItem::new(Line::from(vec![marker, name]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Profiles "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

#[allow(clippy::too_many_lines)]
fn render_detail(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(profile) = app.selected_profile() else {
        let block = Block::default().borders(Borders::ALL).title(" Detail ");
        let empty =
            Paragraph::new("No profiles found. Press 's' to save current config.").block(block);
        frame.render_widget(empty, area);
        return;
    };

    let m = &profile.manifest;
    let label = Style::default().fg(Color::Cyan);
    let active_style = Style::default().fg(Color::Green).bold();
    let dim = Style::default().fg(Color::DarkGray);

    // Split the right pane: metadata header + file tree + keybindings footer
    let [header_area, tree_area, footer_area] = Layout::vertical([
        Constraint::Length(8),
        Constraint::Min(4),
        Constraint::Length(3),
    ])
    .areas(area);

    // ── Header: profile metadata ──
    let name_style = if app.is_active(&profile.name) {
        active_style
    } else {
        Style::default().bold()
    };
    let active_tag = if app.is_active(&profile.name) {
        " ● active"
    } else {
        ""
    };

    let mut header_lines: Vec<Line<'_>> = vec![Line::from(vec![
        Span::styled(&m.name, name_style),
        Span::styled(active_tag, Style::default().fg(Color::Green)),
    ])];
    if !m.description.is_empty() {
        header_lines.push(Line::from(vec![
            Span::styled("  ", dim),
            Span::raw(&m.description),
        ]));
    }
    if !m.tags.is_empty() {
        header_lines.push(Line::from(vec![
            Span::styled("  tags: ", dim),
            Span::raw(m.tags.join(", ")),
        ]));
    }
    let created = m.created_at.format("%Y-%m-%d").to_string();
    let loaded_str = m.last_loaded.map_or_else(
        || "never".to_string(),
        |ll| ll.format("%Y-%m-%d %H:%M").to_string(),
    );
    header_lines.push(Line::from(vec![
        Span::styled("  created ", dim),
        Span::raw(&created),
        Span::styled("  loaded ", dim),
        Span::raw(&loaded_str),
        Span::styled(format!("  ({} loads)", m.load_count), dim),
    ]));

    // Plugin summary line
    if let Some(ref bp) = profile.blueprint {
        if !bp.plugins.is_empty() {
            let plugin_names: Vec<&str> = bp.plugins.iter().map(|p| p.id.as_str()).collect();
            header_lines.push(Line::from(""));
            header_lines.push(Line::from(vec![
                Span::styled("  plugins: ", label),
                Span::raw(plugin_names.join(", ")),
            ]));
        }
    }

    let total_size: u64 = m.files.values().map(|f| f.size).sum();
    header_lines.push(Line::from(""));
    header_lines.push(Line::from(vec![
        Span::styled("  Files ", label),
        Span::raw(format!(
            "{} files, {}",
            m.files.len(),
            fmt_bytes(total_size)
        )),
    ]));

    let header = Paragraph::new(header_lines).block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .title(format!(" {} ", profile.name)),
    );
    frame.render_widget(header, header_area);

    // ── File tree ──
    let tree_block = Block::default().borders(Borders::LEFT | Borders::RIGHT);

    let inner = tree_block.inner(tree_area);
    frame.render_widget(tree_block, tree_area);

    let visible_height = inner.height as usize;

    // Compute scroll offset to keep cursor visible
    let scroll_offset = if app.detail_cursor >= visible_height {
        app.detail_cursor - visible_height + 1
    } else {
        0
    };

    let rows = &app.tree_rows;
    for (i, row) in rows
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
    {
        #[allow(clippy::cast_possible_truncation)]
        let y = inner.y + (i - scroll_offset) as u16;
        if y >= inner.y + inner.height {
            break;
        }

        let indent = "  ".repeat(row.depth);
        let is_selected = i == app.detail_cursor;

        let row_style = if is_selected {
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let icon_style = if row.is_dir {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Green)
        };

        let name_span = if row.is_dir {
            Span::styled(&row.label, icon_style.add_modifier(Modifier::BOLD))
        } else {
            Span::styled(format!("  {}", row.label), Style::default())
        };

        let size_span = Span::styled(
            format!("  {}", row.size_label),
            Style::default().fg(Color::DarkGray),
        );

        let line = Line::from(vec![Span::raw(indent), name_span, size_span]);

        let line_area = ratatui::layout::Rect::new(inner.x, y, inner.width, 1);
        let para = Paragraph::new(line).style(row_style);
        frame.render_widget(para, line_area);
    }

    // Scroll indicator
    if rows.len() > visible_height {
        let pct = if rows.is_empty() {
            0
        } else {
            (app.detail_cursor * 100) / rows.len()
        };
        let indicator = Paragraph::new(format!(" {pct}%"))
            .style(dim)
            .alignment(ratatui::layout::Alignment::Right);
        let ind_area = ratatui::layout::Rect::new(
            inner.x,
            inner.y + inner.height.saturating_sub(1),
            inner.width,
            1,
        );
        frame.render_widget(indicator, ind_area);
    }

    // ── Footer: keybinding hints ──
    let hint = Style::default().fg(Color::Yellow);
    let footer_lines = vec![
        Line::from(vec![
            Span::styled(" j/k", hint),
            Span::raw(" navigate  "),
            Span::styled("Enter", hint),
            Span::raw(" expand/collapse  "),
            Span::styled("l", hint),
            Span::raw(" load"),
        ]),
        Line::from(vec![
            Span::styled(" Tab", hint),
            Span::raw(" next profile  "),
            Span::styled("d", hint),
            Span::raw(" diff  "),
            Span::styled("s", hint),
            Span::raw(" save  "),
            Span::styled("n", hint),
            Span::raw(" new  "),
            Span::styled("c", hint),
            Span::raw(" clone  "),
            Span::styled("?", hint),
            Span::raw(" help"),
        ]),
    ];
    let footer = Paragraph::new(footer_lines)
        .block(Block::default().borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT));
    frame.render_widget(footer, footer_area);
}

#[allow(clippy::cast_precision_loss)]
fn fmt_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[allow(clippy::too_many_lines)]
fn render_diff(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Diff vs Active ");

    let Some(profile) = app.selected_profile() else {
        let empty = Paragraph::new("No profile selected.").block(block);
        frame.render_widget(empty, area);
        return;
    };

    let Some(ref active_name) = app.active_profile else {
        let msg = Paragraph::new("No active profile to diff against.").block(block);
        frame.render_widget(msg, area);
        return;
    };

    if active_name == &profile.name {
        let msg = Paragraph::new("Selected profile is the active profile.").block(block);
        frame.render_widget(msg, area);
        return;
    }

    let left = DiffSide::Profile(active_name);
    let right = DiffSide::Profile(&profile.name);

    let diff_result = match diff_profiles(&app.paths, &left, &right) {
        Ok(d) => d,
        Err(e) => {
            let msg = Paragraph::new(format!("Diff error: {e}")).block(block);
            frame.render_widget(msg, area);
            return;
        }
    };

    // Update the navigable file list (modified files only — those can show content diff).
    app.diff_files = diff_result
        .different_content
        .iter()
        .map(|fd| fd.path.clone())
        .collect();
    if app.diff_cursor >= app.diff_files.len() {
        app.diff_cursor = app.diff_files.len().saturating_sub(1);
    }

    let label = Style::default().fg(Color::Cyan);
    let dim = Style::default().fg(Color::DarkGray);
    let added_style = Style::default().fg(Color::Green);
    let removed_style = Style::default().fg(Color::Red);
    let modified_style = Style::default().fg(Color::Yellow);
    let highlight = Style::default()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [header_area, list_area, footer_area] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .areas(inner);

    // Header: comparison names + summary counts
    let header_lines = vec![
        Line::from(vec![
            Span::styled(&diff_result.left_name, label),
            Span::styled(" vs ", dim),
            Span::styled(&diff_result.right_name, label),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("~{}", diff_result.different_content.len()),
                modified_style,
            ),
            Span::raw("  "),
            Span::styled(format!("+{}", diff_result.only_right.len()), added_style),
            Span::raw("  "),
            Span::styled(format!("-{}", diff_result.only_left.len()), removed_style),
            Span::styled(format!("  ={}", diff_result.shared_same.len()), dim),
        ]),
    ];
    let header = Paragraph::new(header_lines);
    frame.render_widget(header, header_area);

    // File list: modified, added, removed — all in one scrollable list
    let mut rows: Vec<Line<'_>> = Vec::new();
    let mut navigable_idx: usize = 0;

    if !diff_result.different_content.is_empty() {
        rows.push(Line::styled("Modified:", modified_style));
        for fd in &diff_result.different_content {
            let delta_str = if fd.right_size >= fd.left_size {
                format!("+{}B", fd.right_size - fd.left_size)
            } else {
                format!("-{}B", fd.left_size - fd.right_size)
            };
            let is_selected = navigable_idx == app.diff_cursor;
            let row_style = if is_selected {
                highlight
            } else {
                Style::default()
            };
            let marker = if is_selected { "▸ " } else { "  " };
            rows.push(Line::from(vec![
                Span::styled(marker, if is_selected { modified_style } else { dim }),
                Span::styled("~ ", modified_style),
                Span::styled(fd.path.clone(), row_style),
                Span::styled(format!("  {delta_str}"), dim),
            ]));
            navigable_idx += 1;
        }
        rows.push(Line::from(""));
    }

    if !diff_result.only_right.is_empty() {
        rows.push(Line::styled("Added (in selected):", added_style));
        for f in &diff_result.only_right {
            rows.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("+ ", added_style),
                Span::raw(f.as_str()),
            ]));
        }
        rows.push(Line::from(""));
    }

    if !diff_result.only_left.is_empty() {
        rows.push(Line::styled("Removed (only in active):", removed_style));
        for f in &diff_result.only_left {
            rows.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("- ", removed_style),
                Span::raw(f.as_str()),
            ]));
        }
    }

    if rows.is_empty() {
        rows.push(Line::styled("No differences.", dim));
    }

    // Compute scroll to keep cursor visible
    // Find cursor row in our rows vec (modified entries start after their header)
    let cursor_row = if app.diff_cursor < diff_result.different_content.len() {
        app.diff_cursor + 1 // +1 for the "Modified:" header
    } else {
        0
    };
    let visible_height = list_area.height as usize;
    #[allow(clippy::cast_possible_truncation)]
    let scroll = if cursor_row >= visible_height {
        (cursor_row - visible_height + 1) as u16
    } else {
        0
    };

    let list_para = Paragraph::new(rows).scroll((scroll, 0));
    frame.render_widget(list_para, list_area);

    // Footer hints
    let hint = Style::default().fg(Color::Yellow);
    let footer = Paragraph::new(Line::from(vec![
        Span::styled("j/k", hint),
        Span::raw(" navigate  "),
        Span::styled("Enter", hint),
        Span::raw(" view diff  "),
        Span::styled("d/Esc", hint),
        Span::raw(" back"),
    ]));
    frame.render_widget(footer, footer_area);
}

fn render_content_diff(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let file_name = app
        .diff_files
        .get(app.diff_cursor)
        .map_or("?", String::as_str);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {file_name} "));

    let added = Style::default().fg(Color::Green);
    let removed = Style::default().fg(Color::Red);
    let hunk_header = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);

    let lines: Vec<Line<'_>> = app
        .content_diff_text
        .lines()
        .map(|line| {
            if line.starts_with("+++") || line.starts_with("---") {
                Line::styled(line, hunk_header)
            } else if line.starts_with("@@") {
                Line::styled(line, Style::default().fg(Color::Magenta))
            } else if line.starts_with('+') {
                Line::styled(line, added)
            } else if line.starts_with('-') {
                Line::styled(line, removed)
            } else {
                Line::styled(line, dim)
            }
        })
        .collect();

    let total_lines = lines.len();
    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((app.content_diff_scroll, 0));

    frame.render_widget(paragraph, area);

    // Scroll indicator in status area (reuse bottom line of block)
    let inner = Block::default().borders(Borders::ALL).inner(area);
    if total_lines > inner.height as usize {
        let pct = (app.content_diff_scroll as usize * 100) / total_lines;
        let ind = Paragraph::new(format!(" {pct}% | Esc: back  j/k: scroll"))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Right);
        let ind_area = ratatui::layout::Rect::new(
            inner.x,
            inner.y + inner.height.saturating_sub(1),
            inner.width,
            1,
        );
        frame.render_widget(ind, ind_area);
    }
}

fn render_save_dialog(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let is_overwriting_active = app
        .active_profile
        .as_deref()
        .is_some_and(|a| a == app.save_name.trim());

    let title = if is_overwriting_active {
        " Save Current Config (updates active) "
    } else {
        " Save Current Config "
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    let label = Style::default().fg(Color::Cyan);
    let active_field = Style::default().fg(Color::Yellow).bold();

    let field_style = |idx: usize| -> Style {
        if app.save_field_index == idx {
            active_field
        } else {
            Style::default()
        }
    };

    let intro = if is_overwriting_active {
        format!(
            "Updating active profile \"{}\". Tab to change name to fork instead.",
            app.save_name
        )
    } else {
        "Save current ~/.claude/ as a new profile.".to_string()
    };

    let lines = vec![
        Line::from(intro),
        Line::from(""),
        Line::from(vec![
            Span::styled("Name:  ", label),
            Span::styled(app.save_name.clone(), field_style(0)),
            if app.save_field_index == 0 {
                Span::styled("_", active_field)
            } else {
                Span::raw("")
            },
        ]),
        Line::from(vec![
            Span::styled("Desc:  ", label),
            Span::styled(app.save_description.clone(), field_style(1)),
            if app.save_field_index == 1 {
                Span::styled("_", active_field)
            } else {
                Span::raw("")
            },
        ]),
        Line::from(vec![
            Span::styled("Tags:  ", label),
            Span::styled(app.save_tags.clone(), field_style(2)),
            if app.save_field_index == 2 {
                Span::styled("_", active_field)
            } else {
                Span::raw("")
            },
        ]),
        Line::from(""),
        Line::styled(
            "Tab: next field  Enter: save  Esc: cancel",
            Style::default().fg(Color::Yellow),
        ),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_load_confirm(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm Load ");

    let Some(profile) = app.selected_profile() else {
        let empty = Paragraph::new("No profile selected.").block(block);
        frame.render_widget(empty, area);
        return;
    };

    let label = Style::default().fg(Color::Cyan);
    let dim = Style::default().fg(Color::DarkGray);
    let added = Style::default().fg(Color::Green);
    let removed = Style::default().fg(Color::Red);
    let modified = Style::default().fg(Color::Yellow);

    let file_count = profile.manifest.files.len();
    let total_size: u64 = profile.manifest.files.values().map(|f| f.size).sum();

    let mut lines: Vec<Line<'_>> = vec![
        Line::from(vec![
            Span::raw("Load "),
            Span::styled(&profile.name, Style::default().bold()),
            Span::raw("?"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Files: ", label),
            Span::raw(format!("{file_count} ({})", fmt_bytes(total_size))),
        ]),
    ];

    // If there's an active profile, show what will change
    if let Some(ref active_name) = app.active_profile {
        if active_name != &profile.name {
            let left = DiffSide::Profile(active_name);
            let right = DiffSide::Profile(&profile.name);
            if let Ok(diff) = diff_profiles(&app.paths, &left, &right) {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Changes vs current (", dim),
                    Span::styled(active_name, dim),
                    Span::styled("):", dim),
                ]));
                if !diff.different_content.is_empty() {
                    lines.push(Line::from(vec![Span::styled(
                        format!("  ~ {} modified", diff.different_content.len()),
                        modified,
                    )]));
                }
                if !diff.only_right.is_empty() {
                    lines.push(Line::from(vec![Span::styled(
                        format!("  + {} added", diff.only_right.len()),
                        added,
                    )]));
                }
                if !diff.only_left.is_empty() {
                    lines.push(Line::from(vec![Span::styled(
                        format!("  - {} removed", diff.only_left.len()),
                        removed,
                    )]));
                }
                if !diff.shared_same.is_empty() {
                    lines.push(Line::from(vec![Span::styled(
                        format!("  = {} unchanged", diff.shared_same.len()),
                        dim,
                    )]));
                }
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("A backup will be created automatically."));
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "y/Enter: confirm  Esc/n: cancel",
        Style::default().fg(Color::Yellow),
    ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

#[allow(clippy::too_many_lines)]
fn render_clone_dialog(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let source_name = app.selected_profile().map(|p| p.name.as_str());

    let title = match app.clone_mode {
        NewProfileMode::Empty => " New Profile ".to_string(),
        NewProfileMode::CloneFrom => {
            format!(" New Profile (from \"{}\") ", source_name.unwrap_or("?"))
        }
    };

    let block = Block::default().borders(Borders::ALL).title(title);

    let label = Style::default().fg(Color::Cyan);
    let active_field = Style::default().fg(Color::Yellow).bold();
    let dim = Style::default().fg(Color::DarkGray);
    let disabled = Style::default().fg(Color::DarkGray);

    let ptr = |idx: usize| -> Span<'_> {
        if app.clone_field_index == idx {
            Span::styled("▸ ", active_field)
        } else {
            Span::raw("  ")
        }
    };

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Name field (index 0)
    let name_style = if app.clone_field_index == 0 {
        active_field
    } else {
        Style::default()
    };
    let cursor = if app.clone_field_index == 0 { "_" } else { "" };
    lines.push(Line::from(vec![
        ptr(0),
        Span::styled("Name: ", label),
        Span::styled(format!("{}{cursor}", app.clone_name), name_style),
    ]));
    lines.push(Line::from(""));

    // Mode toggle (index 1)
    let mode_label = match app.clone_mode {
        NewProfileMode::Empty => "Empty (fresh start)",
        NewProfileMode::CloneFrom => {
            if source_name.is_some() {
                "Clone from selected"
            } else {
                "Empty (fresh start)"
            }
        }
    };
    let mode_style = if app.clone_field_index == 1 {
        active_field
    } else {
        Style::default()
    };
    lines.push(Line::from(vec![
        ptr(1),
        Span::styled("Mode: ", label),
        Span::styled(format!("< {mode_label} >"), mode_style),
    ]));
    lines.push(Line::from(""));

    // Category toggles and fresh CLAUDE.md — only in CloneFrom mode
    if app.clone_mode == NewProfileMode::CloneFrom {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Include:", label),
        ]));

        let cat_names = [
            "CLAUDE.md",
            "Settings",
            "Skills",
            "Rules",
            "Memory",
            "Commands",
            "Agents",
            "Hooks",
            "Plugins",
        ];

        for (i, ((_, enabled), name)) in app
            .clone_categories
            .iter()
            .zip(cat_names.iter())
            .enumerate()
        {
            let field_idx = i + 2;
            let checkbox = if *enabled { "[x]" } else { "[ ]" };
            // CLAUDE.md category gets a hint when disabled by fresh_md
            let suffix = if i == 0 && app.clone_fresh_md {
                " (using empty instead)"
            } else {
                ""
            };
            let style = if app.clone_field_index == field_idx {
                active_field
            } else if i == 0 && app.clone_fresh_md {
                disabled
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                ptr(field_idx),
                Span::styled(format!("  {checkbox} {name}{suffix}"), style),
            ]));
        }

        lines.push(Line::from(""));

        // Fresh CLAUDE.md toggle (field num_cats + 2)
        let num_cats = app.clone_categories.len();
        let fresh_idx = num_cats + 2;
        let fresh_check = if app.clone_fresh_md { "[x]" } else { "[ ]" };
        let claude_md_on = app.clone_categories[0].1;
        let fresh_suffix = if !app.clone_fresh_md && claude_md_on {
            " (using source)"
        } else {
            ""
        };
        let fresh_style = if app.clone_field_index == fresh_idx {
            active_field
        } else if !app.clone_fresh_md && claude_md_on {
            disabled
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            ptr(fresh_idx),
            Span::styled(
                format!("  {fresh_check} Start with empty CLAUDE.md{fresh_suffix}"),
                fresh_style,
            ),
        ]));
    }

    lines.push(Line::from(""));
    let action = if app.clone_mode == NewProfileMode::Empty {
        "create"
    } else {
        "clone"
    };
    lines.push(Line::from(vec![
        Span::styled("  Tab/↑↓", dim),
        Span::raw(" navigate  "),
        Span::styled("Space", dim),
        Span::raw(" toggle  "),
        Span::styled("Enter", dim),
        Span::raw(format!(" {action}  ")),
        Span::styled("Esc", dim),
        Span::raw(" cancel"),
    ]));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_help(frame: &mut Frame, area: ratatui::layout::Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Help ");

    let hint = Style::default().fg(Color::Yellow);

    let lines = vec![
        Line::from(""),
        Line::styled(" File Tree", Style::default().bold()),
        Line::from(vec![
            Span::styled("  j/k     ", hint),
            Span::raw("navigate files"),
        ]),
        Line::from(vec![
            Span::styled("  Enter   ", hint),
            Span::raw("expand/collapse folder"),
        ]),
        Line::from(""),
        Line::styled(" Profiles", Style::default().bold()),
        Line::from(vec![
            Span::styled("  Tab     ", hint),
            Span::raw("next profile"),
        ]),
        Line::from(vec![
            Span::styled("  S-Tab   ", hint),
            Span::raw("previous profile"),
        ]),
        Line::from(vec![
            Span::styled("  l       ", hint),
            Span::raw("load selected profile"),
        ]),
        Line::from(""),
        Line::styled(" Diff Mode (d to enter)", Style::default().bold()),
        Line::from(vec![
            Span::styled("  j/k     ", hint),
            Span::raw("navigate modified files"),
        ]),
        Line::from(vec![
            Span::styled("  Enter   ", hint),
            Span::raw("view file content diff"),
        ]),
        Line::from(vec![
            Span::styled("  Esc     ", hint),
            Span::raw("back to detail view"),
        ]),
        Line::from(""),
        Line::styled(" Actions", Style::default().bold()),
        Line::from(vec![
            Span::styled("  d       ", hint),
            Span::raw("diff selected vs active"),
        ]),
        Line::from(vec![
            Span::styled("  s       ", hint),
            Span::raw("save current config"),
        ]),
        Line::from(vec![
            Span::styled("  n       ", hint),
            Span::raw("new profile (empty or clone)"),
        ]),
        Line::from(vec![
            Span::styled("  c       ", hint),
            Span::raw("clone selected profile"),
        ]),
        Line::from(vec![
            Span::styled("  T       ", hint),
            Span::raw("change theme"),
        ]),
        Line::from(vec![
            Span::styled("  Esc     ", hint),
            Span::raw("back / cancel"),
        ]),
        Line::from(vec![
            Span::styled("  ?       ", hint),
            Span::raw("toggle help"),
        ]),
        Line::from(vec![Span::styled("  q       ", hint), Span::raw("quit")]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let active = app.active_profile.as_deref().unwrap_or("none");

    let status_text = app.status_message.as_ref().map_or_else(
        || {
            format!(
                " Active: {active}  |  {} profiles  |  ?: help  q: quit",
                app.profiles.len()
            )
        },
        Clone::clone,
    );

    let bar =
        Paragraph::new(status_text).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(bar, area);
}
