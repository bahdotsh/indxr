use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::languages::Language;
use crate::model::declarations::{
    ComplexityMetrics, DeclKind, Declaration, RelKind, Relationship, Visibility,
};
use crate::model::{CodebaseIndex, FileIndex, Import, IndexStats};

use super::helpers::*;
use super::tools::*;
use super::type_flow::*;
use super::{Transport, process_jsonrpc_message};
use crate::indexer::IndexConfig;
use crate::parser::ParserRegistry;

// -----------------------------------------------------------------------
// score_match tests
// -----------------------------------------------------------------------

#[test]
fn test_score_match_exact_full_match() {
    let score = score_match("parse", "parse", &["parse"]);
    // exact substring (10) + exact equality (20) + term match (5) + identifier part (3) = 38
    assert_eq!(score, 38);
}

#[test]
fn test_score_match_substring() {
    let score = score_match("tree_sitter_parser", "parser", &["parser"]);
    // substring (10) + term match (5) + identifier part "parser" (3) = 18
    assert_eq!(score, 18);
}

#[test]
fn test_score_match_no_match() {
    let score = score_match("indexer", "parser", &["parser"]);
    assert_eq!(score, 0);
}

#[test]
fn test_score_match_multi_term() {
    // "token budget" is NOT a substring of "token_budget_manager"
    // term "token" (5) + term "budget" (5) + ident part "token" (3) + ident part "budget" (3) = 16
    let score = score_match("token_budget_manager", "token budget", &["token", "budget"]);
    assert_eq!(score, 16);

    // When full query IS a substring, it scores higher
    let score2 = score_match(
        "apply token budget here",
        "token budget",
        &["token", "budget"],
    );
    // full query substring (10) + term "token" (5) + term "budget" (5) = 20
    // (identifier split of "apply token budget here" won't match since it's a phrase, not an identifier)
    assert_eq!(score2, 20);
}

#[test]
fn test_score_match_partial_term_match() {
    // Only one of two terms matches
    let score = score_match("token_counter", "token budget", &["token", "budget"]);
    // term "token" (5) + ident part "token" (3) = 8
    assert_eq!(score, 8);
}

#[test]
fn test_score_match_empty_query() {
    // Empty string is a substring of everything
    let score = score_match("anything", "", &[""]);
    assert!(score > 0);
}

#[test]
fn test_score_match_case_sensitivity() {
    // score_match expects pre-lowercased inputs (caller responsibility)
    // With bigram matching, near-matches still get a small score as fuzzy fallback
    let score = score_match("parser", "Parser", &["Parser"]);
    // No substring, no term, no ident part, but bigram similarity is high → small fuzzy score
    assert!(score > 0); // fuzzy match kicks in
    assert!(score < 10); // but much weaker than a proper substring match
}

#[test]
fn test_score_match_camel_case_aware() {
    // "parse decl" should match "parseDeclaration" via identifier splitting
    let score = score_match("parsedeclaration", "parse decl", &["parse", "decl"]);
    // No substring "parse decl" in "parsedeclaration" (0)
    // term "parse": "parsedeclaration".contains("parse") → yes (5)
    // term "decl": "parsedeclaration".contains("decl") → yes (5)
    // ident parts of "parsedeclaration" = ["parsedeclaration"] (no camelCase boundary in all-lowercase)
    // So no ident bonus
    assert_eq!(score, 10);

    // With actual camelCase input (pre-lowercased — but split_identifier works on original)
    // This tests the intent: that snake_case splits help
    let score2 = score_match("parse_declaration", "parse decl", &["parse", "decl"]);
    // term "parse" (5) + term "decl" not a substring → "parse_declaration" contains "decl"? yes (5)
    // ident parts: ["parse", "declaration"] — "parse" matches (3), "decl" ≠ "declaration" (0)
    assert_eq!(score2, 13);
}

#[test]
fn test_split_identifier() {
    assert_eq!(
        split_identifier("parseDeclaration"),
        vec!["parse", "declaration"]
    );
    assert_eq!(
        split_identifier("parse_declaration"),
        vec!["parse", "declaration"]
    );
    // Consecutive uppercase letters stay grouped (XMLParser → "xmlparser" as one unit)
    // since we only split on lowercase→uppercase transitions
    assert_eq!(split_identifier("XMLParser"), vec!["xmlparser"]);
    assert_eq!(split_identifier("simple"), vec!["simple"]);
    assert_eq!(
        split_identifier("getHTTPResponse"),
        vec!["get", "httpresponse"]
    );
    assert_eq!(
        split_identifier("src/parser/mod.rs"),
        vec!["src", "parser", "mod", "rs"]
    );
    // Digit→uppercase boundary
    assert_eq!(split_identifier("v2Parser"), vec!["v2", "parser"]);
    assert_eq!(split_identifier("item3DView"), vec!["item3", "dview"]);
}

#[test]
fn test_simple_glob_match() {
    assert!(simple_glob_match("*.rs", "src/main.rs"));
    assert!(!simple_glob_match("*.rs", "src/main.py"));
    assert!(simple_glob_match("src/parser/*", "src/parser/mod.rs"));
    assert!(!simple_glob_match(
        "src/parser/*",
        "src/parser/queries/rust.rs"
    ));
    assert!(simple_glob_match(
        "src/parser/**",
        "src/parser/queries/rust.rs"
    ));
    assert!(simple_glob_match("src/parser/**", "src/parser/mod.rs"));
    // Recursive glob with extension
    assert!(simple_glob_match("**/*.rs", "src/main.rs"));
    assert!(simple_glob_match("**/*.rs", "src/parser/mod.rs"));
    assert!(!simple_glob_match("**/*.rs", "src/main.py"));
    // Recursive glob with filename
    assert!(simple_glob_match("**/mod.rs", "src/parser/mod.rs"));
    assert!(!simple_glob_match("**/mod.rs", "src/parser/lib.rs"));
    // Nested glob mid-path (was unsupported by hand-rolled impl)
    assert!(simple_glob_match(
        "src/**/*.test.rs",
        "src/parser/foo.test.rs"
    ));
    assert!(!simple_glob_match("src/**/*.test.rs", "src/parser/foo.rs"));
    // Exact match (no glob chars)
    assert!(simple_glob_match("src/main.rs", "src/main.rs"));
    assert!(!simple_glob_match("src/main.rs", "src/lib.rs"));
    // Directory prefix (no glob chars)
    assert!(simple_glob_match("src/parser", "src/parser/mod.rs"));
    // Question mark wildcard
    assert!(simple_glob_match("src/ma?n.rs", "src/main.rs"));
    assert!(!simple_glob_match("src/ma?n.rs", "src/maain.rs"));
}

// -----------------------------------------------------------------------
// tool_definitions: verify new tools are registered
// -----------------------------------------------------------------------

#[test]
fn test_tool_definitions_include_new_tools() {
    let defs = tool_definitions();
    let tools = defs["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"get_token_estimate"));
    assert!(names.contains(&"search_relevant"));
    assert!(names.contains(&"lookup_symbol"));
    assert!(names.contains(&"regenerate_index"));
    // New tools
    assert!(names.contains(&"get_diff_summary"));
    assert!(names.contains(&"batch_file_summaries"));
    assert!(names.contains(&"get_callers"));
    assert!(names.contains(&"get_public_api"));
    assert!(names.contains(&"explain_symbol"));
    assert!(names.contains(&"get_related_tests"));
    assert!(names.contains(&"get_dependency_graph"));
    assert!(names.contains(&"get_hotspots"));
    assert!(names.contains(&"get_health"));
    assert!(names.contains(&"get_type_flow"));
    // Total: 12 original + 10 new = 22
    assert_eq!(names.len(), 22);
}

// -----------------------------------------------------------------------
// handle_tool_call: unknown tool
// -----------------------------------------------------------------------

#[test]
fn test_handle_tool_call_unknown_tool() {
    let index = CodebaseIndex {
        root: std::path::PathBuf::from("."),
        root_name: "test".to_string(),
        generated_at: String::new(),
        stats: IndexStats {
            total_files: 0,
            total_lines: 0,
            languages: HashMap::new(),
            duration_ms: 0,
        },
        tree: vec![],
        files: vec![],
    };
    let result = handle_tool_call(&index, "nonexistent_tool", &json!({}));
    // Should return an error
    let content = result["content"][0]["text"].as_str().unwrap();
    assert!(content.contains("Unknown tool"));
}

// -----------------------------------------------------------------------
// search_relevant: scoring weights
// -----------------------------------------------------------------------

#[test]
fn test_name_match_scores_higher_than_signature() {
    // Name matches get 3x multiplier, signature gets 2x
    let name_score = score_match("parse_file", "parse", &["parse"]) * 3;
    let sig_score = score_match("fn parse_file(input: &str)", "parse", &["parse"]) * 2;
    assert!(name_score > 0);
    assert!(sig_score > 0);
    // Both match, but name multiplier is higher
    assert!(name_score > sig_score || name_score == sig_score);
}

// -----------------------------------------------------------------------
// collapse_nested_bodies tests
// -----------------------------------------------------------------------

#[test]
fn test_collapse_simple_nested() {
    let input = "fn foo() {\n    if x {\n        bar();\n    }\n}";
    let result = collapse_nested_bodies(input);
    // The inner `if` block at depth 2 should be collapsed
    assert!(result.contains("{ ... }"));
    // The outer fn brace should remain
    assert!(result.contains("fn foo() {"));
    assert!(result.contains("}"));
    // The inner bar() call should NOT appear
    assert!(!result.contains("bar()"));
}

#[test]
fn test_collapse_string_with_braces() {
    let input = r#"fn foo() {
    let s = "{ not a block }";
    if x {
        bar();
    }
}"#;
    let result = collapse_nested_bodies(input);
    // String braces should not affect depth tracking
    assert!(result.contains(r#""{ not a block }""#));
    // The if block should be collapsed
    assert!(result.contains("{ ... }"));
    assert!(!result.contains("bar()"));
}

#[test]
fn test_collapse_escaped_quotes() {
    // Test that escaped quotes (including double escapes) are handled
    let input = r#"fn foo() {
    let s = "hello \"world\"";
    let t = "path\\";
    if x {
        inner();
    }
}"#;
    let result = collapse_nested_bodies(input);
    // Should not get confused by escaped quotes
    assert!(result.contains("{ ... }"));
    assert!(!result.contains("inner()"));
}

#[test]
fn test_collapse_block_comment_with_braces() {
    let input = "fn foo() {\n    /* { nested } */\n    if x {\n        bar();\n    }\n}";
    let result = collapse_nested_bodies(input);
    // Block comment braces should be ignored
    assert!(result.contains("/* { nested } */"));
    assert!(result.contains("{ ... }"));
}

#[test]
fn test_collapse_line_comment_with_braces() {
    let input = "fn foo() {\n    // { not a block }\n    if x {\n        bar();\n    }\n}";
    let result = collapse_nested_bodies(input);
    // Line comment braces should be ignored
    assert!(result.contains("// { not a block }"));
    assert!(result.contains("{ ... }"));
}

#[test]
fn test_collapse_empty_input() {
    assert_eq!(collapse_nested_bodies(""), "");
}

#[test]
fn test_collapse_no_nesting() {
    let input = "fn foo() { bar(); }";
    let result = collapse_nested_bodies(input);
    // Only depth 1, nothing to collapse
    assert_eq!(result, input);
}

#[test]
fn test_collapse_rust_lifetimes() {
    // Lifetimes like 'a should NOT be treated as string delimiters
    let input = "fn foo<'a>(x: &'a str) {\n    if x.is_empty() {\n        return;\n    }\n}";
    let result = collapse_nested_bodies(input);
    // The lifetime annotations should pass through unchanged
    assert!(result.contains("'a"));
    // The inner if block should still be collapsed
    assert!(result.contains("{ ... }"));
    assert!(!result.contains("return"));
}

// -----------------------------------------------------------------------
// bigram_similarity tests
// -----------------------------------------------------------------------

#[test]
fn test_bigram_identical() {
    let sim = bigram_similarity("parser", "parser");
    assert!((sim - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_bigram_completely_different() {
    let sim = bigram_similarity("abc", "xyz");
    assert_eq!(sim, 0.0);
}

#[test]
fn test_bigram_partial_overlap() {
    // "parse" vs "parser" — substantial overlap but not identical
    let sim = bigram_similarity("parse", "parser");
    assert!(sim > 0.3);
    assert!(sim < 1.0);
}

#[test]
fn test_bigram_short_strings() {
    // Single char strings should return 0
    assert_eq!(bigram_similarity("a", "a"), 0.0);
    assert_eq!(bigram_similarity("", "abc"), 0.0);
}

#[test]
fn test_bigram_no_duplicate_inflation() {
    // "aaa" has bigrams {(a,a)} as a set (size 1)
    // "aab" has bigrams {(a,a), (a,b)} as a set (size 2)
    // intersection = 1, dice = 2*1 / (1+2) = 0.666...
    let sim = bigram_similarity("aaa", "aab");
    let expected = 2.0 / 3.0;
    assert!((sim - expected).abs() < f64::EPSILON);
}

// -----------------------------------------------------------------------
// to_compact_rows tests
// -----------------------------------------------------------------------

#[test]
fn test_compact_rows_basic() {
    let items = vec![
        json!({"name": "foo", "kind": "fn", "line": 10}),
        json!({"name": "bar", "kind": "struct", "line": 20}),
    ];
    let result = to_compact_rows(&["name", "kind", "line"], &items);
    let columns = result["columns"].as_array().unwrap();
    assert_eq!(columns.len(), 3);
    assert_eq!(columns[0], "name");
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], "foo");
    assert_eq!(rows[0][1], "fn");
    assert_eq!(rows[0][2], 10);
    assert_eq!(rows[1][0], "bar");
}

#[test]
fn test_compact_rows_missing_column() {
    let items = vec![json!({"name": "foo"})];
    let result = to_compact_rows(&["name", "missing"], &items);
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows[0][0], "foo");
    assert!(rows[0][1].is_null());
}

// -----------------------------------------------------------------------
// contains_word_boundary tests
// -----------------------------------------------------------------------

#[test]
fn test_word_boundary_basic() {
    assert!(contains_word_boundary("fn get(x: i32)", "get"));
    assert!(!contains_word_boundary("fn budget(x: i32)", "get"));
    assert!(!contains_word_boundary("fn widget(x: i32)", "get"));
}

#[test]
fn test_word_boundary_at_edges() {
    // Underscore is an identifier char, so get in get_value is NOT a word boundary
    assert!(!contains_word_boundary("get_value", "get"));
    assert!(!contains_word_boundary("value_get", "get"));
    // Exact match is a word boundary
    assert!(contains_word_boundary("get", "get"));
    // Punctuation/space boundaries work
    assert!(contains_word_boundary("(get)", "get"));
    assert!(contains_word_boundary("x: get, y", "get"));
}

#[test]
fn test_word_boundary_not_partial() {
    assert!(!contains_word_boundary("getting", "get"));
    assert!(!contains_word_boundary("target", "get"));
}

#[test]
fn test_word_boundary_with_generics() {
    assert!(contains_word_boundary("HashMap<String, Value>", "HashMap"));
    assert!(contains_word_boundary("Result<Cache>", "Cache"));
}

#[test]
fn test_word_boundary_empty() {
    assert!(!contains_word_boundary("anything", ""));
}

// -----------------------------------------------------------------------
// Integration tests for new tool functions
// -----------------------------------------------------------------------

/// Build a minimal CodebaseIndex fixture for integration tests.
fn make_test_index() -> CodebaseIndex {
    let parse_fn = {
        let mut d = Declaration::new(
            DeclKind::Function,
            "parse_file".to_string(),
            "pub fn parse_file(path: &Path) -> Result<FileIndex>".to_string(),
            Visibility::Public,
            10,
        );
        d.body_lines = Some(20);
        d.doc_comment = Some("Parse a single source file.".to_string());
        d.complexity = Some(ComplexityMetrics {
            cyclomatic: 12,
            max_nesting: 4,
            param_count: 1,
        });
        d.relationships.push(Relationship {
            kind: RelKind::Implements,
            target: "Parser".to_string(),
        });
        d
    };

    let test_parse = {
        let mut d = Declaration::new(
            DeclKind::Function,
            "test_parse_file".to_string(),
            "fn test_parse_file()".to_string(),
            Visibility::Private,
            50,
        );
        d.is_test = true;
        d.body_lines = Some(10);
        d
    };

    let helper_fn = {
        let mut d = Declaration::new(
            DeclKind::Function,
            "internal_helper".to_string(),
            "fn internal_helper(x: &str) -> bool".to_string(),
            Visibility::Private,
            35,
        );
        d.body_lines = Some(5);
        d.complexity = Some(ComplexityMetrics {
            cyclomatic: 2,
            max_nesting: 1,
            param_count: 1,
        });
        d
    };

    let cache_struct = {
        let mut d = Declaration::new(
            DeclKind::Struct,
            "Cache".to_string(),
            "pub struct Cache".to_string(),
            Visibility::Public,
            5,
        );
        d.doc_comment = Some("In-memory caching layer.".to_string());
        d.children.push(Declaration::new(
            DeclKind::Method,
            "get".to_string(),
            "pub fn get(&self, key: &str) -> Option<&Value>".to_string(),
            Visibility::Public,
            8,
        ));
        d.children.push(Declaration::new(
            DeclKind::Field,
            "entries".to_string(),
            "HashMap<PathBuf, FileIndex>".to_string(),
            Visibility::Private,
            6,
        ));
        d
    };

    let parser_file = FileIndex {
        path: PathBuf::from("src/parser.rs"),
        language: Language::Rust,
        size: 1200,
        lines: 80,
        imports: vec![
            Import {
                text: "use std::path::Path;".to_string(),
            },
            Import {
                text: "use crate::model::FileIndex;".to_string(),
            },
        ],
        declarations: vec![parse_fn, helper_fn, test_parse],
    };

    let cache_file = FileIndex {
        path: PathBuf::from("src/cache.rs"),
        language: Language::Rust,
        size: 600,
        lines: 40,
        imports: vec![Import {
            text: "use crate::parser::parse_file;".to_string(),
        }],
        declarations: vec![cache_struct],
    };

    CodebaseIndex {
        root: PathBuf::from("/tmp/test_project"),
        root_name: "test_project".to_string(),
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        stats: IndexStats {
            total_files: 2,
            total_lines: 120,
            languages: HashMap::from([("Rust".to_string(), 2)]),
            duration_ms: 10,
        },
        tree: vec![],
        files: vec![parser_file, cache_file],
    }
}

#[test]
fn test_tool_batch_file_summaries_paths() {
    let index = make_test_index();
    let result = tool_batch_file_summaries(
        &index,
        &json!({
            "paths": ["src/parser.rs", "src/cache.rs"]
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 2);
    assert_eq!(content["total_matched"], 2);
    let summaries = content["summaries"].as_array().unwrap();
    assert_eq!(summaries.len(), 2);
}

#[test]
fn test_tool_batch_file_summaries_glob() {
    let index = make_test_index();
    let result = tool_batch_file_summaries(&index, &json!({ "glob": "*.rs" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 2);
}

#[test]
fn test_tool_batch_file_summaries_no_args() {
    let index = make_test_index();
    let result = tool_batch_file_summaries(&index, &json!({}));
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Provide either"));
}

#[test]
fn test_tool_get_callers() {
    let index = make_test_index();
    let result = tool_get_callers(&index, &json!({ "symbol": "parse_file" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // cache.rs imports parse_file
    assert!(content["count"].as_u64().unwrap() >= 1);
    let refs = content["references"].as_array().unwrap();
    let has_import = refs
        .iter()
        .any(|r| r["match_type"] == "import" && r["file"].as_str().unwrap().contains("cache.rs"));
    assert!(has_import, "Expected import reference from cache.rs");
}

#[test]
fn test_tool_get_callers_no_false_positive() {
    let index = make_test_index();
    // "get" should not match "budget" or "widget" — word-boundary matching
    let result = tool_get_callers(&index, &json!({ "symbol": "nonexistent_sym" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 0);
}

#[test]
fn test_tool_get_public_api() {
    let index = make_test_index();
    let result = tool_get_public_api(&index, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Public declarations: parse_file, Cache, Cache::get
    let decls = content["declarations"].as_array().unwrap();
    let names: Vec<&str> = decls.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"parse_file"));
    assert!(names.contains(&"Cache"));
    assert!(names.contains(&"get"));
    // internal_helper and test_parse_file are NOT public
    assert!(!names.contains(&"internal_helper"));
    assert!(!names.contains(&"test_parse_file"));
}

#[test]
fn test_tool_get_public_api_scoped() {
    let index = make_test_index();
    let result = tool_get_public_api(&index, &json!({ "path": "src/cache.rs" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let decls = content["declarations"].as_array().unwrap();
    // Only cache.rs public decls
    let files: Vec<&str> = decls.iter().map(|d| d["file"].as_str().unwrap()).collect();
    assert!(files.iter().all(|f| f.contains("cache.rs")));
}

#[test]
fn test_tool_explain_symbol() {
    let index = make_test_index();
    let result = tool_explain_symbol(&index, &json!({ "name": "parse_file" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 1);
    let sym = &content["symbols"][0];
    assert_eq!(sym["name"], "parse_file");
    assert_eq!(sym["kind"], "fn");
    assert_eq!(sym["visibility"], "pub");
    assert!(
        sym["doc_comment"]
            .as_str()
            .unwrap()
            .contains("Parse a single")
    );
    assert!(
        sym["signature"]
            .as_str()
            .unwrap()
            .contains("Result<FileIndex>")
    );
    // Has relationship
    let rels = sym["relationships"].as_array().unwrap();
    assert!(!rels.is_empty());
    assert_eq!(rels[0]["target"], "Parser");
}

#[test]
fn test_tool_explain_symbol_case_insensitive() {
    let index = make_test_index();
    let result = tool_explain_symbol(&index, &json!({ "name": "CACHE" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 1);
    assert_eq!(content["symbols"][0]["name"], "Cache");
}

#[test]
fn test_tool_explain_symbol_not_found() {
    let index = make_test_index();
    let result = tool_explain_symbol(&index, &json!({ "name": "nonexistent" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 0);
}

#[test]
fn test_tool_get_related_tests() {
    let index = make_test_index();
    let result = tool_get_related_tests(&index, &json!({ "symbol": "parse_file" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["count"].as_u64().unwrap() >= 1);
    let tests = content["tests"].as_array().unwrap();
    let names: Vec<&str> = tests.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"test_parse_file"));
}

#[test]
fn test_tool_get_related_tests_scoped() {
    let index = make_test_index();
    let result = tool_get_related_tests(
        &index,
        &json!({
            "symbol": "parse_file",
            "path": "src/parser.rs"
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_tool_get_related_tests_no_match() {
    let index = make_test_index();
    let result = tool_get_related_tests(&index, &json!({ "symbol": "nonexistent" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 0);
}

#[test]
fn test_tool_get_token_estimate_directory() {
    let index = make_test_index();
    let result = tool_get_token_estimate(&index, &json!({ "directory": "src" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["file_count"], 2);
    assert!(content["total_tokens"].as_u64().unwrap() > 0);
}

#[test]
fn test_tool_get_token_estimate_glob() {
    let index = make_test_index();
    let result = tool_get_token_estimate(&index, &json!({ "glob": "*.rs" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["file_count"], 2);
}

#[test]
fn test_tool_get_token_estimate_no_args() {
    let index = make_test_index();
    let result = tool_get_token_estimate(&index, &json!({}));
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Provide"));
}

// -----------------------------------------------------------------------
// find_file: suffix matching requires `/` boundary
// -----------------------------------------------------------------------

#[test]
fn test_find_file_exact_match() {
    let index = make_test_index();
    let f = find_file(&index, "src/parser.rs");
    assert!(f.is_some());
    assert_eq!(f.unwrap().path.to_string_lossy(), "src/parser.rs");
}

#[test]
fn test_find_file_suffix_with_slash_boundary() {
    let index = make_test_index();
    // "parser.rs" should match "src/parser.rs" via "/parser.rs" suffix
    let f = find_file(&index, "parser.rs");
    assert!(f.is_some());
    assert_eq!(f.unwrap().path.to_string_lossy(), "src/parser.rs");
}

#[test]
fn test_find_file_no_partial_suffix() {
    let index = make_test_index();
    // "rs" should NOT match "src/parser.rs" (no `/` boundary)
    assert!(find_file(&index, "rs").is_none());
    // "arser.rs" should NOT match either
    assert!(find_file(&index, "arser.rs").is_none());
    // "che.rs" should NOT match "src/cache.rs"
    assert!(find_file(&index, "che.rs").is_none());
}

#[test]
fn test_find_file_not_found() {
    let index = make_test_index();
    assert!(find_file(&index, "nonexistent.rs").is_none());
}

// -----------------------------------------------------------------------
// collapse_nested_bodies: raw strings
// -----------------------------------------------------------------------

#[test]
fn test_collapse_raw_string_with_braces() {
    // Build input containing: r#"{ raw braces }"#
    let input = String::from(
        "fn foo() {\n    let s = r#\"{ raw braces }\"#;\n    if x {\n        bar();\n    }\n}",
    );
    // Verify our input is well-formed
    assert!(input.contains("r#"));
    let result = collapse_nested_bodies(&input);
    // Raw string braces should not affect depth tracking
    assert!(result.contains("{ raw braces }"));
    // The if block should be collapsed
    assert!(result.contains("{ ... }"));
    assert!(!result.contains("bar()"));
}

#[test]
fn test_collapse_raw_string_double_hash() {
    // r##"has a " and { braces }"##
    let mut input = String::new();
    input.push_str("fn foo() {\n    let s = r##\"has a ");
    input.push('"');
    input.push_str(" and { braces }\"##;\n    if x {\n        bar();\n    }\n}");
    let result = collapse_nested_bodies(&input);
    assert!(result.contains("{ ... }"));
    assert!(!result.contains("bar()"));
}

#[test]
fn test_collapse_raw_string_no_hash() {
    // r"{ raw }"
    let input = "fn foo() {\n    let s = r\"{ raw }\"; if x {\n        bar();\n    }\n}";
    let result = collapse_nested_bodies(input);
    assert!(result.contains("{ ... }"));
    assert!(!result.contains("bar()"));
}

// -----------------------------------------------------------------------
// compact output mode tests
// -----------------------------------------------------------------------

#[test]
fn test_tool_lookup_symbol_compact() {
    let index = make_test_index();
    let result = tool_lookup_symbol(
        &index,
        &json!({
            "name": "parse_file",
            "compact": true
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["matches"].as_u64().unwrap() >= 1);
    // Compact format has columns and rows
    assert!(content["columns"].is_array());
    assert!(content["rows"].is_array());
    let columns = content["columns"].as_array().unwrap();
    assert!(columns.contains(&json!("name")));
    assert!(columns.contains(&json!("file")));
    let rows = content["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
}

#[test]
fn test_tool_lookup_symbol_non_compact() {
    let index = make_test_index();
    let result = tool_lookup_symbol(&index, &json!({ "name": "parse_file" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Non-compact has "symbols" array of objects
    assert!(content["symbols"].is_array());
    let symbols = content["symbols"].as_array().unwrap();
    assert!(!symbols.is_empty());
    assert!(symbols[0]["name"].is_string());
}

#[test]
fn test_tool_list_declarations_compact() {
    let index = make_test_index();
    let result = tool_list_declarations(
        &index,
        &json!({
            "path": "src/parser.rs",
            "compact": true
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["file"], "src/parser.rs");
    // Compact: declarations has columns/rows format
    let decls = &content["declarations"];
    assert!(decls["columns"].is_array());
    assert!(decls["rows"].is_array());
}

#[test]
fn test_tool_search_signatures_compact() {
    let index = make_test_index();
    let result = tool_search_signatures(
        &index,
        &json!({
            "query": "Result<",
            "compact": true
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["matches"].as_u64().unwrap() >= 1);
    assert!(content["columns"].is_array());
    assert!(content["rows"].is_array());
}

#[test]
fn test_tool_search_relevant_compact() {
    let index = make_test_index();
    let result = tool_search_relevant(
        &index,
        &json!({
            "query": "parse",
            "compact": true
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["matches"].as_u64().unwrap() >= 1);
    let results_val = &content["results"];
    assert!(results_val["columns"].is_array());
    assert!(results_val["rows"].is_array());
}

// -----------------------------------------------------------------------
// search_relevant: kind filter
// -----------------------------------------------------------------------

#[test]
fn test_tool_search_relevant_kind_filter() {
    let index = make_test_index();
    // Filter to only structs
    let result = tool_search_relevant(
        &index,
        &json!({
            "query": "cache",
            "kind": "struct"
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let results_arr = content["results"].as_array().unwrap();
    // All results should be structs (no path matches since kind filter is active)
    for r in results_arr {
        if let Some(kind) = r["kind"].as_str() {
            assert_eq!(
                kind, "struct",
                "Expected only struct results with kind filter"
            );
        }
    }
    // Should find the Cache struct
    let has_cache = results_arr
        .iter()
        .any(|r| r["symbol"].as_str() == Some("Cache"));
    assert!(has_cache, "Expected Cache struct in results");
}

#[test]
fn test_tool_search_relevant_kind_filter_fn() {
    let index = make_test_index();
    // Filter to only functions
    let result = tool_search_relevant(
        &index,
        &json!({
            "query": "parse",
            "kind": "fn"
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let results_arr = content["results"].as_array().unwrap();
    // All symbol results should be functions
    for r in results_arr {
        if r["symbol"].is_string() {
            assert_eq!(r["kind"].as_str().unwrap(), "fn");
        }
    }
}

// -----------------------------------------------------------------------
// read_source: multi-symbol and collapse modes
// -----------------------------------------------------------------------

#[test]
fn test_tool_read_source_multi_symbol() {
    use std::io::Write as IoWrite;

    let mut index = make_test_index();
    // Create a temp file with source content matching the fixture declarations
    let dir = std::env::temp_dir().join("indxr_test_read_source");
    let _ = std::fs::create_dir_all(dir.join("src"));
    let source = "// line 1\n// line 2\n// line 3\n// line 4\n// line 5\n// line 6\n// line 7\n// line 8\n// line 9\nfn parse_file() {\n    // body line 11\n    // body line 12\n    // body line 13\n    // body line 14\n    // body line 15\n    // body line 16\n    // body line 17\n    // body line 18\n    // body line 19\n    // body line 20\n    // body line 21\n    // body line 22\n    // body line 23\n    // body line 24\n    // body line 25\n    // body line 26\n    // body line 27\n    // body line 28\n    // body line 29\n}\n// line 31\n// line 32\n// line 33\n// line 34\nfn internal_helper() {\n    // helper body\n}\n// line 38\n// line 39\n// line 40\n// line 41\n// line 42\n// line 43\n// line 44\n// line 45\n// line 46\n// line 47\n// line 48\n// line 49\nfn test_parse_file() {\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n}\n";
    let file_path = dir.join("src/parser.rs");
    let mut f = std::fs::File::create(&file_path).unwrap();
    f.write_all(source.as_bytes()).unwrap();

    // Point index root at our temp dir
    index.root = dir.clone();

    let result = tool_read_source(
        &index,
        &json!({
            "path": "src/parser.rs",
            "symbols": ["parse_file", "internal_helper"]
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let symbols = content["symbols"].as_array().unwrap();
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0]["symbol"], "parse_file");
    assert_eq!(symbols[1]["symbol"], "internal_helper");
    assert!(
        symbols[0]["source"]
            .as_str()
            .unwrap()
            .contains("parse_file")
    );
    assert!(
        symbols[1]["source"]
            .as_str()
            .unwrap()
            .contains("internal_helper")
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_tool_read_source_multi_symbol_not_found() {
    use std::io::Write as IoWrite;

    let mut index = make_test_index();
    let dir = std::env::temp_dir().join("indxr_test_read_source_nf");
    let _ = std::fs::create_dir_all(dir.join("src"));
    // Need 30+ lines since parse_file starts at line 10 with body_lines=20
    let mut source = String::new();
    for i in 1..=40 {
        source.push_str(&format!("// line {}\n", i));
    }
    let file_path = dir.join("src/parser.rs");
    let mut f = std::fs::File::create(&file_path).unwrap();
    f.write_all(source.as_bytes()).unwrap();
    index.root = dir.clone();

    let result = tool_read_source(
        &index,
        &json!({
            "path": "src/parser.rs",
            "symbols": ["parse_file", "nonexistent_fn"]
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let symbols = content["symbols"].as_array().unwrap();
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["symbol"], "parse_file");
    let not_found = content["not_found"].as_array().unwrap();
    assert!(not_found.contains(&json!("nonexistent_fn")));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_tool_read_source_collapse() {
    use std::io::Write as IoWrite;

    let mut index = make_test_index();
    let dir = std::env::temp_dir().join("indxr_test_read_source_collapse");
    let _ = std::fs::create_dir_all(dir.join("src"));
    let source = "// lines 1-9\n\n\n\n\n\n\n\n\nfn parse_file() {\n    if true {\n        nested();\n    }\n}\n";
    let file_path = dir.join("src/parser.rs");
    let mut f = std::fs::File::create(&file_path).unwrap();
    f.write_all(source.as_bytes()).unwrap();
    index.root = dir.clone();

    let result = tool_read_source(
        &index,
        &json!({
            "path": "src/parser.rs",
            "symbol": "parse_file",
            "collapse": true
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let src = content["source"].as_str().unwrap();
    assert!(content["collapsed"].as_bool().unwrap());
    assert!(src.contains("{ ... }"), "Expected collapsed nested body");
    assert!(
        !src.contains("nested()"),
        "Nested call should be collapsed away"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_tool_read_source_multi_symbol_collapse() {
    use std::io::Write as IoWrite;

    let mut index = make_test_index();
    let dir = std::env::temp_dir().join("indxr_test_multi_collapse");
    let _ = std::fs::create_dir_all(dir.join("src"));
    let source = "// lines 1-9\n\n\n\n\n\n\n\n\nfn parse_file() {\n    if true {\n        nested();\n    }\n}\n// lines 15-34\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\nfn internal_helper() {\n    match x {\n        _ => deep(),\n    }\n}\n";
    let file_path = dir.join("src/parser.rs");
    let mut f = std::fs::File::create(&file_path).unwrap();
    f.write_all(source.as_bytes()).unwrap();
    index.root = dir.clone();

    let result = tool_read_source(
        &index,
        &json!({
            "path": "src/parser.rs",
            "symbols": ["parse_file", "internal_helper"],
            "collapse": true
        }),
    );
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let symbols = content["symbols"].as_array().unwrap();
    assert_eq!(symbols.len(), 2);
    // Both should have collapsed bodies
    for sym in symbols {
        let src = sym["source"].as_str().unwrap();
        assert!(
            src.contains("{ ... }"),
            "Expected collapsed body for {}",
            sym["symbol"]
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// batch_file_summaries: cap boundary
// -----------------------------------------------------------------------

#[test]
fn test_tool_batch_file_summaries_cap() {
    // Create an index with 35 files (over the 30-file cap)
    let mut files = Vec::new();
    for i in 0..35 {
        files.push(FileIndex {
            path: PathBuf::from(format!("src/file_{}.rs", i)),
            language: Language::Rust,
            size: 100,
            lines: 10,
            imports: vec![],
            declarations: vec![],
        });
    }
    let index = CodebaseIndex {
        root: PathBuf::from("/tmp/test_cap"),
        root_name: "test_cap".to_string(),
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        stats: IndexStats {
            total_files: 35,
            total_lines: 350,
            languages: HashMap::from([("Rust".to_string(), 35)]),
            duration_ms: 5,
        },
        tree: vec![],
        files,
    };

    let result = tool_batch_file_summaries(&index, &json!({ "glob": "*.rs" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Should cap at 30
    assert_eq!(content["count"], 30);
    assert_eq!(content["total_matched"], 35);
}

// -----------------------------------------------------------------------
// get_callers: common word symbol
// -----------------------------------------------------------------------

#[test]
fn test_tool_get_callers_common_word() {
    let index = make_test_index();
    // "get" is a method on Cache — should only match word-boundary occurrences
    let result = tool_get_callers(&index, &json!({ "symbol": "get" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Should not produce false positives from "budget", "widget", etc.
    let refs = content["references"].as_array().unwrap();
    for r in refs {
        if let Some(sig) = r.get("match_type").and_then(|v| v.as_str()) {
            if sig == "signature" {
                // The matched signature should contain "get" at a word boundary
                let name = r["name"].as_str().unwrap();
                assert_ne!(name, "get", "Should not match the symbol's own declaration");
            }
        }
    }
}

// -----------------------------------------------------------------------
// Dependency graph tool tests
// -----------------------------------------------------------------------

#[test]
fn test_tool_dependency_graph_file_level_mermaid() {
    let index = make_test_index();
    let result = tool_get_dependency_graph(&index, &json!({ "format": "mermaid" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["format"], "mermaid");
    // cache.rs imports crate::parser::parse_file → should resolve to src/parser.rs
    let node_count = content["nodes"].as_u64().unwrap();
    let edge_count = content["edges"].as_u64().unwrap();
    assert!(
        node_count >= 1,
        "Expected at least 1 node, got {}",
        node_count
    );
    assert!(
        edge_count >= 1,
        "Expected at least 1 edge, got {}",
        edge_count
    );
    let graph = content["graph"].as_str().unwrap();
    assert!(graph.contains("graph LR"));
    assert!(
        graph.contains("parser"),
        "Graph should reference parser file"
    );
    assert!(graph.contains("cache"), "Graph should reference cache file");
}

#[test]
fn test_tool_dependency_graph_file_level_dot() {
    let index = make_test_index();
    let result = tool_get_dependency_graph(&index, &json!({ "format": "dot" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["format"], "dot");
    let graph = content["graph"].as_str().unwrap();
    assert!(graph.contains("digraph dependencies"));
}

#[test]
fn test_tool_dependency_graph_file_level_json() {
    let index = make_test_index();
    let result = tool_get_dependency_graph(&index, &json!({ "format": "json" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["format"], "json");
    let graph = &content["graph"];
    assert!(graph.get("nodes").unwrap().is_array());
    assert!(graph.get("edges").unwrap().is_array());
}

#[test]
fn test_tool_dependency_graph_symbol_level() {
    let index = make_test_index();
    let result = tool_get_dependency_graph(&index, &json!({ "level": "symbol", "format": "json" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["format"], "json");
    let graph = &content["graph"];
    assert!(graph.get("nodes").unwrap().is_array());
    assert!(graph.get("edges").unwrap().is_array());
    // Cache is a struct, and parse_file returns Result<FileIndex> — signature references
    // may produce edges. At minimum the structure is valid.
    assert!(
        content["nodes"].as_u64().is_some(),
        "nodes should be a number"
    );
    assert!(
        content["edges"].as_u64().is_some(),
        "edges should be a number"
    );
}

#[test]
fn test_tool_dependency_graph_scoped() {
    let index = make_test_index();
    let result =
        tool_get_dependency_graph(&index, &json!({ "path": "src/cache", "format": "json" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let graph = &content["graph"];
    let edges = graph["edges"].as_array().unwrap();
    // cache.rs imports from parser — should show that edge
    for edge in edges {
        assert!(
            edge["from"].as_str().unwrap().contains("cache"),
            "Scoped graph should only have edges from cache files"
        );
    }
}

#[test]
fn test_tool_dependency_graph_depth_limit() {
    let index = make_test_index();
    // Full graph: cache.rs → parser.rs (at least 1 edge)
    let full = tool_get_dependency_graph(&index, &json!({ "format": "json" }));
    let full_content: Value =
        serde_json::from_str(full["content"][0]["text"].as_str().unwrap()).unwrap();
    let full_edges = full_content["edges"].as_u64().unwrap();
    assert!(full_edges >= 1, "Full graph should have at least 1 edge");

    // depth=0 scoped to cache: no hops allowed, so no edges
    let d0 = tool_get_dependency_graph(
        &index,
        &json!({ "path": "src/cache", "depth": 0, "format": "json" }),
    );
    let d0_content: Value =
        serde_json::from_str(d0["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        d0_content["edges"].as_u64().unwrap(),
        0,
        "depth=0 should produce no edges"
    );
}

#[test]
fn test_tool_dependency_graph_defaults_to_mermaid() {
    let index = make_test_index();
    let result = tool_get_dependency_graph(&index, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["format"], "mermaid");
}

// -----------------------------------------------------------------------
// get_hotspots
// -----------------------------------------------------------------------

#[test]
fn test_tool_get_hotspots_default() {
    let index = make_test_index();
    let result = tool_get_hotspots(&index, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // parse_file (cc=12) and internal_helper (cc=2) have complexity
    assert_eq!(content["total"], 2);
    let hotspots = content["hotspots"].as_array().unwrap();
    assert_eq!(hotspots.len(), 2);
    // Sorted by score descending — parse_file should be first
    assert_eq!(hotspots[0]["name"], "parse_file");
    assert_eq!(hotspots[1]["name"], "internal_helper");
}

#[test]
fn test_tool_get_hotspots_min_complexity_filter() {
    let index = make_test_index();
    let result = tool_get_hotspots(&index, &json!({ "min_complexity": 10 }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Only parse_file (cc=12) meets min_complexity=10
    assert_eq!(content["total"], 1);
    let hotspots = content["hotspots"].as_array().unwrap();
    assert_eq!(hotspots[0]["name"], "parse_file");
}

#[test]
fn test_tool_get_hotspots_path_filter() {
    let index = make_test_index();
    let result = tool_get_hotspots(&index, &json!({ "path": "cache" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // cache.rs has no complexity data
    assert_eq!(content["total"], 0);
}

#[test]
fn test_tool_get_hotspots_sort_by_complexity() {
    let index = make_test_index();
    let result = tool_get_hotspots(&index, &json!({ "sort_by": "complexity" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let hotspots = content["hotspots"].as_array().unwrap();
    assert_eq!(hotspots[0]["cyclomatic"], 12);
    assert_eq!(hotspots[1]["cyclomatic"], 2);
}

#[test]
fn test_tool_get_hotspots_compact() {
    let index = make_test_index();
    let result = tool_get_hotspots(&index, &json!({ "compact": true }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let hotspots = &content["hotspots"];
    assert!(hotspots["columns"].is_array());
    assert!(hotspots["rows"].is_array());
}

#[test]
fn test_tool_get_hotspots_total_before_truncate() {
    let index = make_test_index();
    // limit=1 but total should reflect all matching hotspots (2)
    let result = tool_get_hotspots(&index, &json!({ "limit": 1 }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["total"], 2);
    let hotspots = content["hotspots"].as_array().unwrap();
    assert_eq!(hotspots.len(), 1);
}

// -----------------------------------------------------------------------
// get_health
// -----------------------------------------------------------------------

#[test]
fn test_tool_get_health_default() {
    let index = make_test_index();
    let result = tool_get_health(&index, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // 3 functions total: parse_file, internal_helper, test_parse_file
    // plus Cache::get method = 4
    assert_eq!(content["total_functions"], 4);
    // 2 have complexity data
    assert_eq!(content["analyzed"], 2);
    // parse_file has cc=12 which is >= 10
    assert_eq!(content["high_complexity_count"], 1);
    // parse_file is documented, others are not
    assert!(content["documented_pct"].as_f64().unwrap() > 0.0);
    // test_parse_file is a test
    assert_eq!(content["test_count"], 1);
    // parse_file is public, Cache::get is public = 2 public
    assert!(content["public_api_count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_tool_get_health_path_filter() {
    let index = make_test_index();
    let result = tool_get_health(&index, &json!({ "path": "src/cache" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Only cache.rs: Cache::get method, no complexity data
    assert_eq!(content["total_functions"], 1);
    assert_eq!(content["analyzed"], 0);
}

#[test]
fn test_tool_get_health_empty_codebase() {
    let index = CodebaseIndex {
        root: PathBuf::from("/tmp/empty"),
        root_name: "empty".to_string(),
        generated_at: String::new(),
        stats: IndexStats {
            total_files: 0,
            total_lines: 0,
            languages: HashMap::new(),
            duration_ms: 0,
        },
        tree: vec![],
        files: vec![],
    };
    let result = tool_get_health(&index, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["total_functions"], 0);
    assert_eq!(content["analyzed"], 0);
    assert_eq!(content["high_complexity_count"], 0);
    assert_eq!(content["documented_pct"], 0.0);
}

// -----------------------------------------------------------------------
// get_diff_summary validation tests
// -----------------------------------------------------------------------

fn make_diff_test_fixtures() -> (
    CodebaseIndex,
    crate::indexer::IndexConfig,
    crate::parser::ParserRegistry,
) {
    let index = CodebaseIndex {
        root: PathBuf::from("/tmp/test"),
        root_name: "test".to_string(),
        generated_at: String::new(),
        stats: IndexStats {
            total_files: 0,
            total_lines: 0,
            languages: HashMap::new(),
            duration_ms: 0,
        },
        tree: vec![],
        files: vec![],
    };
    let config = crate::indexer::IndexConfig {
        root: PathBuf::from("/tmp/test"),
        cache_dir: PathBuf::from("/tmp/test/.cache"),
        max_file_size: 512,
        max_depth: None,
        exclude: vec![],
        no_gitignore: false,
    };
    let registry = crate::parser::ParserRegistry::new();
    (index, config, registry)
}

#[test]
fn test_tool_get_diff_summary_both_params_error() {
    let (index, config, registry) = make_diff_test_fixtures();
    let args = json!({"since_ref": "main", "pr": 42});
    let result = tool_get_diff_summary(&index, &config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("not both"),
        "Expected mutual exclusion error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_neither_param_error() {
    let (index, config, registry) = make_diff_test_fixtures();
    let args = json!({});
    let result = tool_get_diff_summary(&index, &config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("since_ref") && text.contains("pr"),
        "Expected missing param error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_invalid_pr_zero() {
    let (index, config, registry) = make_diff_test_fixtures();
    let args = json!({"pr": 0});
    let result = tool_get_diff_summary(&index, &config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("positive integer"),
        "Expected positive integer error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_invalid_pr_negative() {
    let (index, config, registry) = make_diff_test_fixtures();
    let args = json!({"pr": -1});
    let result = tool_get_diff_summary(&index, &config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("positive integer"),
        "Expected positive integer error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_invalid_pr_string() {
    let (index, config, registry) = make_diff_test_fixtures();
    let args = json!({"pr": "not-a-number"});
    let result = tool_get_diff_summary(&index, &config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("positive integer"),
        "Expected positive integer error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_empty_since_ref() {
    let (index, config, registry) = make_diff_test_fixtures();
    let args = json!({"since_ref": ""});
    let result = tool_get_diff_summary(&index, &config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("must not be empty"),
        "Expected empty ref error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_whitespace_since_ref() {
    let (index, config, registry) = make_diff_test_fixtures();
    let args = json!({"since_ref": "   "});
    let result = tool_get_diff_summary(&index, &config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("must not be empty"),
        "Expected empty ref error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

// -----------------------------------------------------------------------
// Type extraction unit tests
// -----------------------------------------------------------------------

#[test]
fn test_extract_types_rust_function() {
    let sig = "pub fn parse_file(path: &Path, content: &str) -> Result<FileIndex>";
    let info = extract_types_from_signature(sig, &Language::Rust);
    assert!(info.param_types.contains(&"Path".to_string()));
    // &str is primitive, should be filtered
    assert!(
        !info
            .param_types
            .iter()
            .any(|t| t.eq_ignore_ascii_case("str"))
    );
    assert!(info.return_types.contains(&"Result".to_string()));
    assert!(info.return_types.contains(&"FileIndex".to_string()));
}

#[test]
fn test_extract_types_rust_method() {
    let sig = "pub fn get(&self, key: &str) -> Option<&Value>";
    let info = extract_types_from_signature(sig, &Language::Rust);
    assert!(info.return_types.contains(&"Option".to_string()));
    assert!(info.return_types.contains(&"Value".to_string()));
}

#[test]
fn test_extract_types_rust_no_return() {
    let sig = "pub fn process(data: Vec<Item>)";
    let info = extract_types_from_signature(sig, &Language::Rust);
    assert!(info.param_types.contains(&"Vec".to_string()));
    assert!(info.param_types.contains(&"Item".to_string()));
    assert!(info.return_types.is_empty());
}

#[test]
fn test_extract_types_go_function() {
    let sig = "func ParseFile(path string) (*FileIndex, error)";
    let info = extract_types_from_signature(sig, &Language::Go);
    assert!(info.return_types.contains(&"FileIndex".to_string()));
    // error is primitive
    assert!(
        !info
            .return_types
            .iter()
            .any(|t| t.eq_ignore_ascii_case("error"))
    );
}

#[test]
fn test_extract_types_go_method() {
    let sig = "func (s *Server) Handle(req Request) Response";
    let info = extract_types_from_signature(sig, &Language::Go);
    assert!(info.param_types.contains(&"Request".to_string()));
    assert!(info.return_types.contains(&"Response".to_string()));
}

#[test]
fn test_extract_types_typescript() {
    let sig = "function parseFile(path: string): FileIndex";
    let info = extract_types_from_signature(sig, &Language::TypeScript);
    assert!(info.return_types.contains(&"FileIndex".to_string()));
    // string is primitive
    assert!(
        !info
            .param_types
            .iter()
            .any(|t| t.eq_ignore_ascii_case("string"))
    );
}

#[test]
fn test_extract_types_typescript_promise() {
    let sig = "async function fetchData(url: string): Promise<Response>";
    let info = extract_types_from_signature(sig, &Language::TypeScript);
    assert!(info.return_types.contains(&"Promise".to_string()));
    assert!(info.return_types.contains(&"Response".to_string()));
}

#[test]
fn test_extract_types_python() {
    let sig = "def parse_file(path: Path, content: str) -> FileIndex";
    let info = extract_types_from_signature(sig, &Language::Python);
    assert!(info.param_types.contains(&"Path".to_string()));
    assert!(info.return_types.contains(&"FileIndex".to_string()));
}

#[test]
fn test_extract_types_python_optional() {
    let sig = "def find_item(name: str, cache: Optional[Cache]) -> Optional[Item]";
    let info = extract_types_from_signature(sig, &Language::Python);
    assert!(info.param_types.contains(&"Optional".to_string()));
    assert!(info.param_types.contains(&"Cache".to_string()));
    assert!(info.return_types.contains(&"Item".to_string()));
}

#[test]
fn test_extract_types_java() {
    let sig = "public Result<FileIndex> parseFile(Path path)";
    let info = extract_types_from_signature(sig, &Language::Java);
    assert!(info.return_types.contains(&"Result".to_string()));
    assert!(info.return_types.contains(&"FileIndex".to_string()));
    assert!(info.param_types.contains(&"Path".to_string()));
}

#[test]
fn test_extract_types_kotlin() {
    let sig = "fun parseFile(path: Path): FileIndex";
    let info = extract_types_from_signature(sig, &Language::Kotlin);
    assert!(info.param_types.contains(&"Path".to_string()));
    assert!(info.return_types.contains(&"FileIndex".to_string()));
}

#[test]
fn test_extract_types_swift() {
    let sig = "func parseFile(at path: Path) -> FileIndex";
    let info = extract_types_from_signature(sig, &Language::Swift);
    assert!(info.param_types.contains(&"Path".to_string()));
    assert!(info.return_types.contains(&"FileIndex".to_string()));
}

#[test]
fn test_extract_types_empty_signature() {
    let sig = "";
    let info = extract_types_from_signature(sig, &Language::Rust);
    assert!(info.param_types.is_empty());
    assert!(info.return_types.is_empty());
}

#[test]
fn test_extract_types_ruby_no_types() {
    let sig = "def parse_file(path, content)";
    let info = extract_types_from_signature(sig, &Language::Ruby);
    assert!(info.param_types.is_empty());
    assert!(info.return_types.is_empty());
}

// -----------------------------------------------------------------------
// get_type_flow tool integration tests
// -----------------------------------------------------------------------

#[test]
fn test_tool_get_type_flow_producers() {
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({ "type_name": "FileIndex" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    // parse_file returns Result<FileIndex> → producer
    assert!(content["producers_count"].as_u64().unwrap() >= 1);
    let producers = content["producers"].as_array().unwrap();
    let names: Vec<&str> = producers
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"parse_file"),
        "Expected parse_file as producer, got: {:?}",
        names
    );
}

#[test]
fn test_tool_get_type_flow_consumers() {
    // The test index has: Cache.get(&self, key: &str) -> Option<&Value>
    // Value is consumed... but actually it's a return type.
    // parse_file(path: &Path) -> Path is a param → consumer
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({ "type_name": "Path" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    assert!(
        content["consumers_count"].as_u64().unwrap() >= 1,
        "Expected at least 1 consumer of Path"
    );
}

#[test]
fn test_tool_get_type_flow_not_found() {
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({ "type_name": "NonexistentType" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    assert_eq!(content["producers_count"], 0);
    assert_eq!(content["consumers_count"], 0);
}

#[test]
fn test_tool_get_type_flow_case_insensitive() {
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({ "type_name": "fileindex" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    assert!(content["producers_count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_tool_get_type_flow_compact() {
    let index = make_test_index();
    let result = tool_get_type_flow(
        &index,
        &json!({ "type_name": "FileIndex", "compact": true }),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    // Compact mode should have columns/rows format
    assert!(content["producers"].get("columns").is_some());
    assert!(content["consumers"].get("columns").is_some());
}

#[test]
fn test_tool_get_type_flow_path_filter() {
    let index = make_test_index();
    let result = tool_get_type_flow(
        &index,
        &json!({ "type_name": "FileIndex", "path": "src/cache" }),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    // cache.rs doesn't produce FileIndex, so no producers from that path
    assert_eq!(content["producers_count"], 0);
}

#[test]
fn test_tool_get_type_flow_missing_param() {
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({}));
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_type_flow_whitespace_only_param() {
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({ "type_name": "   " }));
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_type_flow_with_limit() {
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({ "type_name": "FileIndex", "limit": 1 }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    let producers = content["producers"].as_array().unwrap();
    assert!(producers.len() <= 1);
}

#[test]
fn test_tool_get_type_flow_include_fields() {
    let index = make_test_index();
    // Without include_fields, the Cache.entries field should not appear
    let result = tool_get_type_flow(&index, &json!({ "type_name": "FileIndex" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    let consumers = content["consumers"].as_array().unwrap();
    let field_names: Vec<&str> = consumers
        .iter()
        .filter(|c| c["kind"].as_str().unwrap() == "field")
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        !field_names.contains(&"entries"),
        "Field should not appear without include_fields"
    );

    // With include_fields, the Cache.entries field should appear as a consumer
    let result = tool_get_type_flow(
        &index,
        &json!({ "type_name": "FileIndex", "include_fields": true }),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    let consumers = content["consumers"].as_array().unwrap();
    let field_names: Vec<&str> = consumers
        .iter()
        .filter(|c| c["kind"].as_str().unwrap() == "field")
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        field_names.contains(&"entries"),
        "Expected entries field as consumer with include_fields, got: {:?}",
        field_names
    );
}

#[test]
fn test_extract_types_c_function() {
    let sig = "int* create_buffer(size_t len)";
    let info = extract_types_from_signature(sig, &Language::C);
    assert!(
        info.param_types.contains(&"size_t".to_string()),
        "Expected size_t in params, got: {:?}",
        info.param_types
    );
    // "int" is primitive, but pointer return type — the identifier is "int" which is filtered
    // The function name is "create_buffer", return type token before it is "int*"
    // After normalize: "int" is primitive so filtered out
}

#[test]
fn test_extract_types_c_struct_return() {
    let sig = "FileIndex* parse_file(const char* path)";
    let info = extract_types_from_signature(sig, &Language::C);
    assert!(
        info.return_types.contains(&"FileIndex".to_string()),
        "Expected FileIndex in return types, got: {:?}",
        info.return_types
    );
}

#[test]
fn test_extract_types_cpp_method() {
    let sig = "virtual std::vector<Node> getChildren(TreeNode* root)";
    let info = extract_types_from_signature(sig, &Language::Cpp);
    assert!(
        info.return_types.contains(&"vector".to_string()),
        "Expected vector in return types, got: {:?}",
        info.return_types
    );
    assert!(
        info.return_types.contains(&"Node".to_string()),
        "Expected Node in return types, got: {:?}",
        info.return_types
    );
    assert!(
        info.param_types.contains(&"TreeNode".to_string()),
        "Expected TreeNode in param types, got: {:?}",
        info.param_types
    );
}

#[test]
fn test_extract_types_go_no_receiver_multi_return() {
    let sig = "func ParseFiles(paths []string) ([]FileIndex, error)";
    let info = extract_types_from_signature(sig, &Language::Go);
    assert!(
        info.return_types.contains(&"FileIndex".to_string()),
        "Expected FileIndex in return types, got: {:?}",
        info.return_types
    );
    // error is primitive, should be filtered
    assert!(
        !info
            .return_types
            .iter()
            .any(|t| t.eq_ignore_ascii_case("error"))
    );
}

#[test]
fn test_extract_types_go_receiver_with_multi_return() {
    let sig = "func (s *Server) Handle(req Request) (Response, error)";
    let info = extract_types_from_signature(sig, &Language::Go);
    assert!(
        info.param_types.contains(&"Request".to_string()),
        "Expected Request in param types, got: {:?}",
        info.param_types
    );
    assert!(
        info.return_types.contains(&"Response".to_string()),
        "Expected Response in return types, got: {:?}",
        info.return_types
    );
}

#[test]
fn test_extract_types_rust_nested_generics() {
    let sig = "pub fn parse(data: &str) -> Result<Vec<FileIndex>, Error>";
    let info = extract_types_from_signature(sig, &Language::Rust);
    assert!(
        info.return_types.contains(&"Result".to_string()),
        "Expected Result in return types, got: {:?}",
        info.return_types
    );
    assert!(
        info.return_types.contains(&"Vec".to_string()),
        "Expected Vec in return types, got: {:?}",
        info.return_types
    );
    assert!(
        info.return_types.contains(&"FileIndex".to_string()),
        "Expected FileIndex in return types, got: {:?}",
        info.return_types
    );
}

#[test]
fn test_tool_get_type_flow_producer_and_consumer() {
    // Path is both a param type (consumer) and could be a return type (producer)
    // in the test index: parse_file(path: &Path) → consumes Path
    let index = make_test_index();
    let result = tool_get_type_flow(&index, &json!({ "type_name": "Value" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    // Cache.get returns Option<&Value> → producer
    assert!(
        content["producers_count"].as_u64().unwrap() >= 1,
        "Expected at least 1 producer of Value"
    );
}

// -----------------------------------------------------------------------
// process_jsonrpc_message tests
// -----------------------------------------------------------------------

fn make_test_config() -> IndexConfig {
    IndexConfig {
        root: PathBuf::from("."),
        cache_dir: PathBuf::from(".indxr-cache"),
        max_file_size: 512,
        max_depth: None,
        exclude: vec![],
        no_gitignore: false,
    }
}

#[test]
fn test_process_jsonrpc_empty_line() {
    let config = make_test_config();
    let registry = ParserRegistry::new();
    let mut index = make_test_index();
    let result = process_jsonrpc_message("", &mut index, &config, &registry, Transport::Stdio);
    assert!(result.unwrap().is_none());
}

#[test]
fn test_process_jsonrpc_notification_returns_none() {
    let config = make_test_config();
    let registry = ParserRegistry::new();
    let mut index = make_test_index();
    let msg = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let result = process_jsonrpc_message(msg, &mut index, &config, &registry, Transport::Stdio);
    assert!(result.unwrap().is_none());
}

#[test]
fn test_process_jsonrpc_parse_error() {
    let config = make_test_config();
    let registry = ParserRegistry::new();
    let mut index = make_test_index();
    let result =
        process_jsonrpc_message("not json", &mut index, &config, &registry, Transport::Stdio);
    let err_resp = result.unwrap_err();
    let json = serde_json::to_value(&err_resp).unwrap();
    assert_eq!(json["error"]["code"], -32700);
}

#[test]
fn test_process_jsonrpc_initialize() {
    let config = make_test_config();
    let registry = ParserRegistry::new();
    let mut index = make_test_index();
    let msg = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let result = process_jsonrpc_message(msg, &mut index, &config, &registry, Transport::Stdio);
    let resp = result.unwrap().unwrap();
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(json["result"]["serverInfo"]["name"], "indxr");
}

#[test]
fn test_process_jsonrpc_tools_list() {
    let config = make_test_config();
    let registry = ParserRegistry::new();
    let mut index = make_test_index();
    let msg = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    let result = process_jsonrpc_message(msg, &mut index, &config, &registry, Transport::Stdio);
    let resp = result.unwrap().unwrap();
    let json = serde_json::to_value(&resp).unwrap();
    let tools = json["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty(), "tools/list should return tool definitions");
}

#[test]
fn test_process_jsonrpc_unknown_method() {
    let config = make_test_config();
    let registry = ParserRegistry::new();
    let mut index = make_test_index();
    let msg = r#"{"jsonrpc":"2.0","id":3,"method":"bogus/method","params":{}}"#;
    let result = process_jsonrpc_message(msg, &mut index, &config, &registry, Transport::Stdio);
    let resp = result.unwrap().unwrap();
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["error"]["code"], -32601);
}
