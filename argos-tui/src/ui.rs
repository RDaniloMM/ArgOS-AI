use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use crate::commands::KNOWN_PROVIDERS;
use crate::composer::CursorPosition;
use crate::state::{AppState, FocusPane, PopupColumn, StatusLevel, SPINNER_FRAMES, VERSION};

const BG_BASE: Color = Color::Black;
const BG_PANEL: Color = Color::DarkGray;
const BG_WORKFLOWS: Color = Color::Rgb(55, 55, 65);
const BG_COMPOSER: Color = Color::Rgb(44, 47, 55);
const BG_HIGHLIGHT: Color = Color::Gray;
const C_ACCENT: Color = Color::Cyan;
const C_SUBTLE: Color = Color::Gray;
const C_TITLE: Color = Color::White;

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(10)])
        .split(frame.area());

    render_header(frame, layout[0], state);
    render_body(frame, layout[1], state);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let provider = format!("Provider: {}", state.provider_status.level.label());
    let n8n = format!("n8n: {}", state.n8n_status.level.label());
    let busy_text = if state.is_submitting_prompt {
        format!("{} Agent thinking…", SPINNER_FRAMES[state.spinner_frame])
    } else if state.is_running_workflow {
        "Workflow running".into()
    } else if state.is_loading_snapshot {
        "Refreshing".into()
    } else {
        "Idle".into()
    };
    let busy_level = if busy_text == "Idle" {
        StatusLevel::Success
    } else {
        StatusLevel::Loading
    };

    let line = Line::from(vec![
        Span::styled(
            " ArgOS ",
            Style::default()
                .fg(Color::Black)
                .bg(C_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        badge(provider, state.provider_status.level),
        Span::raw(" "),
        badge(n8n, state.n8n_status.level),
        Span::raw(" "),
        spinner_badge(&busy_text, busy_level),
    ]);

    frame.render_widget(
        Paragraph::new(line).block(Block::default().style(Style::default().bg(BG_BASE))),
        area,
    );
}

fn render_body(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let right_visible = state.activity_visible;
    let left_visible = state.sidebar_visible;

    let constraints: Vec<Constraint> = match (left_visible, right_visible) {
        (true, true) => vec![
            Constraint::Percentage(25),
            Constraint::Length(1),
            Constraint::Percentage(44),
            Constraint::Length(1),
            Constraint::Percentage(30),
        ],
        (true, false) => vec![
            Constraint::Percentage(30),
            Constraint::Length(1),
            Constraint::Percentage(70),
        ],
        (false, true) => vec![
            Constraint::Percentage(60),
            Constraint::Length(1),
            Constraint::Percentage(40),
        ],
        (false, false) => vec![Constraint::Percentage(100)],
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    frame.render_widget(
        Paragraph::new("").block(Block::default().style(Style::default().bg(BG_BASE))),
        area,
    );

    let mut col_idx = 0;
    if left_visible {
        render_sidebar(frame, columns[col_idx], state);
        col_idx += 2; // skip panel + gap
    }
    // center column
    if left_visible {
        render_center(frame, columns[2], state);
    } else if right_visible {
        render_center(frame, columns[col_idx], state);
        col_idx += 2;
    } else {
        render_center(frame, columns[0], state);
    }

    if right_visible {
        if left_visible {
            render_info_panel(frame, columns[4], state);
        } else {
            render_info_panel(frame, columns[col_idx], state);
        }
    }

    if state.provider_popup.visible {
        render_provider_popup(frame, area, state);
    }

    if let Some(flash) = &state.flash {
        let width = (flash.text.len() + 4).min(area.width as usize - 2) as u16;
        let height = 1;
        let toast = Rect {
            x: area.x + area.width.saturating_sub(width + 2),
            y: area.y + area.height.saturating_sub(2),
            width,
            height,
        };
        let bg = match flash.level {
            StatusLevel::Success => Color::DarkGray,
            StatusLevel::Error => Color::Red,
            StatusLevel::Loading => Color::Yellow,
            StatusLevel::Missing => Color::Blue,
        };
        frame.render_widget(Clear, toast);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", flash.level.label()),
                    Style::default()
                        .fg(Color::Black)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    flash.text.clone(),
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ),
            ])),
            toast,
        );
    }
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(area);

    fill_area(frame, chunks[0], BG_PANEL);
    fill_area(frame, chunks[1], BG_PANEL);

    let status_lines = vec![
        Line::from(vec![Span::styled(
            "Connections",
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        line_with_label("Provider", &state.provider_status.title),
        Line::from(state.provider_status.detail.clone()),
        Line::raw(""),
        line_with_label("n8n", &state.n8n_status.title),
        Line::from(state.n8n_status.detail.clone()),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(status_lines)).wrap(Wrap { trim: true }),
        chunks[0],
    );

    let is_focused = state.focus == FocusPane::Workflows;
    let title_color = if is_focused { C_ACCENT } else { C_TITLE };
    let title = vec![Span::styled(
        "Workflows",
        Style::default()
            .fg(title_color)
            .add_modifier(Modifier::BOLD),
    )];
    fill_area(
        frame,
        Rect {
            height: 1,
            ..chunks[1]
        },
        if is_focused { BG_HIGHLIGHT } else { BG_PANEL },
    );
    frame.render_widget(
        Paragraph::new(Line::from(title)),
        Rect {
            height: 1,
            ..chunks[1]
        },
    );

    let list_area = Rect {
        y: chunks[1].y + 1,
        height: chunks[1].height.saturating_sub(1),
        ..chunks[1]
    };
    fill_area(frame, list_area, BG_WORKFLOWS);

    if state.workflows.is_empty() {
        frame.render_widget(
            Paragraph::new(state.n8n_status.detail.clone()).wrap(Wrap { trim: true }),
            list_area,
        );
        return;
    }

    let items: Vec<ListItem> = state
        .workflows
        .iter()
        .map(|wf| {
            ListItem::new(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Yellow)),
                Span::raw(wf.name.clone()),
            ]))
        })
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_workflow));
    frame.render_stateful_widget(
        List::new(items).highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(C_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        list_area,
        &mut list_state,
    );
}

fn render_center(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(5)])
        .split(area);

    let tf = state.focus == FocusPane::Transcript;
    let transcript_title = Line::from(vec![Span::styled(
        "Transcript",
        Style::default()
            .fg(if tf { C_ACCENT } else { C_TITLE })
            .add_modifier(Modifier::BOLD),
    )]);
    let t_title_rect = Rect {
        height: 1,
        ..chunks[0]
    };
    fill_area(
        frame,
        t_title_rect,
        if tf { BG_HIGHLIGHT } else { BG_PANEL },
    );
    frame.render_widget(Paragraph::new(transcript_title), t_title_rect);

    let transcript_body = Rect {
        y: chunks[0].y + 1,
        height: chunks[0].height.saturating_sub(1),
        ..chunks[0]
    };
    frame.render_widget(
        Paragraph::new(transcript_text(state))
            .scroll((state.transcript_scroll, 0))
            .wrap(Wrap { trim: false }),
        transcript_body,
    );

    let has_suggestions = !state.suggestions.is_empty();
    let composer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_suggestions {
            vec![Constraint::Length(1), Constraint::Min(5)]
        } else {
            vec![Constraint::Min(6)]
        })
        .split(chunks[1]);

    let (suggestion_area, composer_area) = if has_suggestions {
        (Some(composer_chunks[0]), composer_chunks[1])
    } else {
        (None, composer_chunks[0])
    };

    if let Some(area) = suggestion_area {
        render_suggestions(frame, area, state);
    }

    let is_cf = state.focus == FocusPane::Composer;
    let composer_title = Line::from(vec![
        Span::styled(
            "Composer",
            Style::default()
                .fg(if is_cf { C_ACCENT } else { C_TITLE })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(composer_status(state), Style::default().fg(C_SUBTLE)),
    ]);
    let is_focused = state.focus == FocusPane::Composer;
    let c_title_rect = Rect {
        height: 1,
        ..composer_area
    };
    fill_area(
        frame,
        c_title_rect,
        if is_focused { BG_HIGHLIGHT } else { BG_PANEL },
    );
    frame.render_widget(Paragraph::new(composer_title), c_title_rect);

    let inner = Rect {
        y: composer_area.y + 1,
        height: composer_area.height.saturating_sub(1),
        ..composer_area
    };
    let visible_height = inner.height.max(1) as usize;
    let all_lines = state.composer.lines();
    let start = all_lines.len().saturating_sub(visible_height);
    let visible_lines = &all_lines[start..];
    let display = if visible_lines.is_empty() {
        vec![Line::from("Type in the composer, then press Enter.")]
    } else {
        composer_lines(visible_lines, start, state.composer.selection())
    };
    fill_area(frame, inner, BG_COMPOSER);
    frame.render_widget(Paragraph::new(display).wrap(Wrap { trim: true }), inner);

    if is_focused {
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

fn render_info_panel(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(4)])
        .split(area);

    fill_area(frame, chunks[0], BG_PANEL);
    fill_area(frame, chunks[1], BG_PANEL);

    let is_focused = state.focus == FocusPane::Activity;
    let title_color = if is_focused { C_ACCENT } else { C_TITLE };
    let title = Line::from(vec![Span::styled(
        "Info",
        Style::default()
            .fg(title_color)
            .add_modifier(Modifier::BOLD),
    )]);
    frame.render_widget(
        Paragraph::new(title),
        Rect {
            height: 1,
            ..chunks[0]
        },
    );

    let info_body = Rect {
        y: chunks[0].y + 1,
        height: chunks[0].height - 1,
        ..chunks[0]
    };
    let mut lines: Vec<Line> = Vec::new();

    if let Some(ref config) = state.current_config {
        let backend = &config.provider.backend;
        let model = &config.provider.model;
        let max_ctx: u64 = if model.contains("claude") {
            200_000
        } else {
            128_000
        };
        let pct = if max_ctx > 0 {
            (state.session_tokens as f64 / max_ctx as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        let pct_color = if pct > 80.0 {
            Color::Red
        } else if pct > 50.0 {
            Color::Yellow
        } else {
            Color::Green
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{backend} · {model}"),
            Style::default().fg(C_SUBTLE),
        )]));
        lines.push(Line::from(vec![
            Span::raw(format!(
                "Tokens: {} / {}  ",
                stkn(state.session_tokens),
                stkn(max_ctx)
            )),
            Span::styled(format!("{pct:.0}%"), Style::default().fg(pct_color)),
        ]));
        lines.push(Line::raw(format!("Cost: ${:.6}", state.session_cost)));
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(vec![Span::styled(
        format!("{}  v{VERSION}", state.cwd.display()),
        Style::default().fg(C_SUBTLE),
    )]));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        info_body,
    );

    // Activity compact
    let act_title = vec![Span::styled(
        "Activity",
        Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
    )];
    frame.render_widget(
        Paragraph::new(Line::from(act_title)),
        Rect {
            height: 1,
            ..chunks[1]
        },
    );

    let act_body = Rect {
        y: chunks[1].y + 1,
        height: chunks[1].height.saturating_sub(1),
        ..chunks[1]
    };
    let items: Vec<ListItem> = state
        .activity
        .iter()
        .rev()
        .take(8)
        .map(|entry| {
            ListItem::new(Line::from(vec![Span::styled(
                format!("{} {}", focus_icon(entry.level), entry.title),
                focus_style(entry.level),
            )]))
        })
        .collect();
    let mut list_state = ListState::default();
    frame.render_stateful_widget(List::new(items), act_body, &mut list_state);
}

fn focus_icon(level: StatusLevel) -> &'static str {
    match level {
        StatusLevel::Success => "✓",
        StatusLevel::Error => "✗",
        StatusLevel::Loading => "○",
        StatusLevel::Missing => "?",
    }
}

fn stkn(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{tokens}")
    }
}

fn transcript_text(state: &AppState) -> Text<'static> {
    let mut lines = Vec::new();
    for entry in &state.transcript {
        lines.push(Line::from(vec![Span::styled(
            format!("{}:", entry.speaker),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
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

fn fill_area(frame: &mut Frame<'_>, area: Rect, color: Color) {
    frame.render_widget(Paragraph::new("").style(Style::default().bg(color)), area);
}

fn line_with_label(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
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

fn spinner_badge(text: &str, level: StatusLevel) -> Span<'static> {
    Span::styled(
        format!(" {} ", text),
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

fn composer_status(state: &AppState) -> String {
    state.composer_status()
}

fn display_width_up_to_col(line: &str, col: usize) -> usize {
    line.chars()
        .take(col)
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

fn render_suggestions(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let text: String = state
        .suggestions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if i == 0 {
                format!(" → {s}")
            } else {
                format!("  │  {s}")
            }
        })
        .collect::<Vec<_>>()
        .join("");

    let spans = if text.is_empty() {
        vec![Span::raw("")]
    } else {
        vec![Span::styled(
            text,
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        )]
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn composer_lines(
    visible_lines: &[String],
    start: usize,
    selection: Option<(CursorPosition, CursorPosition)>,
) -> Vec<Line<'static>> {
    visible_lines
        .iter()
        .enumerate()
        .map(|(offset, line)| composer_line(line, start + offset, selection))
        .collect()
}

fn composer_line(
    line: &str,
    row: usize,
    selection: Option<(CursorPosition, CursorPosition)>,
) -> Line<'static> {
    let Some((start, end)) = selection else {
        return Line::from(line.to_string());
    };
    if row < start.row || row > end.row {
        return Line::from(line.to_string());
    }
    let line_len = line.chars().count();
    let selected_start = if row == start.row { start.col } else { 0 }.min(line_len);
    let selected_end = if row == end.row { end.col } else { line_len }.min(line_len);
    if selected_start >= selected_end {
        return Line::from(line.to_string());
    }

    let before: String = line.chars().take(selected_start).collect();
    let selected: String = line
        .chars()
        .skip(selected_start)
        .take(selected_end - selected_start)
        .collect();
    let after: String = line.chars().skip(selected_end).collect();

    Line::from(vec![
        Span::raw(before),
        Span::styled(
            selected,
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(after),
    ])
}

fn render_provider_popup(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let popup_width = 70u16;
    let popup_height = 14u16;
    let x = area.x + area.width.saturating_sub(popup_width) / 2;
    let y = area.y + area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect {
        x,
        y,
        width: popup_width.min(area.width),
        height: popup_height.min(area.height),
    };

    frame.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Select Provider  ← → columns  ↑↓ navigate  Enter select  Esc close")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let provider_items: Vec<ListItem> = KNOWN_PROVIDERS
        .iter()
        .enumerate()
        .map(|(i, kp)| {
            let content = format!(
                " {}  {:<20}",
                if i == state.provider_popup.selected_provider
                    && state.provider_popup.column == PopupColumn::Provider
                {
                    "▶"
                } else {
                    " "
                },
                kp.backend
            );
            ListItem::new(content)
        })
        .collect();
    let mut provider_list = ListState::default();
    provider_list.select(Some(state.provider_popup.selected_provider));
    frame.render_stateful_widget(
        List::new(provider_items)
            .block(Block::default().borders(Borders::ALL).title("Provider"))
            .highlight_style(if state.provider_popup.column == PopupColumn::Provider {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            }),
        columns[0],
        &mut provider_list,
    );

    let model_items: Vec<ListItem> = KNOWN_PROVIDERS
        .get(state.provider_popup.selected_provider)
        .map(|kp| {
            let dynamic = state.dynamic_models.get(kp.backend);
            let models: Vec<String> = match dynamic {
                Some(list) if !list.is_empty() => list.iter().map(|m| m.id.clone()).collect(),
                _ => kp.models.iter().map(|m| m.to_string()).collect(),
            };
            models
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let marker = if i == state.provider_popup.selected_model
                        && state.provider_popup.column == PopupColumn::Model
                    {
                        "▶"
                    } else {
                        " "
                    };
                    ListItem::new(format!(" {marker}  {m}"))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut model_list = ListState::default();
    model_list.select(Some(state.provider_popup.selected_model));
    frame.render_stateful_widget(
        List::new(model_items)
            .block(Block::default().borders(Borders::ALL).title("Model"))
            .highlight_style(if state.provider_popup.column == PopupColumn::Model {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            }),
        columns[1],
        &mut model_list,
    );
}

#[cfg(test)]
mod tests {
    use super::{composer_cursor_position, composer_line, display_width_up_to_col};
    use crate::composer::CursorPosition;
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

    #[test]
    fn composer_line_highlights_selected_segment() {
        let line = composer_line(
            "abcdef",
            0,
            Some((
                CursorPosition { row: 0, col: 2 },
                CursorPosition { row: 0, col: 4 },
            )),
        );
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].content.as_ref(), "ab");
        assert_eq!(line.spans[1].content.as_ref(), "cd");
        assert_eq!(line.spans[2].content.as_ref(), "ef");
    }
}
