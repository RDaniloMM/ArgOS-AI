use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use crate::state::{AppState, FocusPane, StatusLevel};

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(2),
        ])
        .split(frame.area());

    render_header(frame, layout[0], state);
    render_body(frame, layout[1], state);
    render_footer(frame, layout[2], state);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let provider = format!("Provider: {}", state.provider_status.level.label());
    let n8n = format!("n8n: {}", state.n8n_status.level.label());
    let focus = format!("Focus: {}", state.focus.title());
    let busy = if state.is_submitting_prompt {
        "Agent request running"
    } else if state.is_running_workflow {
        "Workflow running"
    } else if state.is_loading_snapshot {
        "Refreshing"
    } else {
        "Idle"
    };

    let line = Line::from(vec![
        Span::styled(
            " ArgOS TUI ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw("  "),
        badge(provider, state.provider_status.level),
        Span::raw(" "),
        badge(n8n, state.n8n_status.level),
        Span::raw(" "),
        badge(focus, StatusLevel::Success),
        Span::raw(" "),
        badge(
            format!("State: {busy}"),
            if busy == "Idle" {
                StatusLevel::Success
            } else {
                StatusLevel::Loading
            },
        ),
    ]);

    let block = Block::default().borders(Borders::ALL).title("Session");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(line), inner);
}

fn render_body(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(44),
            Constraint::Percentage(28),
        ])
        .split(area);

    render_sidebar(frame, columns[0], state);
    render_center(frame, columns[1], state);
    render_activity(frame, columns[2], state);

    if let Some(flash) = &state.flash {
        let width = area.width.min(70);
        let height = 3;
        let popup = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + 1,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(flash.text.clone())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(flash.level.label())
                        .border_style(focus_style(flash.level)),
                )
                .wrap(Wrap { trim: true }),
            popup,
        );
    }
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(6)])
        .split(area);

    let status_lines = Text::from(vec![
        line_with_label("Provider", &state.provider_status.title),
        Line::from(state.provider_status.detail.clone()),
        Line::raw(""),
        line_with_label("n8n", &state.n8n_status.title),
        Line::from(state.n8n_status.detail.clone()),
        Line::raw(format!("Shared vault: {}", state.vault_name)),
    ]);
    let status_block = Block::default()
        .borders(Borders::ALL)
        .title("Connections")
        .border_style(if state.focus == FocusPane::Workflows {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    frame.render_widget(
        Paragraph::new(status_lines)
            .wrap(Wrap { trim: true })
            .block(status_block),
        chunks[0],
    );

    let workflow_block = Block::default()
        .borders(Borders::ALL)
        .title("Workflows")
        .border_style(if state.focus == FocusPane::Workflows {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    let inner = workflow_block.inner(chunks[1]);
    frame.render_widget(workflow_block, chunks[1]);

    if state.workflows.is_empty() {
        frame.render_widget(
            Paragraph::new(state.n8n_status.detail.clone()).wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = state
        .workflows
        .iter()
        .map(|workflow| {
            ListItem::new(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Yellow)),
                Span::raw(workflow.name.clone()),
            ]))
        })
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_workflow));
    frame.render_stateful_widget(
        List::new(items).highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        inner,
        &mut list_state,
    );
}

fn render_center(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(7)])
        .split(area);

    let transcript_block = Block::default()
        .borders(Borders::ALL)
        .title("Transcript")
        .border_style(if state.focus == FocusPane::Transcript {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    let transcript_text = transcript_text(state);
    frame.render_widget(
        Paragraph::new(transcript_text)
            .block(transcript_block)
            .scroll((state.transcript_scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    let composer_block = Block::default()
        .borders(Borders::ALL)
        .title("Composer")
        .border_style(if state.focus == FocusPane::Composer {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    let inner = composer_block.inner(chunks[1]);
    frame.render_widget(composer_block, chunks[1]);

    let visible_height = inner.height.max(1) as usize;
    let all_lines = state.composer.lines();
    let start = all_lines.len().saturating_sub(visible_height);
    let visible_lines = &all_lines[start..];
    let display = if visible_lines.is_empty() {
        vec![Line::from("Type in the composer, then press F5.")]
    } else {
        visible_lines
            .iter()
            .map(|line| Line::from(line.clone()))
            .collect()
    };
    frame.render_widget(Paragraph::new(display), inner);

    if state.focus == FocusPane::Composer {
        if let Some((cursor_x, cursor_y)) = composer_cursor_position(
            all_lines,
            state.composer.row(),
            state.composer.col(),
            start,
            inner,
        ) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn render_activity(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Activity")
        .border_style(if state.focus == FocusPane::Activity {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = state
        .activity
        .iter()
        .map(|entry| {
            ListItem::new(vec![
                Line::from(vec![Span::styled(
                    entry.title.clone(),
                    focus_style(entry.level).add_modifier(Modifier::BOLD),
                )]),
                Line::from(entry.detail.clone()),
            ])
        })
        .collect();
    let mut list_state = ListState::default();
    if !items.is_empty() {
        list_state.select(Some(state.selected_activity));
    }
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().fg(Color::Black).bg(Color::White)),
        inner,
        &mut list_state,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let text = Line::from(vec![
        Span::raw(" Tab/Shift+Tab focus  "),
        Span::raw("↑↓ or j/k navigate  "),
        Span::raw("Enter newline  "),
        Span::raw("F5 ask  "),
        Span::raw("F6 run workflow  "),
        Span::raw("r refresh  "),
        Span::raw("PgUp/PgDn scroll  "),
        Span::raw("Esc leave composer  "),
        Span::raw(if state.focus == FocusPane::Composer {
            "q disabled while typing"
        } else {
            "q quit"
        }),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn transcript_text(state: &AppState) -> Text<'static> {
    let mut lines = Vec::new();
    for entry in &state.transcript {
        lines.push(Line::from(vec![Span::styled(
            format!("{}:", entry.speaker),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));
        for body_line in entry.body.lines() {
            lines.push(Line::from(format!("  {body_line}")));
        }
        if let Some(meta) = &entry.meta {
            lines.push(Line::from(vec![Span::styled(
                format!("  {meta}"),
                Style::default().fg(Color::DarkGray),
            )]));
        }
        lines.push(Line::raw(""));
    }
    Text::from(lines)
}

fn line_with_label(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(value.to_string()),
    ])
}

fn badge(text: impl Into<String>, level: StatusLevel) -> Span<'static> {
    Span::styled(
        format!(" {} ", text.into()),
        focus_style(level).add_modifier(Modifier::BOLD),
    )
}

fn focus_style(level: StatusLevel) -> Style {
    match level {
        StatusLevel::Loading => Style::default().fg(Color::Yellow),
        StatusLevel::Success => Style::default().fg(Color::Green),
        StatusLevel::Missing => Style::default().fg(Color::Blue),
        StatusLevel::Error => Style::default().fg(Color::Red),
    }
}

fn composer_cursor_position(
    all_lines: &[String],
    row: usize,
    col: usize,
    start: usize,
    inner: Rect,
) -> Option<(u16, u16)> {
    if inner.width == 0 || inner.height == 0 || row >= all_lines.len() || row < start {
        return None;
    }

    let visible_row = row - start;
    if visible_row >= inner.height as usize {
        return None;
    }

    let cursor_offset = display_width_up_to_col(&all_lines[row], col);
    let x = inner.x + cursor_offset.min(inner.width.saturating_sub(1) as usize) as u16;
    let y = inner.y + visible_row.min(inner.height.saturating_sub(1) as usize) as u16;
    Some((x, y))
}

fn display_width_up_to_col(line: &str, col: usize) -> usize {
    line.chars()
        .take(col)
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{composer_cursor_position, display_width_up_to_col};
    use ratatui::layout::Rect;

    #[test]
    fn cursor_uses_unicode_display_width() {
        assert_eq!(display_width_up_to_col("a界b", 2), 3);
    }

    #[test]
    fn cursor_clamps_to_visible_composer_width() {
        let lines = vec!["abcdef".to_string()];
        let rect = Rect {
            x: 10,
            y: 20,
            width: 4,
            height: 3,
        };

        assert_eq!(
            composer_cursor_position(&lines, 0, 6, 0, rect),
            Some((13, 20))
        );
    }

    #[test]
    fn cursor_returns_none_when_row_is_outside_visible_window() {
        let lines = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        let rect = Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 1,
        };

        assert_eq!(composer_cursor_position(&lines, 1, 2, 2, rect), None);
        assert_eq!(
            composer_cursor_position(&lines, 2, 2, 2, rect),
            Some((2, 0))
        );
    }
}
