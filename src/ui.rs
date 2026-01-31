use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, LogEntry, LogKind};

pub(crate) fn draw_ui(frame: &mut Frame, app: &mut App) {
    let size = frame.size();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3), Constraint::Length(1)])
        .split(size);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(vertical[0]);

    let side = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(main[1]);

    let (log_text, line_count) = build_log_text(&app.log);
    let log_block = Block::default().borders(Borders::ALL).title("Story");
    let max_scroll = line_count.saturating_sub(main[0].height as usize);
    app.scroll = app.scroll.min(max_scroll as u16);

    let log_widget = Paragraph::new(log_text)
        .block(log_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    frame.render_widget(log_widget, main[0]);

    let status_block = Block::default().borders(Borders::ALL).title("Status");
    let status_text = format!(
        "Turn: {}\nLocation: {}\nState: {}",
        app.state.turn, app.state.location, app.status
    );
    let status_widget = Paragraph::new(status_text).block(status_block);
    frame.render_widget(status_widget, side[0]);

    let inventory_block = Block::default().borders(Borders::ALL).title("Inventory");
    let inventory_text = if app.state.inventory.is_empty() {
        "Empty".to_string()
    } else {
        app.state.inventory.join("\n")
    };
    let inventory_widget = Paragraph::new(inventory_text).block(inventory_block);
    frame.render_widget(inventory_widget, side[1]);

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
            LogKind::User => ("You: ", Style::default().fg(Color::Yellow)),
            LogKind::Assistant => ("Narrator: ", Style::default().fg(Color::Green)),
            LogKind::System => ("", Style::default().fg(Color::Blue)),
            LogKind::Error => ("Error: ", Style::default().fg(Color::Red)),
        };
        let indent = " ".repeat(prefix.len());
        let mut first = true;
        for line in entry.text.lines() {
            if first {
                lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), style),
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
