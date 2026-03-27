use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;

use crate::indexer::{self, WorkspaceConfig};
use crate::languages::Language;

/// Keeps the file watcher alive. The watcher stops when this guard is dropped.
pub struct WatchGuard {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

pub struct WatchOptions {
    pub ws_config: WorkspaceConfig,
    pub output: Option<PathBuf>,
    pub debounce_ms: u64,
    pub quiet: bool,
}

/// Run the standalone watch loop. Performs an initial index, then re-indexes on each
/// debounced file change. Blocks indefinitely until Ctrl+C or error.
pub fn run_watch(opts: WatchOptions) -> Result<()> {
    let root = fs::canonicalize(&opts.ws_config.workspace.root)?;
    let output_path = opts.output.clone().unwrap_or_else(|| root.join("INDEX.md"));

    // Initial index
    if !opts.quiet {
        eprintln!("Performing initial index...");
    }

    let ws_index = write_workspace_index(&opts.ws_config, &output_path)?;
    // Canonicalize after write so the file exists — prevents self-triggering
    // loops when the user passes a relative `-o` path.
    let output_path = fs::canonicalize(&output_path).unwrap_or(output_path);

    if !opts.quiet {
        eprintln!(
            "Indexed {} files. Watching {} for changes... (press Ctrl+C to stop)",
            ws_index.stats.total_files,
            root.display()
        );
    }

    let cache_dir = fs::canonicalize(root.join(&opts.ws_config.template.cache_dir))
        .unwrap_or_else(|_| root.join(&opts.ws_config.template.cache_dir));
    let (rx, _guard) = spawn_watcher(&root, &cache_dir, &output_path, opts.debounce_ms)?;

    while let Ok(()) = rx.recv() {
        // Coalesce: drain any additional queued events so we re-index only once per burst
        while rx.try_recv().is_ok() {}

        if !opts.quiet {
            eprintln!("Change detected, re-indexing...");
        }
        match write_workspace_index(&opts.ws_config, &output_path) {
            Ok(new_ws) => {
                if !opts.quiet {
                    eprintln!(
                        "Index updated ({} files, {} lines)",
                        new_ws.stats.total_files, new_ws.stats.total_lines
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

/// Build the workspace index and write it to the given output path.
fn write_workspace_index(
    ws_config: &WorkspaceConfig,
    output_path: &Path,
) -> Result<crate::model::WorkspaceIndex> {
    let ws_index = indexer::build_workspace_index(ws_config)?;
    let markdown = indexer::generate_workspace_markdown(&ws_index)?;
    fs::write(output_path, &markdown)?;
    Ok(ws_index)
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
    // Cheap extension check first — skip non-source files (images, binaries,
    // config, etc.) without any filesystem syscall.
    if Language::detect(path).is_none() {
        return false;
    }

    // Canonicalize the event path once so symlinks / /private/var vs /var
    // differences on macOS don't bypass any of the checks below.
    let canonical = fs::canonicalize(path);
    let check_path = canonical.as_deref().unwrap_or(path);

    // Ignore the output file (INDEX.md) to prevent self-triggering loops.
    if check_path == output_path {
        return false;
    }

    // Ignore cache directory
    if check_path.starts_with(cache_dir) {
        return false;
    }

    // Ignore hidden files/directories (e.g., .git).
    // Uses canonicalized check_path so macOS /private/var vs /var symlinks
    // don't cause strip_prefix to fail and skip this check.
    if let Ok(rel) = check_path.strip_prefix(root) {
        for component in rel.components() {
            if let std::path::Component::Normal(name) = component {
                if name.to_string_lossy().starts_with('.') {
                    return false;
                }
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;

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

    #[test]
    fn test_nonexistent_source_file_triggers() {
        // When canonicalize fails (file doesn't exist), the fallback raw path is used.
        // The filter should still work correctly with raw paths.
        assert!(should_trigger_reindex(
            Path::new("/project/deleted.rs"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_path_outside_root_with_source_ext() {
        // A source file outside the root can't be checked for hidden components
        // via strip_prefix, but should still trigger (it passed Language::detect).
        assert!(should_trigger_reindex(
            Path::new("/other/project/lib.rs"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_nested_source_file_triggers() {
        assert!(should_trigger_reindex(
            Path::new("/project/src/parser/mod.rs"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    #[test]
    fn test_hidden_nested_source_file_ignored() {
        // Source file inside a deeply nested hidden dir
        assert!(!should_trigger_reindex(
            Path::new("/project/src/.secret/deep/main.rs"),
            &root(),
            &output(),
            &cache(),
        ));
    }

    /// Verifies that `spawn_watcher` delivers events while the guard is alive,
    /// and stops delivering after the guard is dropped.
    #[test]
    fn watcher_guard_lifetime() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let output_path = root.join("INDEX.md");
        let cache_dir = root.join(".indxr-cache");
        fs::create_dir_all(&cache_dir).unwrap();

        let (rx, guard) = spawn_watcher(&root, &cache_dir, &output_path, 100).unwrap();

        // Write a source file — should trigger an event
        fs::write(root.join("test.rs"), "fn main() {}").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5));
        assert!(got.is_ok(), "Expected event while guard is alive");

        // Let any remaining debounced events from the initial write settle
        // (Windows NTFS can emit multiple notifications per file operation)
        std::thread::sleep(Duration::from_millis(200));
        while rx.try_recv().is_ok() {}

        // Drop the guard — watcher should stop, channel should disconnect
        drop(guard);
        // Drain any in-flight events
        while rx.try_recv().is_ok() {}
        let got = rx.recv_timeout(Duration::from_secs(1));
        assert!(got.is_err(), "Expected no events after guard is dropped");
    }
}
