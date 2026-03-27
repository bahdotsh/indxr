use std::collections::HashMap;

use serde::Serialize;

use crate::languages::Language;
use crate::model::CodebaseIndex;
use crate::model::declarations::{ComplexityMetrics, DeclKind, Declaration};

/// Annotate declarations with complexity metrics by walking the tree-sitter AST.
///
/// Computes cyclomatic complexity, max nesting depth, and parameter count
/// for each function/method declaration. Only works for tree-sitter parsed languages.
pub fn annotate_complexity(
    declarations: &mut [Declaration],
    root: tree_sitter::Node<'_>,
    source: &str,
    language: &Language,
) {
    let func_kinds = function_node_kinds(language);
    if func_kinds.is_empty() {
        return;
    }

    let mut metrics: HashMap<usize, ComplexityMetrics> = HashMap::new();
    collect_from_ast(root, source, language, func_kinds, &mut metrics);

    if !metrics.is_empty() {
        apply_metrics(declarations, &metrics);
    }
}

// ---------------------------------------------------------------------------
// AST walking
// ---------------------------------------------------------------------------

fn collect_from_ast(
    node: tree_sitter::Node<'_>,
    source: &str,
    language: &Language,
    func_kinds: &[&str],
    metrics: &mut HashMap<usize, ComplexityMetrics>,
) {
    if func_kinds.contains(&node.kind()) {
        if let Some(body) = node.child_by_field_name("body") {
            let line = node.start_position().row + 1; // 1-indexed
            let param_count = count_params(node, source, language);
            let cyclomatic = 1 + count_branches(body, language, func_kinds);
            let max_nesting = compute_max_nesting(body, language, func_kinds, 0);

            metrics.insert(
                line,
                ComplexityMetrics {
                    cyclomatic: cyclomatic as u16,
                    max_nesting: max_nesting as u16,
                    param_count: param_count as u16,
                },
            );
        }
    }

    // Always recurse to find nested functions (methods inside impl/class)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_from_ast(child, source, language, func_kinds, metrics);
        }
    }
}

fn apply_metrics(decls: &mut [Declaration], metrics: &HashMap<usize, ComplexityMetrics>) {
    for decl in decls.iter_mut() {
        if is_function_kind(&decl.kind) {
            if let Some(m) = metrics.get(&decl.line) {
                decl.complexity = Some(m.clone());
            }
        }
        apply_metrics(&mut decl.children, metrics);
    }
}

// ---------------------------------------------------------------------------
// Parameter counting
// ---------------------------------------------------------------------------

fn count_params(func: tree_sitter::Node<'_>, source: &str, language: &Language) -> usize {
    let params_node = match get_params_node(func, language) {
        Some(n) => n,
        None => return 0,
    };

    let mut count = 0;
    for i in 0..params_node.named_child_count() {
        if let Some(child) = params_node.named_child(i) {
            // Skip Python self/cls
            if matches!(language, Language::Python) {
                let text = &source[child.byte_range()];
                let name = text.split(':').next().unwrap_or(text).trim();
                if name == "self" || name == "cls" {
                    continue;
                }
            }
            count += 1;
        }
    }
    count
}

fn get_params_node<'a>(
    func: tree_sitter::Node<'a>,
    language: &Language,
) -> Option<tree_sitter::Node<'a>> {
    // Most languages: parameters field directly on the function node
    if let Some(params) = func.child_by_field_name("parameters") {
        return Some(params);
    }
    // C/C++: parameters live on the function_declarator inside the declarator chain
    if matches!(language, Language::C | Language::Cpp) {
        if let Some(decl) = func.child_by_field_name("declarator") {
            return find_params_in_declarator(decl);
        }
    }
    None
}

fn find_params_in_declarator(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if let Some(params) = node.child_by_field_name("parameters") {
        return Some(params);
    }
    // Traverse pointer_declarator, reference_declarator, etc.
    if let Some(inner) = node.child_by_field_name("declarator") {
        return find_params_in_declarator(inner);
    }
    None
}

// ---------------------------------------------------------------------------
// Cyclomatic complexity
// ---------------------------------------------------------------------------

fn count_branches(node: tree_sitter::Node<'_>, language: &Language, func_kinds: &[&str]) -> usize {
    let mut count = 0;
    let kind = node.kind();

    // Check if this node is a branch point
    if branch_node_kinds(language).contains(&kind) {
        count += 1;
    } else if kind == "binary_expression" && !matches!(language, Language::Python) {
        // Python uses boolean_operator instead of binary_expression for and/or
        if is_logical_binary(node, language) {
            count += 1;
        }
    }

    // Recurse into children, stopping at nested function boundaries
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if !func_kinds.contains(&child.kind()) {
                count += count_branches(child, language, func_kinds);
            }
        }
    }

    count
}

fn is_logical_binary(node: tree_sitter::Node<'_>, language: &Language) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            let op = child.kind();
            if op == "&&" || op == "||" {
                return true;
            }
            if matches!(language, Language::JavaScript | Language::TypeScript) && op == "??" {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Max nesting depth
// ---------------------------------------------------------------------------

fn compute_max_nesting(
    node: tree_sitter::Node<'_>,
    language: &Language,
    func_kinds: &[&str],
    depth: usize,
) -> usize {
    let kind = node.kind();
    let new_depth = if nesting_node_kinds(language).contains(&kind) {
        depth + 1
    } else {
        depth
    };

    let mut max = new_depth;
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if !func_kinds.contains(&child.kind()) {
                max = max.max(compute_max_nesting(child, language, func_kinds, new_depth));
            }
        }
    }
    max
}

// ---------------------------------------------------------------------------
// Language-specific node kind tables
// ---------------------------------------------------------------------------

fn function_node_kinds(language: &Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &["function_item"],
        Language::Python => &["function_definition"],
        Language::TypeScript => &["function_declaration", "method_definition"],
        Language::JavaScript => &["function_declaration", "method_definition"],
        Language::Go => &["function_declaration", "method_declaration"],
        Language::Java => &["method_declaration", "constructor_declaration"],
        Language::C => &["function_definition"],
        Language::Cpp => &["function_definition"],
        _ => &[],
    }
}

fn branch_node_kinds(language: &Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &[
            "if_expression",
            "while_expression",
            "for_expression",
            "loop_expression",
            "match_arm",
        ],
        Language::Python => &[
            "if_statement",
            "elif_clause",
            "while_statement",
            "for_statement",
            "except_clause",
            "conditional_expression",
            "boolean_operator",
        ],
        Language::TypeScript | Language::JavaScript => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "for_in_statement",
            "do_statement",
            "switch_case",
            "catch_clause",
            "ternary_expression",
        ],
        Language::Go => &[
            "if_statement",
            "for_statement",
            "expression_case",
            "type_case",
            "communication_case",
        ],
        Language::Java => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "enhanced_for_statement",
            "do_statement",
            "switch_block_statement_group",
            "catch_clause",
            "ternary_expression",
        ],
        Language::C => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "do_statement",
            "case_statement",
            "conditional_expression",
        ],
        Language::Cpp => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "do_statement",
            "case_statement",
            "conditional_expression",
            "catch_clause",
            "for_range_loop",
        ],
        _ => &[],
    }
}

fn nesting_node_kinds(language: &Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &[
            "if_expression",
            "while_expression",
            "for_expression",
            "loop_expression",
            "match_expression",
        ],
        Language::Python => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "with_statement",
            "try_statement",
        ],
        Language::TypeScript | Language::JavaScript => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "for_in_statement",
            "do_statement",
            "switch_statement",
            "try_statement",
        ],
        Language::Go => &[
            "if_statement",
            "for_statement",
            "select_statement",
            "type_switch_statement",
            "expression_switch_statement",
        ],
        Language::Java => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "enhanced_for_statement",
            "do_statement",
            "switch_expression",
            "try_statement",
        ],
        Language::C => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "do_statement",
            "switch_statement",
        ],
        Language::Cpp => &[
            "if_statement",
            "while_statement",
            "for_statement",
            "do_statement",
            "switch_statement",
            "try_statement",
            "for_range_loop",
        ],
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_function_kind(kind: &DeclKind) -> bool {
    matches!(kind, DeclKind::Function | DeclKind::Method)
}

// ---------------------------------------------------------------------------
// Hotspot scoring and collection
// ---------------------------------------------------------------------------

/// Entry representing a function/method with complexity data, ranked by score.
#[derive(Serialize)]
pub struct HotspotEntry {
    pub file: String,
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub cyclomatic: u16,
    pub max_nesting: u16,
    pub param_count: u16,
    pub body_lines: usize,
    pub score: f64,
}

/// Composite hotspot score combining multiple complexity signals.
///
/// Weights:
/// - cyclomatic: 1x (primary signal)
/// - nesting: 2x (deep nesting is disproportionately hard to reason about)
/// - params: 0.5x (more params = harder interface, but less than structural complexity)
/// - body_lines: 0.05x (longer = harder, but secondary to structural metrics)
pub fn hotspot_score(
    cyclomatic: u16,
    max_nesting: u16,
    param_count: u16,
    body_lines: usize,
) -> f64 {
    cyclomatic as f64
        + max_nesting as f64 * 2.0
        + param_count as f64 * 0.5
        + body_lines as f64 / 20.0
}

/// Collect hotspot entries from an index, optionally filtered by path and minimum complexity.
pub fn collect_hotspots(
    index: &CodebaseIndex,
    path_filter: Option<&str>,
    min_complexity: u16,
) -> Vec<HotspotEntry> {
    let mut entries = Vec::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy();
        if let Some(filter) = path_filter {
            if !file_path.contains(filter) && !file_path.ends_with(filter) {
                continue;
            }
        }
        collect_hotspots_from_decls(&file_path, &file.declarations, min_complexity, &mut entries);
    }

    entries
}

fn collect_hotspots_from_decls(
    file_path: &str,
    decls: &[Declaration],
    min_complexity: u16,
    entries: &mut Vec<HotspotEntry>,
) {
    for decl in decls {
        if let Some(ref cm) = decl.complexity {
            if cm.cyclomatic >= min_complexity {
                let body_lines = decl.body_lines.unwrap_or(0);
                entries.push(HotspotEntry {
                    file: file_path.to_string(),
                    name: decl.name.clone(),
                    kind: decl.kind.to_string(),
                    line: decl.line,
                    cyclomatic: cm.cyclomatic,
                    max_nesting: cm.max_nesting,
                    param_count: cm.param_count,
                    body_lines,
                    score: hotspot_score(cm.cyclomatic, cm.max_nesting, cm.param_count, body_lines),
                });
            }
        }
        collect_hotspots_from_decls(file_path, &decl.children, min_complexity, entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_annotate(source: &str, language: Language) -> Vec<Declaration> {
        let ts_lang: tree_sitter::Language = match language {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            _ => panic!("Unsupported language for test"),
        };

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&ts_lang).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let root = tree.root_node();

        let extractor = crate::parser::queries::get_extractor(&language);
        let (_, mut declarations) = extractor.extract(root, source);
        annotate_complexity(&mut declarations, root, source, &language);

        // Also re-parse to verify (tree is still alive since we own it)
        declarations
    }

    fn get_complexity<'a>(decls: &'a [Declaration], name: &str) -> Option<&'a ComplexityMetrics> {
        for d in decls {
            if d.name == name {
                return d.complexity.as_ref();
            }
            if let Some(m) = get_complexity(&d.children, name) {
                return Some(m);
            }
        }
        None
    }

    #[test]
    fn rust_simple_function() {
        let src = r#"
fn hello(x: i32, y: String) -> bool {
    x > 0
}
"#;
        let decls = parse_and_annotate(src, Language::Rust);
        let c = get_complexity(&decls, "hello").expect("should have complexity");
        assert_eq!(c.param_count, 2);
        assert_eq!(c.cyclomatic, 1); // no branches
        assert_eq!(c.max_nesting, 0);
    }

    #[test]
    fn rust_branchy_function() {
        let src = r#"
fn check(x: i32) -> bool {
    if x > 0 {
        for i in 0..x {
            if i % 2 == 0 {
                return true;
            }
        }
    } else if x < -10 {
        return false;
    }
    x == 0 && x != 1
}
"#;
        let decls = parse_and_annotate(src, Language::Rust);
        let c = get_complexity(&decls, "check").expect("should have complexity");
        assert_eq!(c.param_count, 1);
        // if + for + if + else-if(if_expression) + && = 5, base 1 = 6
        assert!(c.cyclomatic >= 5, "cyclomatic={}", c.cyclomatic);
        assert!(c.max_nesting >= 3, "nesting={}", c.max_nesting); // if > for > if
    }

    #[test]
    fn rust_match_arms() {
        let src = r#"
fn classify(x: i32) -> &'static str {
    match x {
        0 => "zero",
        1..=9 => "single digit",
        _ => "large",
    }
}
"#;
        let decls = parse_and_annotate(src, Language::Rust);
        let c = get_complexity(&decls, "classify").expect("should have complexity");
        assert_eq!(c.param_count, 1);
        // 3 match arms = 3, base 1 = 4
        assert_eq!(c.cyclomatic, 4);
        assert!(c.max_nesting >= 1); // match_expression
    }

    #[test]
    fn python_with_self() {
        let src = r#"
class Foo:
    def bar(self, x, y):
        if x > 0:
            return y
        return x
"#;
        let decls = parse_and_annotate(src, Language::Python);
        let c = get_complexity(&decls, "bar").expect("should have complexity");
        assert_eq!(c.param_count, 2); // self excluded
        assert_eq!(c.cyclomatic, 2); // 1 base + 1 if
    }

    #[test]
    fn go_function() {
        let src = r#"
package main

func process(items []string, limit int) error {
    for _, item := range items {
        if len(item) > limit {
            return fmt.Errorf("too long")
        }
    }
    return nil
}
"#;
        let decls = parse_and_annotate(src, Language::Go);
        let c = get_complexity(&decls, "process").expect("should have complexity");
        assert_eq!(c.param_count, 2);
        assert_eq!(c.cyclomatic, 3); // 1 + for + if
        assert_eq!(c.max_nesting, 2); // for > if
    }

    #[test]
    fn c_function() {
        let src = r#"
int factorial(int n) {
    if (n <= 1) {
        return 1;
    }
    return n * factorial(n - 1);
}
"#;
        let decls = parse_and_annotate(src, Language::C);
        let c = get_complexity(&decls, "factorial").expect("should have complexity");
        assert_eq!(c.param_count, 1);
        assert_eq!(c.cyclomatic, 2); // 1 + if
        assert_eq!(c.max_nesting, 1);
    }

    #[test]
    fn no_complexity_for_bodyless() {
        let src = r#"
trait Foo {
    fn bar(&self);
}
"#;
        // Parse manually - trait method signatures don't have bodies
        let ts_lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&ts_lang).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let root = tree.root_node();
        let extractor = crate::parser::queries::get_extractor(&Language::Rust);
        let (_, mut decls) = extractor.extract(root, src);
        annotate_complexity(&mut decls, root, src, &Language::Rust);

        // Trait method signatures should not have complexity
        let bar = get_complexity(&decls, "bar");
        assert!(
            bar.is_none(),
            "bodyless declarations should not have complexity"
        );
    }
}
