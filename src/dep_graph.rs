use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::mcp::helpers::contains_word_boundary;
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
    Extends,
    Implements,
}

// ---------------------------------------------------------------------------
// Import resolution heuristic
// ---------------------------------------------------------------------------

/// Try to resolve an import text to one of the indexed file paths.
///
/// Strategy:
/// 1. Relative imports (`./` or `../`) — resolve from the importing file's dir
/// 2. Extract path-like segments from import text and match against indexed paths
/// 3. Skip very short segments (<3 chars) to avoid false positives
fn resolve_import<'a>(
    import_text: &str,
    from_file: &Path,
    all_paths: &[&'a Path],
) -> Option<&'a Path> {
    let text = import_text.trim();
    if text.is_empty() {
        return None;
    }

    // --- Relative imports (JS/TS/Python style) ---
    if text.contains("./") || text.contains("../") {
        if let Some(resolved) = resolve_relative_import(text, from_file, all_paths) {
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
    //                       `com.example.MyClass`    → `com/example/MyClass`
    let normalized = cleaned
        .replace("::", "/")
        .replace('.', "/");

    // Strip common prefixes that don't map to paths
    let stripped = strip_import_prefixes(&normalized);

    // Split into segments
    let segments: Vec<&str> = stripped
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

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

        if let Some(found) = match_path_candidate(&candidate_lower, all_paths) {
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

        if let Some(found) = match_path_candidate(&candidate_lower, all_paths) {
            if found != from_file {
                return Some(found);
            }
        }
    }

    None
}

/// Resolve a relative import like `./utils/helper` or `../models/user`.
fn resolve_relative_import<'a>(
    text: &str,
    from_file: &Path,
    all_paths: &[&'a Path],
) -> Option<&'a Path> {
    // Extract the path portion from import text
    // Handles: `{ foo } from './utils/helper'`, `'./utils/helper'`, `./utils/helper`
    let path_part = extract_path_from_import(text)?;

    let from_dir = from_file.parent()?;
    // Normalize: join from_dir with the relative path
    let resolved = from_dir.join(path_part);
    let resolved_str = resolved.to_string_lossy().to_lowercase();

    // Try exact match, then with common extensions
    for path in all_paths {
        let path_str = path.to_string_lossy().to_lowercase();
        let path_no_ext = path
            .with_extension("")
            .to_string_lossy()
            .to_lowercase();

        if (path_str == resolved_str || path_no_ext == resolved_str)
            && *path != from_file
        {
            return Some(path);
        }
    }

    // Also try with /index or /mod suffix for directory-style imports
    for suffix in &["/index", "/mod"] {
        let dir_import = format!("{}{}", resolved_str, suffix);
        for path in all_paths {
            let path_no_ext = path
                .with_extension("")
                .to_string_lossy()
                .to_lowercase();
            if path_no_ext == dir_import && *path != from_file {
                return Some(path);
            }
        }
    }

    None
}

/// Extract a path string from an import statement.
/// e.g., `{ foo } from './utils/helper'` → `./utils/helper`
///       `'./models/user'` → `./models/user`
fn extract_path_from_import(text: &str) -> Option<&str> {
    // Look for quoted path after "from"
    if let Some(from_idx) = text.find("from") {
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
        "require(",
        "require('",
        "require(\"",
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
fn match_path_candidate<'a>(candidate_lower: &str, all_paths: &[&'a Path]) -> Option<&'a Path> {
    let mut best: Option<&'a Path> = None;
    let mut best_len = usize::MAX;

    for path in all_paths {
        let path_str = path.to_string_lossy().to_lowercase();
        let path_no_ext = path
            .with_extension("")
            .to_string_lossy()
            .to_lowercase();

        // Check if the path ends with our candidate (with or without extension)
        let matches = path_str.ends_with(candidate_lower)
            || path_no_ext.ends_with(candidate_lower)
            // Also match mod.rs / index.ts style: candidate "parser" matches "parser/mod.rs"
            || path_no_ext.ends_with(&format!("{}/mod", candidate_lower))
            || path_no_ext.ends_with(&format!("{}/index", candidate_lower));

        if matches {
            // Prefer shorter paths (more specific match)
            let len = path_str.len();
            if len < best_len {
                best = Some(path);
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

    // Determine scoped files
    let scoped_files: Vec<&Path> = if let Some(scope) = scope {
        let scope_lower = scope.to_lowercase();
        all_paths
            .iter()
            .filter(|p| p.to_string_lossy().to_lowercase().contains(&scope_lower))
            .copied()
            .collect()
    } else {
        all_paths.clone()
    };

    let scoped_set: HashSet<&str> = scoped_files
        .iter()
        .map(|p| p.to_str().unwrap_or(""))
        .collect();

    // Build adjacency: source_file → set of target_files
    let mut adjacency: HashMap<String, HashSet<String>> = HashMap::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        if !scoped_set.contains(file_path.as_str()) {
            continue;
        }

        for imp in &file.imports {
            if let Some(target) = resolve_import(&imp.text, &file.path, &all_paths) {
                let target_str = target.to_string_lossy().to_string();
                if target_str != file_path {
                    adjacency
                        .entry(file_path.clone())
                        .or_default()
                        .insert(target_str);
                }
            }
        }
    }

    // Apply depth limit if specified (BFS from scoped files)
    if let Some(max_depth) = depth {
        adjacency = limit_depth_file(&adjacency, &scoped_set, max_depth);
    }

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

    let mut symbols: Vec<SymInfo> = Vec::new();

    // Build symbol list
    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        if let Some(ref sl) = scope_lower {
            if !file_path.to_lowercase().contains(sl.as_str()) {
                continue;
            }
        }
        collect_symbols_ext(&file.declarations, &file_path, &mut symbols);
    }

    // Build name → id index for relationship targets
    let name_to_ids: HashMap<&str, Vec<&str>> = {
        let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
        for sym in &symbols {
            map.entry(sym.name.as_str()).or_default().push(sym.id.as_str());
        }
        map
    };

    // Build edges from relationships
    let mut edge_set: HashSet<(String, String, EdgeKind)> = HashSet::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        if let Some(ref sl) = scope_lower {
            if !file_path.to_lowercase().contains(sl.as_str()) {
                continue;
            }
        }
        collect_relationship_edges(
            &file.declarations,
            &file_path,
            &name_to_ids,
            &mut edge_set,
        );
    }

    // Build edges from signature references (word-boundary matching)
    // Only check type/struct/class/interface/trait names in signatures
    let type_names: Vec<(&str, &str)> = symbols
        .iter()
        .filter(|s| {
            let sig_lower = s.signature.to_lowercase();
            sig_lower.starts_with("struct ")
                || sig_lower.starts_with("class ")
                || sig_lower.starts_with("trait ")
                || sig_lower.starts_with("interface ")
                || sig_lower.starts_with("type ")
                || sig_lower.starts_with("enum ")
        })
        .map(|s| (s.name.as_str(), s.id.as_str()))
        .collect();

    for sym in &symbols {
        for &(type_name, type_id) in &type_names {
            if sym.id == type_id || type_name.len() < 3 {
                continue;
            }
            if contains_word_boundary(&sym.signature, type_name) {
                edge_set.insert((
                    sym.id.clone(),
                    type_id.to_string(),
                    EdgeKind::Imports, // "references" — using Imports as a general edge
                ));
            }
        }
    }

    // Apply depth limiting
    let mut edges_vec: Vec<GraphEdge> = edge_set
        .into_iter()
        .map(|(from, to, kind)| GraphEdge { from, to, kind })
        .collect();

    if let Some(max_depth) = depth {
        let seed_ids: HashSet<&str> = symbols.iter().map(|s| s.id.as_str()).collect();
        edges_vec = limit_depth_symbol(edges_vec, &seed_ids, max_depth);
    }

    edges_vec.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));

    // Collect participating nodes
    let mut node_set: HashSet<String> = HashSet::new();
    for e in &edges_vec {
        node_set.insert(e.from.clone());
        node_set.insert(e.to.clone());
    }

    let sym_map: HashMap<&str, &SymInfo> = symbols.iter().map(|s| (s.id.as_str(), s)).collect();

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
    #[allow(dead_code)]
    file: String,
    signature: String,
}

fn collect_symbols_ext(decls: &[Declaration], file_path: &str, out: &mut Vec<SymInfo>) {
    for decl in decls {
        if decl.name.is_empty() {
            continue;
        }
        out.push(SymInfo {
            id: format!("{}::{}", file_path, decl.name),
            name: decl.name.clone(),
            file: file_path.to_string(),
            signature: decl.signature.clone(),
        });
        collect_symbols_ext(&decl.children, file_path, out);
    }
}

fn collect_relationship_edges(
    decls: &[Declaration],
    file_path: &str,
    name_to_ids: &HashMap<&str, Vec<&str>>,
    edge_set: &mut HashSet<(String, String, EdgeKind)>,
) {
    for decl in decls {
        let from_id = format!("{}::{}", file_path, decl.name);
        for rel in &decl.relationships {
            let edge_kind = match rel.kind {
                RelKind::Extends => EdgeKind::Extends,
                RelKind::Implements => EdgeKind::Implements,
            };
            if let Some(target_ids) = name_to_ids.get(rel.target.as_str()) {
                for &tid in target_ids {
                    if tid != from_id {
                        edge_set.insert((from_id.clone(), tid.to_string(), edge_kind));
                    }
                }
            }
        }
        collect_relationship_edges(&decl.children, file_path, name_to_ids, edge_set);
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
    let mut out = String::new();
    out.push_str("digraph dependencies {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str("  node [shape=box, style=filled, fillcolor=\"#f0f0f0\"];\n");
    out.push('\n');

    for edge in &graph.edges {
        let label = match edge.kind {
            EdgeKind::Imports => "",
            EdgeKind::Extends => "extends",
            EdgeKind::Implements => "implements",
        };
        if label.is_empty() {
            out.push_str(&format!("  \"{}\" -> \"{}\";\n", edge.from, edge.to));
        } else {
            out.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                edge.from, edge.to, label
            ));
        }
    }

    out.push_str("}\n");
    out
}

/// Format graph as Mermaid string.
pub fn format_mermaid(graph: &DepGraph) -> String {
    let mut out = String::new();
    out.push_str("graph LR\n");

    // Build id → sanitized-id map for Mermaid (no special chars in ids)
    let sanitize = |s: &str| -> String {
        s.chars()
            .map(|c| match c {
                '/' | '.' | ':' | '-' | ' ' => '_',
                c if c.is_alphanumeric() || c == '_' => c,
                _ => '_',
            })
            .collect()
    };

    for edge in &graph.edges {
        let from_san = sanitize(&edge.from);
        let to_san = sanitize(&edge.to);
        let arrow = match edge.kind {
            EdgeKind::Imports => "-->",
            EdgeKind::Extends => "-->|extends|",
            EdgeKind::Implements => "-->|implements|",
        };
        out.push_str(&format!(
            "  {}[\"{}\"] {} {}[\"{}\"]\n",
            from_san, edge.from, arrow, to_san, edge.to
        ));
    }

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
    use crate::model::declarations::{Declaration, DeclKind, Relationship, RelKind, Visibility};
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

    // --- Import resolution tests ---

    #[test]
    fn test_resolve_rust_crate_import() {
        let paths: Vec<&Path> = vec![
            Path::new("src/parser/mod.rs"),
            Path::new("src/model/mod.rs"),
            Path::new("src/main.rs"),
        ];
        let from = Path::new("src/main.rs");

        let result = resolve_import("crate::parser", from, &paths);
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
        let from = Path::new("src/main.rs");

        let result = resolve_import("crate::parser::queries::rust", from, &paths);
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
        let from = Path::new("src/components/app.ts");

        let result = resolve_import("{ foo } from '../utils/helper'", from, &paths);
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
        let from = Path::new("src/utils/main.ts");

        let result = resolve_import("{ foo } from './helper'", from, &paths);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("src/utils/helper.ts".to_string())
        );
    }

    #[test]
    fn test_resolve_python_import() {
        let paths: Vec<&Path> = vec![
            Path::new("app/models.py"),
            Path::new("app/views.py"),
        ];
        let from = Path::new("app/views.py");

        let result = resolve_import("app.models", from, &paths);
        assert_eq!(
            result.map(|p| p.to_string_lossy().to_string()),
            Some("app/models.py".to_string())
        );
    }

    #[test]
    fn test_no_resolve_external_import() {
        let paths: Vec<&Path> = vec![
            Path::new("src/main.rs"),
            Path::new("src/lib.rs"),
        ];
        let from = Path::new("src/main.rs");

        let result = resolve_import("std::collections::HashMap", from, &paths);
        assert!(result.is_none());
    }

    #[test]
    fn test_no_resolve_short_stem() {
        let paths: Vec<&Path> = vec![
            Path::new("src/io.rs"),
            Path::new("src/main.rs"),
        ];
        let from = Path::new("src/main.rs");

        // "io" is only 2 chars — should not produce false matches
        let result = resolve_import("io", from, &paths);
        assert!(result.is_none());
    }

    #[test]
    fn test_no_self_import() {
        let paths: Vec<&Path> = vec![Path::new("src/parser/mod.rs")];
        let from = Path::new("src/parser/mod.rs");

        let result = resolve_import("crate::parser", from, &paths);
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
        let index = make_index(vec![
            make_file("src/main.rs", vec!["std::collections::HashMap"], vec![]),
        ]);

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
        assert!(mermaid.contains("graph LR"));
        assert!(mermaid.contains("-->"));
        assert!(mermaid.contains("a_rs"));
        assert!(mermaid.contains("b_rs"));
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
}
