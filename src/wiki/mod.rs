mod generate;
pub mod page;
mod prompts;
pub mod store;

pub(crate) use generate::UpdateResult;
pub(crate) use generate::WikiGenerator;
pub(crate) use generate::build_planning_context;
pub(crate) use generate::extract_wiki_links;
pub(crate) use generate::floor_char_boundary;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;

use crate::cli::WikiAction;
use crate::diff;
use crate::llm::LlmClient;
use crate::model::WorkspaceIndex;

/// Shared health report for wiki status, used by both CLI and MCP tool.
pub(crate) struct WikiHealthReport {
    pub pages: usize,
    pub pages_by_type: HashMap<String, usize>,
    pub generated_at_ref: String,
    pub generated_at: String,
    pub commits_behind: usize,
    pub staleness: String,
    pub covered_files: usize,
    pub total_files: usize,
    pub coverage_pct: String,
    /// Pages whose source files changed since last generation.
    pub affected_pages: Vec<String>,
    /// Files not covered by any wiki page.
    pub uncovered_files: Vec<String>,
}

pub(crate) fn compute_wiki_health(
    store: &store::WikiStore,
    workspace: &WorkspaceIndex,
) -> WikiHealthReport {
    let mut pages_by_type: HashMap<String, usize> = HashMap::new();
    for page in &store.pages {
        *pages_by_type
            .entry(page.frontmatter.page_type.as_str().to_string())
            .or_insert(0) += 1;
    }

    let since_ref = &store.manifest.generated_at_ref;
    let behind = if !since_ref.is_empty() {
        commits_behind(&workspace.root, since_ref).unwrap_or(0)
    } else {
        0
    };

    let staleness = if behind == 0 {
        "up to date".to_string()
    } else {
        format!("{} commit(s) behind HEAD", behind)
    };

    // Affected pages
    let affected_pages = if behind > 0 && !since_ref.is_empty() {
        if let Ok(changed) = diff::get_changed_files(&workspace.root, since_ref) {
            let changed_strs: HashSet<String> = changed
                .iter()
                .filter_map(|p| p.to_str().map(|s| s.to_string()))
                .collect();
            store
                .pages
                .iter()
                .filter(|page| {
                    page.frontmatter
                        .source_files
                        .iter()
                        .any(|sf| changed_strs.contains(sf.as_str()))
                })
                .map(|page| page.frontmatter.title.clone())
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Coverage
    let covered: HashSet<&str> = store
        .pages
        .iter()
        .flat_map(|p| p.frontmatter.source_files.iter().map(|s| s.as_str()))
        .collect();

    let all_files: Vec<String> = workspace
        .members
        .iter()
        .flat_map(|m| {
            m.index
                .files
                .iter()
                .map(|f| f.path.to_string_lossy().to_string())
        })
        .collect();
    let total_files = all_files.len();
    let uncovered_files: Vec<String> = all_files
        .iter()
        .filter(|f| !covered.contains(f.as_str()))
        .cloned()
        .collect();
    let covered_files = total_files - uncovered_files.len();

    let coverage_pct = if total_files > 0 {
        format!(
            "{:.0}%",
            (covered_files as f64 / total_files as f64) * 100.0
        )
    } else {
        "100%".to_string()
    };

    WikiHealthReport {
        pages: store.pages.len(),
        pages_by_type,
        generated_at_ref: store.manifest.generated_at_ref.clone(),
        generated_at: store.manifest.generated_at.clone(),
        commits_behind: behind,
        staleness,
        covered_files,
        total_files,
        coverage_pct,
        affected_pages,
        uncovered_files,
    }
}

/// Result of a compound operation.
pub(crate) struct CompoundResult {
    pub action: String,
    pub id: String,
    pub title: String,
}

/// Compound synthesized knowledge into the wiki. Scores the synthesis against
/// existing pages and either appends to the best match or creates a new topic page.
pub(crate) fn compound_into_wiki(
    store: &mut store::WikiStore,
    synthesis: &str,
    source_pages: &[String],
    title: Option<&str>,
) -> Result<CompoundResult> {
    use page::{Frontmatter, PageType, WikiPage, sanitize_id};

    let source_refs: Vec<&str> = source_pages.iter().map(|s| s.as_str()).collect();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Score existing pages
    let scored = score_pages(store, synthesis, &source_refs);
    let best = scored.first().map(|(score, id)| (*score, id.clone()));

    if let Some((best_score, target_id)) = best {
        if best_score >= 20 {
            let old = match store.get_page(&target_id) {
                Some(p) => p,
                None => anyhow::bail!("Internal error: scored page '{}' not found", target_id),
            };
            let mut new_content = old.content.clone();
            new_content.push_str(&format!(
                "\n\n---\n\n### Compounded insight\n\n{}",
                synthesis
            ));
            let mut links_to = old.frontmatter.links_to.clone();
            for sp in source_pages {
                if !links_to.contains(sp) && *sp != target_id {
                    links_to.push(sp.clone());
                }
            }
            for link in extract_wiki_links(synthesis) {
                if !links_to.contains(&link) {
                    links_to.push(link);
                }
            }
            let title_str = old.frontmatter.title.clone();
            let page = WikiPage {
                frontmatter: Frontmatter {
                    id: target_id.clone(),
                    title: title_str.clone(),
                    page_type: old.frontmatter.page_type.clone(),
                    source_files: old.frontmatter.source_files.clone(),
                    generated_at_ref: old.frontmatter.generated_at_ref.clone(),
                    generated_at: now,
                    links_to,
                    covers: old.frontmatter.covers.clone(),
                    contradictions: old.frontmatter.contradictions.clone(),
                    failures: old.frontmatter.failures.clone(),
                },
                content: new_content,
            };
            store.upsert_page(page);
            store.save_incremental(&target_id)?;
            return Ok(CompoundResult {
                action: "compounded".to_string(),
                id: target_id,
                title: title_str,
            });
        }
    }

    // Create new topic page
    let base_id = if let Some(t) = title {
        sanitize_id(t)
    } else {
        sanitize_id(&derive_topic_id(synthesis))
    };
    if base_id.is_empty() {
        anyhow::bail!("Could not derive a valid page ID. Provide a --title.");
    }

    // Avoid collisions: if the ID already exists, append a numeric suffix
    let page_id = if store.get_page(&base_id).is_none() {
        base_id
    } else {
        let mut suffix = 2;
        loop {
            let candidate = format!("{}-{}", base_id, suffix);
            if store.get_page(&candidate).is_none() {
                break candidate;
            }
            suffix += 1;
        }
    };

    let page_title = title.map(|s| s.to_string()).unwrap_or_else(|| {
        let words: Vec<String> = synthesis
            .split_whitespace()
            .filter(|w| w.len() >= 4)
            .take(5)
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                }
            })
            .collect();
        if words.is_empty() {
            "New Topic".to_string()
        } else {
            words.join(" ")
        }
    });

    let mut links_to: Vec<String> = source_pages.to_vec();
    for link in extract_wiki_links(synthesis) {
        if !links_to.contains(&link) {
            links_to.push(link);
        }
    }

    let page = WikiPage {
        frontmatter: Frontmatter {
            id: page_id.clone(),
            title: page_title.clone(),
            page_type: PageType::Topic,
            source_files: Vec::new(),
            generated_at_ref: store.manifest.generated_at_ref.clone(),
            generated_at: now,
            links_to,
            covers: Vec::new(),
            contradictions: Vec::new(),
            failures: Vec::new(),
        },
        content: format!("# {}\n\n{}", page_title, synthesis),
    };
    store.upsert_page(page);
    store.save_incremental(&page_id)?;
    Ok(CompoundResult {
        action: "created".to_string(),
        id: page_id,
        title: page_title,
    })
}

/// Score pages for synthesis routing (shared between MCP tool and CLI).
pub(crate) fn score_pages(
    store: &store::WikiStore,
    synthesis: &str,
    source_pages: &[&str],
) -> Vec<(usize, String)> {
    let synthesis_lower = synthesis.to_lowercase();
    let synthesis_words: Vec<&str> = synthesis_lower.split_whitespace().collect();

    let mut scored: Vec<(usize, String)> = store
        .pages
        .iter()
        .filter(|p| p.frontmatter.page_type != page::PageType::Index)
        .filter_map(|p| {
            let mut score = 0usize;
            if source_pages.contains(&p.frontmatter.id.as_str()) {
                score += 50;
            }
            let title_lower = p.frontmatter.title.to_lowercase();
            for word in &synthesis_words {
                if word.len() >= 4 && title_lower.contains(word) {
                    score += 10;
                }
            }
            let content_body = p
                .content
                .strip_prefix('#')
                .and_then(|s| s.find('\n').map(|i| &s[i + 1..]))
                .unwrap_or(&p.content);
            let sample_end = floor_char_boundary(content_body, 1000);
            let content_sample = content_body[..sample_end].to_lowercase();
            for word in &synthesis_words {
                if word.len() >= 4 && content_sample.contains(word) {
                    score += 2;
                }
            }
            if score > 0 {
                Some((score, p.frontmatter.id.clone()))
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
}

pub(crate) fn derive_topic_id(synthesis: &str) -> String {
    let synthesis_lower = synthesis.to_lowercase();
    let significant_words: Vec<&str> = synthesis_lower
        .split_whitespace()
        .filter(|w| w.len() >= 4)
        .take(3)
        .collect();
    if significant_words.is_empty() {
        "topic-new".to_string()
    } else {
        format!("topic-{}", significant_words.join("-"))
    }
}

pub async fn run_wiki_command(
    action: &WikiAction,
    workspace: WorkspaceIndex,
    wiki_dir_override: &Option<PathBuf>,
    model_override: Option<&str>,
    exec_cmd: Option<&str>,
) -> Result<()> {
    match action {
        WikiAction::Generate {
            max_response_tokens,
            dry_run,
        } => {
            let wiki_dir = resolve_wiki_dir(wiki_dir_override, &workspace.root);
            let llm = build_llm_client(exec_cmd, model_override, *max_response_tokens)?;

            eprintln!("Using model: {}", llm.model());
            eprintln!("Wiki output: {}", wiki_dir.display());

            let generator = WikiGenerator::new(&llm, &workspace);
            let store = generator.generate_full(&wiki_dir, *dry_run).await?;

            if !dry_run {
                eprintln!(
                    "\nWiki generated: {} pages written to {}",
                    store.pages.len(),
                    wiki_dir.display()
                );
            }

            Ok(())
        }
        WikiAction::Update {
            since,
            max_response_tokens,
        } => {
            let wiki_dir = resolve_wiki_dir(wiki_dir_override, &workspace.root);
            let llm = build_llm_client(exec_cmd, model_override, *max_response_tokens)?;

            let mut store = store::WikiStore::load(&wiki_dir)?;
            if store.pages.is_empty() {
                anyhow::bail!(
                    "No wiki found at {}. Run `indxr wiki generate` first.",
                    wiki_dir.display()
                );
            }

            let since_ref = since
                .clone()
                .unwrap_or_else(|| store.manifest.generated_at_ref.clone());

            if since_ref.is_empty() {
                anyhow::bail!(
                    "No git ref to diff against. Pass --since <ref> or regenerate the wiki."
                );
            }

            eprintln!("Updating wiki from ref: {}", since_ref);
            eprintln!("Using model: {}", llm.model());

            let generator = WikiGenerator::new(&llm, &workspace);
            let result = generator.update_affected(&mut store, &since_ref).await?;
            store.save()?;

            eprintln!(
                "\nWiki updated: {} pages regenerated, {} created, {} removed ({} total pages at {})",
                result.pages_updated,
                result.pages_created,
                result.pages_removed,
                store.pages.len(),
                wiki_dir.display()
            );

            Ok(())
        }
        WikiAction::Status => {
            let wiki_dir = resolve_wiki_dir(wiki_dir_override, &workspace.root);

            if !wiki_dir.exists() {
                eprintln!("No wiki found at {}", wiki_dir.display());
                eprintln!("Run `indxr wiki generate` to create one.");
                return Ok(());
            }

            let store = store::WikiStore::load(&wiki_dir)?;
            let health = compute_wiki_health(&store, &workspace);

            eprintln!("Wiki: {}", wiki_dir.display());
            eprintln!("Pages: {}", health.pages);
            eprintln!("Generated at ref: {}", health.generated_at_ref);
            eprintln!("Generated at: {}", health.generated_at);

            for (ptype, count) in &health.pages_by_type {
                eprintln!("  {}: {}", ptype, count);
            }

            eprintln!("\nStaleness: {}", health.staleness);
            if !health.affected_pages.is_empty() {
                eprintln!("Affected pages ({}):", health.affected_pages.len());
                for title in &health.affected_pages {
                    eprintln!("  - {}", title);
                }
            }

            eprintln!(
                "\nSource file coverage: {}/{} ({})",
                health.covered_files, health.total_files, health.coverage_pct,
            );

            if !health.uncovered_files.is_empty() && health.uncovered_files.len() <= 20 {
                eprintln!("Uncovered files:");
                for f in &health.uncovered_files {
                    eprintln!("  - {}", f);
                }
            } else if !health.uncovered_files.is_empty() {
                eprintln!(
                    "  ({} uncovered files — run with --verbose to list)",
                    health.uncovered_files.len()
                );
            }

            Ok(())
        }
        WikiAction::Compound {
            file,
            source_pages,
            title,
        } => {
            let wiki_dir = resolve_wiki_dir(wiki_dir_override, &workspace.root);
            let mut store = store::WikiStore::load(&wiki_dir)?;
            if store.pages.is_empty() {
                anyhow::bail!(
                    "No wiki found at {}. Run `indxr wiki generate` first.",
                    wiki_dir.display()
                );
            }

            let synthesis = if file == "-" {
                std::io::read_to_string(std::io::stdin())?
            } else {
                std::fs::read_to_string(file)?
            };

            let result =
                compound_into_wiki(&mut store, &synthesis, source_pages, title.as_deref())?;

            eprintln!(
                "Wiki {}: \"{}\" (page: {})",
                result.action, result.title, result.id
            );

            Ok(())
        }
    }
}

pub fn build_llm_client(
    exec_cmd: Option<&str>,
    model_override: Option<&str>,
    max_tokens: usize,
) -> Result<LlmClient> {
    let client = if let Some(cmd) = exec_cmd {
        LlmClient::from_command(cmd.to_string(), model_override)
    } else {
        LlmClient::from_env(model_override)?
    };
    Ok(client.with_max_tokens(max_tokens))
}

fn resolve_wiki_dir(override_dir: &Option<PathBuf>, workspace_root: &std::path::Path) -> PathBuf {
    override_dir
        .clone()
        .unwrap_or_else(|| workspace_root.join(".indxr").join("wiki"))
}

/// Count how many commits exist between `since_ref` and HEAD.
pub(crate) fn commits_behind(root: &std::path::Path, since_ref: &str) -> Result<usize> {
    // Validate that since_ref looks like a hex commit hash to prevent injection
    // of unexpected git arguments via a tampered manifest.
    if since_ref.is_empty() || !since_ref.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(0);
    }
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-list", "--count", &format!("{}..HEAD", since_ref)])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git rev-list failed");
    }
    let count_str = String::from_utf8_lossy(&output.stdout);
    Ok(count_str.trim().parse::<usize>().unwrap_or(0))
}
