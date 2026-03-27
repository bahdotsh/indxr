mod budget;
mod cache;
mod cli;
mod dep_graph;
mod diff;
mod error;
mod filter;
mod github;
mod indexer;
mod init;
mod languages;
mod mcp;
mod model;
mod output;
mod parser;
mod utils;
mod walker;
mod watch;

use std::collections::HashMap;
use std::fs;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;

use crate::cache::Cache;
use crate::cli::{Cli, Command, GraphFormat, GraphLevel, OutputFormat};
use crate::filter::FilterOptions;
use crate::languages::Language;
use crate::model::declarations::DeclKind;
use crate::model::{CodebaseIndex, IndexStats};
use crate::output::OutputFormatter;
use crate::output::markdown::{MarkdownFormatter, MarkdownOptions};
use crate::output::yaml::YamlFormatter;
use crate::parser::ParserRegistry;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle MCP server subcommand
    if let Some(Command::Serve {
        opts,
        watch: enable_watch,
        debounce_ms,
    }) = &cli.command
    {
        let config = index_config_from(opts);
        let index = indexer::build_index(&config)?;

        eprintln!(
            "indxr MCP server starting (indexed {} files)",
            index.files.len()
        );
        return mcp::run_mcp_server(index, config, *enable_watch, *debounce_ms);
    }

    // Handle watch subcommand
    if let Some(Command::Watch {
        opts,
        output,
        debounce_ms,
        quiet,
    }) = &cli.command
    {
        let config = index_config_from(opts);

        let watch_opts = watch::WatchOptions {
            config,
            output: output.clone(),
            debounce_ms: *debounce_ms,
            quiet: *quiet,
        };

        return watch::run_watch(watch_opts);
    }

    // Handle init subcommand
    if let Some(Command::Init {
        path,
        claude,
        cursor,
        windsurf,
        all,
        no_index,
        no_hooks,
        no_rtk,
        force,
        max_file_size,
    }) = &cli.command
    {
        let (claude, cursor, windsurf) = if *all || (!*claude && !*cursor && !*windsurf) {
            (true, true, true)
        } else {
            (*claude, *cursor, *windsurf)
        };

        let opts = init::InitOptions {
            path: path.clone(),
            claude,
            cursor,
            windsurf,
            generate_index: !no_index,
            force: *force,
            include_hooks: !no_hooks,
            include_rtk: !no_rtk,
            max_file_size: *max_file_size,
        };

        return init::run_init(opts);
    }

    // Handle diff subcommand
    if let Some(Command::Diff {
        opts,
        pr,
        since,
        format,
    }) = &cli.command
    {
        let config = index_config_from(opts);
        let root = fs::canonicalize(&config.root)?;

        let since_ref = if let Some(pr_num) = pr {
            let (base_ref, pr_info) = github::resolve_pr_base(&root, *pr_num)?;
            eprintln!(
                "PR #{}: {} ({}...{})",
                pr_info.number, pr_info.title, pr_info.base_ref, pr_info.head_ref
            );
            base_ref
        } else {
            // Safe to unwrap: clap ArgGroup ensures one of --pr/--since is present
            since.as_deref().unwrap().to_string()
        };

        let index = indexer::build_index(&config)?;
        let registry = ParserRegistry::new();

        return handle_git_diff(&root, &since_ref, &index.files, &registry, format);
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
                .is_none_or(|filter| filter.contains(&f.language))
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
    let results = indexer::parse_files(&files, &cache, &registry);
    let (mut file_indices, total_lines, language_counts, cache_hits) =
        indexer::collect_results(results, &mut cache);

    // Sort file indices by path for consistent output
    file_indices.sort_by(|a, b| a.path.cmp(&b.path));

    // Prune stale cache entries
    let existing: Vec<std::path::PathBuf> = files.iter().map(|f| f.relative_path.clone()).collect();
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
        return handle_git_diff(&root, since_ref, &file_indices, &registry, &cli.format);
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

    // Handle --graph mode (uses full index; --filter-path scopes the graph root)
    if let Some(ref graph_format) = cli.graph {
        let graph = match cli.graph_level {
            Some(GraphLevel::Symbol) => {
                dep_graph::build_symbol_graph(&index, cli.filter_path.as_deref(), cli.graph_depth)
            }
            _ => dep_graph::build_file_graph(&index, cli.filter_path.as_deref(), cli.graph_depth),
        };
        let formatted = match graph_format {
            GraphFormat::Dot => dep_graph::format_dot(&graph),
            GraphFormat::Mermaid => dep_graph::format_mermaid(&graph),
            GraphFormat::Json => {
                serde_json::to_string_pretty(&dep_graph::format_json(&graph)).unwrap_or_default()
            }
        };
        if let Some(output_path) = &cli.output {
            fs::write(output_path, &formatted)?;
            if !cli.quiet {
                eprintln!(
                    "Dependency graph written to {} ({} nodes, {} edges)",
                    output_path.display(),
                    graph.nodes.len(),
                    graph.edges.len()
                );
            }
        } else {
            print!("{}", formatted);
        }
        return Ok(());
    }

    // Handle --hotspots mode
    if cli.hotspots {
        return handle_hotspots(&index, cli.filter_path.as_deref());
    }

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

fn index_config_from(opts: &cli::IndexOpts) -> indexer::IndexConfig {
    indexer::IndexConfig {
        root: opts.path.clone(),
        cache_dir: opts.cache_dir.clone(),
        max_file_size: opts.max_file_size,
        max_depth: opts.max_depth,
        exclude: opts.exclude.clone().unwrap_or_default(),
        no_gitignore: opts.no_gitignore,
    }
}

fn handle_git_diff(
    root: &std::path::Path,
    since_ref: &str,
    current_files: &[model::FileIndex],
    registry: &ParserRegistry,
    format: &OutputFormat,
) -> Result<()> {
    let changed_paths = diff::get_changed_files(root, since_ref)?;

    if changed_paths.is_empty() {
        eprintln!("No structural changes since {}", since_ref);
        return Ok(());
    }

    // Get old file contents and parse them
    let mut old_files: HashMap<std::path::PathBuf, model::FileIndex> = HashMap::new();
    for path in &changed_paths {
        if let Ok(Some(old_content)) = diff::get_file_at_ref(root, path, since_ref)
            && let Some(lang) = Language::detect(path)
            && let Some(parser) = registry.get_parser(&lang)
            && let Ok(index) = parser.parse_file(path, &old_content)
        {
            old_files.insert(path.clone(), index);
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

    let structural_diff = diff::compute_structural_diff(&temp_index, &old_files, &changed_paths);

    match format {
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

fn handle_hotspots(index: &CodebaseIndex, path_filter: Option<&str>) -> Result<()> {
    let mut entries = parser::complexity::collect_hotspots(index, path_filter, 1);
    parser::complexity::sort_hotspots(&mut entries, "score");
    entries.truncate(30);

    if entries.is_empty() {
        println!("No complexity data found (only tree-sitter languages are analyzed).");
        return Ok(());
    }

    println!(
        "\n{:>7} {:>4} {:>5} {:>6} {:>6}  Function",
        "Score", "CC", "Nest", "Params", "Lines"
    );
    println!("{}", "-".repeat(78));

    for e in &entries {
        println!(
            "{:>7.1} {:>4} {:>5} {:>6} {:>6}  {}:{}  {}",
            e.score,
            e.cyclomatic,
            e.max_nesting,
            e.param_count,
            e.body_lines,
            e.file,
            e.line,
            e.name
        );
    }

    println!();
    Ok(())
}
