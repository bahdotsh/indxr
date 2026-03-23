use crate::model::CodebaseIndex;
use crate::model::declarations::{Declaration, Visibility};

/// Estimate the number of tokens in a string.
/// Uses the approximation: 1 token ~ 4 characters for English/code text.
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Estimate the total token count of a CodebaseIndex when rendered as markdown.
fn estimate_index_tokens(index: &CodebaseIndex) -> usize {
    let mut tokens = 0usize;

    // Header: "# Codebase Index: {name}\n\n> Generated: ... | Files: ... | Lines: ...\n> Languages: ...\n"
    tokens += 20 + estimate_tokens(&index.root_name) + 30;

    // Tree section header + code fence
    tokens += 10; // "## Directory Structure\n\n```\n{root}/\n```\n"

    // Tree entries: actual path length + indentation
    for entry in &index.tree {
        // "  ".repeat(depth) + path + optional "/" + "\n"
        tokens += estimate_tokens(&entry.path) + entry.depth + 1;
    }

    // Public API surface section (rough estimate)
    tokens += 15; // section header

    // File sections
    for file in &index.files {
        // File section header: "---\n\n## {path}\n\n**Language:** ... | **Size:** ... | **Lines:** ...\n"
        let path_str = file.path.to_string_lossy();
        tokens += 15 + estimate_tokens(&path_str);

        // Imports: "- `{text}`\n" per import
        for import in &file.imports {
            tokens += 3 + estimate_tokens(&import.text);
        }

        // Declarations (recursive)
        tokens += estimate_declarations_tokens(&file.declarations);
    }

    tokens
}

/// Estimate tokens for a list of declarations recursively.
fn estimate_declarations_tokens(decls: &[Declaration]) -> usize {
    let mut tokens = 0usize;
    for decl in decls {
        // Signature line: "`{signature}`\n" + possible visibility prefix + badges
        tokens += 3 + estimate_tokens(&decl.signature);

        // Doc comment: "> {doc}\n"
        if let Some(ref doc) = decl.doc_comment {
            tokens += 2 + estimate_tokens(doc);
        }

        // Line number: "> Line N (M lines)\n"
        if decl.line > 0 {
            tokens += 5;
        }

        // Relationships: "> implements `X`, extends `Y`\n"
        for rel in &decl.relationships {
            tokens += 3 + estimate_tokens(&rel.target);
        }

        // Children
        tokens += estimate_declarations_tokens(&decl.children);
    }
    tokens
}

/// Estimate tokens for a single file section.
fn estimate_file_tokens(file: &crate::model::FileIndex) -> usize {
    let path_str = file.path.to_string_lossy();
    let mut tokens = 15 + estimate_tokens(&path_str);
    for import in &file.imports {
        tokens += 3 + estimate_tokens(&import.text);
    }
    tokens += estimate_declarations_tokens(&file.declarations);
    tokens
}

/// Score a file for importance (higher = more important, keep longer).
fn file_importance(file: &crate::model::FileIndex) -> i64 {
    let mut score: i64 = 0;
    let path = file.path.to_string_lossy();
    let filename = file
        .path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    // Entry points are most important
    match filename.as_str() {
        "main.rs" | "main.py" | "main.go" | "main.ts" | "main.js" | "main.java"
        | "index.ts" | "index.js" | "index.tsx" | "index.jsx" => score += 100,
        "lib.rs" | "lib.py" => score += 90,
        "mod.rs" | "__init__.py" | "mod.ts" => score += 50,
        _ => {}
    }

    // Files closer to root are more important
    let depth = path.matches('/').count();
    score -= depth as i64 * 5;

    // More public declarations = more important
    let public_count = count_public_decls(&file.declarations);
    score += public_count as i64 * 3;

    // Fewer lines = cheaper to keep (tiebreaker)
    score -= (file.lines / 100) as i64;

    score
}

fn count_public_decls(decls: &[Declaration]) -> usize {
    let mut count = 0;
    for decl in decls {
        if matches!(decl.visibility, Visibility::Public) {
            count += 1;
        }
        count += count_public_decls(&decl.children);
    }
    count
}

/// Truncate a CodebaseIndex to fit within a token budget.
///
/// Progressive truncation strategy:
/// 1. Truncate long doc comments to first line
/// 2. Strip all remaining doc comments
/// 3. Remove private declarations
/// 4. Remove children (fields, methods)
/// 5. Drop least-important files
pub fn apply_token_budget(index: &mut CodebaseIndex, max_tokens: usize) {
    let mut current = estimate_index_tokens(index);
    if current <= max_tokens {
        return;
    }

    // Stage 1: Truncate long doc comments to first line (max 80 chars)
    for file in &mut index.files {
        current -= truncate_doc_comments(&mut file.declarations, 80);
    }
    if current <= max_tokens {
        return;
    }

    // Stage 2: Strip all remaining doc comments
    for file in &mut index.files {
        current -= strip_doc_comments(&mut file.declarations);
    }
    if current <= max_tokens {
        return;
    }

    // Stage 3: Remove private declarations
    for file in &mut index.files {
        let old_tokens = estimate_declarations_tokens(&file.declarations);
        file.declarations = remove_private_declarations(&file.declarations);
        let new_tokens = estimate_declarations_tokens(&file.declarations);
        current = current.saturating_sub(old_tokens.saturating_sub(new_tokens));
    }
    index
        .files
        .retain(|f| !f.declarations.is_empty() || !f.imports.is_empty());
    if current <= max_tokens {
        return;
    }

    // Stage 4: Remove all children from declarations
    for file in &mut index.files {
        let old_tokens = estimate_declarations_tokens(&file.declarations);
        strip_children(&mut file.declarations);
        let new_tokens = estimate_declarations_tokens(&file.declarations);
        current = current.saturating_sub(old_tokens.saturating_sub(new_tokens));
    }
    if current <= max_tokens {
        return;
    }

    // Stage 5: Drop least-important files until we fit
    // Sort by importance ascending so we pop the least important
    index.files.sort_by(|a, b| {
        file_importance(a).cmp(&file_importance(b))
    });

    while index.files.len() > 1 && current > max_tokens {
        if let Some(dropped) = index.files.first() {
            current = current.saturating_sub(estimate_file_tokens(dropped));
        }
        index.files.remove(0);
    }

    // Restore path-sorted order for output
    index.files.sort_by(|a, b| a.path.cmp(&b.path));
}

/// Truncate doc comments longer than `max_len` to their first line/sentence.
/// Returns the number of tokens saved.
fn truncate_doc_comments(decls: &mut [Declaration], max_len: usize) -> usize {
    let mut saved = 0usize;
    for decl in decls.iter_mut() {
        if let Some(ref mut doc) = decl.doc_comment {
            if doc.len() > max_len {
                let old_tokens = estimate_tokens(doc);
                // Take first line, or first sentence, whichever is shorter
                let truncated = doc
                    .split('\n')
                    .next()
                    .unwrap_or(doc)
                    .chars()
                    .take(max_len)
                    .collect::<String>();
                let new_doc = if truncated.len() < doc.len() {
                    format!("{}...", truncated.trim_end_matches('.'))
                } else {
                    truncated
                };
                let new_tokens = estimate_tokens(&new_doc);
                saved += old_tokens.saturating_sub(new_tokens);
                *doc = new_doc;
            }
        }
        saved += truncate_doc_comments(&mut decl.children, max_len);
    }
    saved
}

/// Strip all doc comments from declarations. Returns tokens saved.
fn strip_doc_comments(decls: &mut [Declaration]) -> usize {
    let mut saved = 0usize;
    for decl in decls.iter_mut() {
        if let Some(ref doc) = decl.doc_comment {
            saved += 2 + estimate_tokens(doc);
        }
        decl.doc_comment = None;
        saved += strip_doc_comments(&mut decl.children);
    }
    saved
}

/// Recursively remove private declarations, keeping public and pub(crate) ones.
fn remove_private_declarations(decls: &[Declaration]) -> Vec<Declaration> {
    let mut result = Vec::new();
    for decl in decls {
        if matches!(decl.visibility, Visibility::Private) {
            continue;
        }
        let mut filtered = decl.clone();
        filtered.children = remove_private_declarations(&decl.children);
        result.push(filtered);
    }
    result
}

/// Remove all children from declarations.
fn strip_children(decls: &mut [Declaration]) {
    for decl in decls.iter_mut() {
        decl.children.clear();
    }
}
