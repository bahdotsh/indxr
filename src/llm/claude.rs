use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::Message;

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    error: Option<ClaudeError>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeError {
    message: String,
}

#[derive(Deserialize)]
struct ClaudeErrorResponse {
    error: ClaudeError,
}

pub async fn complete(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    system: &str,
    messages: &[Message],
    max_tokens: usize,
) -> Result<String> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": messages,
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to send request to Claude API")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        if let Ok(err) = serde_json::from_str::<ClaudeErrorResponse>(&text) {
            bail!("Claude API error ({}): {}", status, err.error.message);
        }
        bail!("Claude API error ({}): {}", status, text);
    }

    let response: ClaudeResponse = resp
        .json()
        .await
        .context("Failed to parse Claude API response")?;

    if let Some(err) = response.error {
        bail!("Claude API error: {}", err.message);
    }

    let text = response
        .content
        .into_iter()
        .filter_map(|b| b.text)
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        bail!("Claude API returned empty response");
    }

    Ok(text)
}
