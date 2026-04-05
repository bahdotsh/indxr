mod generate;
pub mod page;
mod prompts;
pub mod store;

use std::path::PathBuf;

use anyhow::Result;

use crate::cli::WikiAction;
use crate::llm::LlmClient;
use crate::model::WorkspaceIndex;

use generate::WikiGenerator;

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
            eprintln!("Wiki: {}", wiki_dir.display());
            eprintln!("Pages: {}", store.pages.len());
            eprintln!("Generated at ref: {}", store.manifest.generated_at_ref);
            eprintln!("Generated at: {}", store.manifest.generated_at);

            // Count by type
            let mut by_type = std::collections::HashMap::new();
            for page in &store.pages {
                *by_type
                    .entry(page.frontmatter.page_type.to_string())
                    .or_insert(0usize) += 1;
            }
            for (ptype, count) in &by_type {
                eprintln!("  {}: {}", ptype, count);
            }

            // Coverage
            let covered: std::collections::HashSet<&str> = store
                .pages
                .iter()
                .flat_map(|p| p.frontmatter.source_files.iter().map(|s| s.as_str()))
                .collect();
            let total_files = workspace.stats.total_files;
            eprintln!("Source file coverage: {}/{}", covered.len(), total_files);

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
