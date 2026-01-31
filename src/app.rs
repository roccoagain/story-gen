use std::sync::mpsc::Receiver;
use std::time::Instant;

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
    pub(crate) speaker: Option<String>,
    pub(crate) text: String,
}

#[derive(Clone)]
pub(crate) struct GameState {
    pub(crate) turn: u32,
    pub(crate) location: String,
    pub(crate) inventory: Vec<String>,
    pub(crate) flags: Vec<String>,
    pub(crate) active_speaker: Option<String>,
}

impl GameState {
    pub(crate) fn new() -> Self {
        Self {
            turn: 0,
            location: "Unknown".to_string(),
            inventory: Vec::new(),
            flags: Vec::new(),
            active_speaker: None,
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
    pub(crate) scene_pending_response: Option<Receiver<Result<String>>>,
    pub(crate) state: GameState,
    pub(crate) status: String,
    pub(crate) scene_ascii: String,
    pub(crate) thinking_started: Option<Instant>,
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
            scene_pending_response: None,
            state: GameState::new(),
            status: "Ready".to_string(),
            scene_ascii: "Awaiting scene...".to_string(),
            thinking_started: None,
        };
        app.push_log(LogKind::System, "Welcome! Describe what you do to begin.");
        app
    }

    pub(crate) fn push_log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push(LogEntry {
            kind,
            speaker: None,
            text: text.into(),
        });
    }

    pub(crate) fn push_speaker_log(
        &mut self,
        kind: LogKind,
        speaker: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.log.push(LogEntry {
            kind,
            speaker: Some(speaker.into()),
            text: text.into(),
        });
    }

    pub(crate) fn push_user_log(&mut self, text: impl Into<String>) {
        self.push_speaker_log(LogKind::User, "You", text);
    }

    pub(crate) fn push_assistant_reply(&mut self, reply: &str) {
        let reply = reply.trim();
        if reply.is_empty() {
            return;
        }

        let parsed = parse_speaker_lines(reply);
        if parsed.entries.is_empty() {
            let sanitized = strip_disallowed_speaker_lines(reply);
            if !sanitized.trim().is_empty() {
                self.push_speaker_log(LogKind::Assistant, "Narrator", sanitized.trim());
                self.state.active_speaker = None;
            }
            return;
        }
        for entry in parsed.entries {
            self.push_speaker_log(LogKind::Assistant, entry.speaker, entry.text);
        }

        if let Some(last_speaker) = parsed.last_speaker {
            if is_narrator_label(&last_speaker) {
                self.state.active_speaker = None;
            } else {
                self.state.active_speaker = Some(last_speaker);
            }
        }
    }

    pub(crate) fn push_user_message(&mut self, content: impl Into<String>) {
        let item = json!({
            "role": "user",
            "content": content.into()
        });
        if self.state.active_speaker.is_some() {
            if let Some(text) = item.get("content").and_then(|v| v.as_str()) {
                if is_dialogue_exit(text) {
                    self.state.active_speaker = None;
                }
            }
        }
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
        self.scene_pending_response = None;
        self.state = GameState::new();
        self.status = "Ready".to_string();
        self.scene_ascii = "Awaiting scene...".to_string();
        self.thinking_started = None;
        self.push_log(LogKind::System, "New game. Describe what you do to begin.");
    }

    pub(crate) fn build_scene_context(&self) -> String {
        let inventory = if self.state.inventory.is_empty() {
            "Empty".to_string()
        } else {
            self.state.inventory.join(", ")
        };
        let flags = if self.state.flags.is_empty() {
            "None".to_string()
        } else {
            self.state.flags.join(", ")
        };
        let active_speaker = self
            .state
            .active_speaker
            .as_deref()
            .unwrap_or("Narrator");
        let last_text = self
            .latest_assistant_text()
            .unwrap_or_else(|| "No recent narration.".to_string());

        format!(
            "Turn: {}\nLocation: {}\nInventory: {}\nFlags: {}\nActive speaker: {}\nRecent narration/dialogue:\n{}",
            self.state.turn, self.state.location, inventory, flags, active_speaker, last_text
        )
    }

    pub(crate) fn set_scene_ascii(&mut self, ascii: impl Into<String>) {
        let mut text = ascii.into().replace("\r\n", "\n");
        text = text.trim_matches('\n').to_string();
        if text.trim().is_empty() {
            self.scene_ascii = "Awaiting scene...".to_string();
        } else {
            self.scene_ascii = text;
        }
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

    fn latest_assistant_text(&self) -> Option<String> {
        self.log
            .iter()
            .rev()
            .find(|entry| matches!(entry.kind, LogKind::Assistant) && !entry.text.trim().is_empty())
            .map(|entry| entry.text.trim().to_string())
    }
}

struct ParsedEntry {
    speaker: String,
    text: String,
}

struct ParsedReply {
    entries: Vec<ParsedEntry>,
    last_speaker: Option<String>,
}

fn parse_speaker_lines(text: &str) -> ParsedReply {
    let mut entries = Vec::new();
    let mut current_speaker: Option<String> = None;
    let mut current_text = String::new();
    let mut last_speaker: Option<String> = None;

    for line in text.lines() {
        if let Some((speaker, rest)) = parse_speaker_label(line) {
            if is_disallowed_speaker(&speaker) {
                continue;
            }
            if let Some(prev_speaker) = current_speaker.take() {
                push_or_merge_entry(&mut entries, prev_speaker, current_text.trim_end());
            }
            current_text.clear();

            if !is_narrator_label(&speaker) {
                if let Some((narration, dialogue)) = split_misattributed_narration(&rest) {
                    if !narration.is_empty() {
                        push_or_merge_entry(&mut entries, "Narrator".to_string(), &narration);
                        last_speaker = Some("Narrator".to_string());
                    }
                    if let Some(dialogue) = dialogue {
                        current_speaker = Some(speaker.clone());
                        current_text.push_str(&dialogue);
                        last_speaker = Some(speaker);
                    }
                    continue;
                }
            }

            last_speaker = Some(speaker.clone());
            current_speaker = Some(speaker);
            current_text.push_str(&rest);
            continue;
        }

        if current_speaker.is_none() {
            current_speaker = Some("Narrator".to_string());
            last_speaker = Some("Narrator".to_string());
        }
        if !current_text.is_empty() {
            current_text.push('\n');
        }
        current_text.push_str(line);
    }

    if let Some(prev_speaker) = current_speaker.take() {
        push_or_merge_entry(&mut entries, prev_speaker, current_text.trim_end());
    }

    ParsedReply {
        entries,
        last_speaker,
    }
}

fn parse_speaker_label(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let (label, rest) = trimmed.split_once(':')?;
    let label = label.trim();
    if label.is_empty() || label.len() > 40 {
        return None;
    }
    if !label.chars().any(|ch| ch.is_alphabetic()) {
        return None;
    }
    if !label
        .chars()
        .all(|ch| ch.is_alphanumeric() || ch == ' ' || ch == '\'' || ch == '-')
    {
        return None;
    }
    let normalized = if is_narrator_label(label) {
        "Narrator".to_string()
    } else {
        label.to_string()
    };
    Some((normalized, rest.trim_start().to_string()))
}

fn is_narrator_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("narrator")
}

fn is_disallowed_speaker(label: &str) -> bool {
    let trimmed = label.trim();
    trimmed.eq_ignore_ascii_case("you")
        || trimmed.eq_ignore_ascii_case("player")
        || trimmed.eq_ignore_ascii_case("user")
}

fn is_dialogue_exit(text: &str) -> bool {
    let lower = text.to_lowercase();
    let trimmed = lower.trim();
    if trimmed.starts_with("leave ")
        || trimmed.starts_with("leaving ")
        || trimmed.starts_with("walk away")
        || trimmed.starts_with("walk out")
        || trimmed.starts_with("run out")
        || trimmed.starts_with("head out")
        || trimmed.starts_with("step out")
        || trimmed.starts_with("exit")
        || trimmed.starts_with("go out")
        || trimmed.starts_with("go outside")
    {
        return true;
    }

    let phrases = [
        "leave the store",
        "leave the shop",
        "leave the bodega",
        "leave this place",
        "leave without",
        "walked out",
        "run out",
        "ran out",
        "sprint out",
        "head out",
        "step outside",
        "out of the store",
        "out of the shop",
        "out of the bodega",
        "i'm out",
        "i am out",
        "i'm gone",
        "i am gone",
        "far gone",
        "outside now",
        "outside the store",
        "outside the shop",
        "outside the bodega",
    ];
    phrases.iter().any(|phrase| lower.contains(phrase))
}

fn split_misattributed_narration(text: &str) -> Option<(String, Option<String>)> {
    if !starts_with_you_action(text) {
        return None;
    }
    let trimmed = text.trim();
    if let Some(idx) = trimmed.find('"') {
        let (before, after) = trimmed.split_at(idx);
        let narration = before.trim();
        if narration.is_empty() {
            return None;
        }
        let dialogue = after.trim();
        let dialogue = if dialogue.is_empty() {
            None
        } else {
            Some(dialogue.to_string())
        };
        return Some((narration.to_string(), dialogue));
    }
    Some(split_first_sentence(trimmed))
}

fn starts_with_you_action(text: &str) -> bool {
    let lower = text.trim_start().to_lowercase();
    if !lower.starts_with("you ") {
        return false;
    }
    let rest = lower.trim_start_matches("you ").trim_start();
    let verb = rest
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|ch: char| !ch.is_alphanumeric());
    if verb.is_empty() {
        return false;
    }
    let action_verbs = [
        "pick", "grab", "scoop", "tuck", "walk", "step", "run", "head", "move", "take", "reach",
        "turn", "open", "close", "enter", "leave", "slip", "push", "pull", "drop", "pocket",
        "lift", "set", "place", "climb", "kneel", "sit", "stand", "backflip", "sprint", "brush",
        "touch", "aim", "throw", "swing", "carry", "stow", "hold",
    ];
    action_verbs.iter().any(|action| *action == verb)
}

fn split_first_sentence(text: &str) -> (String, Option<String>) {
    let boundaries = [". ", "? ", "! "];
    let mut best: Option<(usize, usize)> = None;
    for boundary in boundaries {
        if let Some(idx) = text.find(boundary) {
            if best.map_or(true, |(best_idx, _)| idx < best_idx) {
                best = Some((idx, boundary.len()));
            }
        }
    }
    if let Some((idx, boundary_len)) = best {
        let narration = text[..idx + 1].trim().to_string();
        let rest = text[idx + boundary_len..].trim();
        let dialogue = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };
        return (narration, dialogue);
    }
    (text.trim().to_string(), None)
}

fn push_or_merge_entry(entries: &mut Vec<ParsedEntry>, speaker: String, text: &str) {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return;
    }
    if let Some(last) = entries.last_mut() {
        if last.speaker.eq_ignore_ascii_case(&speaker) {
            last.text.push('\n');
            last.text.push_str(trimmed);
            return;
        }
    }
    entries.push(ParsedEntry {
        speaker,
        text: trimmed.to_string(),
    });
}

fn strip_disallowed_speaker_lines(text: &str) -> String {
    let mut kept = Vec::new();
    for line in text.lines() {
        if let Some((label, _)) = parse_speaker_label(line) {
            if is_disallowed_speaker(&label) {
                continue;
            }
        }
        kept.push(line);
    }
    kept.join("\n")
}
