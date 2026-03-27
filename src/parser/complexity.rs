use std::collections::HashMap;

use serde::Serialize;

use crate::languages::Language;
use crate::model::CodebaseIndex;
use crate::model::declarations::{ComplexityMetrics, DeclKind, Declaration, Visibility};
use crate::utils::path_matches_filter;

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
            let max_nesting = compute_max_nesting(body, language, func_kinds, 0, false);

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
        if is_ts_function_kind(&decl.kind) {
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
            // Go groups parameters: `func f(a, b int)` has one parameter_declaration
            // with multiple identifier children. Count the identifiers, not the group.
            if matches!(language, Language::Go) && child.kind() == "parameter_declaration" {
                let names = (0..child.named_child_count())
                    .filter_map(|j| child.named_child(j))
                    .filter(|n| n.kind() == "identifier")
                    .count();
                // A parameter_declaration always has at least one name; fall back to 1
                // for edge cases like unnamed params in interfaces.
                count += names.max(1);
            } else {
                count += 1;
            }
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
    is_else_if: bool,
) -> usize {
    let kind = node.kind();
    let is_if = kind == "if_expression" || kind == "if_statement";
    let new_depth = if nesting_node_kinds(language).contains(&kind) && !(is_if && is_else_if) {
        depth + 1
    } else {
        depth
    };

    let mut max = new_depth;

    // Pre-compute the alternative child of this node (if it's an if node).
    // In Go, `else if` is a direct if_statement child in the "alternative" field
    // with no wrapping else_clause.
    let alt_id = if is_if {
        node.child_by_field_name("alternative").map(|n| n.id())
    } else {
        None
    };

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if !func_kinds.contains(&child.kind()) {
                let child_is_if = child.kind() == "if_expression" || child.kind() == "if_statement";
                // Detect else-if: an if node inside an else_clause/else container,
                // OR an if node that is the "alternative" field of a parent if (Go).
                let child_is_else_if = child_is_if
                    && (is_else_alternative(node) || alt_id.is_some_and(|id| id == child.id()));
                max = max.max(compute_max_nesting(
                    child,
                    language,
                    func_kinds,
                    new_depth,
                    child_is_else_if,
                ));
            }
        }
    }
    max
}

/// Returns true if `node` is an else/alternative container (else_clause, else, etc.)
fn is_else_alternative(node: tree_sitter::Node<'_>) -> bool {
    let kind = node.kind();
    kind == "else_clause" || kind == "else"
}

// ---------------------------------------------------------------------------
// Language-specific node kind tables
// ---------------------------------------------------------------------------

/// AST node kinds that define function boundaries.
///
/// Used for two purposes:
/// 1. Computing metrics for standalone function/method declarations.
/// 2. Stopping branch/nesting recursion at nested function boundaries.
///
/// Closures/lambdas (e.g. `closure_expression`, `arrow_function`) are included for (2)
/// even though the declaration extractors don't create `Declaration` entries for them.
/// Their computed metrics won't be applied — this is intentional.
fn function_node_kinds(language: &Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &["function_item", "closure_expression"],
        Language::Python => &["function_definition", "lambda"],
        Language::TypeScript => &[
            "function_declaration",
            "method_definition",
            "arrow_function",
            "function_expression",
        ],
        Language::JavaScript => &[
            "function_declaration",
            "method_definition",
            "arrow_function",
            "function_expression",
        ],
        Language::Go => &["function_declaration", "method_declaration", "func_literal"],
        Language::Java => &[
            "method_declaration",
            "constructor_declaration",
            "lambda_expression",
        ],
        Language::C => &["function_definition"],
        Language::Cpp => &["function_definition", "lambda_expression"],
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

fn is_ts_function_kind(kind: &DeclKind) -> bool {
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
/// Formula: `cc + nesting*2 + params*0.5 + body_lines/20`
///
/// Weights:
/// - cyclomatic: 1x (primary signal)
/// - nesting: 2x (deep nesting is disproportionately hard to reason about)
/// - params: 0.5x (more params = harder interface, but less than structural complexity)
/// - body_lines: /20 (longer = harder, but secondary to structural metrics)
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

/// Sort hotspot entries by the given criterion with a stable file:line tiebreak.
pub fn sort_hotspots(entries: &mut [HotspotEntry], sort_by: &str) {
    let tiebreak =
        |a: &HotspotEntry, b: &HotspotEntry| a.file.cmp(&b.file).then(a.line.cmp(&b.line));

    match sort_by {
        "complexity" => {
            entries.sort_by(|a, b| b.cyclomatic.cmp(&a.cyclomatic).then_with(|| tiebreak(a, b)))
        }
        "nesting" => entries.sort_by(|a, b| {
            b.max_nesting
                .cmp(&a.max_nesting)
                .then_with(|| tiebreak(a, b))
        }),
        "params" => entries.sort_by(|a, b| {
            b.param_count
                .cmp(&a.param_count)
                .then_with(|| tiebreak(a, b))
        }),
        "body_lines" => {
            entries.sort_by(|a, b| b.body_lines.cmp(&a.body_lines).then_with(|| tiebreak(a, b)))
        }
        _ => entries.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| tiebreak(a, b))
        }),
    }
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
            if !path_matches_filter(&file_path, filter) {
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

// ---------------------------------------------------------------------------
// Codebase health report
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct HottestFile {
    pub file: String,
    pub functions: usize,
    pub avg_complexity: f64,
    pub max_complexity: u16,
}

#[derive(Serialize)]
pub struct HealthReport {
    pub total_functions: usize,
    pub analyzed: usize,
    pub avg_cc: f64,
    pub median_cc: u16,
    pub max_cc: u16,
    pub p90_cc: u16,
    pub avg_nesting: f64,
    pub avg_params: f64,
    pub avg_body_lines: f64,
    pub high_complexity_count: usize,
    pub high_complexity_pct: f64,
    pub documented_pct: f64,
    pub test_count: usize,
    pub deprecated_count: usize,
    pub public_api_count: usize,
    pub hottest_files: Vec<HottestFile>,
}

struct HealthAccumulator {
    total_functions: usize,
    analyzed: usize,
    cc_values: Vec<u16>,
    nesting_sum: f64,
    params_sum: f64,
    body_lines_sum: f64,
    high_complexity: usize,
    documented: usize,
    test_count: usize,
    deprecated_count: usize,
    public_api_count: usize,
    file_stats: HashMap<String, (Vec<u16>, u16)>,
}

impl HealthAccumulator {
    fn new() -> Self {
        Self {
            total_functions: 0,
            analyzed: 0,
            cc_values: Vec::new(),
            nesting_sum: 0.0,
            params_sum: 0.0,
            body_lines_sum: 0.0,
            high_complexity: 0,
            documented: 0,
            test_count: 0,
            deprecated_count: 0,
            public_api_count: 0,
            file_stats: HashMap::new(),
        }
    }

    fn collect(&mut self, file_path: &str, decls: &[Declaration]) {
        for decl in decls {
            let is_func = matches!(decl.kind, DeclKind::Function | DeclKind::Method);

            if is_func {
                self.total_functions += 1;

                if decl.is_test {
                    self.test_count += 1;
                }
                if decl.is_deprecated {
                    self.deprecated_count += 1;
                }
                if matches!(decl.visibility, Visibility::Public) {
                    self.public_api_count += 1;
                }
                if decl.doc_comment.is_some() {
                    self.documented += 1;
                }

                if let Some(ref cm) = decl.complexity {
                    self.analyzed += 1;
                    self.cc_values.push(cm.cyclomatic);
                    self.nesting_sum += cm.max_nesting as f64;
                    self.params_sum += cm.param_count as f64;
                    self.body_lines_sum += decl.body_lines.unwrap_or(0) as f64;

                    if cm.cyclomatic >= 10 {
                        self.high_complexity += 1;
                    }

                    let entry = self
                        .file_stats
                        .entry(file_path.to_string())
                        .or_insert_with(|| (Vec::new(), 0));
                    entry.0.push(cm.cyclomatic);
                    entry.1 = entry.1.max(cm.cyclomatic);
                }
            }

            self.collect(file_path, &decl.children);
        }
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Compute a codebase health report with aggregate complexity metrics.
pub fn compute_health(index: &CodebaseIndex, path_filter: Option<&str>) -> HealthReport {
    let mut acc = HealthAccumulator::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy();
        if let Some(filter) = path_filter {
            if !path_matches_filter(&file_path, filter) {
                continue;
            }
        }
        acc.collect(&file_path, &file.declarations);
    }

    let n = acc.analyzed.max(1) as f64;
    acc.cc_values.sort_unstable();

    let median_cc = if acc.cc_values.is_empty() {
        0
    } else {
        let len = acc.cc_values.len();
        if len % 2 == 1 {
            acc.cc_values[len / 2]
        } else {
            // Average the two middle elements, rounding to nearest u16
            let lo = acc.cc_values[len / 2 - 1] as u32;
            let hi = acc.cc_values[len / 2] as u32;
            (lo + hi).div_ceil(2) as u16
        }
    };
    let p90_cc = if acc.cc_values.is_empty() {
        0
    } else {
        acc.cc_values
            [(((acc.cc_values.len() - 1) as f64 * 0.9) as usize).min(acc.cc_values.len() - 1)]
    };
    let max_cc = acc.cc_values.last().copied().unwrap_or(0);
    let avg_cc = if acc.cc_values.is_empty() {
        0.0
    } else {
        acc.cc_values.iter().map(|&v| v as f64).sum::<f64>() / n
    };

    // Hottest files: top 5 by average complexity (min 2 functions)
    let mut hottest: Vec<HottestFile> = acc
        .file_stats
        .iter()
        .filter(|(_, (ccs, _))| ccs.len() >= 2)
        .map(|(path, (ccs, file_max_cc))| {
            let avg = ccs.iter().map(|&v| v as f64).sum::<f64>() / ccs.len() as f64;
            HottestFile {
                file: path.clone(),
                functions: ccs.len(),
                avg_complexity: round1(avg),
                max_complexity: *file_max_cc,
            }
        })
        .collect();
    hottest.sort_by(|a, b| {
        b.avg_complexity
            .partial_cmp(&a.avg_complexity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hottest.truncate(5);

    HealthReport {
        total_functions: acc.total_functions,
        analyzed: acc.analyzed,
        avg_cc: round1(avg_cc),
        median_cc,
        max_cc,
        p90_cc,
        avg_nesting: round1(acc.nesting_sum / n),
        avg_params: round1(acc.params_sum / n),
        avg_body_lines: round1(acc.body_lines_sum / n),
        high_complexity_count: acc.high_complexity,
        high_complexity_pct: if acc.analyzed > 0 {
            round1(acc.high_complexity as f64 / acc.analyzed as f64 * 100.0)
        } else {
            0.0
        },
        documented_pct: if acc.total_functions > 0 {
            round1(acc.documented as f64 / acc.total_functions as f64 * 100.0)
        } else {
            0.0
        },
        test_count: acc.test_count,
        deprecated_count: acc.deprecated_count,
        public_api_count: acc.public_api_count,
        hottest_files: hottest,
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
        assert_eq!(c.cyclomatic, 6, "cyclomatic={}", c.cyclomatic);
        assert_eq!(c.max_nesting, 3, "nesting={}", c.max_nesting); // if > for > if
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
        assert_eq!(c.max_nesting, 1); // match_expression
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
    fn typescript_ternary_and_nullish_coalescing() {
        let src = r#"
function choose(x: number, fallback: string | null): string {
    const a = x > 0 ? "pos" : "neg";
    const b = fallback ?? "default";
    if (x === 0) {
        return b;
    }
    return a;
}
"#;
        let decls = parse_and_annotate(src, Language::TypeScript);
        let c = get_complexity(&decls, "choose").expect("should have complexity");
        assert_eq!(c.param_count, 2);
        // 1 base + ternary + ?? + if = 4
        assert_eq!(c.cyclomatic, 4);
        assert_eq!(c.max_nesting, 1); // only the if_statement nests
    }

    #[test]
    fn javascript_ternary_and_logical_or() {
        let src = r#"
function greet(name, loud) {
    const greeting = loud ? "HELLO" : "hello";
    return greeting + " " + (name || "world");
}
"#;
        let decls = parse_and_annotate(src, Language::JavaScript);
        let c = get_complexity(&decls, "greet").expect("should have complexity");
        assert_eq!(c.param_count, 2);
        // 1 base + ternary + || = 3
        assert_eq!(c.cyclomatic, 3);
        assert_eq!(c.max_nesting, 0);
    }

    #[test]
    fn java_method() {
        let src = r#"
class Util {
    public int clamp(int value, int min, int max) {
        if (value < min) {
            return min;
        } else if (value > max) {
            return max;
        }
        return value;
    }
}
"#;
        let decls = parse_and_annotate(src, Language::Java);
        let c = get_complexity(&decls, "clamp").expect("should have complexity");
        assert_eq!(c.param_count, 3);
        // 1 base + if + else-if(if) = 3
        assert_eq!(c.cyclomatic, 3);
        assert_eq!(c.max_nesting, 1); // else-if is flat, not nested
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

    #[test]
    fn rust_nested_closure_isolation() {
        let src = r#"
fn outer(x: i32) -> i32 {
    let add = |a: i32, b: i32| -> i32 {
        if a > 0 {
            a + b
        } else {
            b
        }
    };
    if x > 0 {
        add(x, 1)
    } else {
        0
    }
}
"#;
        let decls = parse_and_annotate(src, Language::Rust);
        let c = get_complexity(&decls, "outer").expect("should have complexity");
        // Closure branches should NOT leak into outer function.
        // outer has: 1 base + 1 if = 2
        assert_eq!(c.cyclomatic, 2, "cyclomatic={}", c.cyclomatic);
        assert_eq!(c.max_nesting, 1, "nesting={}", c.max_nesting);
    }

    #[test]
    fn rust_else_if_chain_nesting() {
        let src = r#"
fn classify(x: i32) -> &'static str {
    if x > 100 {
        "large"
    } else if x > 10 {
        "medium"
    } else if x > 0 {
        "small"
    } else {
        "non-positive"
    }
}
"#;
        let decls = parse_and_annotate(src, Language::Rust);
        let c = get_complexity(&decls, "classify").expect("should have complexity");
        // Flat else-if chain should NOT inflate nesting — depth is 1, not 3
        assert_eq!(c.max_nesting, 1, "nesting={}", c.max_nesting);
        // if + else-if + else-if = 3 branches, base 1 = 4
        assert_eq!(c.cyclomatic, 4, "cyclomatic={}", c.cyclomatic);
    }

    #[test]
    fn typescript_else_if_chain_nesting() {
        let src = r#"
function classify(x: number): string {
    if (x > 100) {
        return "large";
    } else if (x > 10) {
        return "medium";
    } else if (x > 0) {
        return "small";
    } else {
        return "non-positive";
    }
}
"#;
        let decls = parse_and_annotate(src, Language::TypeScript);
        let c = get_complexity(&decls, "classify").expect("should have complexity");
        assert_eq!(c.max_nesting, 1, "nesting={}", c.max_nesting);
    }

    #[test]
    fn go_else_if_chain_nesting() {
        let src = r#"
package main

func classify(x int) string {
    if x > 100 {
        return "large"
    } else if x > 10 {
        return "medium"
    } else if x > 0 {
        return "small"
    } else {
        return "non-positive"
    }
}
"#;
        let decls = parse_and_annotate(src, Language::Go);
        let c = get_complexity(&decls, "classify").expect("should have complexity");
        assert_eq!(c.max_nesting, 1, "nesting={}", c.max_nesting);
    }

    #[test]
    fn rust_nested_if_inside_else_if_still_counts() {
        let src = r#"
fn check(x: i32, y: i32) -> &'static str {
    if x > 0 {
        "positive"
    } else if y > 0 {
        if y > 100 {
            "y-large"
        } else {
            "y-small"
        }
    } else {
        "both-non-positive"
    }
}
"#;
        let decls = parse_and_annotate(src, Language::Rust);
        let c = get_complexity(&decls, "check").expect("should have complexity");
        // else-if doesn't add nesting, but the nested if inside the else-if body does:
        // if (depth 1) → else if (still depth 1) → if (depth 2)
        assert_eq!(c.max_nesting, 2, "nesting={}", c.max_nesting);
    }

    #[test]
    fn cpp_range_for_and_catch() {
        let src = r#"
#include <vector>
#include <stdexcept>

int process(std::vector<int> items, int threshold) {
    int total = 0;
    try {
        for (auto& item : items) {
            if (item > threshold) {
                total += item;
            } else if (item < 0) {
                throw std::runtime_error("negative");
            }
        }
    } catch (const std::exception& e) {
        return -1;
    }
    return total;
}
"#;
        let decls = parse_and_annotate(src, Language::Cpp);
        let c = get_complexity(&decls, "process").expect("should have complexity");
        assert_eq!(c.param_count, 2);
        // 1 base + for_range_loop + if + else-if(if) + catch = 5
        assert_eq!(c.cyclomatic, 5, "cyclomatic={}", c.cyclomatic);
        // try > for_range_loop > if = depth 3
        assert_eq!(c.max_nesting, 3, "nesting={}", c.max_nesting);
    }

    #[test]
    fn python_elif_chain_nesting() {
        // Python uses elif_clause which is a direct child of if_statement,
        // not a nested if inside an else_clause. The AST structure naturally
        // keeps elif flat, so nesting should be 1.
        let src = r#"
def classify(x):
    if x > 100:
        return "large"
    elif x > 10:
        return "medium"
    elif x > 0:
        return "small"
    else:
        return "non-positive"
"#;
        let decls = parse_and_annotate(src, Language::Python);
        let c = get_complexity(&decls, "classify").expect("should have complexity");
        assert_eq!(c.max_nesting, 1, "nesting={}", c.max_nesting);
        // 1 base + if + elif + elif = 4
        assert_eq!(c.cyclomatic, 4, "cyclomatic={}", c.cyclomatic);
    }

    #[test]
    fn go_grouped_params() {
        let src = r#"
package main

func multi(a, b int, c string) {}
"#;
        let decls = parse_and_annotate(src, Language::Go);
        let c = get_complexity(&decls, "multi").expect("should have complexity");
        // a and b are grouped under one parameter_declaration — should still count 3
        assert_eq!(c.param_count, 3);
    }
}
