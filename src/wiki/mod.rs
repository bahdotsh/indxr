mod generate;
pub mod page;
mod prompts;
pub mod store;

pub(crate) use generate::WikiGenerator;
pub(crate) use generate::build_planning_context;
pub(crate) use generate::extract_wiki_links;

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
                "\nWiki updated: {} pages regenerated, {} removed ({} total pages at {})",
                result.pages_updated,
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
    }
}

fn build_llm_client(
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
