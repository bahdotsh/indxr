use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{Message, Role, TransientLlmError};

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiError {
    error: OpenAiErrorDetail,
}

#[derive(Deserialize)]
struct OpenAiErrorDetail {
    message: String,
}

pub async fn complete(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    model: &str,
    system: &str,
    messages: &[Message],
    max_tokens: usize,
) -> Result<String> {
    // Build OpenAI-format messages with system message prepended
    let mut oai_messages = Vec::with_capacity(messages.len() + 1);
    oai_messages.push(serde_json::json!({
        "role": "system",
        "content": system,
    }));
    for msg in messages {
        oai_messages.push(serde_json::json!({
            "role": match msg.role { Role::User => "user", Role::Assistant => "assistant" },
            "content": msg.content,
        }));
    }

    let body = serde_json::json!({
        "model": model,
        "max_completion_tokens": max_tokens,
        "messages": oai_messages,
    });

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to send request to OpenAI-compatible API")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let msg = if let Ok(err) = serde_json::from_str::<OpenAiError>(&text) {
            format!("OpenAI API error ({}): {}", status, err.error.message)
        } else {
            format!("OpenAI API error ({}): {}", status, text)
        };
        if status == 429 || status.is_server_error() {
            return Err(TransientLlmError(msg).into());
        }
        bail!("{}", msg);
    }

    let response: ChatResponse = resp
        .json()
        .await
        .context("Failed to parse OpenAI API response")?;

    let text = response
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    if text.is_empty() {
        bail!("OpenAI API returned empty response");
    }

    Ok(text)
}
