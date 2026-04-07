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

pub(super) fn tool_definitions(is_workspace: bool, all_tools: bool, wiki_available: bool) -> Value {
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

    // Append wiki tools when the wiki feature is compiled in.
    // wiki_generate is always listed (it's how you create a wiki).
    // The rest require an existing wiki.
    #[cfg(feature = "wiki")]
    {
        if let Some(tools) = defs["tools"].as_array_mut() {
            tools.push(json!({
                "name": "wiki_generate",
                "description": "Initialize a new wiki and return the codebase structural context for planning pages. After calling this, plan which pages to create (architecture, module, entity, topic) based on the returned context, then call wiki_contribute for each page. Finish with an index page. Fails if a wiki already exists unless force=true.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "force": {
                            "type": "boolean",
                            "description": "Overwrite existing wiki if one exists (default: false)"
                        }
                    }
                }
            }));
        }
        if wiki_available {
            if let Some(tools) = defs["tools"].as_array_mut() {
                tools.push(json!({
                    "name": "wiki_search",
                    "description": "Search the codebase knowledge wiki by keyword or concept. Returns matching pages with excerpts. Use this to understand modules, architecture, or design decisions before diving into source code. If your query synthesizes insights from multiple pages, consider calling wiki_contribute to persist the synthesis.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search term or concept"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Max results (default: 5)"
                            },
                            "include_failures": {
                                "type": "boolean",
                                "description": "If true, include failure pattern details in results (default: false)"
                            }
                        },
                        "required": ["query"]
                    }
                }));
                tools.push(json!({
                    "name": "wiki_read",
                    "description": "Read a wiki page by ID (e.g. 'architecture', 'mod-mcp'). Returns full page content with metadata. If you combine this with other knowledge, call wiki_contribute to file the synthesis.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "page": {
                                "type": "string",
                                "description": "Page ID or partial title to search"
                            }
                        },
                        "required": ["page"]
                    }
                }));
                tools.push(json!({
                    "name": "wiki_status",
                    "description": "Check wiki health: page count, how stale it is, source file coverage.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }));
                tools.push(json!({
                    "name": "wiki_contribute",
                    "description": "Write knowledge back to the wiki. Create a new page or update an existing one. Use this to file synthesized answers, analyses, or discovered connections that should persist beyond this conversation.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "page": {
                                "type": "string",
                                "description": "Page ID (slug). If it exists, the page is updated; if not, a new page is created."
                            },
                            "title": {
                                "type": "string",
                                "description": "Human-readable title (required for new pages, optional for updates)"
                            },
                            "content": {
                                "type": "string",
                                "description": "Markdown content for the page"
                            },
                            "page_type": {
                                "type": "string",
                                "enum": ["architecture", "module", "entity", "topic"],
                                "description": "Page type (default: topic). Only used when creating new pages."
                            },
                            "source_files": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Source files this page relates to (optional)"
                            },
                            "resolve_contradictions": {
                                "type": "boolean",
                                "description": "If true, marks all existing unresolved contradictions on this page as resolved"
                            },
                            "contradictions": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "description": { "type": "string", "description": "What the contradiction is" },
                                        "source": { "type": "string", "description": "Source location, e.g. 'src/foo.rs:42'" }
                                    },
                                    "required": ["description"]
                                },
                                "description": "Contradictions to add to the page"
                            },
                            "failures": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "symptom": { "type": "string", "description": "What was observed" },
                                        "attempted_fix": { "type": "string", "description": "What fix was attempted" },
                                        "diagnosis": { "type": "string", "description": "Why the fix didn't work" },
                                        "actual_fix": { "type": "string", "description": "What actually worked" },
                                        "source_files": { "type": "array", "items": { "type": "string" }, "description": "Source files involved" }
                                    },
                                    "required": ["symptom", "attempted_fix", "diagnosis"]
                                },
                                "description": "Failure patterns to add to the page"
                            },
                            "resolve_failures": {
                                "type": "boolean",
                                "description": "If true, marks all unresolved failures on this page as resolved"
                            }
                        },
                        "required": ["page", "content"]
                    }
                }));
                tools.push(json!({
                    "name": "wiki_update",
                    "description": "Analyze code changes since last wiki generation and return affected pages with diff context. For each affected page, rewrite its content based on the diff and current content, then call wiki_contribute to save. No API keys needed — you drive the updates.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "since": {
                                "type": "string",
                                "description": "Git ref to diff against (default: wiki's stored ref)"
                            }
                        }
                    }
                }));
                tools.push(json!({
                    "name": "wiki_suggest_contribution",
                    "description": "Given a synthesis or analysis, suggest which wiki page to update or whether to create a new one. Lightweight (no LLM call) — uses keyword matching against existing pages.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "synthesis": {
                                "type": "string",
                                "description": "The synthesized knowledge or analysis text"
                            },
                            "source_pages": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Wiki page IDs that were consulted during synthesis"
                            }
                        },
                        "required": ["synthesis"]
                    }
                }));
                tools.push(json!({
                    "name": "wiki_compound",
                    "description": "Compound new knowledge into the wiki. Takes a synthesis (answer, analysis, or insight derived from wiki pages or code exploration) and automatically routes it to the best matching page, or creates a new topic page if no good match exists. Use this after answering questions that required cross-page synthesis.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "synthesis": {
                                "type": "string",
                                "description": "The knowledge to compound into the wiki"
                            },
                            "source_pages": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Wiki page IDs that contributed to this synthesis"
                            },
                            "title": {
                                "type": "string",
                                "description": "Title for new page if one needs to be created"
                            }
                        },
                        "required": ["synthesis"]
                    }
                }));
                tools.push(json!({
                    "name": "wiki_record_failure",
                    "description": "Record a failed fix attempt so future agents can learn from it. Auto-routes to the best matching wiki page, or specify a target page explicitly.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "symptom": {
                                "type": "string",
                                "description": "What was observed (error message, test failure, unexpected behavior)"
                            },
                            "attempted_fix": {
                                "type": "string",
                                "description": "What fix was attempted"
                            },
                            "diagnosis": {
                                "type": "string",
                                "description": "Why the fix didn't work / root cause analysis"
                            },
                            "actual_fix": {
                                "type": "string",
                                "description": "What actually worked (if known at recording time)"
                            },
                            "source_files": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Source files involved in this failure"
                            },
                            "page": {
                                "type": "string",
                                "description": "Target wiki page ID. If omitted, auto-routes to best matching page based on symptom and diagnosis text."
                            }
                        },
                        "required": ["symptom", "attempted_fix", "diagnosis"]
                    }
                }));
            }
        }
    }
    let _ = wiki_available; // suppress unused warning when wiki feature is off

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
        let sd = diff::compute_structural_diff(&member.index.files, old_files, changed_paths);
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

// ---------------------------------------------------------------------------
// Wiki tools (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_search(store: &crate::wiki::store::WikiStore, args: &Value) -> Value {
    let query = match args["query"].as_str() {
        Some(q) if !q.is_empty() => q,
        _ => return tool_error("Missing required parameter: query"),
    };
    let limit = args["limit"].as_u64().unwrap_or(5) as usize;
    let include_failures = args
        .get("include_failures")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<(usize, &crate::wiki::page::WikiPage)> = store
        .pages
        .iter()
        .filter_map(|page| {
            let mut score = 0usize;

            // Title match (highest weight)
            let title_lower = page.frontmatter.title.to_lowercase();
            if title_lower == query_lower {
                score += 100;
            } else if title_lower.contains(&query_lower) {
                score += 50;
            } else if query_words.iter().any(|w| title_lower.contains(w)) {
                score += 25;
            }

            // ID match
            let id_lower = page.frontmatter.id.to_lowercase();
            if id_lower.contains(&query_lower) {
                score += 40;
            } else if query_words.iter().any(|w| id_lower.contains(w)) {
                score += 15;
            }

            // Covers match (declaration references)
            for cover in &page.frontmatter.covers {
                let cover_lower = cover.to_lowercase();
                if cover_lower.contains(&query_lower) {
                    score += 30;
                } else if query_words.iter().any(|w| cover_lower.contains(w)) {
                    score += 10;
                }
            }

            // Content match (lower weight)
            let content_lower = page.content.to_lowercase();
            if content_lower.contains(&query_lower) {
                score += 20;
            } else {
                let word_hits = query_words
                    .iter()
                    .filter(|w| content_lower.contains(*w))
                    .count();
                if word_hits > 0 {
                    score += 5 * word_hits;
                }
            }

            // Source files match
            for sf in &page.frontmatter.source_files {
                let sf_lower = sf.to_lowercase();
                if query_words.iter().any(|w| sf_lower.contains(w)) {
                    score += 10;
                    break;
                }
            }

            // Failure symptom match
            for failure in &page.frontmatter.failures {
                let symptom_lower = failure.symptom.to_lowercase();
                if symptom_lower.contains(&query_lower) {
                    score += 25;
                    break;
                } else if query_words.iter().any(|w| symptom_lower.contains(w)) {
                    score += 8;
                    break;
                }
            }

            if score > 0 { Some((score, page)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(limit);

    let results: Vec<Value> = scored
        .iter()
        .map(|(score, page)| {
            // Extract a short excerpt around the query match
            let excerpt = extract_excerpt(&page.content, &query_lower, 150);

            let mut entry = json!({
                "id": page.frontmatter.id,
                "title": page.frontmatter.title,
                "type": page.frontmatter.page_type.as_str(),
                "score": score,
                "excerpt": excerpt,
            });
            let unresolved = page
                .frontmatter
                .contradictions
                .iter()
                .filter(|c| c.resolved_at.is_none())
                .count();
            if unresolved > 0 {
                entry["has_contradictions"] = json!(true);
                entry["unresolved_contradictions"] = json!(unresolved);
            }
            let unresolved_failures = page
                .frontmatter
                .failures
                .iter()
                .filter(|f| f.resolved_at.is_none())
                .count();
            if !page.frontmatter.failures.is_empty() {
                entry["failure_count"] = json!(page.frontmatter.failures.len());
                entry["unresolved_failures"] = json!(unresolved_failures);
            }
            if include_failures && !page.frontmatter.failures.is_empty() {
                let failures: Vec<Value> = page
                    .frontmatter
                    .failures
                    .iter()
                    .map(|f| f.to_json_summary())
                    .collect();
                entry["failures"] = json!(failures);
            }
            entry
        })
        .collect();

    let touched_pages: Vec<&str> = scored
        .iter()
        .map(|(_, page)| page.frontmatter.id.as_str())
        .collect();

    let mut response = json!({
        "query": query,
        "matches": results.len(),
        "results": results,
    });

    if touched_pages.len() >= 2 {
        response["compound_suggestion"] = json!({
            "hint": "If your answer synthesizes insights from these pages, persist it with wiki_compound.",
            "suggested_call": {
                "tool": "wiki_compound",
                "args": {
                    "synthesis": "<your synthesized answer>",
                    "source_pages": touched_pages,
                }
            }
        });
    }

    tool_result(response)
}

#[cfg(feature = "wiki")]
fn extract_excerpt(content: &str, query: &str, max_chars: usize) -> String {
    if content.trim().is_empty() {
        return "(empty page)".to_string();
    }

    // Build lowered version with per-byte mapping back to original offsets so we
    // can search case-insensitively but return an excerpt with original casing.
    let mut content_lower = String::with_capacity(content.len());
    let mut map: Vec<usize> = Vec::with_capacity(content.len() + 1);
    for (orig_byte, ch) in content.char_indices() {
        for lc in ch.to_lowercase() {
            for _ in 0..lc.len_utf8() {
                map.push(orig_byte);
            }
            content_lower.push(lc);
        }
    }
    map.push(content.len()); // sentinel

    let to_orig =
        |lower_off: usize| -> usize { map.get(lower_off).copied().unwrap_or(content.len()) };

    // Snap a byte offset to the next valid char boundary in `s`.
    let snap = |s: &str, off: usize| -> usize {
        let off = off.min(s.len());
        (off..=s.len())
            .find(|&i| s.is_char_boundary(i))
            .unwrap_or(s.len())
    };

    // Snap a byte offset backward to the nearest word boundary (whitespace).
    let snap_word_start = |s: &str, off: usize| -> usize {
        let off = snap(s, off);
        // Search backward from off for whitespace, then return the position after it.
        s[..off]
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + s[i..].chars().next().map_or(1, |c| c.len_utf8()))
            .unwrap_or(0)
    };

    if let Some(pos) = content_lower.find(query) {
        let orig_start = to_orig(pos);
        let orig_end = to_orig(pos + query.len());

        let start = content[..orig_start]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or_else(|| snap_word_start(content, orig_start.saturating_sub(max_chars / 2)));

        let tentative = snap(content, (orig_end + max_chars / 2).min(content.len()));
        let end = content[..tentative]
            .rfind('\n')
            .unwrap_or(tentative)
            .max(orig_end);

        let mut excerpt = content[start..end].trim().to_string();
        if start > 0 {
            excerpt.insert_str(0, "...");
        }
        if end < content.len() {
            excerpt.push_str("...");
        }
        excerpt
    } else {
        // No exact match — return first max_chars bytes of original content.
        let end_off = snap(content, max_chars.min(content.len()));
        let end = content[..end_off].rfind('\n').unwrap_or(end_off);
        let mut excerpt = content[..end].trim().to_string();
        if end < content.len() {
            excerpt.push_str("...");
        }
        excerpt
    }
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_read(store: &crate::wiki::store::WikiStore, args: &Value) -> Value {
    let page_query = match args["page"].as_str() {
        Some(p) if !p.is_empty() => p,
        _ => return tool_error("Missing required parameter: page"),
    };

    // Try exact ID match first
    if let Some(page) = store.get_page(page_query) {
        return format_wiki_page(page);
    }

    // Try case-insensitive ID match
    let query_lower = page_query.to_lowercase();
    let found = store.pages.iter().find(|p| {
        p.frontmatter.id.to_lowercase() == query_lower
            || p.frontmatter.title.to_lowercase() == query_lower
    });
    if let Some(page) = found {
        return format_wiki_page(page);
    }

    // Try partial title/ID match
    let found = store.pages.iter().find(|p| {
        p.frontmatter.id.to_lowercase().contains(&query_lower)
            || p.frontmatter.title.to_lowercase().contains(&query_lower)
    });
    if let Some(page) = found {
        return format_wiki_page(page);
    }

    // Not found — list available pages
    let available: Vec<String> = store
        .pages
        .iter()
        .map(|p| format!("{} ({})", p.frontmatter.id, p.frontmatter.title))
        .collect();
    tool_error(&format!(
        "Page '{}' not found. Available pages:\n{}",
        page_query,
        available.join("\n")
    ))
}

#[cfg(feature = "wiki")]
fn format_wiki_page(page: &crate::wiki::page::WikiPage) -> Value {
    let fm = &page.frontmatter;
    let header = format!(
        "# {}\n\nType: {} | Sources: {} | Covers: {} declarations\nSource files: {}\nLinks to: {}",
        fm.title,
        fm.page_type.as_str(),
        fm.source_files.len(),
        fm.covers.len(),
        fm.source_files.join(", "),
        if fm.links_to.is_empty() {
            "none".to_string()
        } else {
            fm.links_to.join(", ")
        },
    );

    let mut result = json!({
        "id": fm.id,
        "title": fm.title,
        "type": fm.page_type.as_str(),
        "content": format!("{}\n\n{}", header, page.content),
    });

    if !fm.contradictions.is_empty() {
        let contras: Vec<Value> = fm
            .contradictions
            .iter()
            .map(|c| {
                let mut obj = json!({
                    "description": c.description,
                    "source": c.source,
                    "detected_at": c.detected_at,
                });
                if let Some(ref resolved) = c.resolved_at {
                    obj["resolved_at"] = json!(resolved);
                }
                obj
            })
            .collect();
        result["contradictions"] = json!(contras);
    }

    if !fm.failures.is_empty() {
        let failure_details: Vec<Value> = fm
            .failures
            .iter()
            .enumerate()
            .map(|(i, f)| f.to_json_detail(i))
            .collect();
        result["failures"] = json!(failure_details);
    }

    if !fm.links_to.is_empty() {
        let mut related = fm.links_to.clone();
        if !related.contains(&fm.id) {
            related.push(fm.id.clone());
        }
        result["compound_suggestion"] = json!({
            "hint": "If you combine this with other sources, compound the insight with wiki_compound.",
            "source_pages": related,
        });
    }

    tool_result(result)
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_status(
    store: &crate::wiki::store::WikiStore,
    workspace: &WorkspaceIndex,
) -> Value {
    let health = crate::wiki::compute_wiki_health(store, workspace);

    // Gather contradiction stats
    let mut total_contradictions = 0usize;
    let mut unresolved_contradictions = 0usize;
    let mut pages_with_contradictions = Vec::new();
    for page in &store.pages {
        let unresolved: Vec<_> = page
            .frontmatter
            .contradictions
            .iter()
            .filter(|c| c.resolved_at.is_none())
            .collect();
        if !unresolved.is_empty() {
            pages_with_contradictions.push(page.frontmatter.id.clone());
            unresolved_contradictions += unresolved.len();
        }
        total_contradictions += page.frontmatter.contradictions.len();
    }

    // Gather failure stats
    let mut total_failures = 0usize;
    let mut unresolved_failures = 0usize;
    let mut pages_with_failures = Vec::new();
    for page in &store.pages {
        let unresolved: Vec<_> = page
            .frontmatter
            .failures
            .iter()
            .filter(|f| f.resolved_at.is_none())
            .collect();
        if !unresolved.is_empty() {
            pages_with_failures.push(page.frontmatter.id.clone());
            unresolved_failures += unresolved.len();
        }
        total_failures += page.frontmatter.failures.len();
    }

    tool_result(json!({
        "pages": health.pages,
        "pages_by_type": health.pages_by_type,
        "generated_at_ref": health.generated_at_ref,
        "generated_at": health.generated_at,
        "staleness": health.staleness,
        "commits_behind": health.commits_behind,
        "coverage": {
            "covered_files": health.covered_files,
            "total_files": health.total_files,
            "percentage": health.coverage_pct
        },
        "contradictions": {
            "total": total_contradictions,
            "unresolved": unresolved_contradictions,
            "pages_with_contradictions": pages_with_contradictions
        },
        "failures": {
            "total": total_failures,
            "unresolved": unresolved_failures,
            "pages_with_failures": pages_with_failures
        }
    }))
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_suggest_contribution(
    store: &crate::wiki::store::WikiStore,
    args: &Value,
) -> Value {
    let synthesis = match args.get("synthesis").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return tool_error("Missing required parameter: synthesis"),
    };
    let source_pages: Vec<&str> = args
        .get("source_pages")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let scored = crate::wiki::score_pages(store, synthesis, &source_pages);

    if let Some(&(best_score, ref page_id)) = scored.first() {
        if best_score >= 20 {
            let page = store.get_page(page_id).unwrap();
            return tool_result(json!({
                "suggestion": "update",
                "target_page": page.frontmatter.id,
                "target_title": page.frontmatter.title,
                "confidence": best_score,
                "reason": format!("Synthesis overlaps most with '{}' (score: {})", page.frontmatter.title, best_score),
            }));
        }
    }

    let best_score = scored.first().map(|(s, _)| *s).unwrap_or(0);
    let suggested_id = crate::wiki::derive_topic_id(synthesis);

    tool_result(json!({
        "suggestion": "create",
        "suggested_id": suggested_id,
        "confidence": best_score,
        "reason": if best_score == 0 {
            "Synthesis doesn't overlap with any existing pages".to_string()
        } else {
            format!("Best match score ({}) is too low for a confident update", best_score)
        },
    }))
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_compound(store: &mut crate::wiki::store::WikiStore, args: &Value) -> Value {
    let synthesis = match args.get("synthesis").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return tool_error("Missing required parameter: synthesis"),
    };
    let source_pages: Vec<String> = args
        .get("source_pages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let title = args.get("title").and_then(|v| v.as_str());

    match crate::wiki::compound_into_wiki(store, synthesis, &source_pages, title) {
        Ok(result) => tool_result(json!({
            "action": result.action,
            "page_id": result.id,
            "title": result.title,
            "total_wiki_pages": store.pages.len(),
        })),
        Err(e) => tool_error(&e.to_string()),
    }
}

/// Record a failure on an existing page: push the failure, save, and return a response.
#[cfg(feature = "wiki")]
fn record_failure_on_page(
    store: &mut crate::wiki::store::WikiStore,
    page_id: &str,
    failure: crate::wiki::page::FailurePattern,
    now: &str,
) -> Value {
    let page = match store.pages.iter_mut().find(|p| p.frontmatter.id == page_id) {
        Some(p) => p,
        None => return tool_error(&format!("Page '{}' not found", page_id)),
    };
    let failure_index = page.frontmatter.failures.len();
    page.frontmatter.failures.push(failure);
    page.frontmatter.generated_at = now.to_string();
    if let Err(e) = store.save_incremental(page_id) {
        return tool_error(&format!("Failed to save: {}", e));
    }
    let title = store
        .get_page(page_id)
        .map(|p| p.frontmatter.title.clone())
        .unwrap_or_default();
    tool_result(json!({
        "action": "recorded_on_existing",
        "page_id": page_id,
        "title": title,
        "failure_index": failure_index,
        "total_wiki_pages": store.pages.len(),
    }))
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_record_failure(
    store: &mut crate::wiki::store::WikiStore,
    args: &Value,
) -> Value {
    use crate::wiki::page::{FailurePattern, Frontmatter, PageType, WikiPage, sanitize_id};
    use crate::wiki::{derive_topic_id, extract_wiki_links, score_pages};

    let symptom = match args.get("symptom").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return tool_error("Missing required parameter: symptom"),
    };
    let attempted_fix = match args.get("attempted_fix").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return tool_error("Missing required parameter: attempted_fix"),
    };
    let diagnosis = match args.get("diagnosis").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return tool_error("Missing required parameter: diagnosis"),
    };
    let actual_fix = args.get("actual_fix").and_then(|v| v.as_str());
    let source_files: Vec<String> = args
        .get("source_files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let target_page = args.get("page").and_then(|v| v.as_str());

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let failure = FailurePattern {
        symptom: symptom.to_string(),
        attempted_fix: attempted_fix.to_string(),
        diagnosis: diagnosis.to_string(),
        actual_fix: actual_fix.map(String::from),
        source_files: source_files.clone(),
        recorded_at: now.clone(),
        resolved_at: if actual_fix.is_some() {
            Some(now.clone())
        } else {
            None
        },
    };

    if let Some(page_id_raw) = target_page {
        let page_id = sanitize_id(page_id_raw);
        if page_id.is_empty() {
            return tool_error("Invalid page ID");
        }
        return record_failure_on_page(store, &page_id, failure, &now);
    }

    // Auto-route: score pages using symptom + diagnosis text
    let scoring_text = format!("{} {}", symptom, diagnosis);
    let scored = score_pages(store, &scoring_text, &[]);
    let best = scored.first().map(|(score, id)| (*score, id.clone()));

    if let Some((best_score, target_id)) = best {
        if best_score >= 25 {
            return record_failure_on_page(store, &target_id, failure, &now);
        }
    }

    // No good match — create a new topic page
    let base_id = sanitize_id(&derive_topic_id(symptom));
    let page_id = if base_id.is_empty() {
        "topic-failure".to_string()
    } else if store.get_page(&base_id).is_none() {
        base_id
    } else {
        let mut suffix = 2;
        loop {
            let candidate = format!("{}-{}", base_id, suffix);
            if store.get_page(&candidate).is_none() {
                break candidate;
            }
            suffix += 1;
            if suffix > 100 {
                return tool_error("Too many pages with similar IDs — specify an explicit page ID");
            }
        }
    };

    // Derive title from symptom
    let page_title = {
        let words: Vec<String> = symptom
            .split_whitespace()
            .filter(|w| w.len() >= 4)
            .take(5)
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                }
            })
            .collect();
        if words.is_empty() {
            "Failure Pattern".to_string()
        } else {
            words.join(" ")
        }
    };

    let content = format!(
        "# {}\n\nThis page tracks failure patterns to help agents avoid repeating mistakes.",
        page_title
    );
    let links_to = extract_wiki_links(&content);

    let page = WikiPage {
        frontmatter: Frontmatter {
            id: page_id.clone(),
            title: page_title.clone(),
            page_type: PageType::Topic,
            source_files,
            generated_at_ref: store.manifest.generated_at_ref.clone(),
            generated_at: now,
            links_to,
            covers: Vec::new(),
            contradictions: Vec::new(),
            failures: vec![failure],
        },
        content,
    };
    store.upsert_page(page);
    if let Err(e) = store.save_incremental(&page_id) {
        return tool_error(&format!("Failed to save: {}", e));
    }
    tool_result(json!({
        "action": "recorded_on_new",
        "page_id": page_id,
        "title": page_title,
        "failure_index": 0,
        "total_wiki_pages": store.pages.len(),
    }))
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_contribute(
    store: &mut crate::wiki::store::WikiStore,
    args: &Value,
) -> Value {
    use crate::wiki::extract_wiki_links;
    use crate::wiki::page::{Frontmatter, PageType, WikiPage, sanitize_id};

    let page_id_raw = match args.get("page").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: page"),
    };
    let page_id = sanitize_id(page_id_raw);
    if page_id.is_empty() {
        return tool_error("Invalid page ID: must contain at least one alphanumeric character");
    }

    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return tool_error("Missing required parameter: content"),
    };

    let links_to = extract_wiki_links(&content);
    let source_files: Vec<String> = args
        .get("source_files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let resolve_contradictions = args
        .get("resolve_contradictions")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Parse explicit contradictions from args
    let new_contradictions: Vec<crate::wiki::page::Contradiction> = args
        .get("contradictions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let desc = v.get("description")?.as_str()?;
                    let source = v.get("source")?.as_str().unwrap_or("");
                    Some(crate::wiki::page::Contradiction {
                        description: desc.to_string(),
                        source: source.to_string(),
                        detected_at: now.clone(),
                        resolved_at: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let resolve_failures = args
        .get("resolve_failures")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Parse explicit failures from args
    let new_failures: Vec<crate::wiki::page::FailurePattern> = args
        .get("failures")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| crate::wiki::page::FailurePattern::from_json(v, &now))
                .collect()
        })
        .unwrap_or_default();

    let existing = store.pages.iter().find(|p| p.frontmatter.id == page_id);
    let action;

    if let Some(old) = existing {
        // Update existing page — preserve page_type, merge source_files
        let mut merged_sources = old.frontmatter.source_files.clone();
        for sf in &source_files {
            if !merged_sources.contains(sf) {
                merged_sources.push(sf.clone());
            }
        }
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| old.frontmatter.title.clone());

        // Handle contradictions: resolve existing if requested, then add new ones
        let mut contradictions = if resolve_contradictions {
            old.frontmatter
                .contradictions
                .iter()
                .map(|c| {
                    if c.resolved_at.is_none() {
                        let mut resolved = c.clone();
                        resolved.resolved_at = Some(now.clone());
                        resolved
                    } else {
                        c.clone()
                    }
                })
                .collect::<Vec<_>>()
        } else {
            old.frontmatter.contradictions.clone()
        };
        contradictions.extend(new_contradictions);

        // Handle failures: resolve existing if requested, then add new ones
        let mut failures = if resolve_failures {
            old.frontmatter
                .failures
                .iter()
                .map(|f| {
                    if f.resolved_at.is_none() {
                        let mut resolved = f.clone();
                        resolved.resolved_at = Some(now.clone());
                        resolved
                    } else {
                        f.clone()
                    }
                })
                .collect::<Vec<_>>()
        } else {
            old.frontmatter.failures.clone()
        };
        failures.extend(new_failures);

        let page = WikiPage {
            frontmatter: Frontmatter {
                id: page_id.clone(),
                title,
                page_type: old.frontmatter.page_type.clone(),
                source_files: merged_sources,
                generated_at_ref: old.frontmatter.generated_at_ref.clone(),
                generated_at: now.clone(),
                links_to,
                covers: old.frontmatter.covers.clone(),
                contradictions,
                failures,
            },
            content,
        };
        store.upsert_page(page);
        action = "updated";
    } else {
        // Create new page
        let title = match args.get("title").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => {
                return tool_error("Missing required parameter: title (required for new pages)");
            }
        };

        let page_type = match args.get("page_type").and_then(|v| v.as_str()) {
            Some("architecture") => PageType::Architecture,
            Some("module") => PageType::Module,
            Some("entity") => PageType::Entity,
            Some("topic") | None => PageType::Topic,
            Some(other) => {
                return tool_error(&format!(
                    "Invalid page_type: '{}'. Must be one of: architecture, module, entity, topic",
                    other
                ));
            }
        };

        let page = WikiPage {
            frontmatter: Frontmatter {
                id: page_id.clone(),
                title: title.clone(),
                page_type,
                source_files,
                generated_at_ref: store.manifest.generated_at_ref.clone(),
                generated_at: now.clone(),
                links_to,
                covers: Vec::new(),
                contradictions: new_contradictions,
                failures: new_failures,
            },
            content,
        };
        store.upsert_page(page);
        action = "created";
    };

    // Persist to disk
    if let Err(e) = store.save_incremental(&page_id) {
        return tool_error(&format!("Failed to save wiki page: {}", e));
    }

    let page = store.get_page(&page_id).unwrap();
    tool_result(json!({
        "action": action,
        "page_id": page_id,
        "title": page.frontmatter.title,
        "type": page.frontmatter.page_type.as_str(),
        "links_to": page.frontmatter.links_to,
        "total_wiki_pages": store.pages.len(),
    }))
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_generate(workspace: &WorkspaceIndex, args: &Value) -> Value {
    use crate::wiki::build_planning_context;
    use crate::wiki::store::WikiStore;

    let wiki_dir = workspace.root.join(".indxr").join("wiki");

    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

    // Guard against overwriting an existing wiki
    if !force && wiki_dir.join("manifest.yaml").exists() {
        return tool_error(
            "Wiki already exists. Use `wiki_update` to update it, or pass force=true to regenerate from scratch.",
        );
    }

    // Initialize an empty wiki store on disk so wiki_contribute can write to it
    let store = WikiStore::new(&wiki_dir);
    if let Err(e) = store.save() {
        return tool_error(&format!("Failed to initialize wiki store: {}", e));
    }

    let context = build_planning_context(workspace);

    let instructions = r#"Plan and create wiki pages for this codebase using the structural context above.

## Page types
- **architecture** (exactly 1): High-level design, data flow, key decisions
- **module**: One per significant directory/module (3+ files or 500+ lines)
- **entity**: Key types central to the architecture (major structs/traits/enums used across files)
- **topic**: Cross-cutting concerns spanning 3+ modules (error handling, caching, etc.)
- **index** (exactly 1, create last): Cross-reference table of contents

## Workflow
1. Analyze the structural context to decide which pages to create
2. For each page, use `summarize` to understand source files, then call `wiki_contribute` with:
   - `page`: slug ID (e.g. "architecture", "mod-parser", "entity-cache")
   - `title`: human-readable title
   - `content`: markdown content (use [[page-id]] for cross-references)
   - `page_type`: one of architecture, module, entity, topic
   - `source_files`: array of source file paths this page covers
3. Create the index page last (page: "index", page_type: "topic")

## Content guidelines
- Audience: AI coding agents, not humans. Be precise about types, signatures, invariants.
- Focus on WHY (design decisions, invariants, data flow), not WHAT (the structural index already has that).
- Use [[page-id]] links for cross-references between pages.
- Every source file should appear in at least one page's source_files.
- Aim for 5-20 pages total."#;

    tool_result(json!({
        "action": "initialized",
        "wiki_dir": wiki_dir.to_string_lossy(),
        "context": context,
        "instructions": instructions,
    }))
}

#[cfg(feature = "wiki")]
pub(super) fn tool_wiki_update(
    store: &crate::wiki::store::WikiStore,
    workspace: &WorkspaceIndex,
    registry: &crate::parser::ParserRegistry,
    args: &Value,
) -> Value {
    use std::collections::HashSet;

    use crate::diff;
    use crate::languages::Language;
    use crate::wiki::page::PageType;

    let since_ref = args
        .get("since")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| store.manifest.generated_at_ref.clone());

    if since_ref.is_empty() {
        return tool_error(
            "No git ref to diff against. Pass `since` param or regenerate the wiki.",
        );
    }

    // Get changed files
    let changed_paths = match diff::get_changed_files(&workspace.root, &since_ref) {
        Ok(paths) => paths,
        Err(e) => return tool_error(&format!("Failed to get changed files: {}", e)),
    };

    if changed_paths.is_empty() {
        return tool_result(json!({
            "action": "no_changes",
            "since_ref": since_ref,
            "message": "No file changes detected since the given ref.",
        }));
    }

    // Build structural diff
    let all_files: Vec<&crate::model::FileIndex> = workspace
        .members
        .iter()
        .flat_map(|m| m.index.files.iter())
        .collect();
    let mut old_files = std::collections::HashMap::new();
    for path in &changed_paths {
        if let Ok(Some(old_content)) = diff::get_file_at_ref(&workspace.root, path, &since_ref) {
            if let Some(lang) = Language::detect(path) {
                if let Some(parser) = registry.get_parser(&lang) {
                    if let Ok(index) = parser.parse_file(path, &old_content) {
                        old_files.insert(path.clone(), index);
                    }
                }
            }
        }
    }
    let structural_diff = diff::compute_structural_diff(all_files, &old_files, &changed_paths);
    let diff_summary = diff::format_diff_markdown(&structural_diff);

    // Find affected pages
    let changed_set: HashSet<String> = changed_paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let affected_pages: Vec<Value> = store
        .pages
        .iter()
        .filter(|page| {
            page.frontmatter.page_type != PageType::Index
                && page
                    .frontmatter
                    .source_files
                    .iter()
                    .any(|sf| changed_set.contains(sf))
        })
        .map(|page| {
            json!({
                "id": page.frontmatter.id,
                "title": page.frontmatter.title,
                "page_type": page.frontmatter.page_type.as_str(),
                "source_files": page.frontmatter.source_files,
                "current_content": page.content,
            })
        })
        .collect();

    // Find uncovered changed files (not covered by any existing page)
    let covered_files: HashSet<&str> = store
        .pages
        .iter()
        .flat_map(|p| p.frontmatter.source_files.iter().map(|s| s.as_str()))
        .collect();
    let uncovered_changed_files: Vec<&str> = changed_set
        .iter()
        .filter(|f| !covered_files.contains(f.as_str()))
        .map(|s| s.as_str())
        .collect();

    if affected_pages.is_empty() && uncovered_changed_files.is_empty() {
        return tool_result(json!({
            "action": "no_affected_pages",
            "since_ref": since_ref,
            "changed_files": changed_paths.len(),
            "message": "Files changed but no existing wiki pages cover them.",
        }));
    }

    // All pages list for cross-referencing
    let all_pages: Vec<Value> = store
        .pages
        .iter()
        .map(|p| {
            json!({
                "id": p.frontmatter.id,
                "title": p.frontmatter.title,
            })
        })
        .collect();

    let instructions = r#"Update the affected wiki pages based on the structural diff.

## Workflow
For each affected page:
1. Review its current_content and the diff_summary to understand what changed
2. Use `summarize` on changed source_files for fresh structural details if needed
3. Rewrite the page content: preserve accurate existing knowledge, update what changed, flag significant architectural changes
4. Call `wiki_contribute` with the page ID and new content

## Uncovered files
If `uncovered_changed_files` is non-empty, these files are not covered by any existing wiki page. Either:
- Assign them to an existing page by calling `wiki_contribute` with updated source_files
- Create a new page via `wiki_contribute` if they represent significant new functionality

## Content guidelines
- Preserve existing knowledge that is still accurate
- Update sections that reference changed declarations
- Use [[page-id]] links for cross-references
- If a page's source files were all deleted, note that the page may be obsolete"#;

    let mut response = json!({
        "action": "analysis",
        "since_ref": since_ref,
        "changed_files": changed_paths.len(),
        "diff_summary": diff_summary,
        "affected_pages": affected_pages,
        "all_pages": all_pages,
        "instructions": instructions,
    });

    if !uncovered_changed_files.is_empty() {
        response["uncovered_changed_files"] = json!(uncovered_changed_files);
    }

    tool_result(response)
}
