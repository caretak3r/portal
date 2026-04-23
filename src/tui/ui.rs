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

fn render_detail(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Detail ");

    let Some(profile) = app.selected_profile() else {
        let empty = Paragraph::new("No profiles found. Press 's' to save current config.")
            .block(block);
        frame.render_widget(empty, area);
        return;
    };

    let m = &profile.manifest;
    let label = Style::default().fg(Color::Cyan);
    let active_style = Style::default().fg(Color::Green).bold();

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Name with active indicator
    let name_style = if app.is_active(&profile.name) {
        active_style
    } else {
        Style::default().bold()
    };
    lines.push(Line::from(vec![
        Span::styled("Name: ", label),
        Span::styled(m.name.clone(), name_style),
    ]));

    // Description
    lines.push(Line::from(vec![
        Span::styled("Desc: ", label),
        Span::raw(&m.description),
    ]));

    // Tags
    if !m.tags.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Tags: ", label),
            Span::raw(m.tags.join(", ")),
        ]));
    }

    // Created
    lines.push(Line::from(vec![
        Span::styled("Created: ", label),
        Span::raw(m.created_at.format("%Y-%m-%d %H:%M").to_string()),
    ]));

    // Last loaded
    if let Some(ref last) = m.last_loaded {
        lines.push(Line::from(vec![
            Span::styled("Loaded:  ", label),
            Span::raw(last.format("%Y-%m-%d %H:%M").to_string()),
        ]));
    }

    // Load count
    lines.push(Line::from(vec![
        Span::styled("Loads:   ", label),
        Span::raw(m.load_count.to_string()),
    ]));

    lines.push(Line::from(""));

    // Files
    let file_count = m.files.len();
    let total_size: u64 = m.files.values().map(|f| f.size).sum();
    lines.push(Line::from(vec![
        Span::styled("Files: ", label),
        Span::raw(format!("{file_count} ({total_size} bytes)")),
    ]));

    let mut sorted_files: Vec<_> = m.files.iter().collect();
    sorted_files.sort_by_key(|(k, _)| k.as_str());
    for (path, entry) in &sorted_files {
        lines.push(Line::from(format!("  {path}  ({} B)", entry.size)));
    }

    // Plugins
    if let Some(ref bp) = profile.blueprint {
        if !bp.plugins.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Plugins: ", label),
                Span::raw(bp.plugins.len().to_string()),
            ]));
            for plugin in &bp.plugins {
                let status = if plugin.enabled { "+" } else { "-" };
                lines.push(Line::from(format!("  [{status}] {}", plugin.id)));
            }
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.file_scroll, 0));

    frame.render_widget(paragraph, area);
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

fn render_help(frame: &mut Frame, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ");

    let hint = Style::default().fg(Color::Yellow);

    let lines = vec![
        Line::from(vec![
            Span::styled("j/Down  ", hint),
            Span::raw("next profile"),
        ]),
        Line::from(vec![
            Span::styled("k/Up    ", hint),
            Span::raw("previous profile"),
        ]),
        Line::from(vec![
            Span::styled("Enter   ", hint),
            Span::raw("load selected profile"),
        ]),
        Line::from(vec![
            Span::styled("d       ", hint),
            Span::raw("diff selected vs active"),
        ]),
        Line::from(vec![
            Span::styled("s       ", hint),
            Span::raw("save current config as profile"),
        ]),
        Line::from(vec![
            Span::styled("Esc     ", hint),
            Span::raw("back / cancel"),
        ]),
        Line::from(vec![
            Span::styled("?       ", hint),
            Span::raw("toggle help"),
        ]),
        Line::from(vec![
            Span::styled("q       ", hint),
            Span::raw("quit"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+C  ", hint),
            Span::raw("force quit"),
        ]),
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
