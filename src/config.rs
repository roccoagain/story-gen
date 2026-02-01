use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use serde_json::json;

pub(crate) const MODEL: &str = "gpt-5-mini";
pub(crate) const API_URL: &str = "https://api.openai.com/v1/responses";
pub(crate) const API_INPUT_TOKENS_URL: &str = "https://api.openai.com/v1/responses/input_tokens";
pub(crate) const MAX_HISTORY_ITEMS: usize = 60;
pub(crate) const MAIN_MAX_OUTPUT_TOKENS: u32 = 800;

pub(crate) const SYSTEM_PROMPT: &str = r#"You are a text adventure game narrator.
Write in second person, present tense.
Always prefix each line with a speaker label, e.g. "Narrator:" or "Clerk:".
Only the narrator or in-world characters may speak. Never output lines for the player (no "You:", "Player:", or "User:").
Use one speaker label per block; do not repeat the same label for consecutive lines.
Use the "Current speaker" field below: if it is not "Narrator", the player is addressing that character.
If the player directly addresses a named character, respond as that character until the dialogue ends.
If the player leaves, moves away, or ends the interaction, switch back to "Narrator:" and do not continue the character's dialogue.
If the player addresses "him/her/them" or speaks to someone in the scene, pick the most likely character and respond as them.
During dialogue, the Narrator should stay silent unless ending the dialogue; use "Narrator:" to resume narration.
Narrator describes actions and scene changes; characters only speak dialogue. If both are needed, use two lines: Narrator first, then the character.
When a character speaks, use quotation marks around their words.
Keep character names consistent when labeling lines.
Keep responses concise: 1-2 short paragraphs, then ask what the player does next.
Do not use markdown code fences or JSON in your response.
Avoid meta commentary about being an AI.
"#;

pub(crate) fn load_or_prompt_api_key() -> Result<String> {
    let env_path = Path::new(".env");

    if let Some(key) = read_env_key() {
        match validate_api_key(&key) {
            Ok(()) => return Ok(key),
            Err(err) => {
                println!("OPENAI_API_KEY from environment is invalid: {err}");
            }
        }
    }

    if let Some(key) = read_key_from_env_file(env_path) {
        match validate_api_key(&key) {
            Ok(()) => return Ok(key),
            Err(err) => {
                println!("OPENAI_API_KEY from .env is invalid: {err}");
            }
        }
    }

    loop {
        println!("OPENAI_API_KEY not found. Paste your API key and press Enter:");
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let key = input.trim();
        if key.is_empty() {
            println!("No API key provided. Please try again.");
            continue;
        }

        match validate_api_key(key) {
            Ok(()) => {
                upsert_env_key(env_path, key)?;
                return Ok(key.to_string());
            }
            Err(err) => {
                println!("API key validation failed: {err}");
            }
        }
    }
}

fn validate_api_key(api_key: &str) -> Result<()> {
    println!("Validating OpenAI API key...");
    let _ = io::stdout().flush();
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let body = json!({
        "model": MODEL,
        "input": "Test request to validate API key."
    });
    let response = client
        .post(API_INPUT_TOKENS_URL)
        .bearer_auth(api_key)
        .json(&body)
        .send()?;

    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let text = response.text().unwrap_or_default();
    let message = extract_api_error_message(&text).unwrap_or(text);
    Err(anyhow!("OpenAI API error ({status}): {message}"))
}

fn extract_api_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = value
        .get("error")?
        .get("message")?
        .as_str()?
        .trim();
    if message.is_empty() {
        None
    } else {
        Some(message.to_string())
    }
}

fn read_env_key() -> Option<String> {
    env::var("OPENAI_API_KEY")
        .ok()
        .and_then(|key| normalize_key(&key))
}

fn read_key_from_env_file(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("OPENAI_API_KEY=") {
            if let Some(key) = normalize_key(value) {
                return Some(key);
            }
        }
    }
    None
}

fn normalize_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(trimmed);

    let cleaned = unquoted.trim();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
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
