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
        match self {
            PageType::Architecture => write!(f, "Architecture"),
            PageType::Module => write!(f, "Module"),
            PageType::Entity => write!(f, "Entity"),
            PageType::Topic => write!(f, "Topic"),
            PageType::Index => write!(f, "Index"),
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
            },
            content: "# MCP Server\n\nThis module handles the MCP protocol.".to_string(),
        };

        let rendered = page.render().unwrap();
        let parsed = WikiPage::parse(&rendered).unwrap();
        assert_eq!(parsed.frontmatter.id, "mod-mcp");
        assert_eq!(parsed.frontmatter.page_type, PageType::Module);
        assert!(parsed.content.contains("MCP Server"));
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
