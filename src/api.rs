use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};

use crate::app::GameState;
use crate::config::{API_URL, MODEL, SYSTEM_PROMPT};

fn build_request_body(input: &[Value]) -> Value {
    json!({
        "model": MODEL,
        "input": input,
        "max_output_tokens": 500,
        "text": { "format": { "type": "text" } },
        "reasoning": { "effort": "low" },
        "include": ["reasoning.encrypted_content"]
    })
}

pub(crate) fn advance_turn(
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
        "{SYSTEM_PROMPT}\nCurrent turn: {}\nLocation: {}\nInventory: {}\nFlags: {}\nCurrent speaker: {}",
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
        },
        state
            .active_speaker
            .as_deref()
            .unwrap_or("Narrator")
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
    let body = build_request_body(&input_items);
    let retry_body = build_request_body(&retry_items);

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
            let fallback = value
                .get("output_text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
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
