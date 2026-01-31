use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, LogEntry, LogKind};

pub(crate) fn draw_ui(frame: &mut Frame, app: &mut App) {
    let size = frame.size();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(3), Constraint::Length(1)])
        .split(size);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(100)])
        .split(vertical[0]);

    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main[0]);

    let scene_block = Block::default().borders(Borders::ALL).title("Scene");
    let scene_text = if app.scene_ascii.trim().is_empty() {
        "Awaiting scene..."
    } else {
        app.scene_ascii.as_str()
    };
    let scene_inner = scene_block.inner(panels[0]);
    let centered_scene = build_centered_scene_text(scene_text, scene_inner);
    let scene_widget = Paragraph::new(centered_scene).block(scene_block);
    frame.render_widget(scene_widget, panels[0]);

    let (log_text, line_count) = build_log_text(&app.log);
    let log_block = Block::default().borders(Borders::ALL).title("Story");
    let max_scroll = line_count.saturating_sub(panels[1].height as usize);
    app.scroll = app.scroll.min(max_scroll as u16);

    let log_widget = Paragraph::new(log_text)
        .block(log_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    frame.render_widget(log_widget, panels[1]);

    let input_block = Block::default().borders(Borders::ALL).title("Input");
    let input_widget = Paragraph::new(app.input.as_str())
        .block(input_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(input_widget, vertical[1]);

    let help_text =
        "Enter send | Up/Down scroll | /new | /quit | Ctrl+C quit | /help for commands";
    let help_widget = Paragraph::new(help_text);
    frame.render_widget(help_widget, vertical[2]);

    let cursor_x = vertical[1].x + 1 + app.input.chars().count() as u16;
    let cursor_y = vertical[1].y + 1;
    frame.set_cursor(cursor_x, cursor_y);
}

fn build_log_text(entries: &[LogEntry]) -> (Text<'static>, usize) {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for entry in entries {
        let (prefix, style) = match entry.kind {
            LogKind::User => {
                let label = entry.speaker.as_deref().unwrap_or("You");
                (
                    format!("{label}: "),
                    Style::default().fg(Color::Yellow),
                )
            }
            LogKind::Assistant => {
                let label = entry.speaker.as_deref().unwrap_or("Narrator");
                let color = if is_narrator_label(label) {
                    Color::Green
                } else {
                    Color::Cyan
                };
                (format!("{label}: "), Style::default().fg(color))
            }
            LogKind::System => ("".to_string(), Style::default().fg(Color::Blue)),
            LogKind::Error => ("Error: ".to_string(), Style::default().fg(Color::Red)),
        };
        let indent = " ".repeat(prefix.len());
        let mut first = true;
        for line in entry.text.lines() {
            if first {
                lines.push(Line::from(vec![
                    Span::styled(prefix.clone(), style),
                    Span::raw(line.to_string()),
                ]));
                first = false;
            } else {
                lines.push(Line::from(vec![
                    Span::raw(indent.clone()),
                    Span::raw(line.to_string()),
                ]));
            }
        }
        lines.push(Line::from(""));
    }

    let line_count = lines.len();
    (Text::from(lines), line_count)
}

fn build_centered_scene_text(scene_text: &str, area: Rect) -> Text<'static> {
    let lines: Vec<&str> = scene_text.lines().collect();
    let line_count = lines.len();
    let inner_width = area.width as usize;
    let inner_height = area.height as usize;
    let top_pad = inner_height.saturating_sub(line_count) / 2;

    let mut out: Vec<Line<'static>> = Vec::new();
    for _ in 0..top_pad {
        out.push(Line::from(""));
    }

    for line in lines {
        let line_len = line.chars().count();
        let left_pad = inner_width.saturating_sub(line_len) / 2;
        let mut padded = String::with_capacity(left_pad + line_len);
        if left_pad > 0 {
            padded.push_str(&" ".repeat(left_pad));
        }
        padded.push_str(line);
        out.push(Line::from(padded));
    }

    Text::from(out)
}

fn is_narrator_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("narrator")
}
