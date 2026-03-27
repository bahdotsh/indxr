pub mod declarations;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use self::declarations::Declaration;
use crate::languages::Language;

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum DetailLevel {
    Summary,
    Signatures,
    Full,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub is_dir: bool,
    pub depth: usize,
}

#[derive(Debug, Serialize)]
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
    pub workspace_kind: String,
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
    /// Tries exact match within each member's file list.
    pub fn find_member_by_path(&self, path: &str) -> Option<&MemberIndex> {
        for member in &self.members {
            for file in &member.index.files {
                let file_path = file.path.to_string_lossy();
                if file_path == path || path.ends_with(&*file_path) || file_path.ends_with(path) {
                    return Some(member);
                }
            }
        }
        None
    }

    /// Returns true if this is a single-member "none" workspace (non-monorepo).
    pub fn is_single(&self) -> bool {
        self.workspace_kind == "none"
    }
}
