use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{Message, TransientLlmError};

/// Timeout for external LLM command execution (5 minutes).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

pub async fn complete(
    cmd: &str,
    system: &str,
    messages: &[Message],
    max_tokens: usize,
) -> Result<String> {
    let input = serde_json::json!({
        "system": system,
        "messages": messages,
        "max_tokens": max_tokens,
    });

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to spawn LLM command: {cmd}"))?;

    // Write JSON prompt to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let json_bytes = serde_json::to_vec(&input)?;
        stdin.write_all(&json_bytes).await?;
        // stdin is dropped here, closing the pipe
    }

    // wait_with_output consumes child; on timeout the future is dropped,
    // closing pipes and signaling the subprocess to exit.
    let output = match tokio::time::timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(result) => result.context("Failed to wait for LLM command")?,
        Err(_) => {
            return Err(TransientLlmError(format!(
                "LLM command timed out after {}s",
                COMMAND_TIMEOUT.as_secs()
            ))
            .into());
        }
    };

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        // Treat command failures as transient (could be temporary resource issues)
        return Err(TransientLlmError(format!("LLM command exited with status {code}")).into());
    }

    let text = String::from_utf8(output.stdout)
        .context("LLM command output is not valid UTF-8")?
        .trim()
        .to_string();

    if text.is_empty() {
        bail!("LLM command returned empty response");
    }

    Ok(text)
}
