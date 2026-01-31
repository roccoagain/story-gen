use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, LogKind};

pub(crate) fn handle_key_event(key: KeyEvent, app: &mut App) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => return Ok(true),
            KeyCode::Char('n') => {
                app.reset();
                return Ok(false);
            }
            KeyCode::Char('r') => {
                if let Some(last) = app.last_sent_input.clone() {
                    app.input = last;
                }
                return Ok(false);
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Char(ch) => {
            app.input.push(ch);
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Enter => {
            let input = app.input.trim().to_string();
            app.input.clear();
            if input.is_empty() {
                return Ok(false);
            }
            if input.starts_with('/') {
                if handle_command(&input, app)? {
                    return Ok(true);
                }
                return Ok(false);
            }
            app.push_log(LogKind::User, &input);
            app.push_user_message(&input);
            app.last_sent_input = Some(input.clone());
            app.pending_input = Some(input);
        }
        KeyCode::Up => {
            app.scroll = app.scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            app.scroll = app.scroll.saturating_add(1);
        }
        _ => {}
    }

    Ok(false)
}

fn handle_command(input: &str, app: &mut App) -> Result<bool> {
    if !input.starts_with('/') {
        return Ok(false);
    }

    match input {
        "/quit" | "/exit" => return Ok(true),
        "/new" => {
            app.reset();
        }
        "/help" => {
            app.push_log(
                LogKind::System,
                "Commands: /new, /quit, /set location <name>, /add item <name>, /remove item <name>, /flag <name>, /unflag <name>.",
            );
        }
        _ if input.starts_with("/set location ") => {
            let loc = input.trim_start_matches("/set location ").trim();
            if loc.is_empty() {
                app.push_log(LogKind::System, "Usage: /set location <name>");
            } else {
                app.state.location = loc.to_string();
                app.push_log(LogKind::System, format!("Location set to: {loc}"));
            }
        }
        _ if input.starts_with("/add item ") => {
            let item = input.trim_start_matches("/add item ").trim();
            if item.is_empty() {
                app.push_log(LogKind::System, "Usage: /add item <name>");
            } else {
                app.state.inventory.push(item.to_string());
                app.push_log(LogKind::System, format!("Added item: {item}"));
            }
        }
        _ if input.starts_with("/remove item ") => {
            let item = input.trim_start_matches("/remove item ").trim();
            if item.is_empty() {
                app.push_log(LogKind::System, "Usage: /remove item <name>");
            } else if let Some(pos) = app.state.inventory.iter().position(|i| i == item) {
                app.state.inventory.remove(pos);
                app.push_log(LogKind::System, format!("Removed item: {item}"));
            } else {
                app.push_log(LogKind::System, format!("Item not found: {item}"));
            }
        }
        _ if input.starts_with("/flag ") => {
            let flag = input.trim_start_matches("/flag ").trim();
            if flag.is_empty() {
                app.push_log(LogKind::System, "Usage: /flag <name>");
            } else if app.state.flags.iter().any(|f| f == flag) {
                app.push_log(LogKind::System, format!("Flag already set: {flag}"));
            } else {
                app.state.flags.push(flag.to_string());
                app.push_log(LogKind::System, format!("Flag set: {flag}"));
            }
        }
        _ if input.starts_with("/unflag ") => {
            let flag = input.trim_start_matches("/unflag ").trim();
            if flag.is_empty() {
                app.push_log(LogKind::System, "Usage: /unflag <name>");
            } else if let Some(pos) = app.state.flags.iter().position(|f| f == flag) {
                app.state.flags.remove(pos);
                app.push_log(LogKind::System, format!("Flag cleared: {flag}"));
            } else {
                app.push_log(LogKind::System, format!("Flag not found: {flag}"));
            }
        }
        _ => {
            app.push_log(LogKind::System, "Unknown command. Try /help.");
        }
    }

    Ok(false)
}
