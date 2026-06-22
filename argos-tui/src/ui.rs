use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use crate::commands;
use crate::composer::CursorPosition;
use crate::state::{AppState, FocusPane, PopupColumn, StatusLevel, SPINNER_FRAMES, VERSION};

const BG_BASE: Color = Color::Rgb(8, 11, 18);
const BG_PANEL: Color = Color::Rgb(16, 22, 34);
const BG_PANEL_ALT: Color = Color::Rgb(20, 28, 42);
const BG_WORKFLOWS: Color = Color::Rgb(12, 18, 30);
const BG_COMPOSER: Color = Color::Rgb(13, 20, 32);
const BG_HIGHLIGHT: Color = Color::Rgb(36, 53, 76);
const BG_POPUP: Color = Color::Rgb(11, 16, 28);
const C_ACCENT: Color = Color::Rgb(92, 214, 255);
const C_ACCENT_2: Color = Color::Rgb(178, 125, 255);
const C_SUBTLE: Color = Color::Rgb(142, 158, 181);
const C_BORDER: Color = Color::Rgb(58, 77, 107);
const C_TEXT: Color = Color::Rgb(218, 226, 241);
const C_TITLE: Color = Color::Rgb(245, 249, 255);
const ARGOS_MASCOT: &str = "⌁(•ᴗ•)>_";

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_header(frame, layout[0], state);
    render_body(frame, layout[1], state);
    render_footer(frame, layout[2], state);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let provider = format!("Provider: {}", state.provider_status.level.label());
    let workflows = format!(
        "Workflows: {}",
        workflow_status_label(state.n8n_status.level)
    );
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
            format!(" {ARGOS_MASCOT} "),
            Style::default().fg(C_ACCENT_2).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ArgOS AI ",
            Style::default()
                .fg(BG_BASE)
                .bg(C_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        badge(provider, state.provider_status.level),
        Span::raw(" "),
        badge(workflows, state.n8n_status.level),
        Span::raw(" "),
        spinner_badge(&busy_text, busy_level),
    ]);

    frame.render_widget(
        Paragraph::new(line).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(BG_BASE).fg(C_TEXT)),
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let left = if state.sidebar_visible {
        "F8 hide Connections"
    } else {
        "F8 show Connections"
    };
    let right = if state.activity_visible {
        "F7 hide Info"
    } else {
        "F7 show Info"
    };
    let line = Line::from(vec![
        Span::styled(" Ctrl+P Commands ", Style::default().fg(C_ACCENT)),
        Span::styled(" • ", Style::default().fg(C_BORDER)),
        Span::styled(" F2 Provider/model ", Style::default().fg(C_ACCENT_2)),
        Span::styled(" • ", Style::default().fg(C_BORDER)),
        Span::styled(format!(" {left} "), Style::default().fg(C_SUBTLE)),
        Span::styled(" • ", Style::default().fg(C_BORDER)),
        Span::styled(format!(" {right} "), Style::default().fg(C_SUBTLE)),
        Span::styled(
            " • Enter send • Shift+Enter newline ",
            Style::default().fg(C_SUBTLE),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(BG_BASE).fg(C_TEXT)),
        area,
    );
}

fn render_body(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let right_visible = state.activity_visible;
    let left_visible = state.sidebar_visible;

    let constraints: Vec<Constraint> = match (left_visible, right_visible) {
        (true, true) => vec![
            Constraint::Percentage(24),
            Constraint::Length(1),
            Constraint::Min(20),
            Constraint::Length(1),
            Constraint::Length(right_panel_width(area.width)),
        ],
        (true, false) => vec![
            Constraint::Percentage(30),
            Constraint::Length(1),
            Constraint::Percentage(70),
        ],
        (false, true) => vec![
            Constraint::Min(20),
            Constraint::Length(1),
            Constraint::Length(right_panel_width(area.width)),
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

    if state.command_palette.visible {
        render_command_palette(frame, area, state);
    }

    // Flash toasts only in debug/dev builds — release builds suppress them
    // since the user already sees transcript updates without overlay popups.
    if cfg!(debug_assertions) {
        if let Some(flash) = &state.flash {
            let label = flash.level.label();
            let text = format!(" {} {}", label, flash.text);
            let max_text_w = area.width.saturating_sub(4).min(72).max(20) as usize;
            let chars_per_line = max_text_w.saturating_sub(4);
            let lines = if text.len() <= chars_per_line {
                1
            } else {
                let wrapped = text.len().div_ceil(chars_per_line.max(1));
                wrapped.min(8)
            };
            let pad = 1u16;
            let box_w = (text.len() + 4).min(max_text_w) as u16;
            let inner_h = lines as u16;
            let box_h = inner_h + pad * 2;
            let toast = Rect {
                x: area.x + area.width.saturating_sub(box_w + 1),
                y: area.y + area.height.saturating_sub(box_h + 1),
                width: box_w,
                height: box_h,
            };
            let bg = match flash.level {
                StatusLevel::Success => Color::DarkGray,
                StatusLevel::Error => Color::Red,
                StatusLevel::Loading => Color::Yellow,
                StatusLevel::Missing => Color::Blue,
            };
            frame.render_widget(Clear, toast);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(bg))
                    .style(Style::default().bg(Color::Black)),
                toast,
            );
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    text,
                    Style::default().fg(Color::White).bg(Color::Black),
                )))
                .wrap(Wrap { trim: true }),
                Rect {
                    x: toast.x + pad,
                    y: toast.y + pad,
                    width: box_w.saturating_sub(pad * 2),
                    height: inner_h,
                },
            );
        }
    }
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(area);

    let connections_block = panel_block("Connections", false)
        .title_style(Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD));
    let connections_inner = connections_block.inner(chunks[0]);
    frame.render_widget(connections_block, chunks[0]);
    let status_lines = vec![
        line_with_label("Provider", &state.provider_status.title),
        Line::from(state.provider_status.detail.clone()),
        Line::raw(""),
        line_with_label("n8n", &state.n8n_status.title),
        Line::from(state.n8n_status.detail.clone()),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(status_lines))
            .style(Style::default().fg(C_TEXT).bg(BG_PANEL))
            .wrap(Wrap { trim: true }),
        connections_inner,
    );

    let is_focused = state.focus == FocusPane::Workflows;
    let workflows_block = panel_block("Workflows", is_focused);
    let list_area = workflows_block.inner(chunks[1]);
    frame.render_widget(workflows_block, chunks[1]);
    fill_area(frame, list_area, BG_WORKFLOWS);

    if state.workflows.is_empty() {
        frame.render_widget(
            Paragraph::new(workflow_empty_text(state)).wrap(Wrap { trim: true }),
            list_area,
        );
        return;
    }

    let items: Vec<ListItem> = state
        .workflows
        .iter()
        .map(|wf| {
            ListItem::new(Line::from(vec![
                Span::styled("• ", Style::default().fg(C_ACCENT_2)),
                Span::styled(wf.name.clone(), Style::default().fg(C_TEXT)),
            ]))
        })
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_workflow));
    frame.render_stateful_widget(
        List::new(items).highlight_style(
            Style::default()
                .fg(BG_BASE)
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
    let transcript_block = panel_block(transcript_panel_title(), tf);
    let transcript_body = transcript_block.inner(chunks[0]);
    frame.render_widget(transcript_block, chunks[0]);

    let content_lines = state.transcript_line_count() as u16;
    let view_height = transcript_body.height.max(1);
    let max_scroll = content_lines.saturating_sub(view_height);
    let scroll = state.transcript_scroll.min(max_scroll);
    frame.render_widget(
        Paragraph::new(transcript_text(state))
            .style(Style::default().fg(C_TEXT).bg(BG_PANEL_ALT))
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        transcript_body,
    );

    let composer_area = chunks[1];

    let is_focused = state.focus == FocusPane::Composer;
    let composer_title = format!("Composer {}", composer_status(state));
    let composer_block = panel_block(Line::from(composer_title), is_focused);
    let inner = composer_block.inner(composer_area);
    frame.render_widget(composer_block, composer_area);
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

    if !state.suggestions.is_empty() {
        render_suggestions(frame, area, composer_area, state);
    }
}

fn render_info_panel(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let is_focused = state.focus == FocusPane::Activity;
    let info_block = panel_block("Info", is_focused);
    let info_body = info_block.inner(area);
    frame.render_widget(info_block, area);
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
        lines.push(line_with_label("Provider", backend));
        lines.push(Line::from(Span::styled(
            model.to_string(),
            Style::default().fg(C_SUBTLE),
        )));
        lines.push(Line::from(vec![
            Span::raw(format!(
                "Tk: {}/{} ",
                stkn(state.session_tokens),
                stkn(max_ctx)
            )),
            Span::styled(format!("{pct:.0}%"), Style::default().fg(pct_color)),
        ]));
        lines.push(Line::raw(format!("Cost: ${:.6}", state.session_cost)));
        lines.push(Line::raw(""));
    }

    lines.push(line_with_label(
        "Workflows",
        workflow_status_label(state.n8n_status.level),
    ));
    lines.push(Line::from(Span::styled(
        state.n8n_status.detail.clone(),
        Style::default().fg(C_SUBTLE),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("v{VERSION}"),
        Style::default().fg(C_SUBTLE),
    )));
    lines.push(Line::from(Span::styled(
        state.cwd.display().to_string(),
        Style::default().fg(C_SUBTLE),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(C_TEXT).bg(BG_PANEL))
            .wrap(Wrap { trim: true }),
        info_body,
    );
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
        let style = transcript_style(&entry.speaker);
        for body_line in entry.body.lines() {
            lines.push(transcript_body_line(body_line, style));
        }
        if let Some(meta) = &entry.meta {
            lines.push(Line::from(vec![Span::styled(
                format!("{}{}", style.indent, meta),
                Style::default().fg(C_SUBTLE),
            )]));
        }
        lines.push(Line::raw(""));
    }
    Text::from(lines)
}

fn transcript_panel_title() -> &'static str {
    ""
}

#[derive(Debug, Clone, Copy)]
struct TranscriptStyle {
    glyph: &'static str,
    indent: &'static str,
    glyph_color: Color,
    body_color: Color,
}

fn transcript_style(speaker: &str) -> TranscriptStyle {
    match speaker {
        "You" => TranscriptStyle {
            glyph: "›",
            indent: "      ",
            glyph_color: C_ACCENT,
            body_color: C_TEXT,
        },
        "ArgOS" => TranscriptStyle {
            glyph: "⌁",
            indent: "",
            glyph_color: C_ACCENT_2,
            body_color: C_TEXT,
        },
        _ => TranscriptStyle {
            glyph: "·",
            indent: "  ",
            glyph_color: C_SUBTLE,
            body_color: C_SUBTLE,
        },
    }
}

fn transcript_body_line(body_line: &str, style: TranscriptStyle) -> Line<'static> {
    Line::from(vec![
        Span::raw(style.indent),
        Span::styled(
            format!("{} ", style.glyph),
            Style::default()
                .fg(style.glyph_color)
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(body_line.to_string(), Style::default().fg(style.body_color)),
    ])
}

fn workflow_status_label(level: StatusLevel) -> &'static str {
    match level {
        StatusLevel::Missing => "Optional",
        _ => level.label(),
    }
}

fn workflow_empty_text(state: &AppState) -> String {
    if state.n8n_status.level == StatusLevel::Missing {
        "Workflow automation is optional. Configure n8n only if you want workflow actions."
            .to_string()
    } else {
        state.n8n_status.detail.clone()
    }
}

fn right_panel_width(total_width: u16) -> u16 {
    (total_width / 4).clamp(18, 30)
}

fn fill_area(frame: &mut Frame<'_>, area: Rect, color: Color) {
    frame.render_widget(Paragraph::new("").style(Style::default().bg(color)), area);
}

fn panel_block<'a>(title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    let border = if focused { C_ACCENT } else { C_BORDER };
    let bg = if focused { BG_HIGHLIGHT } else { BG_PANEL };
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(
            Style::default()
                .fg(if focused { C_ACCENT } else { C_TITLE })
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(border))
        .style(Style::default().fg(C_TEXT).bg(BG_PANEL))
}

fn line_with_label(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), Style::default().fg(C_TEXT)),
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
        StatusLevel::Loading => Style::default().fg(Color::Rgb(255, 202, 87)),
        StatusLevel::Success => Style::default().fg(Color::Rgb(108, 240, 166)),
        StatusLevel::Missing => Style::default().fg(Color::Rgb(125, 168, 255)),
        StatusLevel::Error => Style::default().fg(Color::Rgb(255, 105, 130)),
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

fn render_suggestions(
    frame: &mut Frame<'_>,
    center_area: Rect,
    composer_area: Rect,
    state: &AppState,
) {
    let popup_area = suggestions_popup_area(center_area, composer_area, state.suggestions.len());
    if popup_area.is_empty() {
        return;
    }

    frame.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Slash commands ")
        .title_style(Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(C_ACCENT))
        .style(Style::default().fg(C_TEXT).bg(BG_POPUP));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let visible_rows = inner.height as usize;
    let items: Vec<ListItem> = state
        .suggestions
        .iter()
        .take(visible_rows)
        .enumerate()
        .map(|(index, suggestion)| {
            let marker = if index == 0 { "→" } else { " " };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {marker} "),
                    Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(suggestion.clone(), Style::default().fg(C_TEXT)),
            ]))
        })
        .collect();

    frame.render_widget(
        List::new(items).style(Style::default().fg(C_TEXT).bg(BG_POPUP)),
        inner,
    );
}

fn suggestions_popup_area(center_area: Rect, composer_area: Rect, suggestion_count: usize) -> Rect {
    if suggestion_count == 0 || center_area.is_empty() || composer_area.is_empty() {
        return Rect::default();
    }

    let available_above = composer_area.y.saturating_sub(center_area.y);
    let desired_height = (suggestion_count as u16 + 2).clamp(3, 8);
    let height = desired_height.min(available_above).min(center_area.height);
    if height < 3 {
        return Rect::default();
    }

    let width = composer_area.width.min(72).min(center_area.width);
    if width == 0 {
        return Rect::default();
    }
    Rect {
        x: composer_area.x,
        y: composer_area.y.saturating_sub(height).max(center_area.y),
        width,
        height,
    }
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
                .fg(BG_BASE)
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
        .title("Provider & model  ← → columns  ↑↓ navigate  Enter select  Del remove  Esc close")
        .title_style(Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(C_ACCENT_2))
        .style(Style::default().fg(C_TEXT).bg(BG_POPUP));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let mut provider_items: Vec<ListItem> = state
        .configured_providers()
        .iter()
        .enumerate()
        .map(|(i, provider)| {
            let content = format!(
                " {}  {:<20}",
                if i == state.provider_popup.selected_provider
                    && state.provider_popup.column == PopupColumn::Provider
                {
                    "▶"
                } else {
                    " "
                },
                provider.backend
            );
            ListItem::new(Span::styled(content, Style::default().fg(C_TEXT)))
        })
        .collect();
    let add_index = state.provider_popup_add_index();
    let add_marker = if state.provider_popup.selected_provider == add_index
        && state.provider_popup.column == PopupColumn::Provider
    {
        "▶"
    } else {
        " "
    };
    provider_items.push(ListItem::new(Span::styled(
        format!(" {add_marker}  + Add provider"),
        Style::default().fg(C_ACCENT),
    )));
    let mut provider_list = ListState::default();
    provider_list.select(Some(state.provider_popup.selected_provider));
    frame.render_stateful_widget(
        List::new(provider_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Provider — Del to remove ")
                    .border_style(Style::default().fg(C_BORDER))
                    .style(Style::default().bg(BG_POPUP)),
            )
            .highlight_style(if state.provider_popup.column == PopupColumn::Provider {
                Style::default()
                    .fg(BG_BASE)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_SUBTLE).bg(BG_HIGHLIGHT)
            }),
        columns[0],
        &mut provider_list,
    );

    let model_items: Vec<ListItem> = if state.provider_popup_is_add_selected() {
        vec![
            ListItem::new(Span::styled(
                " Press Enter to insert /provider-add",
                Style::default().fg(C_ACCENT),
            )),
            ListItem::new(Span::styled(
                " OpenAI: API key or /provider-add-openai-oauth",
                Style::default().fg(C_TEXT),
            )),
            ListItem::new(Span::styled(
                " Custom endpoints: /provider-add-custom",
                Style::default().fg(C_SUBTLE),
            )),
        ]
    } else if let Some(provider) = state.selected_configured_provider() {
        let fetched = state.dynamic_models.get(&provider.backend);
        let models: Vec<String> = match fetched {
            Some(list) if !list.is_empty() => list.iter().map(|m| m.id.clone()).collect(),
            _ => vec![format!(
                "{} (configured, availability unverified)",
                provider.model
            )],
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
                ListItem::new(Span::styled(
                    format!(" {marker}  {m}"),
                    Style::default().fg(C_TEXT),
                ))
            })
            .collect()
    } else {
        vec![ListItem::new(Span::styled(
            " No providers configured. Select + Add provider.",
            Style::default().fg(C_SUBTLE),
        ))]
    };
    let mut model_list = ListState::default();
    model_list.select(Some(state.provider_popup.selected_model));
    frame.render_stateful_widget(
        List::new(model_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Model")
                    .border_style(Style::default().fg(C_BORDER))
                    .style(Style::default().bg(BG_POPUP)),
            )
            .highlight_style(if state.provider_popup.column == PopupColumn::Model {
                Style::default()
                    .fg(BG_BASE)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_SUBTLE).bg(BG_HIGHLIGHT)
            }),
        columns[1],
        &mut model_list,
    );
}

fn render_command_palette(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let popup_width = 72u16
        .min(area.width.saturating_sub(4).max(20))
        .min(area.width);
    let popup_height = 18u16
        .min(area.height.saturating_sub(4).max(8))
        .min(area.height);
    let popup_area = Rect {
        x: area.x + area.width.saturating_sub(popup_width) / 2,
        y: area.y + area.height.saturating_sub(popup_height) / 3,
        width: popup_width,
        height: popup_height,
    };

    frame.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {ARGOS_MASCOT} Command palette "))
        .title_style(Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(C_ACCENT))
        .style(Style::default().fg(C_TEXT).bg(BG_POPUP));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(4)])
        .split(inner);
    let hint = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(C_ACCENT)),
        Span::raw(" select  "),
        Span::styled("Enter", Style::default().fg(C_ACCENT)),
        Span::raw(" insert command  "),
        Span::styled("Esc", Style::default().fg(C_ACCENT)),
        Span::raw(" close"),
    ]);
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(C_SUBTLE).bg(BG_POPUP)),
        chunks[0],
    );

    let items: Vec<ListItem> = commands::command_definitions()
        .iter()
        .map(|command| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<28}", command.signature),
                    Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(command.description, Style::default().fg(C_SUBTLE)),
            ]))
        })
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.command_palette.selected));
    frame.render_stateful_widget(
        List::new(items)
            .style(Style::default().fg(C_TEXT).bg(BG_POPUP))
            .highlight_style(
                Style::default()
                    .fg(BG_BASE)
                    .bg(C_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
        chunks[1],
        &mut list_state,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        composer_cursor_position, composer_line, display_width_up_to_col, suggestions_popup_area,
        transcript_panel_title, transcript_text,
    };
    use crate::composer::CursorPosition;
    use crate::state::AppState;
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

    #[test]
    fn slash_suggestion_popup_sits_above_composer() {
        let center = Rect {
            x: 10,
            y: 5,
            width: 80,
            height: 30,
        };
        let composer = Rect {
            x: 10,
            y: 30,
            width: 80,
            height: 5,
        };

        let popup = suggestions_popup_area(center, composer, 6);

        assert_eq!(popup.x, composer.x);
        assert_eq!(popup.y + popup.height, composer.y);
        assert!(popup.height >= 3);
        assert!(popup.width <= composer.width);
    }

    #[test]
    fn transcript_hides_explicit_speaker_labels() {
        let mut state = AppState::new();
        state.push_transcript("You", "hello", None);
        state.push_transcript("ArgOS", "hi", None);

        let rendered = transcript_text(&state)
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join(" ");

        assert!(!rendered.contains("You:"));
        assert!(!rendered.contains("ArgOS:"));
        assert!(rendered.contains("hello"));
        assert!(rendered.contains("hi"));
    }

    #[test]
    fn transcript_panel_title_is_blank() {
        assert_eq!(transcript_panel_title(), "");
    }
}
