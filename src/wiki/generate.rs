use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::llm::{LlmClient, Message, Role};
use crate::model::declarations::{Declaration, Visibility};
use crate::model::{FileIndex, TreeEntry, WorkspaceIndex};

use super::page::{Frontmatter, PageType, WikiPage};
use super::prompts;
use super::store::WikiStore;

/// Plan for a single wiki page, returned by the planning LLM call.
#[derive(Debug, Deserialize)]
struct PagePlan {
    id: String,
    page_type: PageType,
    title: String,
    source_files: Vec<String>,
}

pub struct WikiGenerator<'a> {
    llm: &'a LlmClient,
    workspace: &'a WorkspaceIndex,
}

impl<'a> WikiGenerator<'a> {
    pub fn new(llm: &'a LlmClient, workspace: &'a WorkspaceIndex) -> Self {
        Self { llm, workspace }
    }

    /// Full wiki generation from scratch.
    pub async fn generate_full(&self, wiki_dir: &Path, dry_run: bool) -> Result<WikiStore> {
        let git_ref = current_git_ref(&self.workspace.root)?;
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        // Stage 1: Plan the wiki structure
        eprintln!("Planning wiki structure...");
        let plans = self.plan_structure().await?;
        eprintln!("Planned {} pages", plans.len());

        if dry_run {
            eprintln!("\n--- Dry run: wiki plan ---");
            for plan in &plans {
                eprintln!(
                    "  [{:?}] {} — {} ({})",
                    plan.page_type,
                    plan.id,
                    plan.title,
                    plan.source_files.len()
                );
                for f in &plan.source_files {
                    eprintln!("    - {}", f);
                }
            }
            return Ok(WikiStore::new(wiki_dir));
        }

        // Build lookup of all page titles for cross-referencing
        let all_pages_ctx: Vec<String> = plans
            .iter()
            .map(|p| format!("[[{}]] — {}", p.id, p.title))
            .collect();
        let all_pages_str = all_pages_ctx.join("\n");

        let mut store = WikiStore::new(wiki_dir);
        store.manifest.generated_at_ref = git_ref.clone();
        store.manifest.generated_at = timestamp.clone();

        // Stage 2: Generate each page
        let total = plans.len();
        for (i, plan) in plans.iter().enumerate() {
            if plan.page_type == PageType::Index {
                continue; // generate index last
            }
            eprintln!("Generating page {}/{}: {}...", i + 1, total, plan.title);

            let page = self
                .generate_page(plan, &all_pages_str, &git_ref, &timestamp)
                .await?;
            store.upsert_page(page);
        }

        // Stage 3: Generate index page
        eprintln!("Generating cross-reference index...");
        let index_page = self
            .generate_index(&store.pages, &git_ref, &timestamp)
            .await?;
        store.upsert_page(index_page);

        Ok(store)
    }

    /// Ask the LLM to plan the wiki structure from the structural index.
    async fn plan_structure(&self) -> Result<Vec<PagePlan>> {
        let context = self.build_planning_context();

        let response = self
            .llm
            .complete(
                prompts::plan_system_prompt(),
                &[Message {
                    role: Role::User,
                    content: context,
                }],
            )
            .await
            .context("Failed to get wiki plan from LLM")?;

        // Parse JSON from response (handle potential markdown fencing)
        let json_str = extract_json(&response);
        let plans: Vec<PagePlan> =
            serde_json::from_str(json_str).context("Failed to parse wiki plan JSON from LLM")?;

        Ok(plans)
    }

    /// Generate a single wiki page.
    async fn generate_page(
        &self,
        plan: &PagePlan,
        all_pages_str: &str,
        git_ref: &str,
        timestamp: &str,
    ) -> Result<WikiPage> {
        let page_type_str = format!("{:?}", plan.page_type).to_lowercase();
        let system = prompts::page_system_prompt(&page_type_str);

        let context = self.build_page_context(plan, all_pages_str);

        let content = self
            .llm
            .complete(
                &system,
                &[Message {
                    role: Role::User,
                    content: context,
                }],
            )
            .await
            .with_context(|| format!("Failed to generate wiki page: {}", plan.id))?;

        // Extract cross-references from the generated content
        let links_to = extract_wiki_links(&content);

        Ok(WikiPage {
            frontmatter: Frontmatter {
                id: plan.id.clone(),
                title: plan.title.clone(),
                page_type: plan.page_type.clone(),
                source_files: plan.source_files.clone(),
                generated_at_ref: git_ref.to_string(),
                generated_at: timestamp.to_string(),
                links_to,
                covers: self.extract_covers(&plan.source_files),
            },
            content,
        })
    }

    /// Generate the cross-reference index page.
    async fn generate_index(
        &self,
        pages: &[WikiPage],
        git_ref: &str,
        timestamp: &str,
    ) -> Result<WikiPage> {
        let mut ctx = String::from("Wiki pages to index:\n\n");
        for page in pages {
            ctx.push_str(&format!(
                "- [[{}]] (type: {:?}) — {}\n  Covers: {}\n",
                page.frontmatter.id,
                page.frontmatter.page_type,
                page.frontmatter.title,
                if page.frontmatter.covers.is_empty() {
                    "(general)".to_string()
                } else {
                    page.frontmatter.covers.join(", ")
                }
            ));
        }

        let content = self
            .llm
            .complete(
                prompts::index_system_prompt(),
                &[Message {
                    role: Role::User,
                    content: ctx,
                }],
            )
            .await
            .context("Failed to generate wiki index")?;

        let links_to: Vec<String> = pages.iter().map(|p| p.frontmatter.id.clone()).collect();

        Ok(WikiPage {
            frontmatter: Frontmatter {
                id: "index".to_string(),
                title: "Wiki Index".to_string(),
                page_type: PageType::Index,
                source_files: Vec::new(),
                generated_at_ref: git_ref.to_string(),
                generated_at: timestamp.to_string(),
                links_to,
                covers: Vec::new(),
            },
            content,
        })
    }

    /// Build the context string for the planning call.
    fn build_planning_context(&self) -> String {
        let mut ctx = String::new();

        ctx.push_str("# Codebase Structural Index\n\n");

        for member in &self.workspace.members {
            if self.workspace.members.len() > 1 {
                ctx.push_str(&format!("## Workspace member: {}\n\n", member.name));
            }

            // Directory tree
            ctx.push_str("### Directory Tree\n```\n");
            format_tree(&member.index.tree, &mut ctx);
            ctx.push_str("```\n\n");

            // Per-file summaries (compact)
            ctx.push_str("### Files\n\n");
            for file in &member.index.files {
                let path = file.path.to_string_lossy();
                let decl_count = count_declarations(&file.declarations);
                let public_count = count_public(&file.declarations);

                ctx.push_str(&format!(
                    "**{}** ({}, {} lines, {} decls, {} public)\n",
                    path,
                    file.language.name(),
                    file.lines,
                    decl_count,
                    public_count,
                ));

                // List top-level declarations (name + kind only for planning)
                for decl in &file.declarations {
                    ctx.push_str(&format!(
                        "  - {} `{}`",
                        decl.kind, decl.name,
                    ));
                    if !decl.children.is_empty() {
                        ctx.push_str(&format!(" ({} children)", decl.children.len()));
                    }
                    ctx.push('\n');
                }
                ctx.push('\n');
            }
        }

        // Stats
        ctx.push_str(&format!(
            "### Stats\n- Total files: {}\n- Total lines: {}\n",
            self.workspace.stats.total_files, self.workspace.stats.total_lines,
        ));
        for (lang, count) in &self.workspace.stats.languages {
            ctx.push_str(&format!("- {}: {} files\n", lang, count));
        }

        ctx
    }

    /// Build the context for generating a single page.
    fn build_page_context(&self, plan: &PagePlan, all_pages_str: &str) -> String {
        let mut ctx = String::new();

        ctx.push_str(&format!("# Page Plan\n"));
        ctx.push_str(&format!("- ID: {}\n", plan.id));
        ctx.push_str(&format!("- Title: {}\n", plan.title));
        ctx.push_str(&format!("- Type: {:?}\n\n", plan.page_type));

        // All other wiki pages (for cross-referencing)
        ctx.push_str("# Other Wiki Pages\n");
        ctx.push_str(all_pages_str);
        ctx.push_str("\n\n");

        // Structural data for source files
        ctx.push_str("# Source File Details\n\n");
        for source_path in &plan.source_files {
            if let Some(file) = self.find_file(source_path) {
                ctx.push_str(&format!("## {}\n", source_path));
                ctx.push_str(&format!(
                    "Language: {}, Lines: {}, Size: {} bytes\n\n",
                    file.language.name(),
                    file.lines,
                    file.size,
                ));

                // Imports
                if !file.imports.is_empty() {
                    ctx.push_str("**Imports:**\n");
                    for imp in &file.imports {
                        ctx.push_str(&format!("- `{}`\n", imp.text));
                    }
                    ctx.push('\n');
                }

                // Declarations with full signatures
                ctx.push_str("**Declarations:**\n");
                format_declarations(&file.declarations, &mut ctx, 0);
                ctx.push('\n');
            }
        }

        ctx
    }

    /// Extract "kind:name" covers from source files.
    fn extract_covers(&self, source_files: &[String]) -> Vec<String> {
        let mut covers = Vec::new();
        for path in source_files {
            if let Some(file) = self.find_file(path) {
                for decl in &file.declarations {
                    if matches!(decl.visibility, Visibility::Public) {
                        covers.push(format!("{}:{}", decl.kind, decl.name));
                    }
                }
            }
        }
        covers
    }

    fn find_file(&self, path: &str) -> Option<&FileIndex> {
        for member in &self.workspace.members {
            for file in &member.index.files {
                let file_path = file.path.to_string_lossy();
                if file_path == path || file_path.ends_with(path) {
                    return Some(file);
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_tree(entries: &[TreeEntry], out: &mut String) {
    for entry in entries {
        let indent = "  ".repeat(entry.depth);
        let suffix = if entry.is_dir { "/" } else { "" };
        out.push_str(&format!("{}{}{}\n", indent, entry.path, suffix));
    }
}

fn format_declarations(decls: &[Declaration], out: &mut String, depth: usize) {
    let indent = "  ".repeat(depth);
    for decl in decls {
        let vis = match decl.visibility {
            Visibility::Public => "pub ",
            Visibility::PublicCrate => "pub(crate) ",
            Visibility::Private => "",
        };
        out.push_str(&format!(
            "{}- {} {}{}`{}`",
            indent,
            decl.kind,
            vis,
            if decl.is_async { "async " } else { "" },
            decl.signature,
        ));
        if let Some(ref doc) = decl.doc_comment {
            let short = doc.lines().next().unwrap_or("").trim();
            if !short.is_empty() {
                let truncated = if short.len() > 100 {
                    format!("{}...", &short[..100])
                } else {
                    short.to_string()
                };
                out.push_str(&format!(" — {}", truncated));
            }
        }
        out.push('\n');

        if !decl.children.is_empty() {
            format_declarations(&decl.children, out, depth + 1);
        }
    }
}

fn count_declarations(decls: &[Declaration]) -> usize {
    let mut count = decls.len();
    for d in decls {
        count += count_declarations(&d.children);
    }
    count
}

fn count_public(decls: &[Declaration]) -> usize {
    let mut count = 0;
    for d in decls {
        if matches!(d.visibility, Visibility::Public) {
            count += 1;
        }
        count += count_public(&d.children);
    }
    count
}

/// Get the current HEAD commit hash.
fn current_git_ref(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .context("Failed to run git rev-parse HEAD")?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Ok("unknown".to_string())
    }
}

/// Extract JSON content from an LLM response that might be wrapped in markdown fencing.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.starts_with("```json") {
        let after = &trimmed[7..];
        if let Some(end) = after.rfind("```") {
            return after[..end].trim();
        }
    }
    if trimmed.starts_with("```") {
        let after = &trimmed[3..];
        if let Some(end) = after.rfind("```") {
            return after[..end].trim();
        }
    }
    trimmed
}

/// Extract [[page-id]] wiki links from content.
fn extract_wiki_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("]]") {
            let link = after[..end].to_string();
            if !links.contains(&link) {
                links.push(link);
            }
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let input = r#"[{"id": "test"}]"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_fenced() {
        let input = "```json\n[{\"id\": \"test\"}]\n```";
        assert_eq!(extract_json(input), "[{\"id\": \"test\"}]");
    }

    #[test]
    fn test_extract_wiki_links() {
        let content = "See [[architecture]] and [[mod-parser]] for details. Also [[architecture]].";
        let links = extract_wiki_links(content);
        assert_eq!(links, vec!["architecture", "mod-parser"]);
    }
}
