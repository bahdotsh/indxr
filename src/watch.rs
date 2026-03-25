use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;

use crate::indexer::{self, IndexConfig};
use crate::languages::Language;

/// Keeps the file watcher alive. The watcher stops when this guard is dropped.
pub struct WatchGuard {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

pub struct WatchOptions {
    pub config: IndexConfig,
    pub output: Option<PathBuf>,
    pub debounce_ms: u64,
    pub quiet: bool,
}

/// Run the standalone watch loop. Performs an initial index, then re-indexes on each
/// debounced file change. Blocks indefinitely until Ctrl+C or error.
pub fn run_watch(opts: WatchOptions) -> Result<()> {
    let root = fs::canonicalize(&opts.config.root)?;
    let output_path = opts.output.clone().unwrap_or_else(|| root.join("INDEX.md"));

    // Initial index
    if !opts.quiet {
        eprintln!("Performing initial index...");
    }

    let index = write_index(&opts.config, &output_path)?;

    if !opts.quiet {
        eprintln!(
            "Indexed {} files. Watching {} for changes... (press Ctrl+C to stop)",
            index.files.len(),
            root.display()
        );
    }

    let cache_dir = fs::canonicalize(root.join(&opts.config.cache_dir))
        .unwrap_or_else(|_| root.join(&opts.config.cache_dir));
    let (rx, _guard) = spawn_watcher(&root, &cache_dir, &output_path, opts.debounce_ms)?;

    while let Ok(()) = rx.recv() {
        // Coalesce: drain any additional queued events so we re-index only once per burst
        while rx.try_recv().is_ok() {}

        if !opts.quiet {
            eprintln!("Change detected, re-indexing...");
        }
        match write_index(&opts.config, &output_path) {
            Ok(new_index) => {
                if !opts.quiet {
                    eprintln!(
                        "Index updated ({} files, {} lines)",
                        new_index.stats.total_files, new_index.stats.total_lines
                    );
                }
            }
            Err(e) => {
                eprintln!("Re-index failed: {}", e);
            }
        }
    }

    Ok(())
}

/// Build the index and write it to the given output path.
/// Similar to `indexer::regenerate_index_file`, but accepts an explicit output path
/// (rather than always writing to `<root>/INDEX.md`) to support `--output`.
fn write_index(config: &IndexConfig, output_path: &Path) -> Result<crate::model::CodebaseIndex> {
    let index = indexer::build_index(config)?;
    let markdown = indexer::generate_index_markdown(&index)?;
    fs::write(output_path, &markdown)?;
    Ok(index)
}

/// Spawn a file watcher that sends a signal on a channel whenever source files change.
/// Returns a Receiver that yields `()` on each debounced change batch, and a
/// [`WatchGuard`] that keeps the watcher alive — drop it to stop watching.
pub fn spawn_watcher(
    root: &Path,
    cache_dir: &Path,
    output_path: &Path,
    debounce_ms: u64,
) -> Result<(mpsc::Receiver<()>, WatchGuard)> {
    let (tx, rx) = mpsc::channel();
    let root = root.to_path_buf();
    let cache_dir = cache_dir.to_path_buf();
    let output_path = output_path.to_path_buf();

    let watch_root = root.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(debounce_ms),
        move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| match res {
            Ok(events) => {
                let has_relevant_change = events.iter().any(|e| {
                    should_trigger_reindex(&e.path, &watch_root, &output_path, &cache_dir)
                });
                if has_relevant_change {
                    let _ = tx.send(());
                }
            }
            Err(e) => {
                eprintln!("Watcher error: {}", e);
            }
        },
    )?;

    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;

    let guard = WatchGuard {
        _debouncer: debouncer,
    };

    Ok((rx, guard))
}

/// Determines if a path change should trigger re-indexing.
/// Filters out: the output file itself, the cache directory, non-source files, and hidden files.
fn should_trigger_reindex(path: &Path, root: &Path, output_path: &Path, cache_dir: &Path) -> bool {
    // Ignore the output file (INDEX.md) to prevent self-triggering loops
    if path == output_path {
        return false;
    }

    // Ignore cache directory
    if path.starts_with(cache_dir) {
        return false;
    }

    // Ignore hidden files/directories (e.g., .git)
    if let Ok(rel) = path.strip_prefix(root) {
        for component in rel.components() {
            if let std::path::Component::Normal(name) = component {
                if name.to_string_lossy().starts_with('.') {
                    return false;
                }
            }
        }
    }

    // Only trigger for files with a recognized language extension
    Language::detect(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn root() -> PathBuf {
        PathBuf::from("/project")
    }

    fn output() -> PathBuf {
        PathBuf::from("/project/INDEX.md")
    }

    fn cache() -> PathBuf {
        PathBuf::from("/project/.indxr-cache")
    }

    #[test]
    fn test_source_file_triggers() {
        assert!(should_trigger_reindex(
            Path::new("/project/src/main.rs"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_output_file_ignored() {
        assert!(!should_trigger_reindex(
            Path::new("/project/INDEX.md"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_cache_dir_ignored() {
        assert!(!should_trigger_reindex(
            Path::new("/project/.indxr-cache/cache.bin"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_non_source_file_ignored() {
        assert!(!should_trigger_reindex(
            Path::new("/project/image.png"),
            &root(),
            &output(),
            &cache(),
        ));
        assert!(!should_trigger_reindex(
            Path::new("/project/binary.exe"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_hidden_file_ignored() {
        assert!(!should_trigger_reindex(
            Path::new("/project/.git/config"),
            &root(),
            &output(),
            &cache(),
        ));
        assert!(!should_trigger_reindex(
            Path::new("/project/.hidden/test.rs"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_various_source_types() {
        let cases = vec![
            "/project/app.py",
            "/project/index.ts",
            "/project/main.go",
            "/project/App.java",
            "/project/lib.c",
        ];
        for path in cases {
            assert!(
                should_trigger_reindex(Path::new(path), &root(), &output(), &cache()),
                "Expected {} to trigger reindex",
                path
            );
        }
    }
}
