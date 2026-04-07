use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// A wiki page with YAML frontmatter and markdown content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    pub frontmatter: Frontmatter,
    /// Markdown body (without the frontmatter delimiters).
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frontmatter {
    /// Unique page identifier (slug), e.g. "architecture", "mod-mcp", "entity-cache".
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Page type.
    pub page_type: PageType,
    /// Source files that contributed to this page's content.
    #[serde(default)]
    pub source_files: Vec<String>,
    /// Git commit hash at which this page was last generated/updated.
    #[serde(default)]
    pub generated_at_ref: String,
    /// ISO 8601 timestamp of last generation.
    #[serde(default)]
    pub generated_at: String,
    /// Other wiki page IDs that this page links to.
    #[serde(default)]
    pub links_to: Vec<String>,
    /// Declarations covered by this page, e.g. "fn:handle_tool_call", "struct:Cache".
    #[serde(default)]
    pub covers: Vec<String>,
    /// Contradictions detected during updates — where new code state
    /// contradicts what the wiki previously stated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contradictions: Vec<Contradiction>,
    /// Recorded failure patterns — failed fix attempts and their diagnoses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<FailurePattern>,
}

/// A contradiction detected during wiki update — where new code
/// contradicts something the wiki previously stated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contradiction {
    /// Human-readable description of the contradiction.
    pub description: String,
    /// Source location, e.g. "src/mcp/mod.rs:383".
    pub source: String,
    /// ISO 8601 timestamp when the contradiction was detected.
    pub detected_at: String,
    /// ISO 8601 timestamp when the contradiction was resolved, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

/// A recorded failure pattern — what was tried, why it failed, and what actually worked.
/// Helps future agents avoid repeating the same mistakes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    /// What was observed (error message, test failure, unexpected behavior).
    pub symptom: String,
    /// What fix was attempted.
    pub attempted_fix: String,
    /// Why the fix didn't work / root cause analysis.
    pub diagnosis: String,
    /// What actually worked (filled in later when the issue is resolved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_fix: Option<String>,
    /// Source files involved in this failure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_files: Vec<String>,
    /// ISO 8601 timestamp when this failure was recorded.
    pub recorded_at: String,
    /// ISO 8601 timestamp when the actual fix was provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

impl FailurePattern {
    /// Parse a `FailurePattern` from a JSON value, using `now` as the `recorded_at` timestamp.
    /// Returns `None` if required fields (`symptom`, `attempted_fix`, `diagnosis`) are missing.
    pub fn from_json(v: &serde_json::Value, now: &str) -> Option<Self> {
        let symptom = v.get("symptom")?.as_str()?;
        let attempted_fix = v.get("attempted_fix")?.as_str()?;
        let diagnosis = v.get("diagnosis")?.as_str()?;
        let actual_fix = v.get("actual_fix").and_then(|v| v.as_str());
        let source_files: Vec<String> = v
            .get("source_files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            symptom: symptom.to_string(),
            attempted_fix: attempted_fix.to_string(),
            diagnosis: diagnosis.to_string(),
            actual_fix: actual_fix.map(String::from),
            source_files,
            recorded_at: now.to_string(),
            resolved_at: if actual_fix.is_some() {
                Some(now.to_string())
            } else {
                None
            },
        })
    }

    /// Serialize to a summary JSON value (for search results — omits timestamps).
    pub fn to_json_summary(&self) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "symptom": self.symptom,
            "attempted_fix": self.attempted_fix,
            "diagnosis": self.diagnosis,
        });
        if let Some(ref fix) = self.actual_fix {
            obj["actual_fix"] = serde_json::json!(fix);
        }
        if !self.source_files.is_empty() {
            obj["source_files"] = serde_json::json!(self.source_files);
        }
        if self.resolved_at.is_some() {
            obj["resolved"] = serde_json::json!(true);
        }
        obj
    }

    /// Serialize to a detailed JSON value (for page reads — includes index and timestamps).
    pub fn to_json_detail(&self, index: usize) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "index": index,
            "symptom": self.symptom,
            "attempted_fix": self.attempted_fix,
            "diagnosis": self.diagnosis,
            "recorded_at": self.recorded_at,
        });
        if let Some(ref fix) = self.actual_fix {
            obj["actual_fix"] = serde_json::json!(fix);
        }
        if let Some(ref resolved) = self.resolved_at {
            obj["resolved_at"] = serde_json::json!(resolved);
        }
        if !self.source_files.is_empty() {
            obj["source_files"] = serde_json::json!(self.source_files);
        }
        obj
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageType {
    Architecture,
    Module,
    Entity,
    Topic,
    Index,
}

impl std::fmt::Display for PageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str_title())
    }
}

impl PageType {
    /// Lowercase string form for use in prompts and serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            PageType::Architecture => "architecture",
            PageType::Module => "module",
            PageType::Entity => "entity",
            PageType::Topic => "topic",
            PageType::Index => "index",
        }
    }

    /// Title-case string form for display.
    fn as_str_title(&self) -> &'static str {
        match self {
            PageType::Architecture => "Architecture",
            PageType::Module => "Module",
            PageType::Entity => "Entity",
            PageType::Topic => "Topic",
            PageType::Index => "Index",
        }
    }
}

impl PageType {
    /// Subdirectory within the wiki root for this page type.
    pub fn subdir(&self) -> Option<&'static str> {
        match self {
            PageType::Module => Some("modules"),
            PageType::Entity => Some("entities"),
            PageType::Topic => Some("topics"),
            PageType::Architecture | PageType::Index => None,
        }
    }
}

impl WikiPage {
    /// Parse a wiki page from its on-disk representation (YAML frontmatter + markdown).
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim_start();
        if !text.starts_with("---") {
            bail!("Wiki page missing YAML frontmatter delimiter");
        }

        // Find the closing ---
        let after_first = &text[3..];
        let end = after_first
            .find("\n---")
            .context("Wiki page missing closing frontmatter delimiter")?;

        let yaml_str = &after_first[..end];
        let content_start = 3 + end + 4; // skip "---" + yaml + "\n---"
        let content = if content_start < text.len() {
            text[content_start..].trim_start_matches('\n').to_string()
        } else {
            String::new()
        };

        let frontmatter: Frontmatter =
            serde_yaml::from_str(yaml_str).context("Failed to parse wiki page frontmatter")?;

        Ok(WikiPage {
            frontmatter,
            content,
        })
    }

    /// Serialize to the on-disk format (YAML frontmatter + markdown).
    pub fn render(&self) -> Result<String> {
        let yaml =
            serde_yaml::to_string(&self.frontmatter).context("Failed to serialize frontmatter")?;
        Ok(format!("---\n{}---\n\n{}\n", yaml, self.content))
    }

    /// Filename for this page on disk.
    pub fn filename(&self) -> String {
        format!("{}.md", sanitize_id(&self.frontmatter.id))
    }
}

/// Sanitize a page ID to only allow safe filesystem characters: [a-z0-9-_].
/// Lowercases first, then strips everything else to prevent path traversal.
pub fn sanitize_id(id: &str) -> String {
    id.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let page = WikiPage {
            frontmatter: Frontmatter {
                id: "mod-mcp".to_string(),
                title: "MCP Server Module".to_string(),
                page_type: PageType::Module,
                source_files: vec!["src/mcp/mod.rs".to_string()],
                generated_at_ref: "abc123".to_string(),
                generated_at: "2026-04-05T10:00:00Z".to_string(),
                links_to: vec!["architecture".to_string()],
                covers: vec!["fn:run_mcp_server".to_string()],
                contradictions: vec![],
                failures: vec![],
            },
            content: "# MCP Server\n\nThis module handles the MCP protocol.".to_string(),
        };

        let rendered = page.render().unwrap();
        let parsed = WikiPage::parse(&rendered).unwrap();
        assert_eq!(parsed.frontmatter.id, "mod-mcp");
        assert_eq!(parsed.frontmatter.page_type, PageType::Module);
        assert!(parsed.content.contains("MCP Server"));
        assert!(parsed.frontmatter.contradictions.is_empty());
    }

    #[test]
    fn test_roundtrip_with_contradictions() {
        let page = WikiPage {
            frontmatter: Frontmatter {
                id: "mod-mcp".to_string(),
                title: "MCP Server Module".to_string(),
                page_type: PageType::Module,
                source_files: vec!["src/mcp/mod.rs".to_string()],
                generated_at_ref: "abc123".to_string(),
                generated_at: "2026-04-05T10:00:00Z".to_string(),
                links_to: vec![],
                covers: vec![],
                contradictions: vec![Contradiction {
                    description: "Wiki stated sync channels but code now uses async".to_string(),
                    source: "src/mcp/mod.rs:383".to_string(),
                    detected_at: "2026-04-06T10:00:00Z".to_string(),
                    resolved_at: None,
                }],
                failures: vec![],
            },
            content: "# MCP Server".to_string(),
        };

        let rendered = page.render().unwrap();
        let parsed = WikiPage::parse(&rendered).unwrap();
        assert_eq!(parsed.frontmatter.contradictions.len(), 1);
        assert_eq!(
            parsed.frontmatter.contradictions[0].source,
            "src/mcp/mod.rs:383"
        );
        assert!(parsed.frontmatter.contradictions[0].resolved_at.is_none());
    }

    #[test]
    fn test_backward_compat_no_contradictions_field() {
        // Simulate a v1 wiki page with no contradictions field in YAML
        let yaml = r#"---
id: mod-old
title: Old Module
page_type: module
source_files:
  - src/old.rs
generated_at_ref: abc123
generated_at: "2026-01-01T00:00:00Z"
links_to: []
covers: []
---

# Old Module
"#;
        let parsed = WikiPage::parse(yaml).unwrap();
        assert_eq!(parsed.frontmatter.id, "mod-old");
        assert!(parsed.frontmatter.contradictions.is_empty());
    }

    #[test]
    fn test_roundtrip_with_failures() {
        let page = WikiPage {
            frontmatter: Frontmatter {
                id: "mod-parser".to_string(),
                title: "Parser Module".to_string(),
                page_type: PageType::Module,
                source_files: vec!["src/parser/mod.rs".to_string()],
                generated_at_ref: "abc123".to_string(),
                generated_at: "2026-04-05T10:00:00Z".to_string(),
                links_to: vec![],
                covers: vec![],
                contradictions: vec![],
                failures: vec![FailurePattern {
                    symptom: "test_parse_nested panics with double-free".to_string(),
                    attempted_fix: "Added clone() in parse_expression".to_string(),
                    diagnosis: "Root cause was lifetime escape, not missing clone".to_string(),
                    actual_fix: Some("Restructured callback ownership".to_string()),
                    source_files: vec!["src/parser/mod.rs".to_string()],
                    recorded_at: "2026-04-06T10:00:00Z".to_string(),
                    resolved_at: Some("2026-04-06T11:00:00Z".to_string()),
                }],
            },
            content: "# Parser Module".to_string(),
        };

        let rendered = page.render().unwrap();
        let parsed = WikiPage::parse(&rendered).unwrap();
        assert_eq!(parsed.frontmatter.failures.len(), 1);
        assert_eq!(
            parsed.frontmatter.failures[0].symptom,
            "test_parse_nested panics with double-free"
        );
        assert_eq!(
            parsed.frontmatter.failures[0].actual_fix.as_deref(),
            Some("Restructured callback ownership")
        );
        assert!(parsed.frontmatter.failures[0].resolved_at.is_some());
    }

    #[test]
    fn test_backward_compat_no_failures_field() {
        // Simulate an older wiki page with no failures field in YAML
        let yaml = r#"---
id: mod-old
title: Old Module
page_type: module
source_files:
  - src/old.rs
generated_at_ref: abc123
generated_at: "2026-01-01T00:00:00Z"
links_to: []
covers: []
---

# Old Module
"#;
        let parsed = WikiPage::parse(yaml).unwrap();
        assert_eq!(parsed.frontmatter.id, "mod-old");
        assert!(parsed.frontmatter.failures.is_empty());
    }

    #[test]
    fn test_sanitize_id_strips_path_traversal() {
        assert_eq!(sanitize_id("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_id("mod-parser"), "mod-parser");
        assert_eq!(sanitize_id("entity_cache"), "entity_cache");
        assert_eq!(sanitize_id("a b c"), "abc");
    }

    #[test]
    fn test_sanitize_id_lowercases() {
        assert_eq!(sanitize_id("MCP-Server"), "mcp-server");
        assert_eq!(sanitize_id("Hello/World"), "helloworld");
        assert_eq!(sanitize_id("Architecture"), "architecture");
        assert_eq!(sanitize_id("MOD-PARSER"), "mod-parser");
    }

    #[test]
    fn test_sanitize_id_empty_result() {
        assert_eq!(sanitize_id("///"), "");
        assert_eq!(sanitize_id(""), "");
    }
}
