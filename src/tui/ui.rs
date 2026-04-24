use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::core::diff::{diff_profiles, DiffSide};

use super::app::{App, View};

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
        View::SaveDialog => render_save_dialog(frame, app, right_pane),
        View::LoadConfirm => render_load_confirm(frame, app, right_pane),
        View::CloneDialog => render_clone_dialog(frame, app, right_pane),
        View::Help => render_help(frame, right_pane),
    }

    render_status_bar(frame, app, status_area);
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Profiles "),
        )
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
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Detail ");
        let empty = Paragraph::new("No profiles found. Press 's' to save current config.")
            .block(block);
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
    let active_tag = if app.is_active(&profile.name) { " ● active" } else { "" };

    let mut header_lines: Vec<Line<'_>> = vec![
        Line::from(vec![
            Span::styled(&m.name, name_style),
            Span::styled(active_tag, Style::default().fg(Color::Green)),
        ]),
    ];
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
        Span::raw(format!("{} files, {}", m.files.len(), fmt_bytes(total_size))),
    ]));

    let header = Paragraph::new(header_lines)
        .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT).title(format!(" {} ", profile.name)));
    frame.render_widget(header, header_area);

    // ── File tree ──
    let tree_block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT);

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
    for (i, row) in rows.iter().enumerate().skip(scroll_offset).take(visible_height) {
        #[allow(clippy::cast_possible_truncation)]
        let y = inner.y + (i - scroll_offset) as u16;
        if y >= inner.y + inner.height {
            break;
        }

        let indent = "  ".repeat(row.depth);
        let is_selected = i == app.detail_cursor;

        let row_style = if is_selected {
            Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
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

        let line = Line::from(vec![
            Span::raw(indent),
            name_span,
            size_span,
        ]);

        let line_area = ratatui::layout::Rect::new(inner.x, y, inner.width, 1);
        let para = Paragraph::new(line).style(row_style);
        frame.render_widget(para, line_area);
    }

    // Scroll indicator
    if rows.len() > visible_height {
        let pct = if rows.is_empty() { 0 } else { (app.detail_cursor * 100) / rows.len() };
        let indicator = Paragraph::new(format!(" {pct}%"))
            .style(dim)
            .alignment(ratatui::layout::Alignment::Right);
        let ind_area = ratatui::layout::Rect::new(
            inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1
        );
        frame.render_widget(indicator, ind_area);
    }

    // ── Footer: keybinding hints ──
    let hint = Style::default().fg(Color::Yellow);
    let footer_lines = vec![
        Line::from(vec![
            Span::styled(" j/k", hint), Span::raw(" navigate  "),
            Span::styled("Enter", hint), Span::raw(" expand/collapse  "),
            Span::styled("l", hint), Span::raw(" load"),
        ]),
        Line::from(vec![
            Span::styled(" Tab", hint), Span::raw(" next profile  "),
            Span::styled("d", hint), Span::raw(" diff  "),
            Span::styled("s", hint), Span::raw(" save  "),
            Span::styled("c", hint), Span::raw(" clone  "),
            Span::styled("?", hint), Span::raw(" help"),
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

fn render_diff(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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

    let mut lines: Vec<Line<'_>> = Vec::new();
    let label = Style::default().fg(Color::Cyan);

    lines.push(Line::from(vec![
        Span::styled("Left:  ", label),
        Span::raw(&diff_result.left_name),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Right: ", label),
        Span::raw(&diff_result.right_name),
    ]));
    lines.push(Line::from(""));

    lines.push(Line::from(format!(
        "Identical: {}",
        diff_result.shared_same.len()
    )));
    lines.push(Line::from(format!(
        "Modified:  {}",
        diff_result.different_content.len()
    )));
    lines.push(Line::from(format!(
        "Left-only: {}",
        diff_result.only_left.len()
    )));
    lines.push(Line::from(format!(
        "Right-only: {}",
        diff_result.only_right.len()
    )));

    if !diff_result.different_content.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("Modified files:", label));
        for fd in &diff_result.different_content {
            lines.push(Line::from(format!(
                "  {} ({} -> {} B)",
                fd.path, fd.left_size, fd.right_size
            )));
        }
    }

    if !diff_result.only_left.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("Only in active:", label));
        for f in &diff_result.only_left {
            lines.push(Line::from(format!("  {f}")));
        }
    }

    if !diff_result.only_right.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("Only in selected:", label));
        for f in &diff_result.only_right {
            lines.push(Line::from(format!("  {f}")));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.file_scroll, 0));

    frame.render_widget(paragraph, area);
}

fn render_save_dialog(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Save Current Config ");

    let label = Style::default().fg(Color::Cyan);
    let active_field = Style::default().fg(Color::Yellow).bold();

    let field_style = |idx: usize| -> Style {
        if app.save_field_index == idx {
            active_field
        } else {
            Style::default()
        }
    };

    let lines = vec![
        Line::from("Save current ~/.claude/ as a new profile."),
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
        Line::styled("Tab: next field  Enter: save  Esc: cancel", Style::default().fg(Color::Yellow)),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_load_confirm(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm Load ");

    let name = app
        .selected_profile()
        .map_or("<none>", |p| p.name.as_str());

    let lines = vec![
        Line::from(format!("Load profile \"{name}\"?")),
        Line::from(""),
        Line::from("This will replace your current ~/.claude/ directory."),
        Line::from("A backup will be created automatically."),
        Line::from(""),
        Line::styled(
            "y/Enter: confirm  Esc/n: cancel",
            Style::default().fg(Color::Yellow),
        ),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_clone_dialog(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let source_name = app
        .selected_profile()
        .map_or("<none>", |p| p.name.as_str());

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Clone \"{source_name}\" "));

    let label = Style::default().fg(Color::Cyan);
    let active_field = Style::default().fg(Color::Yellow).bold();
    let dim = Style::default().fg(Color::DarkGray);

    let mut lines: Vec<Line<'_>> = Vec::new();

    lines.push(Line::from("Create a new profile from this one."));
    lines.push(Line::from(""));

    // Name field (index 0)
    let name_style = if app.clone_field_index == 0 { active_field } else { Style::default() };
    let cursor = if app.clone_field_index == 0 { "_" } else { "" };
    lines.push(Line::from(vec![
        if app.clone_field_index == 0 { Span::styled("▸ ", active_field) } else { Span::raw("  ") },
        Span::styled("Name: ", label),
        Span::styled(format!("{}{cursor}", app.clone_name), name_style),
    ]));
    lines.push(Line::from(""));

    // Category toggles (indices 1..=N)
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Categories to include:", label),
    ]));

    let cat_names = [
        "CLAUDE.md", "Settings", "Skills", "Rules", "Memory",
        "Commands", "Agents", "Hooks", "Plugins",
    ];

    for (i, ((_, enabled), name)) in app.clone_categories.iter().zip(cat_names.iter()).enumerate() {
        let field_idx = i + 1;
        let checkbox = if *enabled { "[x]" } else { "[ ]" };
        let style = if app.clone_field_index == field_idx { active_field } else { Style::default() };
        let pointer = if app.clone_field_index == field_idx { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            if app.clone_field_index == field_idx { Span::styled(pointer, active_field) } else { Span::raw(pointer) },
            Span::styled(format!("  {checkbox} {name}"), style),
        ]));
    }

    lines.push(Line::from(""));

    // Fresh CLAUDE.md toggle (last index)
    let fresh_idx = app.clone_categories.len() + 1;
    let fresh_check = if app.clone_fresh_md { "[x]" } else { "[ ]" };
    let fresh_style = if app.clone_field_index == fresh_idx { active_field } else { Style::default() };
    let fresh_ptr = if app.clone_field_index == fresh_idx { "▸ " } else { "  " };
    lines.push(Line::from(vec![
        if app.clone_field_index == fresh_idx { Span::styled(fresh_ptr, active_field) } else { Span::raw(fresh_ptr) },
        Span::styled(format!("  {fresh_check} Start with empty CLAUDE.md"), fresh_style),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Tab/↑↓", dim),
        Span::raw(" navigate  "),
        Span::styled("Space", dim),
        Span::raw(" toggle  "),
        Span::styled("Enter", dim),
        Span::raw(" clone  "),
        Span::styled("Esc", dim),
        Span::raw(" cancel"),
    ]));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_help(frame: &mut Frame, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ");

    let hint = Style::default().fg(Color::Yellow);

    let lines = vec![
        Line::from(""),
        Line::styled(" File Tree", Style::default().bold()),
        Line::from(vec![Span::styled("  j/k     ", hint), Span::raw("navigate files")]),
        Line::from(vec![Span::styled("  Enter   ", hint), Span::raw("expand/collapse folder")]),
        Line::from(""),
        Line::styled(" Profiles", Style::default().bold()),
        Line::from(vec![Span::styled("  Tab     ", hint), Span::raw("next profile")]),
        Line::from(vec![Span::styled("  S-Tab   ", hint), Span::raw("previous profile")]),
        Line::from(vec![Span::styled("  l       ", hint), Span::raw("load selected profile")]),
        Line::from(""),
        Line::styled(" Actions", Style::default().bold()),
        Line::from(vec![Span::styled("  d       ", hint), Span::raw("diff selected vs active")]),
        Line::from(vec![Span::styled("  s       ", hint), Span::raw("save current config")]),
        Line::from(vec![Span::styled("  c       ", hint), Span::raw("clone selected profile")]),
        Line::from(vec![Span::styled("  Esc     ", hint), Span::raw("back / cancel")]),
        Line::from(vec![Span::styled("  ?       ", hint), Span::raw("toggle help")]),
        Line::from(vec![Span::styled("  q       ", hint), Span::raw("quit")]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let active = app
        .active_profile
        .as_deref()
        .unwrap_or("none");

    let status_text = app.status_message.as_ref().map_or_else(
        || {
            format!(
                " Active: {active}  |  {} profiles  |  ?: help  q: quit",
                app.profiles.len()
            )
        },
        Clone::clone,
    );

    let bar = Paragraph::new(status_text)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(bar, area);
}
