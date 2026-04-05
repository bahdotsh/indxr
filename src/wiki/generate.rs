use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::diff;
use crate::languages::Language;
use crate::llm::{LlmClient, Message, Role};
use crate::model::declarations::{Declaration, Visibility};
use crate::model::{CodebaseIndex, FileIndex, IndexStats, TreeEntry, WorkspaceIndex};
use crate::parser::ParserRegistry;

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

/// Result of an incremental wiki update.
pub struct UpdateResult {
    pub pages_updated: usize,
    pub pages_removed: usize,
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
        let content_plans: Vec<&PagePlan> = plans
            .iter()
            .filter(|p| p.page_type != PageType::Index)
            .collect();
        let total = content_plans.len();
        for (i, plan) in content_plans.iter().enumerate() {
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

    /// Incremental update: regenerate only wiki pages affected by code changes.
    pub async fn update_affected(
        &self,
        store: &mut WikiStore,
        since_ref: &str,
    ) -> Result<UpdateResult> {
        let root = &self.workspace.root;
        let git_ref = current_git_ref(root)?;
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        // 1. Get changed files since the reference
        let changed_paths = diff::get_changed_files(root, since_ref)?;
        if changed_paths.is_empty() {
            eprintln!("No file changes since {}", since_ref);
            return Ok(UpdateResult {
                pages_updated: 0,
                pages_removed: 0,
            });
        }
        eprintln!(
            "Found {} changed files since {}",
            changed_paths.len(),
            since_ref
        );

        // 2. Build structural diff for context
        let all_files = self.collect_all_files();
        let registry = ParserRegistry::new();
        let mut old_files: HashMap<PathBuf, FileIndex> = HashMap::new();
        for path in &changed_paths {
            if let Ok(Some(old_content)) = diff::get_file_at_ref(root, path, since_ref)
                && let Some(lang) = Language::detect(path)
                && let Some(parser) = registry.get_parser(&lang)
                && let Ok(index) = parser.parse_file(path, &old_content)
            {
                old_files.insert(path.clone(), index);
            }
        }

        let temp_index = CodebaseIndex {
            root: root.to_path_buf(),
            root_name: String::new(),
            generated_at: String::new(),
            files: all_files,
            tree: Vec::new(),
            stats: IndexStats {
                total_files: 0,
                total_lines: 0,
                languages: HashMap::new(),
                duration_ms: 0,
            },
        };
        let structural_diff =
            diff::compute_structural_diff(&temp_index, &old_files, &changed_paths);
        let diff_markdown = diff::format_diff_markdown(&structural_diff);

        // 3. Collect all changed file paths as strings for matching
        let changed_set: HashSet<String> = changed_paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // 4. Find affected pages: any page whose source_files overlap with changed files
        let affected_ids: Vec<String> = store
            .pages
            .iter()
            .filter(|page| {
                page.frontmatter.page_type != PageType::Index
                    && page
                        .frontmatter
                        .source_files
                        .iter()
                        .any(|sf| changed_set.contains(sf))
            })
            .map(|page| page.frontmatter.id.clone())
            .collect();

        if affected_ids.is_empty() {
            eprintln!("No wiki pages are affected by these changes");
            // Still update the ref so we don't re-check the same range
            store.manifest.generated_at_ref = git_ref;
            store.manifest.generated_at = timestamp;
            return Ok(UpdateResult {
                pages_updated: 0,
                pages_removed: 0,
            });
        }

        eprintln!(
            "Updating {} affected pages: {}",
            affected_ids.len(),
            affected_ids.join(", ")
        );

        // 5. Build cross-reference context from all pages
        let all_pages_str: String = store
            .pages
            .iter()
            .map(|p| format!("[[{}]] — {}", p.frontmatter.id, p.frontmatter.title))
            .collect::<Vec<_>>()
            .join("\n");

        // 6. Regenerate each affected page with update context
        let total = affected_ids.len();
        let mut pages_updated = 0;
        for (i, page_id) in affected_ids.iter().enumerate() {
            let existing_page = store.pages.iter().find(|p| &p.frontmatter.id == page_id);
            let existing_page = match existing_page {
                Some(p) => p.clone(),
                None => continue,
            };

            eprintln!(
                "Updating page {}/{}: {}...",
                i + 1,
                total,
                existing_page.frontmatter.title
            );

            let updated = self
                .update_page(
                    &existing_page,
                    &diff_markdown,
                    &all_pages_str,
                    &git_ref,
                    &timestamp,
                )
                .await?;
            store.upsert_page(updated);
            pages_updated += 1;
        }

        // 7. Remove pages whose source files have all been deleted
        let deleted_set: HashSet<String> = structural_diff
            .files_removed
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let mut pages_removed = 0;
        store.pages.retain(|page| {
            if page.frontmatter.page_type == PageType::Index {
                return true;
            }
            if page.frontmatter.source_files.is_empty() {
                return true;
            }
            let all_deleted = page
                .frontmatter
                .source_files
                .iter()
                .all(|sf| deleted_set.contains(sf));
            if all_deleted {
                eprintln!(
                    "Removing page: {} (all source files deleted)",
                    page.frontmatter.id
                );
                pages_removed += 1;
                false
            } else {
                true
            }
        });

        // 8. Regenerate index page if anything changed
        if pages_updated > 0 || pages_removed > 0 {
            eprintln!("Regenerating cross-reference index...");
            let non_index: Vec<WikiPage> = store
                .pages
                .iter()
                .filter(|p| p.frontmatter.page_type != PageType::Index)
                .cloned()
                .collect();
            let index_page = self
                .generate_index(&non_index, &git_ref, &timestamp)
                .await?;
            store.upsert_page(index_page);
        }

        // 9. Update manifest ref
        store.manifest.generated_at_ref = git_ref;
        store.manifest.generated_at = timestamp;

        Ok(UpdateResult {
            pages_updated,
            pages_removed,
        })
    }

    /// Update a single wiki page with diff context.
    async fn update_page(
        &self,
        existing: &WikiPage,
        diff_markdown: &str,
        all_pages_str: &str,
        git_ref: &str,
        timestamp: &str,
    ) -> Result<WikiPage> {
        let mut ctx = String::new();

        ctx.push_str("# Current Wiki Page Content\n\n");
        ctx.push_str(&existing.content);
        ctx.push_str("\n\n");

        ctx.push_str("# Structural Diff\n\n");
        ctx.push_str(diff_markdown);
        ctx.push_str("\n\n");

        ctx.push_str("# Other Wiki Pages\n");
        ctx.push_str(all_pages_str);
        ctx.push_str("\n\n");

        // Fresh structural data for source files
        ctx.push_str("# Current Source File Details\n\n");
        for source_path in &existing.frontmatter.source_files {
            if let Some(file) = self.find_file(source_path) {
                ctx.push_str(&format!("## {}\n", source_path));
                ctx.push_str(&format!(
                    "Language: {}, Lines: {}, Size: {} bytes\n\n",
                    file.language.name(),
                    file.lines,
                    file.size,
                ));
                if !file.imports.is_empty() {
                    ctx.push_str("**Imports:**\n");
                    for imp in &file.imports {
                        ctx.push_str(&format!("- `{}`\n", imp.text));
                    }
                    ctx.push('\n');
                }
                ctx.push_str("**Declarations:**\n");
                format_declarations(&file.declarations, &mut ctx, 0);
                ctx.push('\n');
            }
        }

        let content = self
            .llm
            .complete(
                prompts::update_system_prompt(),
                &[Message {
                    role: Role::User,
                    content: ctx,
                }],
            )
            .await
            .with_context(|| format!("Failed to update wiki page: {}", existing.frontmatter.id))?;

        let links_to = extract_wiki_links(&content);

        Ok(WikiPage {
            frontmatter: Frontmatter {
                id: existing.frontmatter.id.clone(),
                title: existing.frontmatter.title.clone(),
                page_type: existing.frontmatter.page_type.clone(),
                source_files: existing.frontmatter.source_files.clone(),
                generated_at_ref: git_ref.to_string(),
                generated_at: timestamp.to_string(),
                links_to,
                covers: self.extract_covers(&existing.frontmatter.source_files),
            },
            content,
        })
    }

    /// Collect all FileIndex entries across workspace members.
    fn collect_all_files(&self) -> Vec<FileIndex> {
        self.workspace
            .members
            .iter()
            .flat_map(|m| m.index.files.clone())
            .collect()
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

        if plans.is_empty() {
            anyhow::bail!("LLM returned an empty wiki plan — no pages to generate");
        }

        // Sanitize all page IDs to prevent path traversal
        let plans: Vec<PagePlan> = plans
            .into_iter()
            .map(|mut p| {
                p.id = super::page::sanitize_id(&p.id);
                p
            })
            .collect();

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
                    ctx.push_str(&format!("  - {} `{}`", decl.kind, decl.name,));
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

        ctx.push_str("# Page Plan\n");
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
                if file_path == path {
                    return Some(file);
                }
                // Path-component-aware suffix match: the character before the
                // suffix must be a '/' to avoid partial directory matches
                // (e.g. "bar/foo.rs" should not match "foobar/foo.rs").
                if let Some(prefix) = file_path.strip_suffix(path) {
                    if prefix.is_empty() || prefix.ends_with('/') {
                        return Some(file);
                    }
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
    if let Some(after) = trimmed.strip_prefix("```json") {
        if let Some(end) = after.rfind("```") {
            return after[..end].trim();
        }
    }
    if let Some(after) = trimmed.strip_prefix("```") {
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
