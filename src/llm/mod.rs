mod claude;
mod command;
mod openai;

use std::time::Duration;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// A transient LLM error that should be retried (429, 5xx, process crash).
#[derive(Debug)]
pub(crate) struct TransientLlmError(pub String);

impl std::fmt::Display for TransientLlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TransientLlmError {}

/// A single message in an LLM conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Configuration for an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: Provider,
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
}

#[derive(Debug, Clone)]
pub enum Provider {
    Claude,
    OpenAiCompatible {
        base_url: String,
    },
    /// Shell out to an external command for completions.
    /// The command receives JSON on stdin (`{system, messages, max_tokens}`)
    /// and returns the response text on stdout.
    Command {
        cmd: String,
    },
}

/// Provider-agnostic LLM client.
#[derive(Clone)]
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
    max_retries: usize,
}

impl LlmClient {
    /// Auto-detect provider from environment variables.
    ///
    /// Priority: `INDXR_LLM_COMMAND` → `ANTHROPIC_API_KEY` → `OPENAI_API_KEY`.
    /// Model can be overridden; otherwise sensible defaults are used.
    pub fn from_env(model_override: Option<&str>) -> Result<Self> {
        // 1. Check for external command provider (best for coding agents)
        if let Ok(cmd) = std::env::var("INDXR_LLM_COMMAND") {
            let cmd = cmd.trim().to_string();
            if !cmd.is_empty() {
                let model = model_override.unwrap_or("command").to_string();
                return Ok(Self::with_config(LlmConfig {
                    provider: Provider::Command { cmd },
                    api_key: String::new(),
                    model,
                    max_tokens: 4096,
                }));
            }
        }

        // 2. Anthropic API
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                let model = model_override
                    .unwrap_or("claude-sonnet-4-20250514")
                    .to_string();
                return Ok(Self::with_config(LlmConfig {
                    provider: Provider::Claude,
                    api_key: key,
                    model,
                    max_tokens: 4096,
                }));
            }
        }

        // 3. OpenAI-compatible API
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                let base_url = std::env::var("OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
                let model = model_override.unwrap_or("gpt-4o").to_string();
                return Ok(Self::with_config(LlmConfig {
                    provider: Provider::OpenAiCompatible { base_url },
                    api_key: key,
                    model,
                    max_tokens: 4096,
                }));
            }
        }

        bail!(
            "No LLM provider found. Set one of:\n  \
             - INDXR_LLM_COMMAND  (external command, ideal for coding agents)\n  \
             - ANTHROPIC_API_KEY  (Claude API)\n  \
             - OPENAI_API_KEY     (OpenAI-compatible API)\n\n\
             Or pass --exec <CMD> to use an external command directly."
        )
    }

    /// Create a client with an explicit command provider.
    pub fn from_command(cmd: String, model_override: Option<&str>) -> Self {
        Self::with_config(LlmConfig {
            provider: Provider::Command { cmd },
            api_key: String::new(),
            model: model_override.unwrap_or("command").to_string(),
            max_tokens: 4096,
        })
    }

    pub fn with_config(config: LlmConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            max_retries: 2,
        }
    }

    /// Set max tokens for responses.
    pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
        self.config.max_tokens = max_tokens;
        self
    }

    /// Send a system prompt + messages and get a complete response.
    /// Retries transient failures with exponential backoff.
    pub async fn complete(&self, system: &str, messages: &[Message]) -> Result<String> {
        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let delay = Duration::from_millis(1000 * 2u64.pow(attempt as u32 - 1));
                eprintln!(
                    "  Retrying in {}s (attempt {}/{})...",
                    delay.as_secs(),
                    attempt + 1,
                    self.max_retries + 1
                );
                tokio::time::sleep(delay).await;
            }

            match self.complete_once(system, messages).await {
                Ok(text) => return Ok(text),
                Err(e) => {
                    let transient = e.downcast_ref::<TransientLlmError>().is_some();
                    eprintln!("  LLM call failed: {e:#}");
                    if !transient {
                        return Err(e);
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap())
    }

    async fn complete_once(&self, system: &str, messages: &[Message]) -> Result<String> {
        match &self.config.provider {
            Provider::Claude => {
                claude::complete(
                    &self.http,
                    &self.config.api_key,
                    &self.config.model,
                    system,
                    messages,
                    self.config.max_tokens,
                )
                .await
            }
            Provider::OpenAiCompatible { base_url } => {
                openai::complete(
                    &self.http,
                    &self.config.api_key,
                    base_url,
                    &self.config.model,
                    system,
                    messages,
                    self.config.max_tokens,
                )
                .await
            }
            Provider::Command { cmd } => {
                command::complete(cmd, system, messages, self.config.max_tokens).await
            }
        }
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }
}
