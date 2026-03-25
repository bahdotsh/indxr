use std::collections::{HashMap, HashSet};
use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};
use serde::Serialize;
use serde_json::{Value, json};

use crate::model::declarations::{DeclKind, Declaration, Visibility};
use crate::model::{CodebaseIndex, FileIndex};

// ---------------------------------------------------------------------------
// Tool response helpers
// ---------------------------------------------------------------------------

pub(super) fn tool_result(content: Value) -> Value {
    // Use compact JSON instead of pretty-printed to save tokens
    json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string(&content).unwrap_or_default()
            }
        ]
    })
}

pub(super) fn tool_error(msg: &str) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": msg
            }
        ],
        "isError": true
    })
}

// ---------------------------------------------------------------------------
// Declaration search helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct SymbolMatch {
    pub file: String,
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
}

/// Recursively walk declarations and their children, collecting any whose name
/// contains `query` (case-insensitive). Stops at `limit`.
pub(super) fn find_symbols_in_decl(
    decl: &Declaration,
    query: &str,
    file_path: &str,
    results: &mut Vec<SymbolMatch>,
    limit: usize,
) {
    if results.len() >= limit {
        return;
    }
    if decl.name.to_lowercase().contains(query) {
        // Truncate long doc comments in results to save tokens
        let doc = decl.doc_comment.as_ref().map(|d| {
            if d.len() > 120 {
                let truncated: String = d.chars().take(120).collect();
                format!("{}...", truncated.trim_end_matches('.'))
            } else {
                d.clone()
            }
        });
        results.push(SymbolMatch {
            file: file_path.to_string(),
            kind: format!("{}", decl.kind),
            name: decl.name.clone(),
            signature: decl.signature.clone(),
            line: decl.line,
            doc_comment: doc,
        });
    }
    for child in &decl.children {
        find_symbols_in_decl(child, query, file_path, results, limit);
    }
}

#[derive(Serialize)]
pub(super) struct SignatureMatch {
    pub file: String,
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub line: usize,
}

pub(super) fn find_signatures_in_decl(
    decl: &Declaration,
    query: &str,
    file_path: &str,
    results: &mut Vec<SignatureMatch>,
    limit: usize,
) {
    if results.len() >= limit {
        return;
    }
    if decl.signature.to_lowercase().contains(query) {
        results.push(SignatureMatch {
            file: file_path.to_string(),
            kind: format!("{}", decl.kind),
            name: decl.name.clone(),
            signature: decl.signature.clone(),
            line: decl.line,
        });
    }
    for child in &decl.children {
        find_signatures_in_decl(child, query, file_path, results, limit);
    }
}

pub(super) fn filter_declarations<'a>(
    decls: &'a [Declaration],
    kind: &DeclKind,
) -> Vec<&'a Declaration> {
    let mut out = Vec::new();
    for d in decls {
        if d.kind == *kind {
            out.push(d);
        }
        out.extend(filter_declarations(&d.children, kind));
    }
    out
}

/// Shallow representation of a declaration (no children, no doc_comment).
#[derive(Serialize)]
pub(super) struct ShallowDeclaration {
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_count: Option<usize>,
}

pub(super) fn to_shallow(decl: &Declaration) -> ShallowDeclaration {
    ShallowDeclaration {
        kind: format!("{}", decl.kind),
        name: decl.name.clone(),
        signature: decl.signature.clone(),
        line: decl.line,
        children_count: if decl.children.is_empty() {
            None
        } else {
            Some(decl.children.len())
        },
    }
}

// ---------------------------------------------------------------------------
// Shared helpers for per-file tools
// ---------------------------------------------------------------------------

/// Build a summary JSON value for a file (reused by get_file_summary and get_file_context).
pub(super) fn file_summary_data(file: &FileIndex) -> Value {
    let shallow_decls: Vec<ShallowDeclaration> = file.declarations.iter().map(to_shallow).collect();

    // Single-pass traversal: count by kind, public symbols, and test presence
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut public_symbols = 0usize;
    let mut has_tests = false;
    fn scan_decls(
        decls: &[Declaration],
        counts: &mut HashMap<String, usize>,
        public_symbols: &mut usize,
        has_tests: &mut bool,
    ) {
        for d in decls {
            *counts.entry(format!("{}", d.kind)).or_insert(0) += 1;
            if matches!(d.visibility, Visibility::Public) {
                *public_symbols += 1;
            }
            if d.is_test {
                *has_tests = true;
            }
            scan_decls(&d.children, counts, public_symbols, has_tests);
        }
    }
    scan_decls(
        &file.declarations,
        &mut counts,
        &mut public_symbols,
        &mut has_tests,
    );

    let import_texts: Vec<&str> = file.imports.iter().map(|i| i.text.as_str()).collect();

    json!({
        "file": file.path.to_string_lossy(),
        "language": file.language.name(),
        "size": file.size,
        "lines": file.lines,
        "imports": import_texts,
        "declarations": shallow_decls,
        "counts": counts,
        "has_tests": has_tests,
        "public_symbols": public_symbols
    })
}

/// Recursively find a declaration by name within a file's declarations.
pub(super) fn find_decl_by_name<'a>(
    decls: &'a [Declaration],
    name: &str,
) -> Option<&'a Declaration> {
    fn search<'a>(decls: &'a [Declaration], name_lower: &str) -> Option<&'a Declaration> {
        for d in decls {
            if d.name.to_lowercase() == name_lower {
                return Some(d);
            }
            if let Some(found) = search(&d.children, name_lower) {
                return Some(found);
            }
        }
        None
    }
    search(decls, &name.to_lowercase())
}

/// Read a range of lines from a file on disk. Lines are 1-based.
pub(super) fn read_line_range(path: &Path, start: usize, end: usize) -> Result<String, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    if start == 0 || start > total {
        return Err(format!(
            "start_line {} out of range (file has {} lines)",
            start, total
        ));
    }

    let end = end.min(total);
    let selected: Vec<&str> = lines[start - 1..end].to_vec();
    Ok(selected.join("\n"))
}

/// Find a FileIndex whose path matches the given string. Supports both exact
/// match and suffix match (with `/` boundary) so callers can use relative paths.
pub(super) fn find_file<'a>(index: &'a CodebaseIndex, path: &str) -> Option<&'a FileIndex> {
    index.files.iter().find(|f| {
        let file_path = f.path.to_string_lossy();
        file_path == path || file_path.ends_with(&format!("/{}", path))
    })
}

// ---------------------------------------------------------------------------
// Relevance search helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct RelevanceMatch {
    pub file: String,
    pub symbol: Option<String>,
    pub kind: Option<String>,
    pub signature: Option<String>,
    pub line: Option<usize>,
    pub match_on: String,
    pub score: u32,
}

pub(super) fn score_match(text: &str, query: &str, terms: &[&str]) -> u32 {
    let mut score = 0u32;

    // Exact substring match
    if text.contains(query) {
        score += 10;
        // Bonus for exact match (not just substring)
        if text == query {
            score += 20;
        }
    }

    // Individual term matches
    for term in terms {
        if text.contains(term) {
            score += 5;
        }
    }

    // Identifier-part matching (camelCase/snake_case aware)
    let parts = split_identifier(text);
    for term in terms {
        if parts.iter().any(|p| p == *term) {
            score += 3; // word-boundary match bonus
        }
    }

    // Bigram fuzzy matching as fallback for partial matches
    if score == 0 && query.len() >= 4 {
        let sim = bigram_similarity(text, query);
        if sim > 0.4 {
            score += (sim * 8.0) as u32;
        }
    }

    score
}

pub(super) fn score_decls_recursive(
    decls: &[Declaration],
    file_path: &str,
    query: &str,
    terms: &[&str],
    results: &mut Vec<RelevanceMatch>,
    kind_filter: Option<&DeclKind>,
) {
    for decl in decls {
        // Apply kind filter — skip non-matching declarations but still recurse children
        let kind_matches = kind_filter.is_none_or(|k| decl.kind == *k);

        if kind_matches {
            let name_lower = decl.name.to_lowercase();
            let sig_lower = decl.signature.to_lowercase();
            let doc_lower = decl
                .doc_comment
                .as_ref()
                .map(|d| d.to_lowercase())
                .unwrap_or_default();

            let mut score = 0u32;
            let mut match_sources = Vec::new();

            // Name match (highest signal)
            let name_score = score_match(&name_lower, query, terms);
            if name_score > 0 {
                score += name_score * 3; // name matches are 3x more valuable
                match_sources.push("name");
            }

            // Signature match
            let sig_score = score_match(&sig_lower, query, terms);
            if sig_score > 0 {
                score += sig_score * 2;
                match_sources.push("signature");
            }

            // Doc comment match
            if !doc_lower.is_empty() {
                let doc_score = score_match(&doc_lower, query, terms);
                if doc_score > 0 {
                    score += doc_score;
                    match_sources.push("doc");
                }
            }

            // Boost public symbols
            if matches!(decl.visibility, Visibility::Public) && score > 0 {
                score += 5;
            }

            if score > 0 {
                results.push(RelevanceMatch {
                    file: file_path.to_string(),
                    symbol: Some(decl.name.clone()),
                    kind: Some(format!("{}", decl.kind)),
                    signature: Some(decl.signature.clone()),
                    line: Some(decl.line),
                    match_on: match_sources.join("+"),
                    score,
                });
            }
        }

        score_decls_recursive(&decl.children, file_path, query, terms, results, kind_filter);
    }
}

// ---------------------------------------------------------------------------
// Glob helpers
// ---------------------------------------------------------------------------

/// Compile a glob pattern into a reusable matcher, or `None` if the pattern
/// has no glob metacharacters (callers should fall back to exact/prefix matching).
/// Patterns without `/` (e.g., `*.rs`) are treated as recursive (`**/*.rs`).
pub(super) fn compile_glob_matcher(pattern: &str) -> Option<GlobMatcher> {
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return None;
    }

    let effective = if !pattern.contains('/') {
        format!("**/{}", pattern)
    } else {
        pattern.to_string()
    };

    GlobBuilder::new(&effective)
        .literal_separator(true)
        .build()
        .ok()
        .map(|g| g.compile_matcher())
}

/// Glob matching against a path string using the `globset` crate.
/// Falls back to exact/directory-prefix matching if the pattern has no glob chars.
/// Patterns without `/` (e.g., `*.rs`) are treated as recursive (`**/*.rs`).
///
/// For hot loops (matching many paths against the same pattern), prefer
/// [`compile_glob_matcher`] to compile once and reuse.
#[allow(dead_code)] // used in tests
pub(super) fn simple_glob_match(pattern: &str, path: &str) -> bool {
    // If no glob metacharacters, treat as exact or directory prefix match
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return path == pattern || path.starts_with(&format!("{}/", pattern));
    }

    match compile_glob_matcher(pattern) {
        Some(matcher) => matcher.is_match(path),
        None => path == pattern || path.starts_with(&format!("{}/", pattern)),
    }
}

// ---------------------------------------------------------------------------
// String/identifier helpers
// ---------------------------------------------------------------------------

/// Split an identifier into constituent words.
/// Handles snake_case, camelCase, PascalCase, and SCREAMING_SNAKE_CASE.
pub(super) fn split_identifier(name: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();

    for ch in name.chars() {
        if ch == '_' || ch == '-' || ch == '.' || ch == '/' {
            if !current.is_empty() {
                parts.push(current.to_lowercase());
                current.clear();
            }
        } else if ch.is_uppercase()
            && !current.is_empty()
            && current
                .as_bytes()
                .last()
                .is_some_and(|&b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            // camelCase boundary (lowercase→uppercase) or digit→uppercase (e.g. "v2Parser")
            parts.push(current.to_lowercase());
            current.clear();
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        parts.push(current.to_lowercase());
    }
    parts
}

/// Bigram (Dice coefficient) similarity between two strings.
/// Uses set-based intersection to avoid inflating scores for repeated character pairs.
pub(super) fn bigram_similarity(a: &str, b: &str) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let bigrams_a: HashSet<(char, char)> = a.chars().zip(a.chars().skip(1)).collect();
    let bigrams_b: HashSet<(char, char)> = b.chars().zip(b.chars().skip(1)).collect();
    let intersection = bigrams_a.intersection(&bigrams_b).count();
    (2.0 * intersection as f64) / (bigrams_a.len() + bigrams_b.len()) as f64
}

/// Collapse nested block bodies (depth >= 2) to `{ ... }`.
///
/// State machine with these modes:
///   - Normal: track brace depth, emit chars. At depth >= 2 on `{`, emit `{ ... }` and
///     enter Skip mode until the matching `}` is found.
///   - Skip (skip_until_close): consume chars without emitting, tracking depth to find
///     the matching close brace.
///   - LineComment: pass through until `\n`.
///   - BlockComment: pass through until `*/`.
///   - String: pass through until unescaped closing quote (tracks escape state properly
///     so `"\\\\"` is handled as two escaped backslashes followed by an end-quote).
///   - RawString: pass through until `"` followed by the same number of `#` chars that
///     opened the raw string (e.g., `r#"..."#`, `r##"..."##`).
pub(super) fn collapse_nested_bodies(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(source.len());
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false; // tracks backslash escaping inside strings
    let mut in_raw_string = false;
    let mut raw_hash_count: usize = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut prev_char = '\0';
    let mut skip_until_close = false;
    let mut collapse_depth: i32 = 0;
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // --- Line comment mode: pass through until newline ---
        if in_line_comment {
            if !skip_until_close {
                result.push(ch);
            }
            if ch == '\n' {
                in_line_comment = false;
            }
            prev_char = ch;
            i += 1;
            continue;
        }

        // --- Block comment mode: pass through until */ ---
        if in_block_comment {
            if !skip_until_close {
                result.push(ch);
            }
            if prev_char == '*' && ch == '/' {
                in_block_comment = false;
            }
            prev_char = ch;
            i += 1;
            continue;
        }

        // --- Raw string mode: pass through until `"` + N `#` chars ---
        if in_raw_string {
            if !skip_until_close {
                result.push(ch);
            }
            if ch == '"' {
                // Check if followed by raw_hash_count '#' chars
                let mut hashes = 0;
                while i + 1 + hashes < len && chars[i + 1 + hashes] == '#' && hashes < raw_hash_count
                {
                    hashes += 1;
                }
                if hashes == raw_hash_count {
                    // Consume the '#' chars
                    for _ in 0..hashes {
                        i += 1;
                        if !skip_until_close {
                            result.push(chars[i]);
                        }
                    }
                    in_raw_string = false;
                }
            }
            prev_char = chars[i];
            i += 1;
            continue;
        }

        // --- String mode: pass through until unescaped closing quote ---
        if in_string {
            if !skip_until_close {
                result.push(ch);
            }
            if ch == '"' && !escaped {
                in_string = false;
            }
            // Track escape state: `\` flips it on, `\\` flips it back off
            escaped = ch == '\\' && !escaped;
            prev_char = ch;
            i += 1;
            continue;
        }

        // --- Normal mode: detect comment/string starts, track braces ---

        // Detect line comment start: //
        if prev_char == '/' && ch == '/' {
            in_line_comment = true;
            if !skip_until_close {
                result.push(ch);
            }
            prev_char = ch;
            i += 1;
            continue;
        }
        // Detect block comment start: /*
        if prev_char == '/' && ch == '*' {
            in_block_comment = true;
            if !skip_until_close {
                result.push(ch);
            }
            prev_char = ch;
            i += 1;
            continue;
        }
        // Detect raw string start: r"...", r#"..."#, r##"..."##, etc.
        if ch == 'r' {
            let mut hashes = 0;
            while i + 1 + hashes < len && chars[i + 1 + hashes] == '#' {
                hashes += 1;
            }
            if i + 1 + hashes < len && chars[i + 1 + hashes] == '"' {
                // This is a raw string: consume r, #s, and opening "
                in_raw_string = true;
                raw_hash_count = hashes;
                if !skip_until_close {
                    result.push(ch); // 'r'
                }
                for j in 0..hashes {
                    if !skip_until_close {
                        result.push(chars[i + 1 + j]); // '#'
                    }
                }
                if !skip_until_close {
                    result.push(chars[i + 1 + hashes]); // '"'
                }
                prev_char = '"';
                i += 1 + hashes + 1; // skip r + hashes + "
                continue;
            }
        }
        // Detect double-quoted string start.
        // Single quotes are NOT treated as string delimiters because in Rust
        // (and Go, etc.) 'a is a lifetime, not a string. Char literals like 'x'
        // rarely contain braces, so ignoring them is safe for brace-depth tracking.
        if ch == '"' {
            in_string = true;
            escaped = false;
            if !skip_until_close {
                result.push(ch);
            }
            prev_char = ch;
            i += 1;
            continue;
        }

        if ch == '{' {
            depth += 1;
            if depth >= 2 && !skip_until_close {
                result.push_str("{ ... }");
                skip_until_close = true;
                collapse_depth = depth;
            } else if !skip_until_close {
                result.push(ch);
            }
        } else if ch == '}' {
            if skip_until_close && depth == collapse_depth {
                skip_until_close = false;
            } else if !skip_until_close {
                result.push(ch);
            }
            depth -= 1;
        } else if !skip_until_close {
            result.push(ch);
        }

        prev_char = ch;
        i += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Compact output helpers
// ---------------------------------------------------------------------------

/// Check if the caller requested compact columnar output.
pub(super) fn is_compact(args: &Value) -> bool {
    args.get("compact").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Serialize a slice of Serialize items into compact columnar format.
pub(super) fn serialize_compact<T: Serialize>(items: &[T], columns: &[&str]) -> Value {
    let values: Vec<Value> = items
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .collect();
    to_compact_rows(columns, &values)
}

/// Convert an array of objects to compact columnar format.
pub(super) fn to_compact_rows(columns: &[&str], items: &[Value]) -> Value {
    let rows: Vec<Value> = items
        .iter()
        .map(|item| {
            let row: Vec<Value> = columns
                .iter()
                .map(|col| item.get(col).cloned().unwrap_or(Value::Null))
                .collect();
            Value::Array(row)
        })
        .collect();
    json!({
        "columns": columns,
        "rows": rows
    })
}

/// Check if `text` contains `word` at a word boundary (not part of a larger identifier).
/// Word boundaries are non-alphanumeric, non-underscore characters or string edges.
pub(super) fn contains_word_boundary(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let text_bytes = text.as_bytes();
    let word_len = word.len();
    let mut start = 0;
    while start + word_len <= text.len() {
        match text[start..].find(word) {
            Some(pos) => {
                let abs_pos = start + pos;
                let before_ok = abs_pos == 0 || {
                    let b = text_bytes[abs_pos - 1];
                    !b.is_ascii_alphanumeric() && b != b'_'
                };
                let after_pos = abs_pos + word_len;
                let after_ok = after_pos >= text.len() || {
                    let b = text_bytes[after_pos];
                    !b.is_ascii_alphanumeric() && b != b'_'
                };
                if before_ok && after_ok {
                    return true;
                }
                start = abs_pos + 1;
            }
            None => break,
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Declaration metadata helpers
// ---------------------------------------------------------------------------

/// Collect public declarations recursively.
pub(super) fn collect_public_decls(decls: &[Declaration], file_path: &str, out: &mut Vec<Value>) {
    for decl in decls {
        if matches!(decl.visibility, Visibility::Public) {
            out.push(json!({
                "name": decl.name,
                "kind": format!("{}", decl.kind),
                "signature": decl.signature,
                "file": file_path,
                "line": decl.line
            }));
        }
        // Also check children (public methods in impls, etc.)
        collect_public_decls(&decl.children, file_path, out);
    }
}

/// Find test declarations matching a symbol name.
pub(super) fn find_tests_for_symbol(
    decls: &[Declaration],
    symbol_lower: &str,
    file_path: &str,
    results: &mut Vec<Value>,
    reason: &str,
) {
    for decl in decls {
        if decl.is_test {
            let name_lower = decl.name.to_lowercase();
            if name_lower.contains(symbol_lower) {
                results.push(json!({
                    "name": decl.name,
                    "file": file_path,
                    "line": decl.line,
                    "kind": format!("{}", decl.kind),
                    "match_reason": reason
                }));
            }
        }
        find_tests_for_symbol(&decl.children, symbol_lower, file_path, results, reason);
    }
}

/// Explain a single declaration — full metadata without body.
pub(super) fn explain_decl(decl: &Declaration, file_path: &str) -> Value {
    let mut children_counts: HashMap<String, usize> = HashMap::new();
    for child in &decl.children {
        *children_counts
            .entry(format!("{}", child.kind))
            .or_insert(0) += 1;
    }
    let children_summary = if children_counts.is_empty() {
        None
    } else {
        let parts: Vec<String> = children_counts
            .iter()
            .map(|(k, v)| format!("{} {}", v, k))
            .collect();
        Some(parts.join(", "))
    };

    let rels: Vec<Value> = decl
        .relationships
        .iter()
        .map(|r| json!({"kind": format!("{:?}", r.kind), "target": &r.target}))
        .collect();

    let mut result = json!({
        "name": decl.name,
        "kind": format!("{}", decl.kind),
        "file": file_path,
        "line": decl.line,
        "signature": decl.signature,
        "visibility": format!("{}", decl.visibility),
        "is_async": decl.is_async,
        "is_test": decl.is_test,
        "is_deprecated": decl.is_deprecated,
    });
    if let Some(doc) = &decl.doc_comment {
        result["doc_comment"] = json!(doc);
    }
    if !rels.is_empty() {
        result["relationships"] = json!(rels);
    }
    if let Some(summary) = children_summary {
        result["children_summary"] = json!(summary);
    }
    if let Some(body) = decl.body_lines {
        result["body_lines"] = json!(body);
    }
    result
}

/// Approximate token cost of a `get_file_summary` response.
pub(super) const APPROX_SUMMARY_TOKENS: usize = 300;
