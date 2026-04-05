use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::Message;

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

    let output = child
        .wait_with_output()
        .await
        .context("Failed to wait for LLM command")?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        bail!("LLM command exited with status {code}");
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
