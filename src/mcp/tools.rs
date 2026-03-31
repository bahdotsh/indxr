use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::budget::estimate_tokens;
use crate::dep_graph;
use crate::diff;
use crate::github;
use crate::indexer::{self, WorkspaceConfig};
use crate::languages::Language;
use crate::model::declarations::{DeclKind, Declaration};
use crate::model::{CodebaseIndex, FileIndex, WorkspaceIndex};
use crate::parser::ParserRegistry;
use crate::parser::complexity::{collect_hotspots, compute_health_from_file_refs, sort_hotspots};

use super::helpers::*;
use super::type_flow::*;

// ---------------------------------------------------------------------------
// Tool definitions for tools/list
// ---------------------------------------------------------------------------

/// Tools that are only advertised when `--all-tools` is set.
/// They still *work* if called — they just aren't listed by default,
/// reducing per-request schema overhead. See `benchmark.md` for measurements.
///
/// The default surface is 3 compound tools (`find`, `summarize`, `read`)
/// that internally dispatch to the granular tools below. This keeps schema
/// overhead at ~420 tokens/round vs ~1,100+ for 23 granular tools.
const EXTENDED_TOOLS: &[&str] = &[
    // Granular tools (all still callable, just not listed by default)
    "lookup_symbol",
    "list_declarations",
    "search_signatures",
    "get_tree",
    "get_file_summary",
    "read_source",
    "get_file_context",
    "search_relevant",
    "batch_file_summaries",
    "get_callers",
    "get_public_api",
    "explain_symbol",
    // Extended tools
    "get_hotspots",
    "get_health",
    "get_type_flow",
    "get_dependency_graph",
    "get_diff_summary",
    "get_token_estimate",
    "list_workspace_members",
    "regenerate_index",
    "get_stats",
    "get_imports",
    "get_related_tests",
];

/// JSON property definition for the optional `member` parameter, shared across tools.
fn member_property() -> Value {
    json!({
        "type": "string",
        "description": "Workspace member name (omit to search all)"
    })
}

pub(super) fn tool_definitions(is_workspace: bool, all_tools: bool) -> Value {
    let mut defs = json!({
        "tools": [
            // -- Compound tools (default surface: 3 tools, ~420 tok/round) --
            {
                "name": "find",
                "description": "Find symbols, files, or references in the codebase.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search term (name, concept, or pattern)"
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["relevant", "symbol", "callers", "signature"],
                            "description": "relevant (default): ranked by relevance. symbol: exact name match. callers: who references this. signature: search in function signatures."
                        },
                        "kind": {
                            "type": "string",
                            "description": "Filter by declaration kind (e.g. fn, struct, class, trait). Only applies to relevant mode."
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "summarize",
                "description": "Get overview of a file, symbol, or directory without reading source. Pass a file path for file summary, a glob (e.g. 'src/**/*.rs') for batch summaries, or a symbol name (no '/') to explain a symbol's interface.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path, glob pattern, or symbol name"
                        },
                        "scope": {
                            "type": "string",
                            "enum": ["all", "public"],
                            "description": "all (default): all declarations. public: public API only. Only applies to file paths, ignored for symbol names."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "read",
                "description": "Read source code by symbol name or line range. Use symbol to read a specific function/struct, or start_line+end_line for a range. Cap: 200 lines per symbol, 500 total.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path"
                        },
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to read"
                        },
                        "symbols": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Multiple symbols in one call"
                        },
                        "start_line": {
                            "type": "number",
                            "description": "Start line (1-based)"
                        },
                        "end_line": {
                            "type": "number",
                            "description": "End line (1-based, inclusive)"
                        },
                        "collapse": {
                            "type": "boolean",
                            "description": "Fold nested function/block bodies to reduce output"
                        }
                    },
                    "required": ["path"]
                }
            },
            // -- Granular tools (listed only with --all-tools) --
            {
                "name": "list_workspace_members",
                "description": "List monorepo workspace members.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "lookup_symbol",
                "description": "Find declarations by name (case-insensitive substring).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Symbol name"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Max results (default 50)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Columnar output (fewer tokens)"
                        }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "list_declarations",
                "description": "List declarations in a file, optionally filtered by kind.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path"
                        },
                        "kind": {
                            "type": "string",
                            "description": "Filter: fn, struct, class, trait, etc."
                        },
                        "shallow": {
                            "type": "boolean",
                            "description": "Omit children and doc_comments"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Columnar output (fewer tokens)"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "search_signatures",
                "description": "Search function signatures by substring.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Substring to match in signatures"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Max results (default 20)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Columnar output (fewer tokens)"
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_tree",
                "description": "Directory/file tree of the codebase.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path prefix filter"
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "get_imports",
                "description": "Import statements for a file.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "get_stats",
                "description": "Codebase stats: file count, lines, languages.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "get_file_summary",
                "description": "File overview: imports, declarations, kind counts, test presence.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "read_source",
                "description": "Read source by symbol name or line range.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path"
                        },
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to read"
                        },
                        "start_line": {
                            "type": "number",
                            "description": "Start line (1-based)"
                        },
                        "end_line": {
                            "type": "number",
                            "description": "End line (1-based, inclusive)"
                        },
                        "expand": {
                            "type": "number",
                            "description": "Context lines above/below (default 0)"
                        },
                        "symbols": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Multiple symbols in one call (cap: 500 lines)"
                        },
                        "collapse": {
                            "type": "boolean",
                            "description": "Fold nested bodies to { ... }"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "get_file_context",
                "description": "File summary + reverse dependencies + related files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "regenerate_index",
                "description": "Re-index codebase and update INDEX.md. Use after code changes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "get_token_estimate",
                "description": "Estimate token cost of reading a file, symbol, or directory.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path"
                        },
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name (estimates just that symbol)"
                        },
                        "directory": {
                            "type": "string",
                            "description": "Directory path (all files within)"
                        },
                        "glob": {
                            "type": "string",
                            "description": "Glob pattern (all matching files)"
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "search_relevant",
                "description": "Search files/symbols by concept, name, or type pattern. Ranked results.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Concept, partial name, or type pattern"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Max results (default 20)"
                        },
                        "kind": {
                            "type": "string",
                            "description": "Filter: fn, struct, class, trait, etc."
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Columnar output (fewer tokens)"
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_diff_summary",
                "description": "Structural changes (added/removed/modified) since a git ref or PR.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "since_ref": {
                            "type": "string",
                            "description": "Git ref (branch, tag, HEAD~3)"
                        },
                        "pr": {
                            "type": "integer",
                            "description": "GitHub PR number"
                        }
                    }
                }
            },
            {
                "name": "batch_file_summaries",
                "description": "Summaries for multiple files in one call (cap: 30).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "File paths array"
                        },
                        "glob": {
                            "type": "string",
                            "description": "Glob pattern (e.g. 'src/**/*.rs')"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Return columnar {columns, rows} format (default false)"
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "get_callers",
                "description": "Find references to a symbol across all files (name-based).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Max results (default 20)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Return columnar {columns, rows} format (default false)"
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "get_public_api",
                "description": "Public declarations with signatures for a file or directory.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File or directory path"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Max results (default 100)"
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "explain_symbol",
                "description": "Symbol interface: signature, doc comment, relationships. No body.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Symbol name (case-insensitive)"
                        }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "get_related_tests",
                "description": "Find test functions for a symbol by naming convention.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name"
                        },
                        "path": {
                            "type": "string",
                            "description": "Scope to file path"
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "get_dependency_graph",
                "description": "File or symbol dependency graph (DOT/Mermaid/JSON).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Scope to subtree"
                        },
                        "level": {
                            "type": "string",
                            "enum": ["file", "symbol"],
                            "description": "file (default) or symbol granularity"
                        },
                        "format": {
                            "type": "string",
                            "enum": ["dot", "mermaid", "json"],
                            "description": "Output format (default: mermaid)"
                        },
                        "depth": {
                            "type": "number",
                            "description": "Max edge hops (default: unlimited)"
                        }
                    }
                }
            },
            {
                "name": "get_hotspots",
                "description": "Most complex functions ranked by composite score.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {
                            "type": "number",
                            "description": "Max results (default 20)"
                        },
                        "path": {
                            "type": "string",
                            "description": "File or directory filter"
                        },
                        "min_complexity": {
                            "type": "number",
                            "description": "Min cyclomatic complexity (default 1)"
                        },
                        "sort_by": {
                            "type": "string",
                            "enum": ["score", "complexity", "nesting", "params", "body_lines"],
                            "description": "Sort criterion (default: score)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Columnar output (fewer tokens)"
                        }
                    }
                }
            },
            {
                "name": "get_health",
                "description": "Codebase health: complexity, doc coverage, test ratio.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Scope to directory or file"
                        }
                    }
                }
            },
            {
                "name": "get_type_flow",
                "description": "Which functions produce (return) and consume (accept) a type.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "type_name": {
                            "type": "string",
                            "description": "Type name to track"
                        },
                        "path": {
                            "type": "string",
                            "description": "Scope to directory or file"
                        },
                        "include_fields": {
                            "type": "boolean",
                            "description": "Include struct fields holding this type"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Max results per role (default 50)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "Columnar output (fewer tokens)"
                        }
                    },
                    "required": ["type_name"]
                }
            }
        ]
    });

    // Post-process: conditionally add `member` property and filter extended tools.
    let mp = member_property();
    let skip_member = ["list_workspace_members", "regenerate_index"];
    if let Some(tools) = defs["tools"].as_array_mut() {
        // Only inject `member` param when serving a multi-member workspace.
        if is_workspace {
            for tool in tools.iter_mut() {
                let name = tool["name"].as_str().unwrap_or("");
                if skip_member.contains(&name) {
                    continue;
                }
                if let Some(props) = tool["inputSchema"]["properties"].as_object_mut() {
                    if !props.contains_key("member") {
                        props.insert("member".to_string(), mp.clone());
                    }
                }
            }
        }

        // Filter out extended tools unless --all-tools is set.
        if !all_tools {
            tools.retain(|tool| {
                let name = tool["name"].as_str().unwrap_or("");
                !EXTENDED_TOOLS.contains(&name)
            });
        }
    }

    defs
}

// ---------------------------------------------------------------------------
// Workspace resolution helpers
// ---------------------------------------------------------------------------

/// Resolve which member indices to operate on, based on optional `member` arg.
/// Returns a list of (member_name, &CodebaseIndex) pairs.
fn resolve_indices<'a>(
    workspace: &'a WorkspaceIndex,
    args: &Value,
) -> Result<Vec<(&'a str, &'a CodebaseIndex)>, Value> {
    if let Some(member_name) = args.get("member").and_then(|v| v.as_str()) {
        match workspace.find_member(member_name) {
            Some(m) => Ok(vec![(&m.name, &m.index)]),
            None => Err(tool_error(&format!(
                "Unknown workspace member: {}. Use list_workspace_members to see available members.",
                member_name
            ))),
        }
    } else {
        Ok(workspace
            .members
            .iter()
            .map(|m| (m.name.as_str(), &m.index))
            .collect())
    }
}

/// Resolve a single member index from a path argument, searching across members.
///
/// Returns `Err` with a tool error if an explicit `member` param doesn't match.
/// Returns `Ok(None)` only when no member can be found by path and there are
/// multiple members (caller should produce a "file not found" error).
fn resolve_index_by_path<'a>(
    workspace: &'a WorkspaceIndex,
    args: &Value,
    path: &str,
) -> Result<Option<(&'a str, &'a CodebaseIndex)>, Value> {
    // If member is explicitly specified, use it — error if it doesn't match
    if let Some(member_name) = args.get("member").and_then(|v| v.as_str()) {
        return match workspace.find_member(member_name) {
            Some(m) => Ok(Some((&m.name, &m.index))),
            None => Err(tool_error(&format!(
                "Unknown workspace member: {}. Use list_workspace_members to see available members.",
                member_name
            ))),
        };
    }
    // Auto-resolve by finding which member has this file
    if let Some(m) = workspace.find_member_by_path(path) {
        return Ok(Some((&m.name, &m.index)));
    }
    // Single-member fallback
    if workspace.members.len() == 1 {
        let m = &workspace.members[0];
        return Ok(Some((&m.name, &m.index)));
    }
    Ok(None)
}

/// Collect borrowed file references from resolved indices (zero-copy).
fn collect_file_refs<'a>(indices: &[(&str, &'a CodebaseIndex)]) -> Vec<&'a FileIndex> {
    indices
        .iter()
        .flat_map(|(_, index)| index.files.iter())
        .collect()
}

/// Merged structural diff result across workspace members.
struct MergedStructuralDiff {
    files_added: Vec<PathBuf>,
    files_removed: Vec<PathBuf>,
    files_modified: Vec<diff::FileDiff>,
}

/// Compute structural diffs across all workspace members and merge/dedup results.
///
/// Each member is diffed independently against `old_files`, then the results are
/// merged. `files_added` and `files_removed` are deduplicated by path equality.
/// `files_modified` is deduplicated by path — if two members produce a `FileDiff`
/// for the same path, only the first is kept (in practice this shouldn't happen
/// since each file belongs to exactly one member).
fn merge_member_diffs(
    workspace: &WorkspaceIndex,
    old_files: &HashMap<PathBuf, FileIndex>,
    changed_paths: &[PathBuf],
) -> MergedStructuralDiff {
    let mut files_added = Vec::new();
    let mut files_removed = Vec::new();
    let mut files_modified = Vec::new();

    for member in &workspace.members {
        let sd = diff::compute_structural_diff(&member.index, old_files, changed_paths);
        files_added.extend(sd.files_added);
        files_removed.extend(sd.files_removed);
        files_modified.extend(sd.files_modified);
    }

    // Deduplicate by path
    files_added.sort();
    files_added.dedup();
    files_removed.sort();
    files_removed.dedup();
    // FileDiff doesn't impl Ord, so dedup by path manually
    {
        let mut seen = std::collections::HashSet::new();
        files_modified.retain(|fd| seen.insert(fd.path.clone()));
    }

    MergedStructuralDiff {
        files_added,
        files_removed,
        files_modified,
    }
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

pub(super) fn handle_tool_call(workspace: &WorkspaceIndex, name: &str, args: &Value) -> Value {
    match name {
        // Compound tools (default surface)
        "find" => tool_find(workspace, args),
        "summarize" => tool_summarize(workspace, args),
        "read" => tool_read_source(workspace, args),
        // Granular tools (still callable, listed with --all-tools)
        "list_workspace_members" => tool_list_workspace_members(workspace),
        "lookup_symbol" => tool_lookup_symbol(workspace, args),
        "search_signatures" => tool_search_signatures(workspace, args),
        "search_relevant" => tool_search_relevant(workspace, args),
        "get_callers" => tool_get_callers(workspace, args),
        "explain_symbol" => tool_explain_symbol(workspace, args),
        "get_related_tests" => tool_get_related_tests(workspace, args),
        "get_hotspots" => tool_get_hotspots(workspace, args),
        "get_health" => tool_get_health(workspace, args),
        "get_type_flow" => tool_get_type_flow(workspace, args),
        "get_public_api" => tool_get_public_api(workspace, args),
        "get_dependency_graph" => tool_get_dependency_graph(workspace, args),
        // Tools that operate on aggregate/workspace level
        "get_stats" => tool_get_stats(workspace, args),
        "get_tree" => tool_get_tree(workspace, args),
        "get_token_estimate" => tool_get_token_estimate(workspace, args),
        "batch_file_summaries" => tool_batch_file_summaries(workspace, args),
        // Tools that need a specific file (resolve member from path)
        "list_declarations" => tool_list_declarations(workspace, args),
        "get_imports" => tool_get_imports(workspace, args),
        "get_file_summary" => tool_get_file_summary(workspace, args),
        "read_source" => tool_read_source(workspace, args),
        "get_file_context" => tool_get_file_context(workspace, args),
        _ => tool_error(&format!("Unknown tool: {}", name)),
    }
}

// ---------------------------------------------------------------------------
// Compound tool implementations
// ---------------------------------------------------------------------------

/// Copy `member` from outer compound tool args into an inner args object,
/// so workspace scoping is preserved when dispatching to granular tools.
fn forward_member(from: &Value, to: &mut Value) {
    if let Some(member) = from.get("member") {
        to["member"] = member.clone();
    }
}

/// `find` — unified search: relevant (default), symbol, callers, signature.
fn tool_find(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("Missing required parameter: query"),
    };
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("relevant");

    match mode {
        "symbol" => {
            let mut inner = json!({"name": query, "compact": true});
            forward_member(args, &mut inner);
            tool_lookup_symbol(workspace, &inner)
        }
        "callers" => {
            let mut inner = json!({"symbol": query, "compact": true});
            forward_member(args, &mut inner);
            tool_get_callers(workspace, &inner)
        }
        "signature" => {
            let mut inner = json!({"query": query, "compact": true});
            forward_member(args, &mut inner);
            tool_search_signatures(workspace, &inner)
        }
        "relevant" => {
            let mut inner = json!({"query": query, "compact": true});
            if let Some(kind) = args.get("kind") {
                inner["kind"] = kind.clone();
            }
            forward_member(args, &mut inner);
            tool_search_relevant(workspace, &inner)
        }
        other => tool_error(&format!(
            "Unknown find mode: '{}'. Valid modes: relevant, symbol, callers, signature",
            other
        )),
    }
}

/// `summarize` — unified file/symbol/batch overview.
/// Routes based on `path` content: glob → batch, bare name without file
/// extension → explain_symbol, scope=public → get_public_api,
/// else → get_file_summary.
/// Note: `scope` only applies to file paths — it is ignored for symbol names
/// (which always route to `explain_symbol`).
fn tool_summarize(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("all");

    // Glob pattern → batch_file_summaries
    if path.contains('*') || path.contains('?') {
        let mut inner = json!({"glob": path, "compact": true});
        forward_member(args, &mut inner);
        return tool_batch_file_summaries(workspace, &inner);
    }

    // No "/" and no file extension → could be a symbol name or a bare directory name.
    // Check if it matches a known directory prefix in the index before treating as symbol.
    if !path.contains('/') && !looks_like_file(path) && !is_known_directory(workspace, path) {
        let mut inner = json!({"name": path});
        forward_member(args, &mut inner);
        return tool_explain_symbol(workspace, &inner);
    }

    // Public scope → get_public_api
    if scope == "public" {
        let mut inner = json!({"path": path});
        forward_member(args, &mut inner);
        return tool_get_public_api(workspace, &inner);
    }

    // Default → get_file_summary
    let mut inner = json!({"path": path});
    forward_member(args, &mut inner);
    tool_get_file_summary(workspace, &inner)
}

/// Returns true if the string looks like a filename (has a recognized extension).
/// This list is intentionally broader than parser-supported languages — it includes
/// config, doc, and data formats so they route to `get_file_summary` rather than
/// `explain_symbol` in the `summarize` compound tool.
pub(super) fn looks_like_file(s: &str) -> bool {
    matches!(
        s.rsplit('.').next(),
        Some(
            "rs" | "py"
                | "js"
                | "ts"
                | "tsx"
                | "jsx"
                | "go"
                | "java"
                | "c"
                | "cpp"
                | "cc"
                | "h"
                | "hpp"
                | "rb"
                | "swift"
                | "kt"
                | "scala"
                | "cs"
                | "php"
                | "lua"
                | "zig"
                | "ex"
                | "exs"
                | "erl"
                | "hs"
                | "ml"
                | "toml"
                | "yaml"
                | "yml"
                | "json"
                | "md"
                | "txt"
                | "sh"
                | "bash"
                | "zsh"
                | "r"
                | "R"
                | "vue"
                | "svelte"
                | "css"
                | "html"
                | "xml"
                | "sql"
                | "proto"
                | "thrift"
                | "dart"
                | "nim"
                | "v"
                | "tf"
                | "hcl"
        )
    ) && s.contains('.')
}

/// Returns true if `name` matches a directory prefix of any indexed file path.
/// This prevents bare directory names like `"src"` or `"."` from being misrouted
/// to `explain_symbol` in the `summarize` compound tool.
fn is_known_directory(workspace: &WorkspaceIndex, name: &str) -> bool {
    if name == "." {
        return true;
    }
    let prefix = format!("{}/", name);
    workspace
        .members
        .iter()
        .flat_map(|m| m.index.files.iter())
        .any(|f| {
            f.path
                .to_str()
                .is_some_and(|p| p.starts_with(&prefix) || p == name)
        })
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

pub(super) fn tool_regenerate_index(
    workspace: &mut WorkspaceIndex,
    config: &WorkspaceConfig,
) -> Value {
    // Snapshot current state for delta computation
    let mut old_files: HashMap<PathBuf, FileIndex> = HashMap::new();
    for member in &workspace.members {
        for f in &member.index.files {
            old_files.insert(f.path.clone(), f.clone());
        }
    }

    match indexer::regenerate_workspace_index(config) {
        Ok(new_ws) => {
            let file_count = new_ws.stats.total_files;
            let line_count = new_ws.stats.total_lines;
            let output_path = new_ws.root.join("INDEX.md");

            // Collect all paths (old + new) for structural diff
            let mut all_paths: Vec<PathBuf> = old_files.keys().cloned().collect();
            for member in &new_ws.members {
                for f in &member.index.files {
                    if !old_files.contains_key(&f.path) {
                        all_paths.push(f.path.clone());
                    }
                }
            }

            let merged = merge_member_diffs(&new_ws, &old_files, &all_paths);
            let has_changes = !merged.files_added.is_empty()
                || !merged.files_removed.is_empty()
                || !merged.files_modified.is_empty();

            let mut result = json!({
                "status": "ok",
                "message": format!(
                    "INDEX.md regenerated ({} files, {} lines)",
                    file_count, line_count
                ),
                "path": output_path.to_string_lossy(),
                "files_indexed": file_count,
                "total_lines": line_count
            });

            if has_changes {
                result["changes"] = json!({
                    "files_added": merged.files_added.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
                    "files_removed": merged.files_removed.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
                    "files_modified": merged.files_modified.iter().map(|fd| json!({
                        "path": fd.path.to_string_lossy().to_string(),
                        "added": fd.declarations_added.len(),
                        "removed": fd.declarations_removed.len(),
                        "modified": fd.declarations_modified.len(),
                    })).collect::<Vec<_>>()
                });
            }

            *workspace = new_ws;
            tool_result(result)
        }
        Err(e) => tool_error(&format!("Failed to regenerate index: {}", e)),
    }
}

fn tool_list_workspace_members(workspace: &WorkspaceIndex) -> Value {
    let members: Vec<Value> = workspace
        .members
        .iter()
        .map(|m| {
            json!({
                "name": m.name,
                "path": m.relative_path.to_string_lossy(),
                "files": m.index.stats.total_files,
                "lines": m.index.stats.total_lines,
            })
        })
        .collect();

    tool_result(json!({
        "workspace_kind": workspace.workspace_kind,
        "workspace_root": workspace.root.to_string_lossy(),
        "member_count": members.len(),
        "members": members,
    }))
}

pub(super) fn tool_lookup_symbol(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_error("Missing required parameter: name"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(200) as usize;

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let query = name.to_lowercase();
    let mut results = Vec::new();

    for (_member_name, index) in &indices {
        for file in &index.files {
            if results.len() >= limit {
                break;
            }
            let file_path = file.path.to_string_lossy().to_string();
            for decl in &file.declarations {
                find_symbols_in_decl(decl, &query, &file_path, &mut results, limit);
                if results.len() >= limit {
                    break;
                }
            }
        }
    }

    let total = results.len();
    let truncated = total >= limit;

    let mut result = if is_compact(args) {
        let mut r = serialize_compact(&results, &["file", "kind", "name", "signature", "line"]);
        r["matches"] = json!(total);
        r
    } else {
        json!({
            "matches": total,
            "symbols": results
        })
    };
    if truncated {
        result["truncated"] = json!(true);
        result["limit"] = json!(limit);
    }
    tool_result(result)
}

pub(super) fn tool_list_declarations(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let (_member_name, index) = match resolve_index_by_path(workspace, args, path) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return tool_error(&format!("File not found in any workspace member: {}", path));
        }
        Err(e) => return e,
    };

    let file = find_file(index, path);
    let file = match file {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    let shallow = args
        .get("shallow")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let compact = is_compact(args);

    let kind_filter = args
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(DeclKind::from_name);

    if shallow || compact {
        // Shallow/compact mode: return without children/doc_comments
        let decls: Vec<ShallowDeclaration> = if let Some(ref kind) = kind_filter {
            filter_declarations(&file.declarations, kind)
                .into_iter()
                .map(to_shallow)
                .collect()
        } else {
            file.declarations.iter().map(to_shallow).collect()
        };
        if compact {
            return tool_result(json!({
                "file": path,
                "count": decls.len(),
                "declarations": serialize_compact(&decls, &["kind", "name", "signature", "line"])
            }));
        }
        tool_result(json!({
            "file": path,
            "count": decls.len(),
            "declarations": decls
        }))
    } else if let Some(ref kind) = kind_filter {
        let filtered = filter_declarations(&file.declarations, kind);
        let serialized: Vec<Value> = filtered
            .iter()
            .map(|d| serde_json::to_value(d).unwrap_or(Value::Null))
            .collect();
        tool_result(json!({
            "file": path,
            "count": serialized.len(),
            "declarations": serialized
        }))
    } else {
        tool_result(json!({
            "file": path,
            "count": file.declarations.len(),
            "declarations": file.declarations
        }))
    }
}

pub(super) fn tool_search_signatures(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("Missing required parameter: query"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(100) as usize;

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for (_member_name, index) in &indices {
        for file in &index.files {
            if results.len() >= limit {
                break;
            }
            let file_path = file.path.to_string_lossy().to_string();
            for decl in &file.declarations {
                find_signatures_in_decl(decl, &query_lower, &file_path, &mut results, limit);
                if results.len() >= limit {
                    break;
                }
            }
        }
    }

    let total = results.len();
    let truncated = total >= limit;

    let mut result = if is_compact(args) {
        let mut r = serialize_compact(&results, &["file", "kind", "name", "signature", "line"]);
        r["matches"] = json!(total);
        r
    } else {
        json!({
            "matches": total,
            "signatures": results
        })
    };
    if truncated {
        result["truncated"] = json!(true);
        result["limit"] = json!(limit);
    }
    tool_result(result)
}

pub(super) fn tool_get_tree(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path_prefix = args.get("path").and_then(|v| v.as_str());

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let mut entries: Vec<Value> = Vec::new();
    for (member_name, index) in &indices {
        let prefix_label = if !workspace.is_single() {
            Some(*member_name)
        } else {
            None
        };
        for entry in &index.tree {
            if let Some(prefix) = path_prefix {
                if !entry.path.starts_with(prefix) {
                    continue;
                }
            }
            let mut e = json!({
                "path": entry.path,
                "is_dir": entry.is_dir,
                "depth": entry.depth
            });
            if let Some(label) = prefix_label {
                e["member"] = json!(label);
            }
            entries.push(e);
        }
    }

    tool_result(json!({
        "count": entries.len(),
        "entries": entries
    }))
}

pub(super) fn tool_get_imports(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let (_member_name, index) = match resolve_index_by_path(workspace, args, path) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return tool_error(&format!("File not found in any workspace member: {}", path));
        }
        Err(e) => return e,
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    tool_result(json!({
        "file": path,
        "count": file.imports.len(),
        "imports": file.imports
    }))
}

pub(super) fn tool_get_stats(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    if indices.len() == 1 {
        let index = indices[0].1;
        return tool_result(json!({
            "root": index.root.to_string_lossy(),
            "root_name": index.root_name,
            "generated_at": index.generated_at,
            "total_files": index.stats.total_files,
            "total_lines": index.stats.total_lines,
            "languages": index.stats.languages,
            "duration_ms": index.stats.duration_ms
        }));
    }

    tool_result(json!({
        "root": workspace.root.to_string_lossy(),
        "root_name": workspace.root_name,
        "workspace_kind": workspace.workspace_kind,
        "generated_at": workspace.generated_at,
        "total_files": workspace.stats.total_files,
        "total_lines": workspace.stats.total_lines,
        "languages": workspace.stats.languages,
        "member_count": workspace.members.len(),
        "duration_ms": workspace.stats.duration_ms
    }))
}

pub(super) fn tool_get_file_summary(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let (_member_name, index) = match resolve_index_by_path(workspace, args, path) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return tool_error(&format!("File not found in any workspace member: {}", path));
        }
        Err(e) => return e,
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    tool_result(file_summary_data(file))
}

pub(super) fn tool_read_source(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let (_member_name, index) = match resolve_index_by_path(workspace, args, path) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return tool_error(&format!("File not found in any workspace member: {}", path));
        }
        Err(e) => return e,
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    let expand = args.get("expand").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let collapse = args
        .get("collapse")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Multi-symbol mode
    let symbols = args.get("symbols").and_then(|v| v.as_array());
    if let Some(sym_arr) = symbols {
        let abs_path = index.root.join(&file.path);
        let mut entries = Vec::new();
        let mut not_found = Vec::new();
        let mut total_lines = 0usize;
        let max_total_lines = 500;
        let mut truncated_at_limit = false;

        for sym_val in sym_arr {
            let sym = match sym_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            if total_lines >= max_total_lines {
                truncated_at_limit = true;
                break;
            }
            let decl = match find_decl_by_name(&file.declarations, sym) {
                Some(d) => d,
                None => {
                    not_found.push(sym.to_string());
                    continue;
                }
            };
            let body = decl.body_lines.unwrap_or(1);
            let s = if expand < decl.line {
                decl.line - expand
            } else {
                1
            };
            let e = (decl.line + body + expand).min(s + max_total_lines - total_lines - 1);

            match read_line_range(&abs_path, s, e) {
                Ok(source) => {
                    let lines_read = e - s + 1;
                    total_lines += lines_read;
                    let source = if collapse {
                        collapse_nested_bodies(&source)
                    } else {
                        source
                    };
                    entries.push(json!({
                        "symbol": decl.name,
                        "kind": format!("{}", decl.kind),
                        "start_line": s,
                        "end_line": e,
                        "source": source
                    }));
                }
                Err(_) => continue,
            }
        }

        let mut result = json!({
            "file": file.path.to_string_lossy(),
            "symbols": entries
        });
        if !not_found.is_empty() {
            result["not_found"] = json!(not_found);
        }
        if truncated_at_limit {
            result["truncated"] = json!(true);
            result["line_limit"] = json!(max_total_lines);
        }
        return tool_result(result);
    }

    // Single symbol or line range mode
    let symbol_name = args.get("symbol").and_then(|v| v.as_str());
    let start_line = args.get("start_line").and_then(|v| v.as_u64());
    let end_line = args.get("end_line").and_then(|v| v.as_u64());

    let (start, end, symbol_info) = if let Some(sym) = symbol_name {
        let decl = match find_decl_by_name(&file.declarations, sym) {
            Some(d) => d,
            None => {
                return tool_error(&format!(
                    "Symbol '{}' not found in {}",
                    sym,
                    file.path.to_string_lossy()
                ));
            }
        };
        let body = decl.body_lines.unwrap_or(1);
        let s = decl.line;
        let e = decl.line + body;
        (s, e, Some((decl.name.clone(), format!("{}", decl.kind))))
    } else if let (Some(s), Some(e)) = (start_line, end_line) {
        (s as usize, e as usize, None)
    } else {
        return tool_error("Provide 'symbol', 'symbols', or both 'start_line' and 'end_line'");
    };

    // Apply expand and cap at 200 lines
    let start = if expand < start { start - expand } else { 1 };
    let end = end + expand;
    let max_lines = 200;
    let end = if end - start + 1 > max_lines {
        start + max_lines - 1
    } else {
        end
    };

    let abs_path = index.root.join(&file.path);
    let source = match read_line_range(&abs_path, start, end) {
        Ok(s) => s,
        Err(e) => return tool_error(&e),
    };

    let source = if collapse {
        collapse_nested_bodies(&source)
    } else {
        source
    };

    let mut result = json!({
        "file": file.path.to_string_lossy(),
        "start_line": start,
        "end_line": end,
        "source": source
    });
    if let Some((name, kind)) = symbol_info {
        result["symbol"] = json!(name);
        result["kind"] = json!(kind);
    }
    if collapse {
        result["collapsed"] = json!(true);
    }
    tool_result(result)
}

pub(super) fn tool_get_file_context(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let (_member_name, index) = match resolve_index_by_path(workspace, args, path) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return tool_error(&format!("File not found in any workspace member: {}", path));
        }
        Err(e) => return e,
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    let mut summary = file_summary_data(file);

    // --- Reverse dependencies: find files whose imports reference this file ---
    let file_path_str = file.path.to_string_lossy().to_string();
    let file_stem = file
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // Build path-based match targets (e.g., "parser/mod", "parser")
    let path_no_ext = file.path.with_extension("").to_string_lossy().to_string();
    // For mod.rs files, also match on the parent directory name
    let parent_module = if file_stem == "mod" || file_stem == "index" {
        file.path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
    } else {
        None
    };

    #[derive(Serialize)]
    struct ImportedBy {
        file: String,
        import: String,
    }

    let mut imported_by: Vec<ImportedBy> = Vec::new();
    for other_file in &index.files {
        let other_path = other_file.path.to_string_lossy().to_string();
        if other_path == file_path_str {
            continue; // skip self
        }
        for imp in &other_file.imports {
            let text = &imp.text;
            // Skip stem-based matching for very short stems (high false-positive rate)
            let matches = (file_stem.len() >= 3 && text.contains(&file_stem))
                || text.contains(&path_no_ext)
                || parent_module
                    .as_ref()
                    .is_some_and(|pm| pm.len() >= 3 && text.contains(pm.as_str()));
            if matches {
                imported_by.push(ImportedBy {
                    file: other_path.clone(),
                    import: text.clone(),
                });
                break; // one match per file is enough
            }
        }
    }

    // --- Related files: tests and siblings ---
    #[derive(Serialize)]
    struct RelatedFile {
        file: String,
        relation: String,
    }

    let mut related: Vec<RelatedFile> = Vec::new();
    let file_dir = file.path.parent().unwrap_or(Path::new(""));

    // Test file patterns
    let test_patterns: Vec<String> = vec![
        format!("{}_test.", file_stem),
        format!("{}_spec.", file_stem),
        format!("test_{}.", file_stem),
        format!("{}.test.", file_stem),
        format!("{}.spec.", file_stem),
    ];

    let mut sibling_count = 0usize;
    for other_file in &index.files {
        let other_path_str = other_file.path.to_string_lossy().to_string();
        if other_path_str == file_path_str {
            continue;
        }

        // Check for test files (anywhere in the project)
        let other_name = other_file
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if test_patterns.iter().any(|p| other_name.starts_with(p)) {
            related.push(RelatedFile {
                file: other_path_str.clone(),
                relation: "test".to_string(),
            });
            continue;
        }

        // Check for siblings (same directory, cap at 10)
        let other_dir = other_file.path.parent().unwrap_or(Path::new(""));
        if other_dir == file_dir && sibling_count < 10 {
            related.push(RelatedFile {
                file: other_path_str,
                relation: "sibling".to_string(),
            });
            sibling_count += 1;
        }
    }

    summary["imported_by"] = json!(imported_by);
    summary["related_files"] = json!(related);

    tool_result(summary)
}

pub(super) fn tool_get_token_estimate(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = args.get("path").and_then(|v| v.as_str());
    let directory = args.get("directory").and_then(|v| v.as_str());
    let glob = args.get("glob").and_then(|v| v.as_str());

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    // Directory/glob mode: estimate tokens for multiple files
    if let Some(dir_or_glob) = directory.or(glob) {
        let is_dir = directory.is_some();
        let glob_matcher = if !is_dir {
            compile_glob_matcher(dir_or_glob)
        } else {
            None
        };
        let mut matched_files: Vec<&FileIndex> = Vec::new();
        for (_member_name, index) in &indices {
            for f in &index.files {
                let fp = f.path.to_string_lossy();
                let matches = if is_dir {
                    fp.starts_with(dir_or_glob) || fp.starts_with(&format!("{}/", dir_or_glob))
                } else {
                    match &glob_matcher {
                        Some(m) => m.is_match(fp.as_ref()),
                        None => fp == dir_or_glob || fp.starts_with(&format!("{}/", dir_or_glob)),
                    }
                };
                if matches {
                    matched_files.push(f);
                }
            }
        }

        let mut total_tokens = 0usize;
        let mut total_lines = 0usize;
        let mut breakdown = Vec::new();
        for f in matched_files.iter() {
            let tokens = (f.size as usize).div_ceil(4);
            total_tokens += tokens;
            total_lines += f.lines;
            if breakdown.len() < 50 {
                breakdown.push(json!({
                    "path": f.path.to_string_lossy(),
                    "tokens": tokens,
                    "lines": f.lines
                }));
            }
        }

        return tool_result(json!({
            "query": dir_or_glob,
            "file_count": matched_files.len(),
            "total_tokens": total_tokens,
            "total_lines": total_lines,
            "breakdown": breakdown,
            "recommendation": if total_tokens > 2000 {
                format!("Large scope (~{} tokens across {} files). Use get_file_summary or search_relevant to narrow down before reading.", total_tokens, matched_files.len())
            } else {
                format!("Manageable scope (~{} tokens across {} files).", total_tokens, matched_files.len())
            }
        }));
    }

    // Single file mode (original behavior)
    let path = match path {
        Some(p) => p,
        None => return tool_error("Provide 'path', 'directory', or 'glob'"),
    };

    let (_member_name, index) = match resolve_index_by_path(workspace, args, path) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return tool_error(&format!("File not found in any workspace member: {}", path));
        }
        Err(e) => return e,
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    // Read file content once for all estimates
    let abs_path = index.root.join(&file.path);
    let file_content = std::fs::read_to_string(&abs_path).unwrap_or_default();
    let full_file_tokens = estimate_tokens(&file_content);

    let symbol_name = args.get("symbol").and_then(|v| v.as_str());

    if let Some(sym) = symbol_name {
        let decl = match find_decl_by_name(&file.declarations, sym) {
            Some(d) => d,
            None => {
                return tool_error(&format!(
                    "Symbol '{}' not found in {}",
                    sym,
                    file.path.to_string_lossy()
                ));
            }
        };
        let body_lines = decl.body_lines.unwrap_or(1);
        let start = decl.line;
        let end = decl.line + body_lines;
        let symbol_tokens = match read_line_range(&abs_path, start, end) {
            Ok(source) => estimate_tokens(&source),
            Err(_) => body_lines * 10, // fallback heuristic
        };
        tool_result(json!({
            "file": file.path.to_string_lossy(),
            "symbol": sym,
            "symbol_tokens": symbol_tokens,
            "symbol_lines": body_lines,
            "full_file_tokens": full_file_tokens,
            "full_file_lines": file.lines,
            "savings": format!("read_source saves ~{} tokens ({}% reduction)",
                full_file_tokens.saturating_sub(symbol_tokens),
                if full_file_tokens > 0 { 100 - (symbol_tokens * 100 / full_file_tokens) } else { 0 }
            )
        }))
    } else {
        tool_result(json!({
            "file": file.path.to_string_lossy(),
            "full_file_tokens": full_file_tokens,
            "full_file_lines": file.lines,
            "summary_tokens": APPROX_SUMMARY_TOKENS,
            "declaration_count": file.declarations.len(),
            "recommendation": if full_file_tokens > 500 {
                format!("Use get_file_summary (~{} tokens) instead of Read (~{} tokens). Use read_source for specific symbols.", APPROX_SUMMARY_TOKENS, full_file_tokens)
            } else {
                format!("File is small (~{} tokens) — Read is fine here.", full_file_tokens)
            }
        }))
    }
}

pub(super) fn tool_search_relevant(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("Missing required parameter: query"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(50) as usize;
    let kind_filter = args
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(DeclKind::from_name);

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
    let mut results: Vec<RelevanceMatch> = Vec::new();

    for (_member_name, index) in &indices {
        for file in &index.files {
            let file_path = file.path.to_string_lossy().to_string();
            let file_path_lower = file_path.to_lowercase();

            // Score file path matches (skip when kind filter is active)
            if kind_filter.is_none() {
                let path_score = score_match(&file_path_lower, &query_lower, &query_terms);
                if path_score > 0 {
                    results.push(RelevanceMatch {
                        file: file_path.clone(),
                        symbol: None,
                        kind: None,
                        signature: None,
                        line: None,
                        match_on: "path".to_string(),
                        score: path_score,
                    });
                }
            }

            // Score declaration matches
            score_decls_recursive(
                &file.declarations,
                &file_path,
                &query_lower,
                &query_terms,
                &mut results,
                kind_filter.as_ref(),
            );
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(limit);

    let total = results.len();

    if is_compact(args) {
        return tool_result(json!({
            "query": query,
            "matches": total,
            "results": serialize_compact(&results, &["file", "symbol", "kind", "signature", "line", "score"])
        }));
    }

    tool_result(json!({
        "query": query,
        "matches": total,
        "results": results
    }))
}

pub(super) fn tool_get_diff_summary(
    workspace: &WorkspaceIndex,
    _config: &WorkspaceConfig,
    registry: &ParserRegistry,
    args: &Value,
) -> Value {
    if workspace.members.is_empty() {
        return tool_error("No workspace members available");
    }
    let has_pr = args.get("pr").is_some_and(|v| !v.is_null());
    let has_since = args.get("since_ref").is_some_and(|v| !v.is_null());

    if has_pr && has_since {
        return tool_error("Provide either 'pr' or 'since_ref', not both");
    }

    // Resolve the git ref — either from a PR number or a direct since_ref
    let (resolved_ref, pr_info) = if let Some(pr_val) = args.get("pr") {
        if let Some(pr_num) = pr_val.as_u64().filter(|&n| n > 0) {
            match github::resolve_pr_base(&workspace.root, pr_num) {
                Ok((local_ref, info)) => (Some(local_ref), Some(info)),
                Err(e) => return tool_error(&format!("Failed to resolve PR #{}: {}", pr_num, e)),
            }
        } else {
            return tool_error("'pr' must be a positive integer");
        }
    } else {
        (None, None)
    };

    let since_ref = if let Some(r) = resolved_ref {
        r
    } else if let Some(r) = args.get("since_ref").and_then(|v| v.as_str()) {
        let r = r.trim();
        if r.is_empty() {
            return tool_error("'since_ref' must not be empty");
        }
        r.to_string()
    } else {
        return tool_error("Missing required parameter: either 'since_ref' or 'pr'");
    };

    let changed_paths = match diff::get_changed_files(&workspace.root, &since_ref) {
        Ok(paths) => paths,
        Err(e) => return tool_error(&format!("Git diff failed: {}", e)),
    };

    if changed_paths.is_empty() {
        let mut result = json!({
            "since_ref": since_ref,
            "changes": 0,
            "files_added": [],
            "files_removed": [],
            "files_modified": []
        });
        if let Some(ref info) = pr_info {
            result["pr"] = json!({
                "number": info.number,
                "title": &info.title,
                "base": &info.base_ref,
                "head": &info.head_ref
            });
        }
        return tool_result(result);
    }

    // Parse old file versions using cached registry
    let mut old_files: HashMap<PathBuf, FileIndex> = HashMap::new();
    for path in &changed_paths {
        if let Ok(Some(old_content)) = diff::get_file_at_ref(&workspace.root, path, &since_ref) {
            if let Some(lang) = Language::detect(path) {
                if let Some(parser) = registry.get_parser(&lang) {
                    if let Ok(fi) = parser.parse_file(path, &old_content) {
                        old_files.insert(path.clone(), fi);
                    }
                }
            }
        }
    }

    let merged = merge_member_diffs(workspace, &old_files, &changed_paths);

    let total_changes =
        merged.files_added.len() + merged.files_removed.len() + merged.files_modified.len();

    let mut result = json!({
        "since_ref": since_ref,
        "changes": total_changes,
        "files_added": merged.files_added.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "files_removed": merged.files_removed.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "files_modified": merged.files_modified.iter().map(|fd| json!({
            "path": fd.path.to_string_lossy().to_string(),
            "added": fd.declarations_added.iter().map(|d| json!({"kind": format!("{}", d.kind), "name": &d.name, "signature": &d.signature})).collect::<Vec<_>>(),
            "removed": fd.declarations_removed.iter().map(|d| json!({"kind": format!("{}", d.kind), "name": &d.name, "signature": &d.signature})).collect::<Vec<_>>(),
            "modified": fd.declarations_modified.iter().map(|d| json!({"kind": format!("{}", d.kind), "name": &d.name, "old_signature": &d.old_signature, "new_signature": &d.new_signature})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()
    });

    if let Some(ref info) = pr_info {
        result["pr"] = json!({
            "number": info.number,
            "title": &info.title,
            "base": &info.base_ref,
            "head": &info.head_ref
        });
    }

    tool_result(result)
}

pub(super) fn tool_batch_file_summaries(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let paths = args.get("paths").and_then(|v| v.as_array());
    let glob = args.get("glob").and_then(|v| v.as_str());

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let files: Vec<&FileIndex> = if let Some(path_arr) = paths {
        let path_strs: Vec<&str> = path_arr.iter().filter_map(|v| v.as_str()).collect();
        let mut result = Vec::new();
        for p in &path_strs {
            for (_member_name, index) in &indices {
                if let Some(f) = find_file(index, p) {
                    result.push(f);
                    break;
                }
            }
        }
        result
    } else if let Some(pattern) = glob {
        let matcher = compile_glob_matcher(pattern);
        let mut result = Vec::new();
        for (_member_name, index) in &indices {
            for f in &index.files {
                let fp = f.path.to_string_lossy();
                let matches = match &matcher {
                    Some(m) => m.is_match(fp.as_ref()),
                    None => fp == pattern || fp.starts_with(&format!("{}/", pattern)),
                };
                if matches {
                    result.push(f);
                }
            }
        }
        result
    } else {
        return tool_error("Provide either 'paths' array or 'glob' pattern");
    };

    let cap = 30;
    let total = files.len();
    let files = &files[..files.len().min(cap)];
    let summaries: Vec<Value> = files.iter().map(|f| file_summary_data(f)).collect();

    if is_compact(args) {
        let cols = &["file", "language", "lines", "has_tests", "public_symbols"];
        let mut compact = to_compact_rows(cols, &summaries);
        compact["count"] = json!(summaries.len());
        compact["total_matched"] = json!(total);
        return tool_result(compact);
    }

    tool_result(json!({
        "count": summaries.len(),
        "total_matched": total,
        "summaries": summaries
    }))
}

pub(super) fn tool_get_callers(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("Missing required parameter: symbol"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(50) as usize;

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let mut references: Vec<Value> = Vec::new();

    fn search_decl_refs(
        decls: &[Declaration],
        symbol: &str,
        file_path: &str,
        refs: &mut Vec<Value>,
    ) {
        for decl in decls {
            // Use word-boundary matching to avoid false positives
            // (e.g., searching for "get" shouldn't match "budget" or "widget")
            if decl.name != symbol && contains_word_boundary(&decl.signature, symbol) {
                refs.push(json!({
                    "file": file_path,
                    "name": decl.name,
                    "kind": format!("{}", decl.kind),
                    "line": decl.line,
                    "match_type": "signature"
                }));
            }
            search_decl_refs(&decl.children, symbol, file_path, refs);
        }
    }

    for (_member_name, index) in &indices {
        for file in &index.files {
            let file_path = file.path.to_string_lossy().to_string();

            // Check imports (word boundary matching)
            for imp in &file.imports {
                if contains_word_boundary(&imp.text, symbol) {
                    references.push(json!({
                        "file": &file_path,
                        "match_type": "import",
                        "import": &imp.text
                    }));
                }
            }

            // Check declaration signatures
            search_decl_refs(&file.declarations, symbol, &file_path, &mut references);
        }
    }

    references.truncate(limit);

    if is_compact(args) {
        // Normalize import refs to match signature ref columns
        for r in &mut references {
            if r.get("match_type").and_then(|v| v.as_str()) == Some("import") {
                r["name"] = r.get("import").cloned().unwrap_or(Value::Null);
                r["kind"] = json!("import");
            }
        }
        let cols = &["file", "name", "kind", "line", "match_type"];
        let mut compact = to_compact_rows(cols, &references);
        compact["symbol"] = json!(symbol);
        compact["count"] = json!(references.len());
        return tool_result(compact);
    }

    tool_result(json!({
        "symbol": symbol,
        "count": references.len(),
        "references": references
    }))
}

pub(super) fn tool_get_public_api(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path = args.get("path").and_then(|v| v.as_str());
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .min(500) as usize;

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let mut declarations = Vec::new();

    for (_member_name, index) in &indices {
        let files: Vec<&FileIndex> = if let Some(p) = path {
            if let Some(f) = find_file(index, p) {
                vec![f]
            } else {
                index
                    .files
                    .iter()
                    .filter(|f| f.path.to_string_lossy().starts_with(p))
                    .collect()
            }
        } else {
            index.files.iter().collect()
        };

        for file in &files {
            let file_path = file.path.to_string_lossy().to_string();
            collect_public_decls(&file.declarations, &file_path, &mut declarations);
        }
    }

    let total = declarations.len();
    let truncated = total > limit;
    declarations.truncate(limit);

    let mut result = json!({
        "path": path.unwrap_or("(all)"),
        "count": declarations.len(),
        "declarations": declarations
    });
    if truncated {
        result["truncated"] = json!(true);
        result["total"] = json!(total);
        result["limit"] = json!(limit);
    }
    tool_result(result)
}

pub(super) fn tool_explain_symbol(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_error("Missing required parameter: name"),
    };
    let name_lower = name.to_lowercase();

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    fn find_matching_decls(
        decls: &[Declaration],
        name_lower: &str,
        file_path: &str,
        results: &mut Vec<Value>,
    ) {
        for decl in decls {
            if decl.name.to_lowercase() == name_lower {
                results.push(explain_decl(decl, file_path));
            }
            find_matching_decls(&decl.children, name_lower, file_path, results);
        }
    }

    let mut results = Vec::new();
    for (_member_name, index) in &indices {
        for file in &index.files {
            let file_path = file.path.to_string_lossy().to_string();
            find_matching_decls(&file.declarations, &name_lower, &file_path, &mut results);
        }
    }
    results.truncate(10);

    tool_result(json!({
        "name": name,
        "count": results.len(),
        "symbols": results
    }))
}

pub(super) fn tool_get_related_tests(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("Missing required parameter: symbol"),
    };
    let scope_path = args.get("path").and_then(|v| v.as_str());
    let symbol_lower = symbol.to_lowercase();

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let mut results = Vec::new();

    // If path scoped, search that file first
    if let Some(p) = scope_path {
        for (_member_name, index) in &indices {
            if let Some(file) = find_file(index, p) {
                let file_path = file.path.to_string_lossy().to_string();
                find_tests_for_symbol(
                    &file.declarations,
                    &symbol_lower,
                    &file_path,
                    &mut results,
                    "same_file",
                );
                break;
            }
        }
    }

    // Search all files for test declarations matching the symbol
    for (_member_name, index) in &indices {
        for file in &index.files {
            let file_path = file.path.to_string_lossy().to_string();
            let file_path_lower = file_path.to_lowercase();

            // Check if this is a test file
            let is_test_file = file_path_lower.contains("_test.")
                || file_path_lower.contains("_spec.")
                || file_path_lower.contains(".test.")
                || file_path_lower.contains(".spec.")
                || file_path_lower.contains("/test_")
                || file_path_lower.contains("/tests/");

            let reason = if is_test_file {
                "test_file"
            } else {
                "name_convention"
            };

            // Skip if we already searched this file in scope_path
            if scope_path.is_some_and(|p| file_path == p || file_path.ends_with(&format!("/{}", p)))
            {
                continue;
            }

            find_tests_for_symbol(
                &file.declarations,
                &symbol_lower,
                &file_path,
                &mut results,
                reason,
            );
        }
    } // end for indices

    results.truncate(20);
    tool_result(json!({
        "symbol": symbol,
        "count": results.len(),
        "tests": results
    }))
}

// ---------------------------------------------------------------------------
// Dependency graph
// ---------------------------------------------------------------------------

pub(super) fn tool_get_dependency_graph(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let file_refs = collect_file_refs(&indices);
    let path = args.get("path").and_then(|v| v.as_str());
    let level = args.get("level").and_then(|v| v.as_str()).unwrap_or("file");
    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("mermaid");
    let depth = args
        .get("depth")
        .and_then(|v| v.as_u64())
        .map(|d| d as usize);

    let graph = match level {
        "symbol" => dep_graph::build_symbol_graph_from_file_refs(&file_refs, path, depth),
        _ => dep_graph::build_file_graph_from_file_refs(&file_refs, path, depth),
    };

    let node_count = graph.nodes.len();
    let edge_count = graph.edges.len();

    match format {
        "dot" => tool_result(json!({
            "format": "dot",
            "nodes": node_count,
            "edges": edge_count,
            "graph": dep_graph::format_dot(&graph)
        })),
        "json" => tool_result(json!({
            "format": "json",
            "nodes": node_count,
            "edges": edge_count,
            "graph": dep_graph::format_json(&graph)
        })),
        _ => tool_result(json!({
            "format": "mermaid",
            "nodes": node_count,
            "edges": edge_count,
            "graph": dep_graph::format_mermaid(&graph)
        })),
    }
}

// ---------------------------------------------------------------------------
// Complexity hotspots & health
// ---------------------------------------------------------------------------

pub(super) fn tool_get_hotspots(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(100) as usize;
    let path_filter = args.get("path").and_then(|v| v.as_str());
    let min_complexity = args
        .get("min_complexity")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u16;
    let sort_by = args
        .get("sort_by")
        .and_then(|v| v.as_str())
        .unwrap_or("score");

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let mut entries = Vec::new();
    for (_member_name, index) in &indices {
        entries.extend(collect_hotspots(index, path_filter, min_complexity));
    }
    sort_hotspots(&mut entries, sort_by);

    let total = entries.len();
    entries.truncate(limit);

    if is_compact(args) {
        let compact = serialize_compact(
            &entries,
            &[
                "file",
                "name",
                "kind",
                "line",
                "cyclomatic",
                "max_nesting",
                "param_count",
                "body_lines",
                "score",
            ],
        );
        return tool_result(json!({ "total": total, "hotspots": compact }));
    }

    tool_result(json!({ "total": total, "hotspots": entries }))
}

pub(super) fn tool_get_health(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let path_filter = args.get("path").and_then(|v| v.as_str());

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let file_refs = collect_file_refs(&indices);
    let h = compute_health_from_file_refs(&file_refs, path_filter);

    tool_result(json!({
        "total_functions": h.total_functions,
        "analyzed": h.analyzed,
        "complexity": {
            "avg": h.avg_cc,
            "median": h.median_cc,
            "max": h.max_cc,
            "p90": h.p90_cc
        },
        "nesting": { "avg": h.avg_nesting },
        "params": { "avg": h.avg_params },
        "body_lines": { "avg": h.avg_body_lines },
        "high_complexity_count": h.high_complexity_count,
        "high_complexity_pct": h.high_complexity_pct,
        "documented_pct": h.documented_pct,
        "test_count": h.test_count,
        "deprecated_count": h.deprecated_count,
        "public_api_count": h.public_api_count,
        "hottest_files": h.hottest_files
    }))
}

pub(super) fn tool_get_type_flow(workspace: &WorkspaceIndex, args: &Value) -> Value {
    let type_name = match args.get("type_name").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return tool_error("Missing required parameter: type_name"),
    };
    let path_filter = args.get("path").and_then(|v| v.as_str());
    let include_fields = args
        .get("include_fields")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(200) as usize;

    let indices = match resolve_indices(workspace, args) {
        Ok(i) => i,
        Err(e) => return e,
    };

    let mut producers = Vec::new();
    let mut consumers = Vec::new();
    for (_member_name, index) in &indices {
        let (p, c) = build_type_flow(index, type_name, path_filter, include_fields);
        producers.extend(p);
        consumers.extend(c);
    }

    let producers_total = producers.len();
    let consumers_total = consumers.len();
    producers.truncate(limit);
    consumers.truncate(limit);

    if is_compact(args) {
        let cols = &["file", "name", "kind", "line", "signature"];
        let compact_producers = serialize_compact(&producers, cols);
        let compact_consumers = serialize_compact(&consumers, cols);
        return tool_result(json!({
            "type_name": type_name,
            "producers_count": producers_total,
            "consumers_count": consumers_total,
            "producers": compact_producers,
            "consumers": compact_consumers,
        }));
    }

    tool_result(json!({
        "type_name": type_name,
        "producers_count": producers_total,
        "consumers_count": consumers_total,
        "producers": producers,
        "consumers": consumers,
    }))
}
