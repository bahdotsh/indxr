use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::utils::contains_word_boundary;
use crate::model::CodebaseIndex;
use crate::model::declarations::{Declaration, RelKind};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DepGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NodeKind {
    File,
    Symbol,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum EdgeKind {
    Imports,
    References,
    Extends,
    Implements,
}

// ---------------------------------------------------------------------------
// Import resolution heuristic
// ---------------------------------------------------------------------------

/// Pre-computed lowercase path info to avoid repeated allocations during import
/// resolution. Built once per graph build, then reused for every import.
struct PathInfo<'a> {
    path: &'a Path,
    lower: String,
    no_ext_lower: String,
}

/// Try to resolve an import text to one of the indexed file paths.
///
/// This is a best-effort heuristic. It handles Rust `crate::`, JS/TS relative
/// (`./`, `../`), Python `module.path`, and generic path-segment matching.
/// External/third-party imports that don't map to indexed files return `None`.
///
/// Strategy:
/// 1. Relative imports (`./` or `../`) — resolve from the importing file's dir
/// 2. Extract path-like segments from import text and match against indexed paths
/// 3. Skip very short segments (<3 chars) to avoid false positives
fn resolve_import<'a>(
    import_text: &str,
    from_file: &Path,
    path_infos: &'a [PathInfo<'a>],
) -> Option<&'a Path> {
    let text = import_text.trim();
    if text.is_empty() {
        return None;
    }

    // --- Relative imports (JS/TS/Python style) ---
    if text.starts_with("./")
        || text.starts_with("../")
        || text.contains(" from './")
        || text.contains(" from \"./")
        || text.contains(" from '../")
        || text.contains(" from \"../")
    {
        if let Some(resolved) = resolve_relative_import(text, from_file, path_infos) {
            return Some(resolved);
        }
    }

    // --- Extract candidate segments from the import text ---
    // Clean up: strip leading `use `, `import `, `from `, trailing `;`
    let cleaned = text
        .trim_start_matches("use ")
        .trim_start_matches("import ")
        .trim_start_matches("from ")
        .trim_end_matches(';')
        .trim();

    // Normalize separators: `crate::parser::queries` → `crate/parser/queries`
    // For dots, only replace those between alphanumeric segments (not file extensions
    // like `.h`, `.rs`, `.py` which should be preserved).
    let normalized = normalize_import_separators(cleaned);

    // Strip common prefixes that don't map to paths
    let stripped = strip_import_prefixes(&normalized);

    // Split into segments
    let segments: Vec<&str> = stripped.split('/').filter(|s| !s.is_empty()).collect();

    if segments.is_empty() {
        return None;
    }

    // Try matching: full path, then progressively shorter trailing slices,
    // then progressively shorter leading slices (handles `module::symbol` where
    // only the leading portion maps to a file).
    // e.g., ["parser", "parse_file"] tries:
    //   "parser/parse_file", "parse_file", "parser"

    // Trailing slices (most to least specific)
    for start in 0..segments.len() {
        let candidate: String = segments[start..].join("/");
        let candidate_lower = candidate.to_lowercase();

        if candidate_lower.len() < 3 {
            continue;
        }

        if let Some(found) = match_path_candidate(&candidate_lower, path_infos) {
            if found != from_file {
                return Some(found);
            }
        }
    }

    // Leading slices (drop trailing segments which may be symbol names)
    // e.g., for ["parser", "parse_file"], try "parser"
    for end in (1..segments.len()).rev() {
        let candidate: String = segments[..end].join("/");
        let candidate_lower = candidate.to_lowercase();

        if candidate_lower.len() < 3 {
            continue;
        }

        if let Some(found) = match_path_candidate(&candidate_lower, path_infos) {
            if found != from_file {
                return Some(found);
            }
        }
    }

    None
}

/// Normalize import separators: `::` → `/`, dots → `/` only between identifier segments.
/// Preserves dots that look like file extensions (e.g., `.h`, `.rs`, `.py`, `.json`).
fn normalize_import_separators(text: &str) -> String {
    // First replace `::` with `/`
    let after_colons = text.replace("::", "/");

    // Replace dots only if not preceded by a `/` and followed by a short extension-like suffix
    let mut result = String::with_capacity(after_colons.len());
    for (byte_pos, c) in after_colons.char_indices() {
        if c == '.' {
            let rest = &after_colons[byte_pos + c.len_utf8()..];
            let ext_byte_len: usize = rest
                .chars()
                .take_while(|c| c.is_alphanumeric())
                .map(|c| c.len_utf8())
                .sum();
            let ext_char_len = rest.chars().take_while(|c| c.is_alphanumeric()).count();
            let after_ext = rest.chars().nth(ext_char_len);

            // Before a delimiter (quote, paren, whitespace): the path has ended,
            // so the dot is definitely a file extension — accept up to 5 chars.
            let before_delimiter = after_ext
                .is_some_and(|ch| !ch.is_alphanumeric() && ch != '_' && ch != '.');
            // At end of string: ambiguous — could be extension or module name.
            // Use a known-extension set for 4-5 char suffixes to avoid
            // misclassifying module names like `user`, `views`, `admin`.
            let at_end = after_ext.is_none();

            let is_extension = if before_delimiter {
                (1..=5).contains(&ext_char_len)
            } else if at_end {
                (1..=3).contains(&ext_char_len)
                    || is_known_extension(&rest[..ext_byte_len])
            } else {
                false
            };

            if is_extension {
                result.push('.');
            } else {
                result.push('/');
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Check if a suffix (without the dot) is a known file extension that's 4+ chars.
/// Used to disambiguate end-of-string dots from module separators.
fn is_known_extension(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "json" | "yaml" | "toml" | "wasm" | "lock" | "html" | "scss" | "less" | "svelte"
    )
}

/// Resolve a relative import like `./utils/helper` or `../models/user`.
fn resolve_relative_import<'a>(
    text: &str,
    from_file: &Path,
    path_infos: &'a [PathInfo<'a>],
) -> Option<&'a Path> {
    // Extract the path portion from import text
    // Handles: `{ foo } from './utils/helper'`, `'./utils/helper'`, `./utils/helper`
    let path_part = extract_path_from_import(text)?;

    let from_dir = from_file.parent()?;
    // Normalize: join from_dir with the relative path
    let resolved = from_dir.join(path_part);
    let resolved_str = resolved.to_string_lossy().to_lowercase();

    // Try exact match, then with common extensions
    for info in path_infos {
        if (info.lower == resolved_str || info.no_ext_lower == resolved_str)
            && info.path != from_file
        {
            return Some(info.path);
        }
    }

    // Also try with /index or /mod suffix for directory-style imports
    for suffix in &["/index", "/mod"] {
        let dir_import = format!("{}{}", resolved_str, suffix);
        for info in path_infos {
            if info.no_ext_lower == dir_import && info.path != from_file {
                return Some(info.path);
            }
        }
    }

    None
}

/// Find the position of "from" as a keyword (not inside another word).
fn find_from_keyword(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while start + 4 <= text.len() {
        match text[start..].find("from") {
            Some(pos) => {
                let abs = start + pos;
                let before_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
                let after = abs + 4;
                let after_ok =
                    after >= text.len() || !bytes[after].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    return Some(abs);
                }
                start = abs + 1;
            }
            None => break,
        }
    }
    None
}

/// Extract a path string from an import statement.
/// e.g., `{ foo } from './utils/helper'` → `./utils/helper`
///       `'./models/user'` → `./models/user`
fn extract_path_from_import(text: &str) -> Option<&str> {
    // Look for quoted path after "from" keyword (word-boundary match to avoid
    // false hits inside words like "platform", "transform")
    if let Some(from_idx) = find_from_keyword(text) {
        let after_from = &text[from_idx + 4..].trim_start();
        return extract_quoted_path(after_from);
    }

    // Look for any quoted path containing ./ or ../
    if let Some(path) = extract_quoted_path(text) {
        if path.starts_with("./") || path.starts_with("../") {
            return Some(path);
        }
    }

    // Bare relative path
    let trimmed = text.trim();
    if trimmed.starts_with("./") || trimmed.starts_with("../") {
        // Take until whitespace or end
        let end = trimmed
            .find(|c: char| c.is_whitespace() || c == ';')
            .unwrap_or(trimmed.len());
        return Some(&trimmed[..end]);
    }

    None
}

/// Extract a string between quotes (single or double).
fn extract_quoted_path(text: &str) -> Option<&str> {
    for quote in ['"', '\''] {
        if let Some(start) = text.find(quote) {
            if let Some(end) = text[start + 1..].find(quote) {
                return Some(&text[start + 1..start + 1 + end]);
            }
        }
    }
    None
}

/// Strip common import prefixes that don't correspond to file paths.
fn strip_import_prefixes(normalized: &str) -> &str {
    let prefixes = [
        "crate/",
        "super/",
        "self/",
        "import ",
        "from ",
        "require('",
        "require(\"",
        "require(",
    ];
    let mut result = normalized;
    for prefix in &prefixes {
        if let Some(stripped) = result.strip_prefix(prefix) {
            result = stripped;
            break;
        }
    }
    // Also strip trailing `)`, `'`, `"` from require-style
    result
        .trim_end_matches(')')
        .trim_end_matches('\'')
        .trim_end_matches('"')
}

/// Try to match a candidate path fragment against indexed file paths.
fn match_path_candidate<'a>(candidate_lower: &str, path_infos: &'a [PathInfo<'a>]) -> Option<&'a Path> {
    let mut best: Option<&'a Path> = None;
    let mut best_len = usize::MAX;

    let mod_candidate = format!("{}/mod", candidate_lower);
    let index_candidate = format!("{}/index", candidate_lower);

    for info in path_infos {
        // Check if the path ends with our candidate (with or without extension)
        let matches = info.lower.ends_with(candidate_lower)
            || info.no_ext_lower.ends_with(candidate_lower)
            // Also match mod.rs / index.ts style: candidate "parser" matches "parser/mod.rs"
            || info.no_ext_lower.ends_with(&mod_candidate)
            || info.no_ext_lower.ends_with(&index_candidate);

        if matches {
            // Prefer shorter paths (more specific match)
            let len = info.lower.len();
            if len < best_len {
                best = Some(info.path);
                best_len = len;
            }
        }
    }

    best
}

// ---------------------------------------------------------------------------
// File-level graph builder
// ---------------------------------------------------------------------------

pub fn build_file_graph(
    index: &CodebaseIndex,
    scope: Option<&str>,
    depth: Option<usize>,
) -> DepGraph {
    let all_paths: Vec<&Path> = index.files.iter().map(|f| f.path.as_path()).collect();

    // Pre-compute lowercase path info once (avoids repeated allocations in inner loops)
    let path_infos: Vec<PathInfo> = all_paths
        .iter()
        .map(|p| PathInfo {
            path: p,
            lower: p.to_string_lossy().to_lowercase(),
            no_ext_lower: p.with_extension("").to_string_lossy().to_lowercase(),
        })
        .collect();

    // Determine scoped files (seeds for depth limiting)
    let scoped_set: HashSet<&str> = if let Some(scope) = scope {
        let scope_lower = scope.to_lowercase();
        path_infos
            .iter()
            .filter(|info| info.lower.contains(&scope_lower))
            .map(|info| info.path.to_str().unwrap_or(""))
            .collect()
    } else {
        all_paths
            .iter()
            .map(|p| p.to_str().unwrap_or(""))
            .collect()
    };

    // Build full adjacency for all files (needed so depth limiting can
    // discover transitive dependencies through non-scoped files)
    let mut full_adjacency: HashMap<String, HashSet<String>> = HashMap::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();

        for imp in &file.imports {
            if let Some(target) = resolve_import(&imp.text, &file.path, &path_infos) {
                let target_str = target.to_string_lossy().to_string();
                if target_str != file_path {
                    full_adjacency
                        .entry(file_path.clone())
                        .or_default()
                        .insert(target_str);
                }
            }
        }
    }

    // Apply scope and depth limiting
    let adjacency = if depth.is_some() || scope.is_some() {
        let max_depth = depth.unwrap_or(usize::MAX);
        limit_depth_file(&full_adjacency, &scoped_set, max_depth)
    } else {
        full_adjacency
    };

    // Collect nodes and edges
    let mut node_set: HashSet<String> = HashSet::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    for (from, targets) in &adjacency {
        node_set.insert(from.clone());
        for to in targets {
            node_set.insert(to.clone());
            edges.push(GraphEdge {
                from: from.clone(),
                to: to.clone(),
                kind: EdgeKind::Imports,
            });
        }
    }

    // Sort for deterministic output
    edges.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));

    let mut nodes: Vec<GraphNode> = node_set
        .into_iter()
        .map(|id| {
            let label = Path::new(&id)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| id.clone());
            GraphNode {
                id,
                label,
                kind: NodeKind::File,
            }
        })
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    DepGraph { nodes, edges }
}

/// Limit graph to `max_depth` hops from the seed set.
fn limit_depth_file(
    adjacency: &HashMap<String, HashSet<String>>,
    seeds: &HashSet<&str>,
    max_depth: usize,
) -> HashMap<String, HashSet<String>> {
    let mut reachable: HashSet<String> = seeds.iter().map(|s| s.to_string()).collect();
    let mut frontier: Vec<String> = reachable.iter().cloned().collect();

    for _ in 0..max_depth {
        let mut next_frontier = Vec::new();
        for node in &frontier {
            if let Some(targets) = adjacency.get(node) {
                for t in targets {
                    if reachable.insert(t.clone()) {
                        next_frontier.push(t.clone());
                    }
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }

    adjacency
        .iter()
        .filter(|(k, _)| reachable.contains(k.as_str()))
        .map(|(k, v)| {
            let filtered: HashSet<String> = v
                .iter()
                .filter(|t| reachable.contains(t.as_str()))
                .cloned()
                .collect();
            (k.clone(), filtered)
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Symbol-level graph builder
// ---------------------------------------------------------------------------

pub fn build_symbol_graph(
    index: &CodebaseIndex,
    scope: Option<&str>,
    depth: Option<usize>,
) -> DepGraph {
    let scope_lower = scope.map(|s| s.to_lowercase());

    // Collect ALL symbols (needed so relationship targets across files can be resolved)
    let mut name_counts: HashMap<String, usize> = HashMap::new();
    let mut all_symbols: Vec<SymInfo> = Vec::new();
    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        collect_symbols_ext(&file.declarations, &file_path, &mut name_counts, &mut all_symbols);
    }

    // Build name → id index for relationship targets
    let name_to_ids: HashMap<&str, Vec<&str>> = {
        let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
        for sym in &all_symbols {
            map.entry(sym.name.as_str())
                .or_default()
                .push(sym.id.as_str());
        }
        map
    };

    // Build edges from relationships (single-pass — IDs come from all_symbols directly,
    // no second traversal needed)
    let mut edge_set: HashSet<(String, String, EdgeKind)> = HashSet::new();
    for sym in &all_symbols {
        for (target_name, rel_kind) in &sym.relationships {
            let edge_kind = match rel_kind {
                RelKind::Extends => EdgeKind::Extends,
                RelKind::Implements => EdgeKind::Implements,
            };
            if let Some(target_ids) = name_to_ids.get(target_name.as_str()) {
                for &tid in target_ids {
                    if tid != sym.id {
                        edge_set.insert((sym.id.clone(), tid.to_string(), edge_kind));
                    }
                }
            }
        }
    }

    // Build edges from signature references (word-boundary matching).
    // Only check type/struct/class/interface/trait names in signatures.
    // Note: this is O(N*M) where N = all symbols and M = type-like symbols.
    // For very large codebases (10K+ symbols) this could become a bottleneck;
    // an inverted index of type names per signature would reduce it to O(N).
    let type_keywords: &[&str] = &["struct ", "class ", "trait ", "interface ", "type ", "enum "];
    let type_names: Vec<(&str, &str)> = all_symbols
        .iter()
        .filter(|s| type_keywords.iter().any(|kw| s.signature_lower.contains(kw)))
        .map(|s| (s.name.as_str(), s.id.as_str()))
        .collect();

    for sym in &all_symbols {
        for &(type_name, type_id) in &type_names {
            if sym.id == type_id || type_name.len() < 3 {
                continue;
            }
            if contains_word_boundary(&sym.signature, type_name) {
                edge_set.insert((sym.id.clone(), type_id.to_string(), EdgeKind::References));
            }
        }
    }

    // Apply scope and depth limiting
    let mut edges_vec: Vec<GraphEdge> = edge_set
        .into_iter()
        .map(|(from, to, kind)| GraphEdge { from, to, kind })
        .collect();

    if scope.is_some() || depth.is_some() {
        let seed_ids: HashSet<&str> = if let Some(ref sl) = scope_lower {
            all_symbols
                .iter()
                .filter(|s| s.id.to_lowercase().contains(sl.as_str()))
                .map(|s| s.id.as_str())
                .collect()
        } else {
            all_symbols.iter().map(|s| s.id.as_str()).collect()
        };
        let max_depth = depth.unwrap_or(usize::MAX);
        edges_vec = limit_depth_symbol(edges_vec, &seed_ids, max_depth);
    }

    edges_vec.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));

    // Collect participating nodes
    let mut node_set: HashSet<String> = HashSet::new();
    for e in &edges_vec {
        node_set.insert(e.from.clone());
        node_set.insert(e.to.clone());
    }

    let sym_map: HashMap<&str, &SymInfo> =
        all_symbols.iter().map(|s| (s.id.as_str(), s)).collect();

    let mut nodes: Vec<GraphNode> = node_set
        .into_iter()
        .map(|id| {
            let label = sym_map
                .get(id.as_str())
                .map(|s| s.name.clone())
                .unwrap_or_else(|| id.clone());
            GraphNode {
                id,
                label,
                kind: NodeKind::Symbol,
            }
        })
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    DepGraph {
        nodes,
        edges: edges_vec,
    }
}

// Helper: collect symbols from declarations (avoids struct-in-fn issues)
struct SymInfo {
    id: String,
    name: String,
    signature: String,
    signature_lower: String,
    relationships: Vec<(String, RelKind)>,
}

/// Build a unique symbol ID. Appends a counter suffix when the same name appears
/// multiple times in the same file (e.g. `fn new()` in two different `impl` blocks).
fn symbol_id(file_path: &str, name: &str, name_counts: &mut HashMap<String, usize>) -> String {
    let key = format!("{}::{}", file_path, name);
    let count = name_counts.entry(key.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        key
    } else {
        format!("{}#{}", key, count)
    }
}

fn collect_symbols_ext(
    decls: &[Declaration],
    file_path: &str,
    name_counts: &mut HashMap<String, usize>,
    out: &mut Vec<SymInfo>,
) {
    for decl in decls {
        if decl.name.is_empty() {
            continue;
        }
        let sig = decl.signature.clone();
        out.push(SymInfo {
            id: symbol_id(file_path, &decl.name, name_counts),
            name: decl.name.clone(),
            signature_lower: sig.to_lowercase(),
            signature: sig,
            relationships: decl
                .relationships
                .iter()
                .map(|r| (r.target.clone(), r.kind.clone()))
                .collect(),
        });
        collect_symbols_ext(&decl.children, file_path, name_counts, out);
    }
}

fn limit_depth_symbol(
    edges: Vec<GraphEdge>,
    seeds: &HashSet<&str>,
    max_depth: usize,
) -> Vec<GraphEdge> {
    // Build adjacency from edges
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &edges {
        adj.entry(e.from.as_str()).or_default().push(e.to.as_str());
    }

    // BFS from seeds
    let mut reachable: HashSet<String> = seeds.iter().map(|s| s.to_string()).collect();
    let mut frontier: Vec<String> = reachable.iter().cloned().collect();

    for _ in 0..max_depth {
        let mut next = Vec::new();
        for node in &frontier {
            if let Some(targets) = adj.get(node.as_str()) {
                for &t in targets {
                    if reachable.insert(t.to_string()) {
                        next.push(t.to_string());
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }

    edges
        .into_iter()
        .filter(|e| reachable.contains(&e.from) && reachable.contains(&e.to))
        .collect()
}

// ---------------------------------------------------------------------------
// Output formatters
// ---------------------------------------------------------------------------

/// Format graph as DOT (Graphviz) string.
pub fn format_dot(graph: &DepGraph) -> String {
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = String::new();
    out.push_str("digraph dependencies {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str("  node [shape=box, style=filled, fillcolor=\"#f0f0f0\"];\n");
    out.push('\n');

    for edge in &graph.edges {
        let from = escape(&edge.from);
        let to = escape(&edge.to);
        let label = match edge.kind {
            EdgeKind::Imports => "",
            EdgeKind::References => "references",
            EdgeKind::Extends => "extends",
            EdgeKind::Implements => "implements",
        };
        if label.is_empty() {
            out.push_str(&format!("  \"{}\" -> \"{}\";\n", from, to));
        } else {
            out.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                from, to, label
            ));
        }
    }

    out.push_str("}\n");
    out
}

/// Format graph as Mermaid string.
pub fn format_mermaid(graph: &DepGraph) -> String {
    let mut out = String::new();
    out.push_str("```mermaid\n");
    out.push_str("graph LR\n");

    // Build id → unique Mermaid-safe node ID map (indexed to avoid collisions
    // from different IDs that would sanitize to the same string, e.g. a-b vs a_b)
    let mut id_map: HashMap<&str, String> = HashMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        id_map.insert(&node.id, format!("n{}", i));
    }

    // Emit node declarations once (escape chars that break Mermaid label syntax)
    let escape_mermaid = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('"', "#quot;")
            .replace('[', "#91;")
            .replace(']', "#93;")
            .replace('|', "#124;")
            .replace('(', "#40;")
            .replace(')', "#41;")
            .replace('{', "#123;")
            .replace('}', "#125;")
    };
    for node in &graph.nodes {
        let nid = &id_map[node.id.as_str()];
        out.push_str(&format!("  {}[\"{}\"]\n", nid, escape_mermaid(&node.id)));
    }

    // Emit edges referencing mapped ids
    for edge in &graph.edges {
        let from_nid = &id_map[edge.from.as_str()];
        let to_nid = &id_map[edge.to.as_str()];
        let arrow = match edge.kind {
            EdgeKind::Imports => "-->",
            EdgeKind::References => "-.->|references|",
            EdgeKind::Extends => "-->|extends|",
            EdgeKind::Implements => "-->|implements|",
        };
        out.push_str(&format!("  {} {} {}\n", from_nid, arrow, to_nid));
    }

    out.push_str("```\n");
    out
}

/// Format graph as JSON Value.
pub fn format_json(graph: &DepGraph) -> Value {
    json!({
        "nodes": graph.nodes,
        "edges": graph.edges
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::languages::Language;
    use crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility};
    use crate::model::{CodebaseIndex, FileIndex, Import, IndexStats};

    fn make_index(files: Vec<FileIndex>) -> CodebaseIndex {
        CodebaseIndex {
            root: PathBuf::from("/test"),
            root_name: "test".to_string(),
            generated_at: String::new(),
            files,
            tree: Vec::new(),
            stats: IndexStats {
                total_files: 0,
                total_lines: 0,
                languages: HashMap::new(),
                duration_ms: 0,
            },
        }
    }

    fn make_file(path: &str, imports: Vec<&str>, decls: Vec<Declaration>) -> FileIndex {
        FileIndex {
            path: PathBuf::from(path),
            language: Language::Rust,
            size: 100,
            lines: 10,
            imports: imports
                .into_iter()
                .map(|t| Import {
                    text: t.to_string(),
                })
                .collect(),
            declarations: decls,
        }
    }

    fn make_decl(name: &str, sig: &str) -> Declaration {
        Declaration::new(
            DeclKind::Function,
            name.to_string(),
            sig.to_string(),
            Visibility::Public,
            1,
        )
    }

    fn make_struct(name: &str) -> Declaration {
        Declaration::new(
            DeclKind::Struct,
            name.to_string(),
            format!("struct {}", name),
            Visibility::Public,
            1,
        )
    }

    fn make_path_infos<'a>(paths: &'a [&'a Path]) -> Vec<PathInfo<'a>> {
        paths
            .iter()
            .map(|p| PathInfo {
                path: p,
                lower: p.to_string_lossy().to_lowercase(),
                no_ext_lower: p.with_extension("").to_string_lossy().to_lowercase(),
            })
            .collect()
    }

    // --- Import resolution tests ---

    #[test]
    fn test_resolve_rust_crate_import() {
        let paths: Vec<&Path> = vec![
            Path::new("src/parser/mod.rs"),
            Path::new("src/model/mod.rs"),
            Path::new("src/main.rs"),
        ];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/main.rs");

        let result = resolve_import("crate::parser", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/parser/mod.rs".to_string())
        );
    }

    #[test]
    fn test_resolve_rust_crate_nested() {
        let paths: Vec<&Path> = vec![
            Path::new("src/parser/queries/rust.rs"),
            Path::new("src/parser/mod.rs"),
            Path::new("src/main.rs"),
        ];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/main.rs");

        let result = resolve_import("crate::parser::queries::rust", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/parser/queries/rust.rs".to_string())
        );
    }

    #[test]
    fn test_resolve_relative_import() {
        let paths: Vec<&Path> = vec![
            Path::new("src/utils/helper.ts"),
            Path::new("src/components/app.ts"),
        ];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/components/app.ts");

        let result = resolve_import("{ foo } from '../utils/helper'", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/utils/helper.ts".to_string())
        );
    }

    #[test]
    fn test_resolve_relative_import_same_dir() {
        let paths: Vec<&Path> = vec![
            Path::new("src/utils/helper.ts"),
            Path::new("src/utils/main.ts"),
        ];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/utils/main.ts");

        let result = resolve_import("{ foo } from './helper'", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/utils/helper.ts".to_string())
        );
    }

    #[test]
    fn test_resolve_python_import() {
        let paths: Vec<&Path> = vec![Path::new("app/models.py"), Path::new("app/views.py")];
        let infos = make_path_infos(&paths);
        let from = Path::new("app/views.py");

        let result = resolve_import("app.models", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("app/models.py".to_string())
        );
    }

    #[test]
    fn test_no_resolve_external_import() {
        let paths: Vec<&Path> = vec![Path::new("src/main.rs"), Path::new("src/lib.rs")];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/main.rs");

        let result = resolve_import("std::collections::HashMap", from, &infos);
        assert!(result.is_none());
    }

    #[test]
    fn test_no_resolve_short_stem() {
        let paths: Vec<&Path> = vec![Path::new("src/io.rs"), Path::new("src/main.rs")];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/main.rs");

        // "io" is only 2 chars — should not produce false matches
        let result = resolve_import("io", from, &infos);
        assert!(result.is_none());
    }

    #[test]
    fn test_no_self_import() {
        let paths: Vec<&Path> = vec![Path::new("src/parser/mod.rs")];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/parser/mod.rs");

        let result = resolve_import("crate::parser", from, &infos);
        assert!(result.is_none());
    }

    // --- File graph tests ---

    #[test]
    fn test_file_graph_basic() {
        let index = make_index(vec![
            make_file("src/main.rs", vec!["crate::parser"], vec![]),
            make_file("src/parser/mod.rs", vec![], vec![]),
        ]);

        let graph = build_file_graph(&index, None, None);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "src/main.rs");
        assert_eq!(graph.edges[0].to, "src/parser/mod.rs");
        assert_eq!(graph.nodes.len(), 2);
    }

    #[test]
    fn test_file_graph_scoped() {
        let index = make_index(vec![
            make_file("src/main.rs", vec!["crate::parser"], vec![]),
            make_file("src/parser/mod.rs", vec!["crate::model"], vec![]),
            make_file("src/model/mod.rs", vec![], vec![]),
        ]);

        // Scope to parser — should only show parser's deps
        let graph = build_file_graph(&index, Some("parser"), None);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "src/parser/mod.rs");
        assert_eq!(graph.edges[0].to, "src/model/mod.rs");
    }

    #[test]
    fn test_file_graph_no_external_edges() {
        let index = make_index(vec![make_file(
            "src/main.rs",
            vec!["std::collections::HashMap"],
            vec![],
        )]);

        let graph = build_file_graph(&index, None, None);
        assert!(graph.edges.is_empty());
        assert!(graph.nodes.is_empty());
    }

    #[test]
    fn test_file_graph_deduplicates_edges() {
        let index = make_index(vec![
            make_file(
                "src/main.rs",
                vec!["crate::parser::Parser", "crate::parser::Language"],
                vec![],
            ),
            make_file("src/parser/mod.rs", vec![], vec![]),
        ]);

        let graph = build_file_graph(&index, None, None);
        // Both imports resolve to parser/mod.rs → should be one edge
        assert_eq!(graph.edges.len(), 1);
    }

    // --- Depth limiting tests ---

    #[test]
    fn test_file_graph_depth_limit() {
        // Chain: main → parser → model → utils
        let index = make_index(vec![
            make_file("src/main.rs", vec!["crate::parser"], vec![]),
            make_file("src/parser/mod.rs", vec!["crate::model"], vec![]),
            make_file("src/model/mod.rs", vec!["crate::utils"], vec![]),
            make_file("src/utils/mod.rs", vec![], vec![]),
        ]);

        // depth=1 from all files: each file can reach 1 hop
        let graph_full = build_file_graph(&index, None, None);
        assert_eq!(graph_full.edges.len(), 3);

        // Scope to main with depth=1 → should only show main → parser
        let graph_d1 = build_file_graph(&index, Some("main"), Some(1));
        assert_eq!(graph_d1.edges.len(), 1);
        assert_eq!(graph_d1.edges[0].from, "src/main.rs");
        assert_eq!(graph_d1.edges[0].to, "src/parser/mod.rs");

        // Scope to main with depth=2 → main → parser and parser → model
        let graph_d2 = build_file_graph(&index, Some("main"), Some(2));
        assert_eq!(graph_d2.edges.len(), 2);
    }

    #[test]
    fn test_symbol_graph_depth_limit() {
        // A → B → C via extends chain
        let mut a = make_decl("A", "class A extends B");
        a.kind = DeclKind::Class;
        a.relationships.push(Relationship {
            kind: RelKind::Extends,
            target: "B".to_string(),
        });

        let mut b = make_decl("B", "class B extends C");
        b.kind = DeclKind::Class;
        b.relationships.push(Relationship {
            kind: RelKind::Extends,
            target: "C".to_string(),
        });

        let mut c = make_struct("C");
        c.kind = DeclKind::Class;
        c.signature = "class C".to_string();

        let index = make_index(vec![
            make_file("src/a.ts", vec![], vec![a]),
            make_file("src/b.ts", vec![], vec![b]),
            make_file("src/c.ts", vec![], vec![c]),
        ]);

        // Full graph should have extends edges: A→B, B→C
        // Plus possible signature references (A sig mentions B, B sig mentions C)
        let graph_full = build_symbol_graph(&index, None, None);
        let extends_edges: Vec<_> = graph_full
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Extends)
            .collect();
        assert_eq!(extends_edges.len(), 2);

        // Scope to a.ts with depth=1 → only A's direct relationships visible
        let graph_d1 = build_symbol_graph(&index, Some("a.ts"), Some(1));
        let extends_d1: Vec<_> = graph_d1
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Extends)
            .collect();
        assert_eq!(extends_d1.len(), 1);
        assert!(extends_d1[0].from.contains("A"));
        assert!(extends_d1[0].to.contains("B"));
    }

    // --- Symbol graph tests ---

    #[test]
    fn test_symbol_graph_extends() {
        let mut child = make_decl("ChildClass", "class ChildClass extends BaseClass");
        child.kind = DeclKind::Class;
        child.relationships.push(Relationship {
            kind: RelKind::Extends,
            target: "BaseClass".to_string(),
        });

        let mut base = make_decl("BaseClass", "class BaseClass");
        base.kind = DeclKind::Class;

        let index = make_index(vec![
            make_file("src/child.ts", vec![], vec![child]),
            make_file("src/base.ts", vec![], vec![base]),
        ]);

        let graph = build_symbol_graph(&index, None, None);
        let extends_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Extends)
            .collect();
        assert_eq!(extends_edges.len(), 1);
        assert!(extends_edges[0].from.contains("ChildClass"));
        assert!(extends_edges[0].to.contains("BaseClass"));
    }

    #[test]
    fn test_symbol_graph_implements() {
        let mut implementor = make_decl("MyService", "class MyService implements Service");
        implementor.kind = DeclKind::Class;
        implementor.relationships.push(Relationship {
            kind: RelKind::Implements,
            target: "Service".to_string(),
        });

        let mut iface = make_decl("Service", "interface Service");
        iface.kind = DeclKind::Interface;

        let index = make_index(vec![
            make_file("src/my_service.ts", vec![], vec![implementor]),
            make_file("src/service.ts", vec![], vec![iface]),
        ]);

        let graph = build_symbol_graph(&index, None, None);
        let impl_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Implements)
            .collect();
        assert_eq!(impl_edges.len(), 1);
        assert!(impl_edges[0].from.contains("MyService"));
        assert!(impl_edges[0].to.contains("Service"));
    }

    #[test]
    fn test_symbol_graph_signature_reference() {
        let parser_struct = make_struct("Parser");
        let func = make_decl("run_parser", "fn run_parser(p: &Parser) -> Result<()>");

        let index = make_index(vec![
            make_file("src/parser.rs", vec![], vec![parser_struct]),
            make_file("src/main.rs", vec![], vec![func]),
        ]);

        let graph = build_symbol_graph(&index, None, None);
        // run_parser's signature references Parser → should have an edge
        let ref_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.from.contains("run_parser") && e.to.contains("Parser"))
            .collect();
        assert_eq!(ref_edges.len(), 1);
    }

    // --- Formatter tests ---

    #[test]
    fn test_format_dot() {
        let graph = DepGraph {
            nodes: vec![
                GraphNode {
                    id: "a.rs".to_string(),
                    label: "a.rs".to_string(),
                    kind: NodeKind::File,
                },
                GraphNode {
                    id: "b.rs".to_string(),
                    label: "b.rs".to_string(),
                    kind: NodeKind::File,
                },
            ],
            edges: vec![GraphEdge {
                from: "a.rs".to_string(),
                to: "b.rs".to_string(),
                kind: EdgeKind::Imports,
            }],
        };

        let dot = format_dot(&graph);
        assert!(dot.contains("digraph dependencies"));
        assert!(dot.contains("\"a.rs\" -> \"b.rs\""));
        assert!(dot.contains("rankdir=LR"));
    }

    #[test]
    fn test_format_mermaid() {
        let graph = DepGraph {
            nodes: vec![
                GraphNode {
                    id: "a.rs".to_string(),
                    label: "a.rs".to_string(),
                    kind: NodeKind::File,
                },
                GraphNode {
                    id: "b.rs".to_string(),
                    label: "b.rs".to_string(),
                    kind: NodeKind::File,
                },
            ],
            edges: vec![GraphEdge {
                from: "a.rs".to_string(),
                to: "b.rs".to_string(),
                kind: EdgeKind::Imports,
            }],
        };

        let mermaid = format_mermaid(&graph);
        assert!(mermaid.starts_with("```mermaid\n"));
        assert!(mermaid.ends_with("```\n"));
        assert!(mermaid.contains("graph LR"));
        assert!(mermaid.contains("-->"));
        assert!(mermaid.contains("n0[\"a.rs\"]"));
        assert!(mermaid.contains("n1[\"b.rs\"]"));
    }

    #[test]
    fn test_format_dot_with_labels() {
        let graph = DepGraph {
            nodes: vec![],
            edges: vec![GraphEdge {
                from: "A".to_string(),
                to: "B".to_string(),
                kind: EdgeKind::Extends,
            }],
        };

        let dot = format_dot(&graph);
        assert!(dot.contains("[label=\"extends\"]"));
    }

    #[test]
    fn test_format_json() {
        let graph = DepGraph {
            nodes: vec![GraphNode {
                id: "a".to_string(),
                label: "a".to_string(),
                kind: NodeKind::File,
            }],
            edges: vec![],
        };

        let json = format_json(&graph);
        assert!(json.get("nodes").unwrap().as_array().unwrap().len() == 1);
        assert!(json.get("edges").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn test_symbol_graph_pub_struct_signature_reference() {
        // Signatures with visibility prefixes (as produced by real parsers)
        let mut parser_struct = make_struct("Parser");
        parser_struct.signature = "pub struct Parser".to_string();

        let mut func = make_decl("run_parser", "pub fn run_parser(p: &Parser) -> Result<()>");
        func.signature = "pub fn run_parser(p: &Parser) -> Result<()>".to_string();

        let index = make_index(vec![
            make_file("src/parser.rs", vec![], vec![parser_struct]),
            make_file("src/main.rs", vec![], vec![func]),
        ]);

        let graph = build_symbol_graph(&index, None, None);
        let ref_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.from.contains("run_parser") && e.to.contains("Parser"))
            .collect();
        assert_eq!(
            ref_edges.len(),
            1,
            "pub struct signatures must be detected for reference edges"
        );
    }

    #[test]
    fn test_file_graph_depth_zero() {
        let index = make_index(vec![
            make_file("src/main.rs", vec!["crate::parser"], vec![]),
            make_file("src/parser/mod.rs", vec![], vec![]),
        ]);

        // depth=0 from main: no hops allowed, so no edges
        let graph = build_file_graph(&index, Some("main"), Some(0));
        assert!(
            graph.edges.is_empty(),
            "depth=0 should produce no edges beyond seed set"
        );
    }

    #[test]
    fn test_empty_codebase_graph() {
        let index = make_index(vec![]);

        let file_graph = build_file_graph(&index, None, None);
        assert!(file_graph.nodes.is_empty());
        assert!(file_graph.edges.is_empty());

        let sym_graph = build_symbol_graph(&index, None, None);
        assert!(sym_graph.nodes.is_empty());
        assert!(sym_graph.edges.is_empty());
    }

    #[test]
    fn test_mermaid_no_id_collision() {
        // Two IDs that would collide under naive char-replacement sanitization
        let graph = DepGraph {
            nodes: vec![
                GraphNode {
                    id: "src/a-b.rs".to_string(),
                    label: "a-b.rs".to_string(),
                    kind: NodeKind::File,
                },
                GraphNode {
                    id: "src/a_b.rs".to_string(),
                    label: "a_b.rs".to_string(),
                    kind: NodeKind::File,
                },
            ],
            edges: vec![GraphEdge {
                from: "src/a-b.rs".to_string(),
                to: "src/a_b.rs".to_string(),
                kind: EdgeKind::Imports,
            }],
        };

        let mermaid = format_mermaid(&graph);
        // Both nodes must appear with distinct IDs
        assert!(mermaid.contains("n0[\"src/a-b.rs\"]"));
        assert!(mermaid.contains("n1[\"src/a_b.rs\"]"));
        assert!(mermaid.contains("n0 --> n1"));
    }

    // --- Cyclic import test ---

    #[test]
    fn test_file_graph_cyclic_imports() {
        let index = make_index(vec![
            make_file("src/alpha.rs", vec!["crate::beta"], vec![]),
            make_file("src/beta.rs", vec!["crate::alpha"], vec![]),
        ]);

        let graph = build_file_graph(&index, None, None);
        assert_eq!(graph.edges.len(), 2);
        assert_eq!(graph.nodes.len(), 2);
        // Both directions present
        let a_to_b = graph.edges.iter().any(|e| e.from == "src/alpha.rs" && e.to == "src/beta.rs");
        let b_to_a = graph.edges.iter().any(|e| e.from == "src/beta.rs" && e.to == "src/alpha.rs");
        assert!(a_to_b, "should have edge alpha → beta");
        assert!(b_to_a, "should have edge beta → alpha");
    }

    // --- normalize_import_separators edge cases ---

    #[test]
    fn test_normalize_preserves_extension_mid_string() {
        // file.h should preserve the dot (extension before non-alnum quote)
        assert_eq!(normalize_import_separators("path/file.h"), "path/file.h");
        assert_eq!(normalize_import_separators("file.rs"), "file.rs");
    }

    #[test]
    fn test_normalize_replaces_module_dots() {
        // Python-style module separators become slashes
        assert_eq!(normalize_import_separators("app.models.user"), "app/models/user");
        assert_eq!(normalize_import_separators("app.models.views"), "app/models/views");
        // 1-3 char final segments still preserved as extensions
        assert_eq!(normalize_import_separators("app.models.py"), "app/models.py");
    }

    #[test]
    fn test_normalize_preserves_known_long_extensions() {
        // Known 4-5 char extensions at end of string are preserved
        assert_eq!(normalize_import_separators("config.json"), "config.json");
        assert_eq!(normalize_import_separators("config.yaml"), "config.yaml");
        assert_eq!(normalize_import_separators("config.toml"), "config.toml");
        assert_eq!(normalize_import_separators("module.wasm"), "module.wasm");
        assert_eq!(normalize_import_separators("page.html"), "page.html");
    }

    #[test]
    fn test_normalize_long_ext_before_delimiter() {
        // Before a delimiter (quote), any 1-5 char suffix is treated as extension
        assert_eq!(
            normalize_import_separators("config.json'"),
            "config.json'"
        );
        assert_eq!(
            normalize_import_separators("data.yaml)"),
            "data.yaml)"
        );
    }

    #[test]
    fn test_normalize_double_colon() {
        assert_eq!(normalize_import_separators("crate::parser::queries"), "crate/parser/queries");
    }

    // --- strip_import_prefixes with require() ---

    #[test]
    fn test_strip_require_style() {
        assert_eq!(strip_import_prefixes("require('./utils')"), "./utils");
        assert_eq!(strip_import_prefixes("require('lodash')"), "lodash");
        assert_eq!(strip_import_prefixes("require(\"fs\")"), "fs");
    }

    // --- Deeply nested relative imports ---

    #[test]
    fn test_resolve_deeply_nested_relative() {
        let paths: Vec<&Path> = vec![
            Path::new("src/lib/core/utils.ts"),
            Path::new("src/features/admin/settings/page.ts"),
        ];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/features/admin/settings/page.ts");

        let result = resolve_import("{ u } from '../../../lib/core/utils'", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/lib/core/utils.ts".to_string())
        );
    }

    // --- Edge case: empty / whitespace import text ---

    #[test]
    fn test_resolve_empty_import() {
        let paths: Vec<&Path> = vec![Path::new("src/main.rs")];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/main.rs");

        assert!(resolve_import("", from, &infos).is_none());
        assert!(resolve_import("   ", from, &infos).is_none());
    }

    // --- Mixed relative path with module-style dots ---

    #[test]
    fn test_resolve_relative_with_extension() {
        let paths: Vec<&Path> = vec![
            Path::new("src/config.json"),
            Path::new("src/app.ts"),
        ];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/app.ts");

        let result = resolve_import("{ c } from './config.json'", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/config.json".to_string())
        );
    }

    // --- Symbol graph depth=0 ---

    #[test]
    fn test_symbol_graph_depth_zero() {
        let mut a = make_decl("A", "class A extends B");
        a.kind = DeclKind::Class;
        a.relationships.push(Relationship {
            kind: RelKind::Extends,
            target: "B".to_string(),
        });

        let mut b = make_struct("B");
        b.kind = DeclKind::Class;
        b.signature = "class B".to_string();

        let index = make_index(vec![
            make_file("src/a.ts", vec![], vec![a]),
            make_file("src/b.ts", vec![], vec![b]),
        ]);

        // depth=0 from a.ts: no hops allowed, so no edges
        let graph = build_symbol_graph(&index, Some("a.ts"), Some(0));
        assert!(
            graph.edges.is_empty(),
            "depth=0 should produce no edges beyond seed set"
        );
    }

    // --- Symbol ID uniqueness ---

    #[test]
    fn test_symbol_id_uniqueness_same_name() {
        // Two functions named "new" in the same file (e.g. different impl blocks)
        // Each references a distinct type so they produce edges and appear as nodes.
        let func1 = make_decl("new", "fn new() -> Foo");
        let func2 = make_decl("new", "fn new() -> Bar");
        let foo = make_struct("Foo");
        let bar = make_struct("Bar");

        let index = make_index(vec![
            make_file("src/lib.rs", vec![], vec![func1, func2]),
            make_file("src/types.rs", vec![], vec![foo, bar]),
        ]);

        let graph = build_symbol_graph(&index, None, None);
        // Both "new" symbols should appear as distinct nodes (via signature references)
        let new_nodes: Vec<_> = graph
            .nodes
            .iter()
            .filter(|n| n.label == "new")
            .collect();
        assert_eq!(
            new_nodes.len(),
            2,
            "Two declarations named 'new' should produce two distinct nodes"
        );
        assert_ne!(
            new_nodes[0].id, new_nodes[1].id,
            "Duplicate-named symbols must have distinct IDs"
        );
    }

    // --- Mermaid special character escaping ---

    #[test]
    fn test_format_mermaid_escapes_special_chars() {
        let graph = DepGraph {
            nodes: vec![
                GraphNode {
                    id: "src/foo\"bar.rs".to_string(),
                    label: "foo\"bar.rs".to_string(),
                    kind: NodeKind::File,
                },
                GraphNode {
                    id: "src/baz[x]qux.rs".to_string(),
                    label: "baz[x]qux.rs".to_string(),
                    kind: NodeKind::File,
                },
            ],
            edges: vec![GraphEdge {
                from: "src/foo\"bar.rs".to_string(),
                to: "src/baz[x]qux.rs".to_string(),
                kind: EdgeKind::Imports,
            }],
        };

        let mermaid = format_mermaid(&graph);
        // Quotes and brackets must be escaped to avoid breaking Mermaid syntax
        assert!(
            mermaid.contains("#quot;"),
            "Double quotes should be escaped"
        );
        assert!(mermaid.contains("#91;"), "Opening brackets should be escaped");
        assert!(mermaid.contains("#93;"), "Closing brackets should be escaped");
        assert!(
            !mermaid.contains("foo\"bar"),
            "Raw double quotes should not appear in node labels"
        );
    }

    #[test]
    fn test_format_mermaid_escapes_pipes_and_parens() {
        let graph = DepGraph {
            nodes: vec![
                GraphNode {
                    id: "fn foo(x: i32)".to_string(),
                    label: "foo(x: i32)".to_string(),
                    kind: NodeKind::Symbol,
                },
                GraphNode {
                    id: "trait A|B".to_string(),
                    label: "A|B".to_string(),
                    kind: NodeKind::Symbol,
                },
            ],
            edges: vec![GraphEdge {
                from: "fn foo(x: i32)".to_string(),
                to: "trait A|B".to_string(),
                kind: EdgeKind::References,
            }],
        };

        let mermaid = format_mermaid(&graph);
        assert!(mermaid.contains("#40;"), "Opening parens should be escaped");
        assert!(mermaid.contains("#41;"), "Closing parens should be escaped");
        assert!(mermaid.contains("#124;"), "Pipes should be escaped");
    }

    #[test]
    fn test_format_mermaid_extends_edge() {
        let graph = DepGraph {
            nodes: vec![
                GraphNode {
                    id: "Child".to_string(),
                    label: "Child".to_string(),
                    kind: NodeKind::Symbol,
                },
                GraphNode {
                    id: "Parent".to_string(),
                    label: "Parent".to_string(),
                    kind: NodeKind::Symbol,
                },
            ],
            edges: vec![GraphEdge {
                from: "Child".to_string(),
                to: "Parent".to_string(),
                kind: EdgeKind::Extends,
            }],
        };

        let mermaid = format_mermaid(&graph);
        assert!(
            mermaid.contains("-->|extends|"),
            "Extends edges should have label"
        );
    }

    #[test]
    fn test_format_mermaid_implements_edge() {
        let graph = DepGraph {
            nodes: vec![
                GraphNode {
                    id: "Impl".to_string(),
                    label: "Impl".to_string(),
                    kind: NodeKind::Symbol,
                },
                GraphNode {
                    id: "Iface".to_string(),
                    label: "Iface".to_string(),
                    kind: NodeKind::Symbol,
                },
            ],
            edges: vec![GraphEdge {
                from: "Impl".to_string(),
                to: "Iface".to_string(),
                kind: EdgeKind::Implements,
            }],
        };

        let mermaid = format_mermaid(&graph);
        assert!(
            mermaid.contains("-->|implements|"),
            "Implements edges should have label"
        );
    }

    // --- normalize_import_separators mixed input ---

    #[test]
    fn test_normalize_mixed_colons_and_dots() {
        // Rust-style with a dotted extension at the end
        assert_eq!(
            normalize_import_separators("crate::parser.queries"),
            "crate/parser/queries"
        );
        // Mixed: colons then dots for module path
        assert_eq!(
            normalize_import_separators("crate::utils.helpers.string"),
            "crate/utils/helpers/string"
        );
    }

    // --- require() import resolution ---

    #[test]
    fn test_resolve_require_import() {
        let paths: Vec<&Path> = vec![
            Path::new("src/utils.js"),
            Path::new("src/app.js"),
        ];
        let infos = make_path_infos(&paths);
        let from = Path::new("src/app.js");

        let result = resolve_import("require('./utils')", from, &infos);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/utils.js".to_string())
        );
    }

    // --- "from" as substring should not false-match ---

    #[test]
    fn test_extract_path_from_import_no_false_from() {
        // "from" inside "platform" should not match
        assert!(extract_path_from_import("import platform").is_none());
        assert!(extract_path_from_import("transform").is_none());
    }

    #[test]
    fn test_find_from_keyword() {
        assert_eq!(find_from_keyword("{ x } from './y'"), Some(6));
        assert_eq!(find_from_keyword("import platform"), None);
        assert_eq!(find_from_keyword("transform('./x')"), None);
        assert_eq!(find_from_keyword("from './x'"), Some(0));
    }
}
