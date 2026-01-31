use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use reqwest::blocking::Client;
use serde_json::{json, Value};

const MODEL: &str = "gpt-5-mini";
const API_URL: &str = "https://api.openai.com/v1/responses";
const MAX_HISTORY_ITEMS: usize = 60;

const SYSTEM_PROMPT: &str = r#"You are a text adventure game narrator.
Write in second person, present tense.
Keep responses concise: 1-2 short paragraphs, then ask what the player does next.
Do not use markdown code fences or JSON in your response.
Avoid meta commentary about being an AI.
"#;

#[derive(Clone, Copy)]
enum LogKind {
    User,
    Assistant,
    System,
    Error,
}

struct LogEntry {
    kind: LogKind,
    text: String,
}

#[derive(Clone)]
struct GameState {
    turn: u32,
    location: String,
    inventory: Vec<String>,
    flags: Vec<String>,
}

impl GameState {
    fn new() -> Self {
        Self {
            turn: 0,
            location: "Unknown".to_string(),
            inventory: Vec::new(),
            flags: Vec::new(),
        }
    }
}

struct App {
    input: String,
    log: Vec<LogEntry>,
    history: Vec<Vec<Value>>,
    scroll: u16,
    busy: bool,
    pending_input: Option<String>,
    last_sent_input: Option<String>,
    pending_response: Option<Receiver<Result<(String, Vec<Value>, String)>>>,
    state: GameState,
    status: String,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            input: String::new(),
            log: Vec::new(),
            history: Vec::new(),
            scroll: 0,
            busy: false,
            pending_input: None,
            last_sent_input: None,
            pending_response: None,
            state: GameState::new(),
            status: "Ready".to_string(),
        };
        app.push_log(LogKind::System, "Welcome! Describe what you do to begin.");
        app
    }

    fn push_log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push(LogEntry {
            kind,
            text: text.into(),
        });
    }

    fn push_user_message(&mut self, content: impl Into<String>) {
        let item = json!({
            "role": "user",
            "content": content.into()
        });
        self.push_history_chunk(vec![item]);
    }

    fn push_history_chunk(&mut self, items: Vec<Value>) {
        if items.is_empty() {
            return;
        }
        self.history.push(items);
        self.trim_history();
    }

    fn trim_history(&mut self) {
        while self.history_item_count() > MAX_HISTORY_ITEMS {
            if self.history.is_empty() {
                break;
            }
            self.history.remove(0);
        }
    }

    fn history_item_count(&self) -> usize {
        self.history.iter().map(|chunk| chunk.len()).sum()
    }

    fn reset(&mut self) {
        self.input.clear();
        self.log.clear();
        self.history.clear();
        self.scroll = 0;
        self.busy = false;
        self.pending_input = None;
        self.last_sent_input = None;
        self.pending_response = None;
        self.state = GameState::new();
        self.status = "Ready".to_string();
        self.push_log(LogKind::System, "New game. Describe what you do to begin.");
    }
}

fn load_or_prompt_api_key() -> Result<String> {
    let env_key = env::var("OPENAI_API_KEY")
        .ok()
        .and_then(|key| {
            let trimmed = key.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

    let env_path = Path::new(".env");
    if !env_path.exists() {
        fs::write(env_path, "OPENAI_API_KEY=\n")?;
    }

    let _ = dotenvy::from_filename(env_path);

    let file_key = env::var("OPENAI_API_KEY")
        .ok()
        .and_then(|key| {
            let trimmed = key.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

    if let Some(key) = env_key.or(file_key) {
        return Ok(key);
    }

    println!("OPENAI_API_KEY not found. Paste your API key and press Enter:");
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let key = input.trim().to_string();
    if key.is_empty() {
        return Err(anyhow!("No API key provided."));
    }

    upsert_env_key(env_path, &key)?;
    Ok(key)
}

fn upsert_env_key(path: &Path, key: &str) -> Result<()> {
    let contents = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();
    let mut found = false;

    for line in &mut lines {
        if line.trim_start().starts_with("OPENAI_API_KEY=") {
            *line = format!("OPENAI_API_KEY={key}");
            found = true;
            break;
        }
    }

    if !found {
        lines.push(format!("OPENAI_API_KEY={key}"));
    }

    let mut output = lines.join("\n");
    output.push('\n');
    fs::write(path, output)?;
    Ok(())
}

fn main() -> Result<()> {
    let debug = env::args().any(|arg| arg == "--debug" || arg == "-d");
    let api_key = load_or_prompt_api_key()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, api_key, debug);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    api_key: String,
    debug: bool,
) -> Result<()> {
    let mut app = App::new();

    loop {
        terminal.draw(|frame| draw_ui(frame, &mut app))?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key_event(key, &mut app)? {
                        break;
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if app.busy {
            if let Some(rx) = &app.pending_response {
                match rx.try_recv() {
                    Ok(result) => {
                        app.pending_response = None;
                        app.busy = false;
                        match result {
                            Ok((reply, output_items, debug_summary)) => {
                                app.push_log(LogKind::Assistant, reply.trim());
                                app.push_history_chunk(output_items);
                                if debug {
                                    app.push_log(LogKind::System, debug_summary);
                                }
                                app.state.turn = app.state.turn.saturating_add(1);
                                app.status = "Ready".to_string();
                            }
                            Err(err) => {
                                app.push_log(LogKind::Error, err.to_string());
                                app.status = "Error".to_string();
                            }
                        }
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        app.pending_response = None;
                        app.busy = false;
                        app.push_log(LogKind::Error, "Response channel disconnected.");
                        app.status = "Error".to_string();
                    }
                }
            }
            continue;
        }

        if let Some(_user_input) = app.pending_input.take() {
            let api_key = api_key.clone();
            let history = app.history.clone();
            let state = app.state.clone();
            let (tx, rx) = mpsc::channel();
            app.pending_response = Some(rx);
            app.busy = true;
            app.status = "Thinking...".to_string();
            terminal.draw(|frame| draw_ui(frame, &mut app))?;

            thread::spawn(move || {
                let result = advance_turn(&api_key, &history, &state, debug);
                let _ = tx.send(result);
            });
        }
    }

    Ok(())
}

fn handle_key_event(key: KeyEvent, app: &mut App) -> Result<bool> {
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
            if handle_command(&input, app)? {
                return Ok(true);
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

fn draw_ui(frame: &mut Frame, app: &mut App) {
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

fn advance_turn(
    api_key: &str,
    history: &[Vec<Value>],
    state: &GameState,
    debug: bool,
) -> Result<(String, Vec<Value>, String)> {
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    let mut input_items = Vec::new();

    let system_with_state = format!(
        "{SYSTEM_PROMPT}\nCurrent turn: {}\nLocation: {}\nInventory: {}\nFlags: {}",
        state.turn,
        state.location,
        if state.inventory.is_empty() {
            "Empty".to_string()
        } else {
            state.inventory.join(", ")
        },
        if state.flags.is_empty() {
            "None".to_string()
        } else {
            state.flags.join(", ")
        }
    );

    input_items.push(json!({
        "role": "system",
        "content": system_with_state
    }));

    for chunk in history {
        for item in chunk {
            input_items.push(item.clone());
        }
    }

    let mut retry_items = input_items.clone();
    retry_items.push(json!({
        "role": "user",
        "content": "Please respond with visible text only."
    }));
    let body = json!({
        "model": MODEL,
        "input": input_items,
        "max_output_tokens": 500,
        "text": { "format": { "type": "text" } },
        "reasoning": { "effort": "low" },
        "include": ["reasoning.encrypted_content"]
    });
    let retry_body = json!({
        "model": MODEL,
        "input": retry_items,
        "max_output_tokens": 500,
        "text": { "format": { "type": "text" } },
        "reasoning": { "effort": "low" },
        "include": ["reasoning.encrypted_content"]
    });

    let mut last_debug = String::new();
    for attempt in 0..2 {
        let body_ref = if attempt == 0 { &body } else { &retry_body };
        let response = client
            .post(API_URL)
            .bearer_auth(api_key)
            .json(body_ref)
            .send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            return Err(anyhow!("OpenAI API error ({status}): {text}"));
        }

        let value: Value = response.json()?;
        let (text_opt, output_items, debug_summary) = extract_output_text_and_items(&value);
        last_debug = debug_summary;
        if let Some(text) = text_opt {
            return Ok((text, output_items, last_debug));
        }
        if attempt == 0 {
            continue;
        }
    }

    let message = if debug {
        format!("No output text found in response. Output summary: {last_debug}")
    } else {
        "No output text found in response.".to_string()
    };
    Err(anyhow!(message))
}

fn extract_output_text_and_items(value: &Value) -> (Option<String>, Vec<Value>, String) {
    let output = match value.get("output").and_then(|v| v.as_array()) {
        Some(output) => output,
        None => {
            let fallback = value.get("output_text").and_then(|v| v.as_str()).map(|s| s.to_string());
            return (fallback, Vec::new(), "output: <missing>".to_string());
        }
    };
    let mut texts = Vec::new();
    let mut items = Vec::new();
    let mut debug_lines = Vec::new();
    let mut refusals = Vec::new();
    let fallback_text = value
        .get("output_text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(text) = &fallback_text {
        debug_lines.push(format!("output_text:len={}", text.chars().count()));
    }

    for item in output {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
        let item_role = item.get("role").and_then(|v| v.as_str()).unwrap_or("-");
        let mut content_types = Vec::new();
        if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
            for part in content {
                if let Some(ty) = part.get("type").and_then(|v| v.as_str()) {
                    content_types.push(ty.to_string());
                    if ty == "refusal" {
                        if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                            refusals.push(text.to_string());
                        }
                    }
                }
            }
        }
        if content_types.is_empty() {
            debug_lines.push(format!("output: type={item_type} role={item_role} content=[]"));
        } else {
            debug_lines.push(format!(
                "output: type={item_type} role={item_role} content={}",
                content_types.join(",")
            ));
        }
        items.push(item.clone());
        if item.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        if item.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
            for part in content {
                if part.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        texts.push(text.to_string());
                    }
                }
            }
        }
    }

    if texts.is_empty() {
        if !refusals.is_empty() {
            (
                Some(format!("Refusal: {}", refusals.join("\n"))),
                items,
                debug_lines.join(" | "),
            )
        } else if fallback_text.is_some() {
            (fallback_text, items, debug_lines.join(" | "))
        } else {
            (None, items, debug_lines.join(" | "))
        }
    } else {
        (Some(texts.join("")), items, debug_lines.join(" | "))
    }
}
