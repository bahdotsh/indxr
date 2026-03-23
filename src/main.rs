mod budget;
mod cache;
mod cli;
mod diff;
mod error;
mod filter;
mod languages;
mod mcp;
mod model;
mod output;
mod parser;
mod walker;

use std::collections::HashMap;
use std::fs;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use rayon::prelude::*;

use crate::cache::Cache;
use crate::cli::{Cli, Command, OutputFormat};
use crate::filter::FilterOptions;
use crate::languages::Language;
use crate::model::declarations::DeclKind;
use crate::model::{CodebaseIndex, IndexStats};
use crate::output::markdown::{MarkdownFormatter, MarkdownOptions};
use crate::output::yaml::YamlFormatter;
use crate::output::OutputFormatter;
use crate::parser::ParserRegistry;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle MCP server subcommand
    if let Some(Command::Serve {
        path,
        cache_dir,
        max_file_size,
        max_depth,
        exclude,
        no_gitignore,
    }) = &cli.command
    {
        let root = fs::canonicalize(path)?;
        let root_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        let exclude_patterns = exclude.as_deref().unwrap_or(&[]);
        let walk_result = walker::walk_directory(
            &root,
            !no_gitignore,
            *max_file_size,
            *max_depth,
            exclude_patterns,
        )?;

        let mut cache = Cache::load(cache_dir);
        let registry = ParserRegistry::new();

        let file_refs: Vec<&walker::FileEntry> = walk_result.files.iter().collect();
        let results = parse_files(&file_refs, &cache, &registry);
        let (file_indices, total_lines, language_counts, _) =
            collect_results(results, &mut cache);
        cache.prune(
            &walk_result
                .files
                .iter()
                .map(|f| f.relative_path.clone())
                .collect::<Vec<_>>(),
        );
        cache.save()?;

        let index = CodebaseIndex {
            root,
            root_name,
            generated_at: chrono::Utc::now()
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
            files: file_indices,
            tree: walk_result.tree,
            stats: IndexStats {
                total_files: walk_result.files.len(),
                total_lines,
                languages: language_counts,
                duration_ms: 0,
            },
        };

        eprintln!(
            "indxr MCP server starting (indexed {} files)",
            index.files.len()
        );
        return mcp::run_mcp_server(index);
    }

    // Normal indexing mode
    let start = Instant::now();

    // Resolve the root path
    let root = fs::canonicalize(&cli.path)?;
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    // Walk the directory
    let exclude_patterns = cli.exclude.as_deref().unwrap_or(&[]);
    let walk_result = walker::walk_directory(
        &root,
        !cli.no_gitignore,
        cli.max_file_size,
        cli.max_depth,
        exclude_patterns,
    )?;

    // Filter by language if specified
    let lang_filter: Option<Vec<Language>> = cli.languages.as_ref().map(|langs| {
        langs
            .iter()
            .filter_map(|l| Language::from_name(l))
            .collect()
    });

    let files: Vec<&walker::FileEntry> = walk_result
        .files
        .iter()
        .filter(|f| {
            lang_filter
                .as_ref()
                .map_or(true, |filter| filter.contains(&f.language))
        })
        .collect();

    if !cli.quiet {
        eprintln!("Found {} files to index", files.len());
    }

    // Load cache
    let mut cache = if cli.no_cache {
        Cache::disabled()
    } else {
        Cache::load(&cli.cache_dir)
    };

    // Parse files in parallel
    let registry = ParserRegistry::new();
    let results = parse_files(&files, &cache, &registry);
    let (mut file_indices, total_lines, language_counts, cache_hits) =
        collect_results(results, &mut cache);

    // Sort file indices by path for consistent output
    file_indices.sort_by(|a, b| a.path.cmp(&b.path));

    // Prune stale cache entries
    let existing: Vec<std::path::PathBuf> =
        files.iter().map(|f| f.relative_path.clone()).collect();
    cache.prune(&existing);
    cache.save()?;

    let duration = start.elapsed();

    if !cli.quiet && cache_hits > 0 {
        eprintln!(
            "{} files from cache, {} freshly parsed",
            cache_hits,
            file_indices.len() - cache_hits
        );
    }

    // Handle --since (git diff mode)
    if let Some(ref since_ref) = cli.since {
        return handle_git_diff(&root, since_ref, &file_indices, &registry, &cli);
    }

    // Build the index
    let mut index = CodebaseIndex {
        root: root.clone(),
        root_name,
        generated_at: chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
        files: file_indices,
        tree: walk_result.tree,
        stats: IndexStats {
            total_files: files.len(),
            total_lines,
            languages: language_counts,
            duration_ms: duration.as_millis() as u64,
        },
    };

    // Apply filters
    let filter_opts = FilterOptions {
        filter_path: cli.filter_path.clone(),
        symbol: cli.symbol.clone(),
        kind: cli.kind.as_ref().and_then(|k| DeclKind::from_name(k)),
        public_only: cli.public_only,
    };

    if filter_opts.is_active() {
        filter::apply_filters(&mut index, &filter_opts);
    }

    // Apply token budget
    if let Some(max_tokens) = cli.max_tokens {
        budget::apply_token_budget(&mut index, max_tokens);
    }

    // Format output
    let formatted = match cli.format {
        OutputFormat::Markdown => {
            let formatter = MarkdownFormatter::with_options(MarkdownOptions {
                omit_imports: cli.omit_imports,
                omit_tree: cli.omit_tree,
            });
            formatter.format(&index, cli.detail)?
        }
        OutputFormat::Json => serde_json::to_string_pretty(&index)?,
        OutputFormat::Yaml => {
            let formatter = YamlFormatter;
            formatter.format(&index, cli.detail)?
        }
    };

    // Write output
    if let Some(output_path) = &cli.output {
        fs::write(output_path, &formatted)?;
        if !cli.quiet {
            eprintln!("Index written to {}", output_path.display());
        }
    } else {
        print!("{}", formatted);
    }

    // Print stats
    if cli.stats {
        eprintln!("\n--- Statistics ---");
        eprintln!("Files indexed: {}", index.stats.total_files);
        eprintln!("Total lines: {}", index.stats.total_lines);
        eprintln!("Duration: {}ms", index.stats.duration_ms);
        if !cli.no_cache {
            eprintln!("Cache: {} hits, {} entries", cache_hits, cache.len());
        }
        for (lang, count) in &index.stats.languages {
            eprintln!("  {}: {} files", lang, count);
        }
    }

    Ok(())
}

struct ParseResult {
    file_index: model::FileIndex,
    relative_path: std::path::PathBuf,
    size: u64,
    mtime: u64,
    content_bytes: Option<Vec<u8>>,
}

fn parse_files(
    files: &[&walker::FileEntry],
    cache: &Cache,
    registry: &ParserRegistry,
) -> Vec<ParseResult> {
    files
        .par_iter()
        .filter_map(|file_entry| {
            let file_entry = *file_entry;

            // Check cache first
            if let Some(cached) = cache.get(
                &file_entry.relative_path,
                file_entry.size,
                file_entry.mtime,
            ) {
                return Some(ParseResult {
                    file_index: cached,
                    relative_path: file_entry.relative_path.clone(),
                    size: file_entry.size,
                    mtime: file_entry.mtime,
                    content_bytes: None,
                });
            }

            // Parse the file
            let parser = registry.get_parser(&file_entry.language)?;
            let content = fs::read_to_string(&file_entry.path).ok()?;
            let mut index = parser
                .parse_file(&file_entry.relative_path, &content)
                .ok()?;
            index.size = file_entry.size;

            Some(ParseResult {
                file_index: index,
                relative_path: file_entry.relative_path.clone(),
                size: file_entry.size,
                mtime: file_entry.mtime,
                content_bytes: Some(content.into_bytes()),
            })
        })
        .collect()
}

fn collect_results(
    results: Vec<ParseResult>,
    cache: &mut Cache,
) -> (Vec<model::FileIndex>, usize, HashMap<String, usize>, usize) {
    let mut file_indices = Vec::new();
    let mut total_lines = 0;
    let mut language_counts: HashMap<String, usize> = HashMap::new();
    let mut cache_hits = 0usize;

    for result in results {
        if let Some(ref bytes) = result.content_bytes {
            cache.insert(
                &result.relative_path,
                result.size,
                result.mtime,
                bytes,
                result.file_index.clone(),
            );
        } else {
            cache_hits += 1;
        }
        total_lines += result.file_index.lines;
        *language_counts
            .entry(result.file_index.language.name().to_string())
            .or_insert(0) += 1;
        file_indices.push(result.file_index);
    }

    (file_indices, total_lines, language_counts, cache_hits)
}

fn handle_git_diff(
    root: &std::path::Path,
    since_ref: &str,
    current_files: &[model::FileIndex],
    registry: &ParserRegistry,
    cli: &Cli,
) -> Result<()> {
    let changed_paths = diff::get_changed_files(root, since_ref)?;

    if changed_paths.is_empty() {
        eprintln!("No structural changes since {}", since_ref);
        return Ok(());
    }

    // Get old file contents and parse them
    let mut old_files: HashMap<std::path::PathBuf, model::FileIndex> = HashMap::new();
    for path in &changed_paths {
        if let Ok(Some(old_content)) = diff::get_file_at_ref(root, path, since_ref) {
            if let Some(lang) = Language::detect(path) {
                if let Some(parser) = registry.get_parser(&lang) {
                    if let Ok(index) = parser.parse_file(path, &old_content) {
                        old_files.insert(path.clone(), index);
                    }
                }
            }
        }
    }

    // Build a temporary CodebaseIndex for diff computation
    let temp_index = CodebaseIndex {
        root: root.to_path_buf(),
        root_name: String::new(),
        generated_at: String::new(),
        files: current_files.to_vec(),
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

    match cli.format {
        OutputFormat::Json => {
            let json = diff::format_diff_json(&structural_diff)?;
            println!("{}", json);
        }
        _ => {
            let markdown = diff::format_diff_markdown(&structural_diff);
            println!("{}", markdown);
        }
    }

    Ok(())
}
