use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GitHubPullResponse {
    number: u64,
    title: String,
    state: String,
    base: GitHubRef,
    head: GitHubRef,
}

#[derive(Debug, Deserialize)]
struct GitHubRef {
    #[serde(rename = "ref")]
    ref_name: String,
}

/// PR metadata returned to callers.
#[derive(Debug, Clone)]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub base_ref: String,
    pub head_ref: String,
    pub state: String,
}

/// Resolve a GitHub PR number to a local git ref for its base branch.
///
/// Returns `(local_git_ref, pr_info)` — the ref can be passed directly to
/// `diff::get_changed_files`.
pub fn resolve_pr_base(root: &Path, pr_number: u64) -> Result<(String, PrInfo)> {
    let (owner, repo) = detect_github_repo(root)?;
    let token = get_github_token()?;
    let pr_info = fetch_pr_info(&owner, &repo, pr_number, &token)?;
    let local_ref = resolve_base_ref(root, &pr_info.base_ref)?;
    Ok((local_ref, pr_info))
}

/// Get a GitHub token from environment or `gh` CLI.
fn get_github_token() -> Result<String> {
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }
    if let Ok(token) = std::env::var("GH_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    // Fall back to `gh auth token`
    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .context("Failed to run `gh auth token`. Set GITHUB_TOKEN env var or install the GitHub CLI (`gh`).")?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    bail!(
        "GitHub token not found. Set GITHUB_TOKEN or GH_TOKEN env var, \
         or authenticate with the GitHub CLI (`gh auth login`)."
    )
}

/// Extract `(owner, repo)` from the git remote URL.
fn detect_github_repo(root: &Path) -> Result<(String, String)> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(root)
        .output()
        .context("Failed to run `git remote get-url origin`")?;

    if !output.status.success() {
        bail!("No git remote 'origin' found. PR diffs require a GitHub remote.");
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_github_url(&url)
}

/// Parse a GitHub URL (HTTPS or SSH) into `(owner, repo)`.
fn parse_github_url(url: &str) -> Result<(String, String)> {
    let re = regex::Regex::new(r"github\.com[:/]([^/]+)/([^/.]+)").unwrap();
    let caps = re
        .captures(url)
        .with_context(|| format!("Remote 'origin' is not a GitHub URL: {url}"))?;
    Ok((caps[1].to_string(), caps[2].to_string()))
}

/// Fetch PR metadata from the GitHub REST API.
fn fetch_pr_info(owner: &str, repo: &str, pr_number: u64, token: &str) -> Result<PrInfo> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{pr_number}");

    let response = ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "indxr")
        .call();

    match response {
        Ok(resp) => {
            let pr: GitHubPullResponse = resp.into_json().context("Failed to parse GitHub API response")?;
            Ok(PrInfo {
                number: pr.number,
                title: pr.title,
                base_ref: pr.base.ref_name,
                head_ref: pr.head.ref_name,
                state: pr.state,
            })
        }
        Err(ureq::Error::Status(404, _)) => {
            bail!("PR #{pr_number} not found in {owner}/{repo}")
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            bail!(
                "GitHub API authentication failed (HTTP 401/403). \
                 Check that your token has repo access."
            )
        }
        Err(e) => {
            bail!("GitHub API request failed: {e}")
        }
    }
}

/// Resolve a branch name to a local git ref, preferring `origin/{branch}`.
fn resolve_base_ref(root: &Path, base_branch: &str) -> Result<String> {
    let remote_ref = format!("origin/{base_branch}");

    // Check if origin/<branch> exists locally
    let status = Command::new("git")
        .args(["rev-parse", "--verify", &remote_ref])
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(s) = status {
        if s.success() {
            return Ok(remote_ref);
        }
    }

    // Check if bare branch name exists locally
    let status = Command::new("git")
        .args(["rev-parse", "--verify", base_branch])
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(s) = status {
        if s.success() {
            return Ok(base_branch.to_string());
        }
    }

    bail!(
        "Base branch '{base_branch}' not found locally. \
         Run `git fetch origin {base_branch}` and retry."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url_https() {
        let (owner, repo) = parse_github_url("https://github.com/bahdotsh/indxr.git").unwrap();
        assert_eq!(owner, "bahdotsh");
        assert_eq!(repo, "indxr");
    }

    #[test]
    fn test_parse_github_url_https_no_suffix() {
        let (owner, repo) = parse_github_url("https://github.com/bahdotsh/indxr").unwrap();
        assert_eq!(owner, "bahdotsh");
        assert_eq!(repo, "indxr");
    }

    #[test]
    fn test_parse_github_url_ssh() {
        let (owner, repo) = parse_github_url("git@github.com:bahdotsh/indxr.git").unwrap();
        assert_eq!(owner, "bahdotsh");
        assert_eq!(repo, "indxr");
    }

    #[test]
    fn test_parse_github_url_ssh_no_suffix() {
        let (owner, repo) = parse_github_url("git@github.com:bahdotsh/indxr").unwrap();
        assert_eq!(owner, "bahdotsh");
        assert_eq!(repo, "indxr");
    }

    #[test]
    fn test_parse_github_url_non_github() {
        let result = parse_github_url("https://gitlab.com/owner/repo.git");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_github_url_malformed() {
        let result = parse_github_url("not-a-url");
        assert!(result.is_err());
    }
}
