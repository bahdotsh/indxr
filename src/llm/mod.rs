mod claude;
mod openai;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

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
    OpenAiCompatible { base_url: String },
}

/// Provider-agnostic LLM client.
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    /// Auto-detect provider from environment variables.
    ///
    /// Checks `ANTHROPIC_API_KEY` first (→ Claude), then `OPENAI_API_KEY` (→ OpenAI-compatible).
    /// Model can be overridden; otherwise sensible defaults are used.
    pub fn from_env(model_override: Option<&str>) -> Result<Self> {
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
            "No LLM API key found. Set ANTHROPIC_API_KEY (for Claude) \
             or OPENAI_API_KEY (for OpenAI-compatible endpoints)."
        )
    }

    pub fn with_config(config: LlmConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Set max tokens for responses.
    pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
        self.config.max_tokens = max_tokens;
        self
    }

    /// Send a system prompt + messages and get a complete response.
    pub async fn complete(&self, system: &str, messages: &[Message]) -> Result<String> {
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
        }
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }
}
