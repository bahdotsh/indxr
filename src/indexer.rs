use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::cache::Cache;
use crate::model::{CodebaseIndex, FileIndex, IndexStats};
use crate::output::OutputFormatter;
use crate::output::markdown::{MarkdownFormatter, MarkdownOptions};
use crate::parser::ParserRegistry;
use crate::walker::{self, FileEntry};

/// Configuration needed to (re-)build an index.
#[derive(Clone, Debug)]
pub struct IndexConfig {
    pub root: PathBuf,
    pub cache_dir: PathBuf,
    pub max_file_size: u64,
    pub max_depth: Option<usize>,
    pub exclude: Vec<String>,
    pub no_gitignore: bool,
}

pub struct ParseResult {
    pub file_index: FileIndex,
    pub relative_path: PathBuf,
    pub size: u64,
    pub mtime: u64,
    pub content_bytes: Option<Vec<u8>>,
}

pub fn parse_files(
    files: &[&FileEntry],
    cache: &Cache,
    registry: &ParserRegistry,
) -> Vec<ParseResult> {
    files
        .par_iter()
        .filter_map(|file_entry| {
            let file_entry = *file_entry;

            // Check cache first
            if let Some(cached) =
                cache.get(&file_entry.relative_path, file_entry.size, file_entry.mtime)
            {
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

pub fn collect_results(
    results: Vec<ParseResult>,
    cache: &mut Cache,
) -> (Vec<FileIndex>, usize, HashMap<String, usize>, usize) {
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

/// Build a fresh `CodebaseIndex` from the given configuration.
pub fn build_index(config: &IndexConfig) -> anyhow::Result<CodebaseIndex> {
    let root = fs::canonicalize(&config.root)?;
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    let exclude_patterns = &config.exclude;
    let walk_result = walker::walk_directory(
        &root,
        !config.no_gitignore,
        config.max_file_size,
        config.max_depth,
        exclude_patterns,
    )?;

    let mut cache = Cache::load(&config.cache_dir);
    let registry = ParserRegistry::new();

    let file_refs: Vec<&FileEntry> = walk_result.files.iter().collect();
    let results = parse_files(&file_refs, &cache, &registry);
    let (mut file_indices, total_lines, language_counts, _) = collect_results(results, &mut cache);

    file_indices.sort_by(|a, b| a.path.cmp(&b.path));

    cache.prune(
        &walk_result
            .files
            .iter()
            .map(|f| f.relative_path.clone())
            .collect::<Vec<_>>(),
    );
    cache.save()?;

    Ok(CodebaseIndex {
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
    })
}

/// Generate INDEX.md content from a `CodebaseIndex`.
pub fn generate_index_markdown(index: &CodebaseIndex) -> anyhow::Result<String> {
    use crate::model::DetailLevel;
    let formatter = MarkdownFormatter::with_options(MarkdownOptions {
        omit_imports: false,
        omit_tree: false,
    });
    formatter.format(index, DetailLevel::Signatures)
}

/// Rebuild the index and write INDEX.md to the project root.
/// Returns the new `CodebaseIndex`.
pub fn regenerate_index_file(config: &IndexConfig) -> anyhow::Result<CodebaseIndex> {
    let index = build_index(config)?;
    let markdown = generate_index_markdown(&index)?;
    let output_path = index.root.join("INDEX.md");
    fs::write(&output_path, &markdown)?;
    Ok(index)
}
