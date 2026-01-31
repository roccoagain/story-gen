use std::sync::mpsc::Receiver;

use anyhow::Result;
use serde_json::{json, Value};

use crate::config::MAX_HISTORY_ITEMS;

#[derive(Clone, Copy)]
pub(crate) enum LogKind {
    User,
    Assistant,
    System,
    Error,
}

pub(crate) struct LogEntry {
    pub(crate) kind: LogKind,
    pub(crate) text: String,
}

#[derive(Clone)]
pub(crate) struct GameState {
    pub(crate) turn: u32,
    pub(crate) location: String,
    pub(crate) inventory: Vec<String>,
    pub(crate) flags: Vec<String>,
}

impl GameState {
    pub(crate) fn new() -> Self {
        Self {
            turn: 0,
            location: "Unknown".to_string(),
            inventory: Vec::new(),
            flags: Vec::new(),
        }
    }
}

pub(crate) struct App {
    pub(crate) input: String,
    pub(crate) log: Vec<LogEntry>,
    pub(crate) history: Vec<Vec<Value>>,
    pub(crate) scroll: u16,
    pub(crate) busy: bool,
    pub(crate) pending_input: Option<String>,
    pub(crate) last_sent_input: Option<String>,
    pub(crate) pending_response: Option<Receiver<Result<(String, Vec<Value>, String)>>>,
    pub(crate) state: GameState,
    pub(crate) status: String,
}

impl App {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn push_log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push(LogEntry {
            kind,
            text: text.into(),
        });
    }

    pub(crate) fn push_user_message(&mut self, content: impl Into<String>) {
        let item = json!({
            "role": "user",
            "content": content.into()
        });
        self.push_history_chunk(vec![item]);
    }

    pub(crate) fn push_history_chunk(&mut self, items: Vec<Value>) {
        if items.is_empty() {
            return;
        }
        self.history.push(items);
        self.trim_history();
    }

    pub(crate) fn reset(&mut self) {
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
}
