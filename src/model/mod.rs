pub mod declarations;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use self::declarations::Declaration;
use crate::languages::Language;
use crate::workspace::WorkspaceKind;

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum DetailLevel {
    Summary,
    Signatures,
    Full,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodebaseIndex {
    pub root: PathBuf,
    pub root_name: String,
    pub generated_at: String,
    pub files: Vec<FileIndex>,
    pub tree: Vec<TreeEntry>,
    pub stats: IndexStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    pub path: PathBuf,
    pub language: Language,
    pub size: u64,
    pub lines: usize,
    pub imports: Vec<Import>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub is_dir: bool,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    pub total_files: usize,
    pub total_lines: usize,
    pub languages: HashMap<String, usize>,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Workspace-level index (wraps multiple member CodebaseIndex instances)
// ---------------------------------------------------------------------------

/// A workspace-level index containing per-member indices.
#[derive(Debug, Serialize)]
pub struct WorkspaceIndex {
    /// Absolute path to the workspace root.
    pub root: PathBuf,
    pub root_name: String,
    pub workspace_kind: WorkspaceKind,
    pub generated_at: String,
    /// Per-member indices.
    pub members: Vec<MemberIndex>,
    /// Aggregate stats across all members.
    pub stats: IndexStats,
}

/// A single workspace member with its own `CodebaseIndex`.
#[derive(Debug, Serialize)]
pub struct MemberIndex {
    /// Short name (e.g. package/crate name).
    pub name: String,
    /// Path relative to the workspace root.
    pub relative_path: PathBuf,
    /// The member's own index.
    pub index: CodebaseIndex,
}

impl WorkspaceIndex {
    /// Find a member by name (case-insensitive).
    pub fn find_member(&self, name: &str) -> Option<&MemberIndex> {
        let lower = name.to_lowercase();
        self.members.iter().find(|m| m.name.to_lowercase() == lower)
    }

    /// Find the member that contains a given file path.
    ///
    /// Matching strategy (in priority order):
    /// 1. Exact match: `file_path == path`
    /// 2. Suffix match: `file_path` ends with `/<path>` (path-separator-aware)
    ///
    /// When multiple members match at the same specificity level,
    /// returns `None` to avoid silently picking the wrong member.
    pub fn find_member_by_path(&self, path: &str) -> Option<&MemberIndex> {
        let mut exact_count = 0usize;
        let mut exact_matched: Option<&MemberIndex> = None;
        let mut suffix_count = 0usize;
        let mut suffix_matched: Option<&MemberIndex> = None;

        let suffix_needle = format!("/{}", path);

        for member in &self.members {
            for file in &member.index.files {
                let file_path = file.path.to_string_lossy();

                if file_path == path {
                    exact_count += 1;
                    exact_matched = Some(member);
                    break;
                }

                if file_path.ends_with(&suffix_needle) {
                    suffix_count += 1;
                    suffix_matched = Some(member);
                    break;
                }
            }
        }

        if exact_count == 1 {
            return exact_matched;
        }
        if exact_count > 1 {
            return None; // Ambiguous: multiple members have this exact relative path
        }
        if suffix_count == 1 {
            return suffix_matched;
        }
        None // No match, or ambiguous suffix
    }

    /// Returns true if this is a single-member "none" workspace (non-monorepo).
    pub fn is_single(&self) -> bool {
        self.workspace_kind == WorkspaceKind::None
    }
}
