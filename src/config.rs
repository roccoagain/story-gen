use std::env;
use std::fs;
use std::io;
use std::path::Path;

use anyhow::{anyhow, Result};

pub(crate) const MODEL: &str = "gpt-5-mini";
pub(crate) const API_URL: &str = "https://api.openai.com/v1/responses";
pub(crate) const MAX_HISTORY_ITEMS: usize = 60;

pub(crate) const SYSTEM_PROMPT: &str = r#"You are a text adventure game narrator.
Write in second person, present tense.
Keep responses concise: 1-2 short paragraphs, then ask what the player does next.
Do not use markdown code fences or JSON in your response.
Avoid meta commentary about being an AI.
"#;

pub(crate) fn load_or_prompt_api_key() -> Result<String> {
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
