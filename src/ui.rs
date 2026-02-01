use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, LogEntry, LogKind};

pub(crate) fn draw_ui(frame: &mut Frame, app: &mut App) {
    let size = frame.size();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(size);

    let (log_text, line_count) = build_log_text(&app.log);
    let log_block = Block::default().borders(Borders::ALL).title("Story");
    let max_scroll = line_count.saturating_sub(vertical[0].height as usize);
    app.scroll = app.scroll.min(max_scroll as u16);

    let log_widget = Paragraph::new(log_text)
        .block(log_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    frame.render_widget(log_widget, vertical[0]);

    let input_block = Block::default().borders(Borders::ALL).title("Input");
    let input_widget = Paragraph::new(app.input.as_str())
        .block(input_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(input_widget, vertical[1]);

    let status_line = build_status_line(app);
    let status_widget = Paragraph::new(status_line);
    frame.render_widget(status_widget, vertical[2]);

    let help_text =
        "Enter send | Up/Down scroll | /new | /quit | Ctrl+C quit | /help for commands";
    let help_widget = Paragraph::new(help_text);
    frame.render_widget(help_widget, vertical[3]);

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

fn is_narrator_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("narrator")
}

fn build_status_line(app: &App) -> Line<'static> {
    let (text, color) = if app.busy {
        (build_thinking_indicator(app), Color::Yellow)
    } else if app.status.eq_ignore_ascii_case("error") {
        (app.status.clone(), Color::Red)
    } else {
        (app.status.clone(), Color::Green)
    };

    Line::from(Span::styled(text, Style::default().fg(color)))
}

fn build_thinking_indicator(app: &App) -> String {
    const FRAMES: [&str; 8] = [
        "[>     ]",
        "[>>    ]",
        "[>>>   ]",
        "[ >>>  ]",
        "[  >>> ]",
        "[   >>>]",
        "[    >>]",
        "[     >]",
    ];
    let Some(start) = app.thinking_started else {
        return "Thinking...".to_string();
    };
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let idx = ((elapsed_ms / 120) % FRAMES.len() as u64) as usize;
    format!("Thinking {}", FRAMES[idx])
}
