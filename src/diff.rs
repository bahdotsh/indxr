use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::model::FileIndex;
use crate::model::declarations::{DeclKind, Declaration};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct StructuralDiff {
    pub since_ref: String,
    pub files_added: Vec<PathBuf>,
    pub files_removed: Vec<PathBuf>,
    pub files_modified: Vec<FileDiff>,
}

#[derive(Debug, Serialize)]
pub struct FileDiff {
    pub path: PathBuf,
    pub declarations_added: Vec<DeclChange>,
    pub declarations_removed: Vec<DeclChange>,
    pub declarations_modified: Vec<DeclModification>,
}

#[derive(Debug, Serialize)]
pub struct DeclChange {
    pub kind: DeclKind,
    pub name: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct DeclModification {
    pub kind: DeclKind,
    pub name: String,
    pub old_signature: String,
    pub new_signature: String,
}

// ---------------------------------------------------------------------------
// 1. get_changed_files
// ---------------------------------------------------------------------------

/// Run `git diff --name-only <ref>...HEAD` to find all changed files.
///
/// Also determines which files were purely added (`--diff-filter=A`) and purely
/// deleted (`--diff-filter=D`). Returns a tuple of
/// `(all_changed, added_only, deleted_only)`.
pub fn get_changed_files(root: &Path, since_ref: &str) -> Result<Vec<PathBuf>> {
    git_diff_names(root, since_ref, None)
}

/// Return only the files that were **added** since `since_ref`.
#[allow(dead_code)]
pub fn get_added_files(root: &Path, since_ref: &str) -> Result<Vec<PathBuf>> {
    git_diff_names(root, since_ref, Some("A"))
}

/// Return only the files that were **deleted** since `since_ref`.
#[allow(dead_code)]
pub fn get_deleted_files(root: &Path, since_ref: &str) -> Result<Vec<PathBuf>> {
    git_diff_names(root, since_ref, Some("D"))
}

/// Helper: execute `git diff --name-only [--diff-filter=<filter>] <ref>...HEAD`
/// and return the list of paths.
fn git_diff_names(root: &Path, since_ref: &str, diff_filter: Option<&str>) -> Result<Vec<PathBuf>> {
    let mut cmd = Command::new("git");
    cmd.current_dir(root).arg("diff").arg("--name-only");

    if let Some(filter) = diff_filter {
        cmd.arg(format!("--diff-filter={filter}"));
    }

    cmd.arg(format!("{since_ref}...HEAD"));

    let output = cmd.output().context("failed to execute git diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git diff failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();

    Ok(paths)
}

// ---------------------------------------------------------------------------
// 2. get_file_at_ref
// ---------------------------------------------------------------------------

/// Retrieve the contents of `file_path` (relative to the repo root) at the
/// given `git_ref` via `git show <ref>:<path>`.
///
/// Returns `None` when the file did not exist at that ref.
pub fn get_file_at_ref(root: &Path, file_path: &Path, git_ref: &str) -> Result<Option<String>> {
    let spec = format!("{git_ref}:{}", file_path.display());

    let output = Command::new("git")
        .current_dir(root)
        .arg("show")
        .arg(&spec)
        .output()
        .context("failed to execute git show")?;

    if !output.status.success() {
        // If git show fails the file likely did not exist at that ref.
        return Ok(None);
    }

    let content = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(Some(content))
}

// ---------------------------------------------------------------------------
// 3. compute_structural_diff
// ---------------------------------------------------------------------------

/// Compare declarations between the old and new versions of changed files,
/// producing a [`StructuralDiff`].
///
/// * `current_files` -- the freshly-parsed file indices from the current
///   working tree.
/// * `old_files` -- a map from relative path to the [`FileIndex`] that was
///   parsed from the old ref.  Only files that existed at the old ref need to
///   appear here.
/// * `changed_paths` -- every path that `git diff --name-only` reported as
///   changed.  Files present in `current_files` but absent from `old_files`
///   are treated as added; files in `old_files` but absent from `current_files`
///   are treated as removed; files in both are diffed at the declaration level.
pub fn compute_structural_diff(
    current_files: &[FileIndex],
    old_files: &HashMap<PathBuf, FileIndex>,
    changed_paths: &[PathBuf],
) -> StructuralDiff {
    // Build a lookup from path -> &FileIndex for the current files.
    let current_map: HashMap<&PathBuf, &FileIndex> =
        current_files.iter().map(|fi| (&fi.path, fi)).collect();

    let current_paths: HashSet<&PathBuf> = current_map.keys().copied().collect();
    let old_paths: HashSet<&PathBuf> = old_files.keys().collect();

    let mut files_added = Vec::new();
    let mut files_removed = Vec::new();
    let mut files_modified = Vec::new();

    for path in changed_paths {
        let in_current = current_paths.contains(path);
        let in_old = old_paths.contains(path);

        match (in_old, in_current) {
            (false, true) => {
                // File was added.
                files_added.push(path.clone());
            }
            (true, false) => {
                // File was removed.
                files_removed.push(path.clone());
            }
            (true, true) => {
                // File exists in both -- diff declarations.
                let old_decls = old_files
                    .get(path)
                    .map(|fi| &fi.declarations[..])
                    .unwrap_or(&[]);
                let new_decls = current_map
                    .get(path)
                    .map(|fi| &fi.declarations[..])
                    .unwrap_or(&[]);

                let file_diff = diff_declarations(path.clone(), old_decls, new_decls);

                // Only include files that actually have declaration-level changes.
                if !file_diff.declarations_added.is_empty()
                    || !file_diff.declarations_removed.is_empty()
                    || !file_diff.declarations_modified.is_empty()
                {
                    files_modified.push(file_diff);
                }
            }
            (false, false) => {
                // The path is in neither index (could be a non-indexed file
                // type). Nothing to do.
            }
        }
    }

    // Sort added / removed for deterministic output.
    files_added.sort();
    files_removed.sort();
    files_modified.sort_by(|a, b| a.path.cmp(&b.path));

    StructuralDiff {
        since_ref: String::new(), // caller should fill this in
        files_added,
        files_removed,
        files_modified,
    }
}

/// Diff two slices of declarations by matching on `(kind, name)` pairs.
fn diff_declarations(path: PathBuf, old: &[Declaration], new: &[Declaration]) -> FileDiff {
    let old_map = flatten_declarations(old);
    let new_map = flatten_declarations(new);

    let mut declarations_added = Vec::new();
    let mut declarations_removed = Vec::new();
    let mut declarations_modified = Vec::new();

    // Removed or modified
    for (key, old_sig) in &old_map {
        match new_map.get(key) {
            None => {
                declarations_removed.push(DeclChange {
                    kind: key.0.clone(),
                    name: key.1.clone(),
                    signature: old_sig.clone(),
                });
            }
            Some(new_sig) if new_sig != old_sig => {
                declarations_modified.push(DeclModification {
                    kind: key.0.clone(),
                    name: key.1.clone(),
                    old_signature: old_sig.clone(),
                    new_signature: new_sig.clone(),
                });
            }
            _ => {} // unchanged
        }
    }

    // Added
    for (key, new_sig) in &new_map {
        if !old_map.contains_key(key) {
            declarations_added.push(DeclChange {
                kind: key.0.clone(),
                name: key.1.clone(),
                signature: new_sig.clone(),
            });
        }
    }

    // Sort for deterministic output.
    declarations_added.sort_by(|a, b| a.name.cmp(&b.name));
    declarations_removed.sort_by(|a, b| a.name.cmp(&b.name));
    declarations_modified.sort_by(|a, b| a.name.cmp(&b.name));

    FileDiff {
        path,
        declarations_added,
        declarations_removed,
        declarations_modified,
    }
}

/// Recursively flatten a slice of `Declaration` (including children) into a
/// map from `(DeclKind, name)` to signature.
fn flatten_declarations(decls: &[Declaration]) -> HashMap<(DeclKind, String), String> {
    let mut map = HashMap::new();
    for decl in decls {
        map.insert(
            (decl.kind.clone(), decl.name.clone()),
            decl.signature.clone(),
        );
        // Recurse into children (e.g. methods inside an impl block).
        for child in &decl.children {
            map.insert(
                (child.kind.clone(), child.name.clone()),
                child.signature.clone(),
            );
        }
    }
    map
}

// ---------------------------------------------------------------------------
// 4. format_diff_markdown
// ---------------------------------------------------------------------------

/// Render a [`StructuralDiff`] as human-readable Markdown.
pub fn format_diff_markdown(diff: &StructuralDiff) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "# Structural Changes (since {})\n",
        diff.since_ref
    ));

    // Added files
    out.push_str("\n## Added Files\n");
    if diff.files_added.is_empty() {
        out.push_str("\n_none_\n");
    } else {
        for p in &diff.files_added {
            out.push_str(&format!("- {}\n", p.display()));
        }
    }

    // Removed files
    out.push_str("\n## Removed Files\n");
    if diff.files_removed.is_empty() {
        out.push_str("\n_none_\n");
    } else {
        for p in &diff.files_removed {
            out.push_str(&format!("- {}\n", p.display()));
        }
    }

    // Modified files
    out.push_str("\n## Modified Files\n");
    if diff.files_modified.is_empty() {
        out.push_str("\n_none_\n");
    } else {
        for file_diff in &diff.files_modified {
            out.push_str(&format!("\n### {}\n", file_diff.path.display()));

            for added in &file_diff.declarations_added {
                out.push_str(&format!("+ `{}`\n", added.signature));
            }
            for removed in &file_diff.declarations_removed {
                out.push_str(&format!("- `{}`\n", removed.signature));
            }
            for modified in &file_diff.declarations_modified {
                out.push_str(&format!(
                    "~ `{}` -> `{}`\n",
                    modified.old_signature, modified.new_signature
                ));
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// 5. format_diff_json
// ---------------------------------------------------------------------------

/// Serialize the [`StructuralDiff`] as pretty-printed JSON.
pub fn format_diff_json(diff: &StructuralDiff) -> Result<String> {
    serde_json::to_string_pretty(diff).context("failed to serialize diff as JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::declarations::{DeclKind, Declaration, Visibility};

    fn make_decl(kind: DeclKind, name: &str, sig: &str) -> Declaration {
        Declaration::new(
            kind,
            name.to_string(),
            sig.to_string(),
            Visibility::Public,
            1,
        )
    }

    #[test]
    fn test_diff_declarations_detects_added() {
        let old: Vec<Declaration> = vec![];
        let new = vec![make_decl(DeclKind::Function, "foo", "pub fn foo()")];

        let diff = diff_declarations(PathBuf::from("test.rs"), &old, &new);
        assert_eq!(diff.declarations_added.len(), 1);
        assert_eq!(diff.declarations_added[0].name, "foo");
        assert!(diff.declarations_removed.is_empty());
        assert!(diff.declarations_modified.is_empty());
    }

    #[test]
    fn test_diff_declarations_detects_removed() {
        let old = vec![make_decl(DeclKind::Function, "bar", "fn bar(x: i32)")];
        let new: Vec<Declaration> = vec![];

        let diff = diff_declarations(PathBuf::from("test.rs"), &old, &new);
        assert!(diff.declarations_added.is_empty());
        assert_eq!(diff.declarations_removed.len(), 1);
        assert_eq!(diff.declarations_removed[0].name, "bar");
    }

    #[test]
    fn test_diff_declarations_detects_modified() {
        let old = vec![make_decl(DeclKind::Function, "baz", "fn baz(x: i32)")];
        let new = vec![make_decl(
            DeclKind::Function,
            "baz",
            "fn baz(x: i32, y: i32)",
        )];

        let diff = diff_declarations(PathBuf::from("test.rs"), &old, &new);
        assert!(diff.declarations_added.is_empty());
        assert!(diff.declarations_removed.is_empty());
        assert_eq!(diff.declarations_modified.len(), 1);
        assert_eq!(
            diff.declarations_modified[0].old_signature,
            "fn baz(x: i32)"
        );
        assert_eq!(
            diff.declarations_modified[0].new_signature,
            "fn baz(x: i32, y: i32)"
        );
    }

    #[test]
    fn test_diff_declarations_unchanged_ignored() {
        let decl = make_decl(DeclKind::Struct, "Foo", "pub struct Foo");
        let old = vec![decl.clone()];
        let new = vec![decl];

        let diff = diff_declarations(PathBuf::from("test.rs"), &old, &new);
        assert!(diff.declarations_added.is_empty());
        assert!(diff.declarations_removed.is_empty());
        assert!(diff.declarations_modified.is_empty());
    }

    #[test]
    fn test_format_diff_markdown_basic() {
        let diff = StructuralDiff {
            since_ref: "v1.0".to_string(),
            files_added: vec![PathBuf::from("src/new.rs")],
            files_removed: vec![PathBuf::from("src/old.rs")],
            files_modified: vec![FileDiff {
                path: PathBuf::from("src/lib.rs"),
                declarations_added: vec![DeclChange {
                    kind: DeclKind::Function,
                    name: "new_fn".to_string(),
                    signature: "pub fn new_fn(x: i32) -> bool".to_string(),
                }],
                declarations_removed: vec![DeclChange {
                    kind: DeclKind::Function,
                    name: "old_fn".to_string(),
                    signature: "fn old_fn()".to_string(),
                }],
                declarations_modified: vec![DeclModification {
                    kind: DeclKind::Function,
                    name: "changed_fn".to_string(),
                    old_signature: "fn changed_fn(x: i32)".to_string(),
                    new_signature: "fn changed_fn(x: i32, y: i32)".to_string(),
                }],
            }],
        };

        let md = format_diff_markdown(&diff);
        assert!(md.contains("# Structural Changes (since v1.0)"));
        assert!(md.contains("- src/new.rs"));
        assert!(md.contains("- src/old.rs"));
        assert!(md.contains("+ `pub fn new_fn(x: i32) -> bool`"));
        assert!(md.contains("- `fn old_fn()`"));
        assert!(md.contains("~ `fn changed_fn(x: i32)` -> `fn changed_fn(x: i32, y: i32)`"));
    }

    #[test]
    fn test_format_diff_json_roundtrip() {
        let diff = StructuralDiff {
            since_ref: "abc123".to_string(),
            files_added: vec![],
            files_removed: vec![],
            files_modified: vec![],
        };

        let json = format_diff_json(&diff).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["since_ref"], "abc123");
    }
}
