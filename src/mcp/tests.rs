use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::languages::Language;
use crate::model::declarations::{
    ComplexityMetrics, DeclKind, Declaration, RelKind, Relationship, Visibility,
};
use crate::model::{CodebaseIndex, FileIndex, Import, IndexStats, MemberIndex, WorkspaceIndex};
use crate::workspace::WorkspaceKind;

use super::helpers::*;
use super::tools::*;
use super::type_flow::*;
use super::{Transport, WikiStoreOption, process_jsonrpc_message};
use crate::indexer::{IndexConfig, WorkspaceConfig};
use crate::parser::ParserRegistry;

#[cfg(feature = "wiki")]
fn test_wiki_store() -> WikiStoreOption {
    None
}

#[cfg(not(feature = "wiki"))]
#[allow(clippy::unused_unit)]
fn test_wiki_store() -> WikiStoreOption {
    ()
}

/// Wrap a `CodebaseIndex` in a single-member `WorkspaceIndex` for testing.
fn wrap_workspace(index: CodebaseIndex) -> WorkspaceIndex {
    WorkspaceIndex {
        root: index.root.clone(),
        root_name: index.root_name.clone(),
        workspace_kind: WorkspaceKind::None,
        generated_at: index.generated_at.clone(),
        stats: IndexStats {
            total_files: index.stats.total_files,
            total_lines: index.stats.total_lines,
            languages: index.stats.languages.clone(),
            duration_ms: index.stats.duration_ms,
        },
        members: vec![MemberIndex {
            name: index.root_name.clone(),
            relative_path: PathBuf::from("."),
            index,
        }],
    }
}

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
fn test_tool_definitions_all_tools() {
    // With all_tools=true, all 23 tools should be present
    let defs = tool_definitions(false, true, false);
    let tools = defs["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"get_token_estimate"));
    assert!(names.contains(&"search_relevant"));
    assert!(names.contains(&"lookup_symbol"));
    assert!(names.contains(&"regenerate_index"));
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
    assert!(names.contains(&"list_workspace_members"));
    // 3 compound + 23 granular; +1 for wiki_generate when wiki feature is compiled
    #[cfg(feature = "wiki")]
    assert_eq!(names.len(), 27);
    #[cfg(not(feature = "wiki"))]
    assert_eq!(names.len(), 26);
}

#[test]
fn test_tool_definitions_default_excludes_extended() {
    // Default (all_tools=false) should only show compound tools
    let defs = tool_definitions(false, false, false);
    let tools = defs["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    // Compound tools present
    assert!(names.contains(&"find"));
    assert!(names.contains(&"summarize"));
    assert!(names.contains(&"read"));
    // Granular tools absent (moved to --all-tools)
    assert!(!names.contains(&"lookup_symbol"));
    assert!(!names.contains(&"search_relevant"));
    assert!(!names.contains(&"read_source"));
    assert!(!names.contains(&"get_file_summary"));
    assert!(!names.contains(&"get_hotspots"));
    assert!(!names.contains(&"get_health"));
    assert!(!names.contains(&"list_workspace_members"));
    assert!(!names.contains(&"regenerate_index"));
    // Wiki tools absent (no wiki available)
    assert!(!names.contains(&"wiki_search"));
    assert!(!names.contains(&"wiki_read"));
    assert!(!names.contains(&"wiki_status"));
    // compound tools only; +1 for wiki_generate when wiki feature is compiled
    #[cfg(feature = "wiki")]
    assert_eq!(names.len(), 4);
    #[cfg(not(feature = "wiki"))]
    assert_eq!(names.len(), 3);
}

#[test]
fn test_tool_definitions_member_param_only_in_workspace() {
    // Single-project: no member param
    let defs = tool_definitions(false, true, false);
    let tools = defs["tools"].as_array().unwrap();
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let props = tool["inputSchema"]["properties"].as_object().unwrap();
        assert!(
            !props.contains_key("member"),
            "{name} should not have member param in single-project mode"
        );
    }

    // Multi-member workspace: member param added
    let defs = tool_definitions(true, true, false);
    let tools = defs["tools"].as_array().unwrap();
    let skip = ["list_workspace_members", "regenerate_index"];
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        if skip.contains(&name) {
            continue;
        }
        let props = tool["inputSchema"]["properties"].as_object().unwrap();
        assert!(
            props.contains_key("member"),
            "{name} should have member param in workspace mode"
        );
    }
}

// -----------------------------------------------------------------------
// Extended tools remain callable even when not listed (all_tools=false)
// -----------------------------------------------------------------------

#[test]
fn test_extended_tools_callable_when_hidden() {
    // Even with all_tools=false, calling an extended tool should succeed (not
    // return "unknown tool").  The filtering only affects tools/list, not
    // tools/call dispatch.
    let (ws_config, _config) = make_test_config();
    let registry = ParserRegistry::new();
    let mut ws = wrap_workspace(make_test_index());

    for tool_name in &[
        "get_hotspots",
        "get_health",
        "get_token_estimate",
        "regenerate_index",
    ] {
        let msg = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{}","arguments":{{}}}}}}"#,
            tool_name
        );
        let result = process_jsonrpc_message(
            &msg,
            &mut ws,
            &ws_config,
            &registry,
            Transport::Stdio,
            false,
            &mut test_wiki_store(),
        );
        let resp = result.unwrap().unwrap();
        let json = serde_json::to_value(&resp).unwrap();
        // Should NOT be a "method not found" or "unknown tool" error
        assert!(
            json.get("error").is_none(),
            "{tool_name} should be callable even when all_tools=false, got error: {}",
            json
        );
    }
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
    let ws = wrap_workspace(index);
    let result = handle_tool_call(&ws, "nonexistent_tool", &json!({}));
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
    assert!(name_score >= sig_score);
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
    let ws = wrap_workspace(index);
    let result = tool_batch_file_summaries(
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_batch_file_summaries(&ws, &json!({ "glob": "*.rs" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 2);
}

#[test]
fn test_tool_batch_file_summaries_no_args() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_batch_file_summaries(&ws, &json!({}));
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Provide either"));
}

#[test]
fn test_tool_get_callers() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_callers(&ws, &json!({ "symbol": "parse_file" }));
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
    let ws = wrap_workspace(index);
    // "get" should not match "budget" or "widget" — word-boundary matching
    let result = tool_get_callers(&ws, &json!({ "symbol": "nonexistent_sym" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 0);
}

#[test]
fn test_tool_get_public_api() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_public_api(&ws, &json!({}));
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
    let ws = wrap_workspace(index);
    let result = tool_get_public_api(&ws, &json!({ "path": "src/cache.rs" }));
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
    let ws = wrap_workspace(index);
    let result = tool_explain_symbol(&ws, &json!({ "name": "parse_file" }));
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
    let ws = wrap_workspace(index);
    let result = tool_explain_symbol(&ws, &json!({ "name": "CACHE" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 1);
    assert_eq!(content["symbols"][0]["name"], "Cache");
}

#[test]
fn test_tool_explain_symbol_not_found() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_explain_symbol(&ws, &json!({ "name": "nonexistent" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 0);
}

#[test]
fn test_tool_get_related_tests() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_related_tests(&ws, &json!({ "symbol": "parse_file" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_related_tests(
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_get_related_tests(&ws, &json!({ "symbol": "nonexistent" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["count"], 0);
}

#[test]
fn test_tool_get_token_estimate_directory() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_token_estimate(&ws, &json!({ "directory": "src" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["file_count"], 2);
    assert!(content["total_tokens"].as_u64().unwrap() > 0);
}

#[test]
fn test_tool_get_token_estimate_glob() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_token_estimate(&ws, &json!({ "glob": "*.rs" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["file_count"], 2);
}

#[test]
fn test_tool_get_token_estimate_no_args() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_token_estimate(&ws, &json!({}));
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
    let ws = wrap_workspace(index);
    let result = tool_lookup_symbol(
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_lookup_symbol(&ws, &json!({ "name": "parse_file" }));
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
    let ws = wrap_workspace(index);
    let result = tool_list_declarations(
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_search_signatures(
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_search_relevant(
        &ws,
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
    let ws = wrap_workspace(index);
    // Filter to only structs
    let result = tool_search_relevant(
        &ws,
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
    let ws = wrap_workspace(index);
    // Filter to only functions
    let result = tool_search_relevant(
        &ws,
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

    let ws = wrap_workspace(index);
    let result = tool_read_source(
        &ws,
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

    let ws = wrap_workspace(index);
    let result = tool_read_source(
        &ws,
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

    let ws = wrap_workspace(index);
    let result = tool_read_source(
        &ws,
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

    let ws = wrap_workspace(index);
    let result = tool_read_source(
        &ws,
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

    let ws = wrap_workspace(index);
    let result = tool_batch_file_summaries(&ws, &json!({ "glob": "*.rs" }));
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
    let ws = wrap_workspace(index);
    // "get" is a method on Cache — should only match word-boundary occurrences
    let result = tool_get_callers(&ws, &json!({ "symbol": "get" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_dependency_graph(&ws, &json!({ "format": "mermaid" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_dependency_graph(&ws, &json!({ "format": "dot" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["format"], "dot");
    let graph = content["graph"].as_str().unwrap();
    assert!(graph.contains("digraph dependencies"));
}

#[test]
fn test_tool_dependency_graph_file_level_json() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_dependency_graph(&ws, &json!({ "format": "json" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_dependency_graph(&ws, &json!({ "level": "symbol", "format": "json" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_dependency_graph(&ws, &json!({ "path": "src/cache", "format": "json" }));
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
    let ws = wrap_workspace(index);
    // Full graph: cache.rs → parser.rs (at least 1 edge)
    let full = tool_get_dependency_graph(&ws, &json!({ "format": "json" }));
    let full_content: Value =
        serde_json::from_str(full["content"][0]["text"].as_str().unwrap()).unwrap();
    let full_edges = full_content["edges"].as_u64().unwrap();
    assert!(full_edges >= 1, "Full graph should have at least 1 edge");

    // depth=0 scoped to cache: no hops allowed, so no edges
    let d0 = tool_get_dependency_graph(
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_get_dependency_graph(&ws, &json!({}));
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
    let ws = wrap_workspace(index);
    let result = tool_get_hotspots(&ws, &json!({}));
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
    let ws = wrap_workspace(index);
    let result = tool_get_hotspots(&ws, &json!({ "min_complexity": 10 }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_hotspots(&ws, &json!({ "path": "cache" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // cache.rs has no complexity data
    assert_eq!(content["total"], 0);
}

#[test]
fn test_tool_get_hotspots_sort_by_complexity() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_hotspots(&ws, &json!({ "sort_by": "complexity" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let hotspots = content["hotspots"].as_array().unwrap();
    assert_eq!(hotspots[0]["cyclomatic"], 12);
    assert_eq!(hotspots[1]["cyclomatic"], 2);
}

#[test]
fn test_tool_get_hotspots_compact() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_hotspots(&ws, &json!({ "compact": true }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let hotspots = &content["hotspots"];
    assert!(hotspots["columns"].is_array());
    assert!(hotspots["rows"].is_array());
}

#[test]
fn test_tool_get_hotspots_total_before_truncate() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    // limit=1 but total should reflect all matching hotspots (2)
    let result = tool_get_hotspots(&ws, &json!({ "limit": 1 }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_health(&ws, &json!({}));
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
    let ws = wrap_workspace(index);
    let result = tool_get_health(&ws, &json!({ "path": "src/cache" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_health(&ws, &json!({}));
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
    WorkspaceIndex,
    WorkspaceConfig,
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
    let ws_config = WorkspaceConfig {
        workspace: crate::workspace::single_root_workspace(&config.root),
        template: config.clone(),
    };
    let registry = crate::parser::ParserRegistry::new();
    let ws = wrap_workspace(index);
    (ws, ws_config, registry)
}

#[test]
fn test_tool_get_diff_summary_both_params_error() {
    let (ws, ws_config, registry) = make_diff_test_fixtures();
    let args = json!({"since_ref": "main", "pr": 42});
    let result = tool_get_diff_summary(&ws, &ws_config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("not both"),
        "Expected mutual exclusion error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_neither_param_error() {
    let (ws, ws_config, registry) = make_diff_test_fixtures();
    let args = json!({});
    let result = tool_get_diff_summary(&ws, &ws_config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("since_ref") && text.contains("pr"),
        "Expected missing param error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_invalid_pr_zero() {
    let (ws, ws_config, registry) = make_diff_test_fixtures();
    let args = json!({"pr": 0});
    let result = tool_get_diff_summary(&ws, &ws_config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("positive integer"),
        "Expected positive integer error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_invalid_pr_negative() {
    let (ws, ws_config, registry) = make_diff_test_fixtures();
    let args = json!({"pr": -1});
    let result = tool_get_diff_summary(&ws, &ws_config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("positive integer"),
        "Expected positive integer error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_invalid_pr_string() {
    let (ws, ws_config, registry) = make_diff_test_fixtures();
    let args = json!({"pr": "not-a-number"});
    let result = tool_get_diff_summary(&ws, &ws_config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("positive integer"),
        "Expected positive integer error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_empty_since_ref() {
    let (ws, ws_config, registry) = make_diff_test_fixtures();
    let args = json!({"since_ref": ""});
    let result = tool_get_diff_summary(&ws, &ws_config, &registry, &args);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("must not be empty"),
        "Expected empty ref error, got: {text}"
    );
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_diff_summary_whitespace_since_ref() {
    let (ws, ws_config, registry) = make_diff_test_fixtures();
    let args = json!({"since_ref": "   "});
    let result = tool_get_diff_summary(&ws, &ws_config, &registry, &args);
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
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "FileIndex" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "Path" }));
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
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "NonexistentType" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    assert_eq!(content["producers_count"], 0);
    assert_eq!(content["consumers_count"], 0);
}

#[test]
fn test_tool_get_type_flow_case_insensitive() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "fileindex" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    assert!(content["producers_count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_tool_get_type_flow_compact() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "FileIndex", "compact": true }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    // Compact mode should have columns/rows format
    assert!(content["producers"].get("columns").is_some());
    assert!(content["consumers"].get("columns").is_some());
}

#[test]
fn test_tool_get_type_flow_path_filter() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({}));
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_type_flow_whitespace_only_param() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "   " }));
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn test_tool_get_type_flow_with_limit() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "FileIndex", "limit": 1 }));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();

    let producers = content["producers"].as_array().unwrap();
    assert!(producers.len() <= 1);
}

#[test]
fn test_tool_get_type_flow_include_fields() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    // Without include_fields, the Cache.entries field should not appear
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "FileIndex" }));
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
        &ws,
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
    let ws = wrap_workspace(index);
    let result = tool_get_type_flow(&ws, &json!({ "type_name": "Value" }));
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

fn make_test_config() -> (WorkspaceConfig, IndexConfig) {
    let config = IndexConfig {
        root: PathBuf::from("."),
        cache_dir: PathBuf::from(".indxr-cache"),
        max_file_size: 512,
        max_depth: None,
        exclude: vec![],
        no_gitignore: false,
    };
    let ws_config = WorkspaceConfig {
        workspace: crate::workspace::single_root_workspace(&config.root),
        template: config.clone(),
    };
    (ws_config, config)
}

#[test]
fn test_process_jsonrpc_empty_line() {
    let (ws_config, _config) = make_test_config();
    let registry = ParserRegistry::new();
    let mut ws = wrap_workspace(make_test_index());
    let result = process_jsonrpc_message(
        "",
        &mut ws,
        &ws_config,
        &registry,
        Transport::Stdio,
        false,
        &mut test_wiki_store(),
    );
    assert!(result.unwrap().is_none());
}

#[test]
fn test_process_jsonrpc_notification_returns_none() {
    let (ws_config, _config) = make_test_config();
    let registry = ParserRegistry::new();
    let mut ws = wrap_workspace(make_test_index());
    let msg = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let result = process_jsonrpc_message(
        msg,
        &mut ws,
        &ws_config,
        &registry,
        Transport::Stdio,
        false,
        &mut test_wiki_store(),
    );
    assert!(result.unwrap().is_none());
}

#[test]
fn test_process_jsonrpc_parse_error() {
    let (ws_config, _config) = make_test_config();
    let registry = ParserRegistry::new();
    let mut ws = wrap_workspace(make_test_index());
    let result = process_jsonrpc_message(
        "not json",
        &mut ws,
        &ws_config,
        &registry,
        Transport::Stdio,
        false,
        &mut test_wiki_store(),
    );
    let err_resp = result.unwrap_err();
    let json = serde_json::to_value(&err_resp).unwrap();
    assert_eq!(json["error"]["code"], -32700);
}

#[test]
fn test_process_jsonrpc_initialize() {
    let (ws_config, _config) = make_test_config();
    let registry = ParserRegistry::new();
    let mut ws = wrap_workspace(make_test_index());
    let msg = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let result = process_jsonrpc_message(
        msg,
        &mut ws,
        &ws_config,
        &registry,
        Transport::Stdio,
        false,
        &mut test_wiki_store(),
    );
    let resp = result.unwrap().unwrap();
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(json["result"]["serverInfo"]["name"], "indxr");
}

#[test]
fn test_process_jsonrpc_tools_list() {
    let (ws_config, _config) = make_test_config();
    let registry = ParserRegistry::new();
    let mut ws = wrap_workspace(make_test_index());
    let msg = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    let result = process_jsonrpc_message(
        msg,
        &mut ws,
        &ws_config,
        &registry,
        Transport::Stdio,
        false,
        &mut test_wiki_store(),
    );
    let resp = result.unwrap().unwrap();
    let json = serde_json::to_value(&resp).unwrap();
    let tools = json["result"]["tools"].as_array().unwrap();
    assert!(
        !tools.is_empty(),
        "tools/list should return tool definitions"
    );
}

#[test]
fn test_process_jsonrpc_unknown_method() {
    let (ws_config, _config) = make_test_config();
    let registry = ParserRegistry::new();
    let mut ws = wrap_workspace(make_test_index());
    let msg = r#"{"jsonrpc":"2.0","id":3,"method":"bogus/method","params":{}}"#;
    let result = process_jsonrpc_message(
        msg,
        &mut ws,
        &ws_config,
        &registry,
        Transport::Stdio,
        false,
        &mut test_wiki_store(),
    );
    let resp = result.unwrap().unwrap();
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["error"]["code"], -32601);
}

// -----------------------------------------------------------------------
// Multi-member workspace tests
// -----------------------------------------------------------------------

/// Build a two-member workspace for testing cross-member behavior.
fn make_multi_member_workspace() -> WorkspaceIndex {
    // Member 1: "frontend" with a React component
    let component = Declaration::new(
        DeclKind::Function,
        "App".to_string(),
        "export function App(): JSX.Element".to_string(),
        Visibility::Public,
        1,
    );
    let hook = Declaration::new(
        DeclKind::Function,
        "useAuth".to_string(),
        "export function useAuth(): AuthState".to_string(),
        Visibility::Public,
        10,
    );
    let frontend_file = FileIndex {
        path: PathBuf::from("src/App.tsx"),
        language: Language::TypeScript,
        size: 500,
        lines: 30,
        imports: vec![Import {
            text: "import { useAuth } from './hooks';".to_string(),
        }],
        declarations: vec![component],
    };
    let hooks_file = FileIndex {
        path: PathBuf::from("src/hooks.ts"),
        language: Language::TypeScript,
        size: 300,
        lines: 20,
        imports: vec![],
        declarations: vec![hook],
    };
    let frontend_index = CodebaseIndex {
        root: PathBuf::from("/tmp/monorepo/packages/frontend"),
        root_name: "frontend".to_string(),
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        stats: IndexStats {
            total_files: 2,
            total_lines: 50,
            languages: HashMap::from([("TypeScript".to_string(), 2)]),
            duration_ms: 5,
        },
        tree: vec![],
        files: vec![frontend_file, hooks_file],
    };

    // Member 2: "backend" with a Rust API
    let handler = {
        let mut d = Declaration::new(
            DeclKind::Function,
            "handle_login".to_string(),
            "pub async fn handle_login(req: Request) -> Response".to_string(),
            Visibility::Public,
            5,
        );
        d.complexity = Some(ComplexityMetrics {
            cyclomatic: 8,
            max_nesting: 3,
            param_count: 1,
        });
        d.body_lines = Some(25);
        d
    };
    let auth_struct = Declaration::new(
        DeclKind::Struct,
        "AuthState".to_string(),
        "pub struct AuthState".to_string(),
        Visibility::Public,
        1,
    );
    let backend_file = FileIndex {
        path: PathBuf::from("src/handlers.rs"),
        language: Language::Rust,
        size: 800,
        lines: 60,
        imports: vec![Import {
            text: "use crate::auth::AuthState;".to_string(),
        }],
        declarations: vec![handler],
    };
    let auth_file = FileIndex {
        path: PathBuf::from("src/auth.rs"),
        language: Language::Rust,
        size: 400,
        lines: 30,
        imports: vec![],
        declarations: vec![auth_struct],
    };
    let backend_index = CodebaseIndex {
        root: PathBuf::from("/tmp/monorepo/packages/backend"),
        root_name: "backend".to_string(),
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        stats: IndexStats {
            total_files: 2,
            total_lines: 90,
            languages: HashMap::from([("Rust".to_string(), 2)]),
            duration_ms: 5,
        },
        tree: vec![],
        files: vec![backend_file, auth_file],
    };

    WorkspaceIndex {
        root: PathBuf::from("/tmp/monorepo"),
        root_name: "monorepo".to_string(),
        workspace_kind: WorkspaceKind::Npm,
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        stats: IndexStats {
            total_files: 4,
            total_lines: 140,
            languages: HashMap::from([("TypeScript".to_string(), 2), ("Rust".to_string(), 2)]),
            duration_ms: 10,
        },
        members: vec![
            MemberIndex {
                name: "frontend".to_string(),
                relative_path: PathBuf::from("packages/frontend"),
                index: frontend_index,
            },
            MemberIndex {
                name: "backend".to_string(),
                relative_path: PathBuf::from("packages/backend"),
                index: backend_index,
            },
        ],
    }
}

#[test]
fn test_list_workspace_members() {
    let ws = make_multi_member_workspace();
    let result = handle_tool_call(&ws, "list_workspace_members", &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["workspace_kind"], "npm");
    assert_eq!(content["member_count"], 2);
    let members = content["members"].as_array().unwrap();
    let names: Vec<&str> = members
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"frontend"));
    assert!(names.contains(&"backend"));
}

#[test]
fn test_lookup_symbol_across_members() {
    let ws = make_multi_member_workspace();
    // "Auth" should match symbols in both members
    let result = tool_lookup_symbol(&ws, &json!({ "name": "Auth" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let symbols = content["symbols"].as_array().unwrap();
    let names: Vec<&str> = symbols
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    // Should find useAuth (frontend) and AuthState (backend)
    assert!(names.contains(&"useAuth"));
    assert!(names.contains(&"AuthState"));
}

#[test]
fn test_lookup_symbol_scoped_to_member() {
    let ws = make_multi_member_workspace();
    // Scoping to "backend" should only return AuthState, not useAuth
    let result = tool_lookup_symbol(&ws, &json!({ "name": "Auth", "member": "backend" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let symbols = content["symbols"].as_array().unwrap();
    let names: Vec<&str> = symbols
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"AuthState"));
    assert!(!names.contains(&"useAuth"));
}

#[test]
fn test_member_param_invalid_returns_error() {
    let ws = make_multi_member_workspace();
    let result = tool_lookup_symbol(&ws, &json!({ "name": "Auth", "member": "nonexistent" }));
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Unknown workspace member"));
}

#[test]
fn test_get_stats_multi_member() {
    let ws = make_multi_member_workspace();
    let result = tool_get_stats(&ws, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["total_files"], 4);
    assert_eq!(content["total_lines"], 140);
    assert_eq!(content["member_count"], 2);
    assert_eq!(content["workspace_kind"], "npm");
}

#[test]
fn test_get_stats_single_member() {
    let ws = make_multi_member_workspace();
    let result = tool_get_stats(&ws, &json!({ "member": "frontend" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["total_files"], 2);
    assert_eq!(content["total_lines"], 50);
}

#[test]
fn test_get_file_summary_auto_resolves_member() {
    let ws = make_multi_member_workspace();
    // Should auto-resolve to backend member
    let result = tool_get_file_summary(&ws, &json!({ "path": "src/handlers.rs" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["file"], "src/handlers.rs");
    assert_eq!(content["language"], "Rust");
}

#[test]
fn test_get_callers_across_members() {
    let ws = make_multi_member_workspace();
    // AuthState is referenced in backend's handlers.rs imports
    let result = tool_get_callers(&ws, &json!({ "symbol": "AuthState" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_get_hotspots_across_members() {
    let ws = make_multi_member_workspace();
    let result = tool_get_hotspots(&ws, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Only handle_login has complexity data
    let hotspots = content["hotspots"].as_array().unwrap();
    assert!(!hotspots.is_empty());
    assert_eq!(hotspots[0]["name"], "handle_login");
}

#[test]
fn test_get_health_across_members() {
    let ws = make_multi_member_workspace();
    let result = tool_get_health(&ws, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // 3 functions total across both members: App, useAuth, handle_login
    assert_eq!(content["total_functions"], 3);
}

#[test]
fn test_get_public_api_across_members() {
    let ws = make_multi_member_workspace();
    let result = tool_get_public_api(&ws, &json!({}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Public: App, useAuth (frontend), handle_login, AuthState (backend)
    assert_eq!(content["count"], 4);
}

#[test]
fn test_search_relevant_across_members() {
    let ws = make_multi_member_workspace();
    let result = tool_search_relevant(&ws, &json!({ "query": "auth" }));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    // Should find auth-related symbols from both members
    assert!(content["matches"].as_u64().unwrap() >= 2);
}

#[test]
fn test_find_member_by_path() {
    let ws = make_multi_member_workspace();
    let m = ws.find_member_by_path("src/App.tsx");
    assert!(m.is_some());
    assert_eq!(m.unwrap().name, "frontend");

    let m = ws.find_member_by_path("src/handlers.rs");
    assert!(m.is_some());
    assert_eq!(m.unwrap().name, "backend");

    let m = ws.find_member_by_path("nonexistent.rs");
    assert!(m.is_none());
}

#[test]
fn test_find_member_by_path_ambiguous() {
    // Build a workspace where two members both have files whose full paths end with
    // "src/lib.rs" but with different prefixes. A suffix query for "src/lib.rs" should
    // return None because it's ambiguous (matches both members).
    let make_member = |name: &str, root: &str| {
        let file = FileIndex {
            path: PathBuf::from(format!("crates/{}/src/lib.rs", name)),
            language: Language::Rust,
            size: 100,
            lines: 10,
            imports: vec![],
            declarations: vec![],
        };
        MemberIndex {
            name: name.to_string(),
            relative_path: PathBuf::from(format!("crates/{}", name)),
            index: CodebaseIndex {
                root: PathBuf::from(root),
                root_name: name.to_string(),
                generated_at: "2026-01-01T00:00:00Z".to_string(),
                stats: IndexStats {
                    total_files: 1,
                    total_lines: 10,
                    languages: HashMap::from([("Rust".to_string(), 1)]),
                    duration_ms: 1,
                },
                tree: vec![],
                files: vec![file],
            },
        }
    };

    let ws = WorkspaceIndex {
        root: PathBuf::from("/tmp/ambiguous"),
        root_name: "ambiguous".to_string(),
        workspace_kind: WorkspaceKind::Cargo,
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        stats: IndexStats {
            total_files: 2,
            total_lines: 20,
            languages: HashMap::from([("Rust".to_string(), 2)]),
            duration_ms: 2,
        },
        members: vec![
            make_member("alpha", "/tmp/ambiguous/crates/alpha"),
            make_member("beta", "/tmp/ambiguous/crates/beta"),
        ],
    };

    // Both members have a file ending with "src/lib.rs" — should be ambiguous
    let m = ws.find_member_by_path("src/lib.rs");
    assert!(
        m.is_none(),
        "expected None for ambiguous path, got {:?}",
        m.map(|m| &m.name)
    );

    // Shorter suffix also ambiguous
    let m = ws.find_member_by_path("lib.rs");
    assert!(
        m.is_none(),
        "expected None for ambiguous suffix, got {:?}",
        m.map(|m| &m.name)
    );

    // Exact full path resolves unambiguously
    let m = ws.find_member_by_path("crates/alpha/src/lib.rs");
    assert_eq!(m.unwrap().name, "alpha");

    // Member-specific prefix resolves unambiguously (longer file_path wins)
    let m = ws.find_member_by_path("beta/src/lib.rs");
    assert_eq!(m.unwrap().name, "beta");
}

#[test]
fn test_find_member_by_path_same_relative_path() {
    // When two members both have files with the identical relative path (e.g. both
    // have "src/lib.rs"), auto-resolution should return None (ambiguous).
    let make_member = |name: &str| {
        let file = FileIndex {
            path: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            size: 100,
            lines: 10,
            imports: vec![],
            declarations: vec![],
        };
        MemberIndex {
            name: name.to_string(),
            relative_path: PathBuf::from(format!("crates/{}", name)),
            index: CodebaseIndex {
                root: PathBuf::from(format!("/tmp/ws/crates/{}", name)),
                root_name: name.to_string(),
                generated_at: "2026-01-01T00:00:00Z".to_string(),
                stats: IndexStats {
                    total_files: 1,
                    total_lines: 10,
                    languages: HashMap::from([("Rust".to_string(), 1)]),
                    duration_ms: 1,
                },
                tree: vec![],
                files: vec![file],
            },
        }
    };

    let ws = WorkspaceIndex {
        root: PathBuf::from("/tmp/ws"),
        root_name: "ws".to_string(),
        workspace_kind: WorkspaceKind::Cargo,
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        stats: IndexStats {
            total_files: 2,
            total_lines: 20,
            languages: HashMap::from([("Rust".to_string(), 2)]),
            duration_ms: 2,
        },
        members: vec![make_member("alpha"), make_member("beta")],
    };

    // Both members have "src/lib.rs" as exact path — should be ambiguous
    let m = ws.find_member_by_path("src/lib.rs");
    assert!(
        m.is_none(),
        "expected None for ambiguous exact path, got {:?}",
        m.map(|m| &m.name)
    );
}

#[test]
fn test_find_member_by_name() {
    let ws = make_multi_member_workspace();
    assert!(ws.find_member("frontend").is_some());
    assert!(ws.find_member("FRONTEND").is_some()); // case-insensitive
    assert!(ws.find_member("nonexistent").is_none());
}

#[test]
fn test_workspace_is_single() {
    let ws = make_multi_member_workspace();
    assert!(!ws.is_single());

    let single = wrap_workspace(make_test_index());
    assert!(single.is_single());
}

// ---------------------------------------------------------------------------
// Compound tool tests
// ---------------------------------------------------------------------------

#[test]
fn test_compound_find_relevant_mode() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "find", &json!({"query": "parse"}));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    // Should find parse_file via search_relevant
    assert!(content["matches"].as_u64().unwrap() >= 1);
}

#[test]
fn test_compound_find_symbol_mode() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "parse_file", "mode": "symbol"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert!(content["matches"].as_u64().unwrap() >= 1);
}

#[test]
fn test_compound_find_callers_mode() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "parse_file", "mode": "callers"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    // cache.rs imports parse_file
    assert!(content["count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_compound_find_signature_mode() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "-> Result<", "mode": "signature"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert!(content["matches"].as_u64().unwrap() >= 1);
}

#[test]
fn test_compound_find_invalid_mode_returns_error() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "parse", "mode": "invalid_mode"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Unknown find mode"));
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn test_compound_find_relevant_with_kind_filter() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "find", &json!({"query": "Cache", "kind": "struct"}));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert!(content["matches"].as_u64().unwrap() >= 1);
}

#[test]
fn test_compound_find_missing_query_returns_error() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "find", &json!({}));
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn test_compound_summarize_file_path() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "summarize", &json!({"path": "src/parser.rs"}));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert_eq!(content["file"], "src/parser.rs");
    assert!(!content["declarations"].as_array().unwrap().is_empty());
}

#[test]
fn test_compound_summarize_glob_pattern() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "summarize", &json!({"path": "*.rs"}));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert!(content["count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_compound_summarize_symbol_name() {
    let ws = wrap_workspace(make_test_index());
    // "Cache" has no "/" and no file extension → symbol
    let result = handle_tool_call(&ws, "summarize", &json!({"path": "Cache"}));
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert_eq!(content["name"], "Cache");
}

#[test]
fn test_compound_summarize_bare_filename_routes_to_file() {
    // "main.rs" has no "/" but has a file extension → should route to get_file_summary, not explain_symbol
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "summarize", &json!({"path": "parser.rs"}));
    let text = result["content"][0]["text"].as_str().unwrap();
    // Should NOT try to explain "parser.rs" as a symbol
    // It will return a "File not found" error because our test paths are "src/parser.rs",
    // but the important thing is it doesn't route to explain_symbol
    assert!(!text.contains("\"name\":\"parser.rs\""));
}

#[test]
fn test_compound_summarize_public_scope() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(
        &ws,
        "summarize",
        &json!({"path": "src/parser.rs", "scope": "public"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    // get_public_api returns only public declarations
    let decls = content["declarations"].as_array().unwrap();
    assert!(!decls.is_empty());
    // All returned declarations should be public (parse_file is public, internal_helper is not)
    let names: Vec<&str> = decls.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"parse_file"));
    assert!(!names.contains(&"internal_helper"));
}

#[test]
fn test_compound_summarize_missing_path_returns_error() {
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "summarize", &json!({}));
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn test_compound_read_forwards_to_read_source() {
    use std::io::Write as IoWrite;

    let mut index = make_test_index();
    let dir = std::env::temp_dir().join("indxr_test_compound_read");
    let _ = std::fs::create_dir_all(dir.join("src"));
    let source = "// lines 1-9\n\n\n\n\n\n\n\n\npub fn parse_file() {\n    do_stuff();\n}\n";
    let mut f = std::fs::File::create(dir.join("src/parser.rs")).unwrap();
    f.write_all(source.as_bytes()).unwrap();
    index.root = dir.clone();

    let ws = wrap_workspace(index);
    let result = handle_tool_call(
        &ws,
        "read",
        &json!({"path": "src/parser.rs", "symbol": "parse_file"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("parse_file"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_compound_read_with_collapse() {
    use std::io::Write as IoWrite;

    let mut index = make_test_index();
    let dir = std::env::temp_dir().join("indxr_test_compound_read_collapse");
    let _ = std::fs::create_dir_all(dir.join("src"));
    let source = "// lines 1-9\n\n\n\n\n\n\n\n\nfn parse_file() {\n    if true {\n        nested();\n    }\n}\n";
    let mut f = std::fs::File::create(dir.join("src/parser.rs")).unwrap();
    f.write_all(source.as_bytes()).unwrap();
    index.root = dir.clone();

    let ws = wrap_workspace(index);
    let result = handle_tool_call(
        &ws,
        "read",
        &json!({"path": "src/parser.rs", "symbol": "parse_file", "collapse": true}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert!(content["collapsed"].as_bool().unwrap());

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// looks_like_file edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_looks_like_file_recognized_extensions() {
    use super::tools::looks_like_file;
    assert!(looks_like_file("main.rs"));
    assert!(looks_like_file("app.py"));
    assert!(looks_like_file("index.ts"));
    assert!(looks_like_file("file.test.rs")); // multiple dots
    assert!(looks_like_file("config.toml"));
    assert!(looks_like_file("styles.css"));
}

#[test]
fn test_looks_like_file_not_files() {
    use super::tools::looks_like_file;
    assert!(!looks_like_file("Cache")); // symbol name
    assert!(!looks_like_file("parse_file")); // symbol name
    assert!(!looks_like_file("src")); // directory
    assert!(!looks_like_file(".")); // current dir
    assert!(!looks_like_file("")); // empty string
    assert!(!looks_like_file(".gitignore")); // dotfile (no recognized ext)
    assert!(!looks_like_file("Makefile")); // no extension
}

// ---------------------------------------------------------------------------
// summarize: directory routing
// ---------------------------------------------------------------------------

#[test]
fn test_compound_summarize_bare_directory_routes_to_file_summary() {
    // "src" is a known directory prefix (files are "src/parser.rs", etc.)
    // It should NOT route to explain_symbol.
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "summarize", &json!({"path": "src"}));
    let text = result["content"][0]["text"].as_str().unwrap();
    // Should not try to explain "src" as a symbol
    assert!(!text.contains("\"name\":\"src\""));
}

#[test]
fn test_compound_summarize_dot_routes_to_file_summary() {
    // "." should route to get_file_summary, not explain_symbol
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(&ws, "summarize", &json!({"path": "."}));
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("\"name\":\".\""));
}

#[test]
fn test_compound_find_kind_ignored_for_non_relevant_modes() {
    // kind param should be silently ignored for non-relevant modes
    let ws = wrap_workspace(make_test_index());
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "parse_file", "mode": "symbol", "kind": "struct"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    // Should still find parse_file (kind is ignored in symbol mode)
    assert!(content["matches"].as_u64().unwrap() >= 1);
}

// ---------------------------------------------------------------------------
// Compound tool: member forwarding in workspace mode
// ---------------------------------------------------------------------------

#[test]
fn test_compound_find_forwards_member_symbol_mode() {
    let ws = make_multi_member_workspace();
    // "Auth" exists in both members, but scoping to "backend" should only find AuthState
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "Auth", "mode": "symbol", "member": "backend"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    let rows = content["rows"].as_array().unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r[2].as_str().unwrap()).collect();
    assert!(names.contains(&"AuthState"));
    assert!(!names.contains(&"useAuth"));
}

#[test]
fn test_compound_find_forwards_member_relevant_mode() {
    let ws = make_multi_member_workspace();
    // "handle_login" only exists in backend — scoping to "frontend" should not find it
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "handle_login", "member": "frontend"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert_eq!(content["matches"].as_u64().unwrap(), 0);
}

#[test]
fn test_compound_find_forwards_member_callers_mode() {
    let ws = make_multi_member_workspace();
    // Scoping callers to "frontend" — backend's import of AuthState should not appear
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "useAuth", "mode": "callers", "member": "frontend"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    // App.tsx imports useAuth — should appear in frontend-scoped results
    assert!(content["count"].as_u64().unwrap() >= 1);
    // Compound find callers now returns compact format (columns + rows)
    let columns = content["columns"].as_array().unwrap();
    let file_idx = columns
        .iter()
        .position(|c| c.as_str() == Some("file"))
        .unwrap();
    let rows = content["rows"].as_array().unwrap();
    let files: Vec<&str> = rows
        .iter()
        .filter_map(|r| r.as_array().and_then(|a| a[file_idx].as_str()))
        .collect();
    assert!(files.iter().any(|f| f.contains("App.tsx")));
}

#[test]
fn test_compound_find_forwards_member_signature_mode() {
    let ws = make_multi_member_workspace();
    // "-> Response" only exists in backend's handle_login
    // Scoping to "frontend" should find nothing
    let result = handle_tool_call(
        &ws,
        "find",
        &json!({"query": "-> Response", "mode": "signature", "member": "frontend"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    assert_eq!(content["matches"].as_u64().unwrap(), 0);
}

#[test]
fn test_compound_summarize_forwards_member() {
    let ws = make_multi_member_workspace();
    // summarize with glob scoped to backend — should only find backend files
    let result = handle_tool_call(
        &ws,
        "summarize",
        &json!({"path": "src/*.rs", "member": "backend"}),
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let content: Value = serde_json::from_str(text).unwrap();
    // Compound summarize with glob now returns compact format (columns + rows)
    let columns = content["columns"].as_array().unwrap();
    let file_idx = columns
        .iter()
        .position(|c| c.as_str() == Some("file"))
        .unwrap();
    let rows = content["rows"].as_array().unwrap();
    let files: Vec<&str> = rows
        .iter()
        .filter_map(|r| r.as_array().and_then(|a| a[file_idx].as_str()))
        .collect();
    assert!(
        files
            .iter()
            .any(|f| f.contains("handlers.rs") || f.contains("auth.rs"))
    );
    assert!(
        !files
            .iter()
            .any(|f| f.contains("App.tsx") || f.contains("hooks.ts"))
    );
}

#[test]
fn test_tool_get_callers_compact() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    // cache.rs imports parse_file, so get_callers should find it
    let result = tool_get_callers(&ws, &json!({"symbol": "parse_file", "compact": true}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["symbol"], "parse_file");
    assert!(content["count"].as_u64().unwrap() >= 1);
    // Compact format: columns + rows
    let columns = content["columns"].as_array().unwrap();
    assert!(columns.contains(&json!("file")));
    assert!(columns.contains(&json!("name")));
    assert!(columns.contains(&json!("kind")));
    assert!(columns.contains(&json!("match_type")));
    let rows = content["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    // Import refs should have kind = "import" and null line
    let kind_idx = columns.iter().position(|c| c == "kind").unwrap();
    let line_idx = columns.iter().position(|c| c == "line").unwrap();
    let match_idx = columns.iter().position(|c| c == "match_type").unwrap();
    let import_row = rows
        .iter()
        .find(|r| r.as_array().unwrap()[match_idx] == "import")
        .expect("should have an import reference");
    assert_eq!(import_row.as_array().unwrap()[kind_idx], "import");
    assert!(import_row.as_array().unwrap()[line_idx].is_null());
}

#[test]
fn test_tool_get_callers_non_compact() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    // Without compact, should return "references" array of objects
    let result = tool_get_callers(&ws, &json!({"symbol": "parse_file"}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["references"].is_array());
    let refs = content["references"].as_array().unwrap();
    assert!(!refs.is_empty());
    assert!(refs[0]["file"].is_string());
}

#[test]
fn test_tool_batch_file_summaries_compact() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    let result = tool_batch_file_summaries(&ws, &json!({"glob": "src/*.rs", "compact": true}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["count"].as_u64().unwrap() >= 1);
    // Compact format: columns + rows
    let columns = content["columns"].as_array().unwrap();
    assert!(columns.contains(&json!("file")));
    assert!(columns.contains(&json!("language")));
    assert!(columns.contains(&json!("lines")));
    let rows = content["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    // Should NOT have "summaries" key
    assert!(content.get("summaries").is_none());
}

#[test]
fn test_tool_batch_file_summaries_non_compact() {
    let index = make_test_index();
    let ws = wrap_workspace(index);
    // Without compact, should return "summaries" array of objects
    let result = tool_batch_file_summaries(&ws, &json!({"glob": "src/*.rs"}));
    let content: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(content["summaries"].is_array());
    let summaries = content["summaries"].as_array().unwrap();
    assert!(!summaries.is_empty());
    assert!(summaries[0]["file"].is_string());
}

// ---------------------------------------------------------------------------
// Wiki tool tests
// ---------------------------------------------------------------------------

#[cfg(feature = "wiki")]
mod wiki_tests {
    use super::*;
    use crate::wiki::page::{Frontmatter, PageType, WikiPage};
    use crate::wiki::store::{WikiManifest, WikiStore};

    fn make_test_wiki_store() -> WikiStore {
        WikiStore {
            root: PathBuf::from("/tmp/test-wiki"),
            manifest: WikiManifest {
                version: 1,
                generated_at_ref: "abc1234".to_string(),
                generated_at: "2026-04-05T10:00:00Z".to_string(),
                pages: vec![],
            },
            pages: vec![
                WikiPage {
                    frontmatter: Frontmatter {
                        id: "architecture".to_string(),
                        title: "Architecture Overview".to_string(),
                        page_type: PageType::Architecture,
                        source_files: vec![
                            "src/main.rs".to_string(),
                            "src/indexer.rs".to_string(),
                        ],
                        generated_at_ref: "abc1234".to_string(),
                        generated_at: "2026-04-05T10:00:00Z".to_string(),
                        links_to: vec!["mod-mcp".to_string()],
                        covers: vec![
                            "fn:main".to_string(),
                            "fn:build_workspace_index".to_string(),
                        ],
                        contradictions: vec![],
                        failures: vec![],
                    },
                    content: "# Architecture\n\nThis codebase uses tree-sitter for parsing and rayon for parallelism.\n\n## Key Components\n- Indexer\n- MCP Server\n- Parser".to_string(),
                },
                WikiPage {
                    frontmatter: Frontmatter {
                        id: "mod-mcp".to_string(),
                        title: "MCP Server Module".to_string(),
                        page_type: PageType::Module,
                        source_files: vec![
                            "src/mcp/mod.rs".to_string(),
                            "src/mcp/tools.rs".to_string(),
                        ],
                        generated_at_ref: "abc1234".to_string(),
                        generated_at: "2026-04-05T10:00:00Z".to_string(),
                        links_to: vec!["architecture".to_string()],
                        covers: vec![
                            "fn:run_mcp_server".to_string(),
                            "fn:handle_tool_call".to_string(),
                        ],
                        contradictions: vec![],
                        failures: vec![],
                    },
                    content: "# MCP Server\n\nHandles JSON-RPC protocol for tool dispatch.\n\n## Tools\nThe server exposes structural analysis tools via MCP protocol.".to_string(),
                },
                WikiPage {
                    frontmatter: Frontmatter {
                        id: "mod-parser".to_string(),
                        title: "Parser Module".to_string(),
                        page_type: PageType::Module,
                        source_files: vec!["src/parser/mod.rs".to_string()],
                        generated_at_ref: "abc1234".to_string(),
                        generated_at: "2026-04-05T10:00:00Z".to_string(),
                        links_to: vec![],
                        covers: vec!["struct:ParserRegistry".to_string()],
                        contradictions: vec![],
                        failures: vec![],
                    },
                    content: "# Parser\n\nTree-sitter and regex-based parsing for 27 languages.".to_string(),
                },
            ],
        }
    }

    #[test]
    fn test_wiki_search_by_title() {
        let store = make_test_wiki_store();
        let result = tool_wiki_search(&store, &json!({"query": "MCP Server"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(content["matches"].as_u64().unwrap() > 0);
        let results = content["results"].as_array().unwrap();
        // MCP Server Module should be top result
        assert_eq!(results[0]["id"], "mod-mcp");
    }

    #[test]
    fn test_wiki_search_by_covers() {
        let store = make_test_wiki_store();
        let result = tool_wiki_search(&store, &json!({"query": "run_mcp_server"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(content["matches"].as_u64().unwrap() > 0);
        let results = content["results"].as_array().unwrap();
        assert_eq!(results[0]["id"], "mod-mcp");
    }

    #[test]
    fn test_wiki_search_by_content() {
        let store = make_test_wiki_store();
        let result = tool_wiki_search(&store, &json!({"query": "tree-sitter"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(content["matches"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_wiki_search_no_match() {
        let store = make_test_wiki_store();
        let result = tool_wiki_search(&store, &json!({"query": "xyznonexistent"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["matches"].as_u64().unwrap(), 0);
    }

    #[test]
    fn test_wiki_search_with_limit() {
        let store = make_test_wiki_store();
        let result = tool_wiki_search(&store, &json!({"query": "module", "limit": 1}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        let results = content["results"].as_array().unwrap();
        assert!(results.len() <= 1);
    }

    #[test]
    fn test_wiki_search_missing_query() {
        let store = make_test_wiki_store();
        let result = tool_wiki_search(&store, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter"));
    }

    #[test]
    fn test_wiki_read_by_exact_id() {
        let store = make_test_wiki_store();
        let result = tool_wiki_read(&store, &json!({"page": "architecture"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["id"], "architecture");
        assert_eq!(content["title"], "Architecture Overview");
        let text = content["content"].as_str().unwrap();
        assert!(text.contains("Architecture"));
        assert!(text.contains("tree-sitter"));
    }

    #[test]
    fn test_wiki_read_by_title() {
        let store = make_test_wiki_store();
        let result = tool_wiki_read(&store, &json!({"page": "MCP Server Module"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["id"], "mod-mcp");
    }

    #[test]
    fn test_wiki_read_partial_match() {
        let store = make_test_wiki_store();
        let result = tool_wiki_read(&store, &json!({"page": "parser"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["id"], "mod-parser");
    }

    #[test]
    fn test_wiki_read_not_found() {
        let store = make_test_wiki_store();
        let result = tool_wiki_read(&store, &json!({"page": "nonexistent-page"}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("not found"));
        assert!(text.contains("Available pages"));
    }

    #[test]
    fn test_wiki_read_missing_param() {
        let store = make_test_wiki_store();
        let result = tool_wiki_read(&store, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter"));
    }

    #[test]
    fn test_wiki_status() {
        let store = make_test_wiki_store();
        let ws = wrap_workspace(make_test_index());
        let result = tool_wiki_status(&store, &ws);
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["pages"].as_u64().unwrap(), 3);
        assert_eq!(content["generated_at_ref"], "abc1234");
        assert!(content["staleness"].is_string());
        assert!(content["coverage"]["total_files"].is_number());
        assert!(content["coverage"]["percentage"].is_string());
        let by_type = content["pages_by_type"].as_object().unwrap();
        assert_eq!(by_type["architecture"].as_u64().unwrap(), 1);
        assert_eq!(by_type["module"].as_u64().unwrap(), 2);
    }

    #[test]
    fn test_wiki_tools_listed_when_available() {
        let defs = tool_definitions(false, false, true);
        let tools = defs["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"wiki_search"));
        assert!(names.contains(&"wiki_read"));
        assert!(names.contains(&"wiki_status"));
        assert!(names.contains(&"wiki_suggest_contribution"));
        assert!(names.contains(&"wiki_compound"));
        assert!(names.contains(&"wiki_record_failure"));
        assert_eq!(names.len(), 12); // 3 compound + 9 wiki
    }

    #[test]
    fn test_wiki_tools_hidden_when_unavailable() {
        let defs = tool_definitions(false, false, false);
        let tools = defs["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        // wiki_generate should always be listed (it's how you create a wiki)
        assert!(names.contains(&"wiki_generate"));
        // The rest require an existing wiki
        assert!(!names.contains(&"wiki_search"));
        assert!(!names.contains(&"wiki_read"));
        assert!(!names.contains(&"wiki_status"));
        assert!(!names.contains(&"wiki_contribute"));
        assert!(!names.contains(&"wiki_update"));
    }

    #[test]
    fn test_wiki_search_unicode_content() {
        let store = WikiStore {
            root: PathBuf::from("/tmp/test-wiki"),
            manifest: WikiManifest {
                version: 1,
                generated_at_ref: "abc1234".to_string(),
                generated_at: "2026-04-05T10:00:00Z".to_string(),
                pages: vec![],
            },
            pages: vec![WikiPage {
                frontmatter: Frontmatter {
                    id: "unicode-test".to_string(),
                    title: "Unicode Test".to_string(),
                    page_type: PageType::Module,
                    source_files: vec![],
                    generated_at_ref: "abc1234".to_string(),
                    generated_at: "2026-04-05T10:00:00Z".to_string(),
                    links_to: vec![],
                    covers: vec![],
                    contradictions: vec![],
                    failures: vec![],
                },
                content: "# Ünïcödé Tëst\n\nThis module uses résumé and naïve approaches with Ñoño patterns.".to_string(),
            }],
        };
        // Should not panic on non-ASCII content
        let result = tool_wiki_search(&store, &json!({"query": "résumé"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(content["matches"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_wiki_contribute_create_new_page() {
        let mut store = make_test_wiki_store();
        let initial_count = store.pages.len();
        let result = tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "error-handling",
                "title": "Error Handling Patterns",
                "content": "# Error Handling\n\nThis codebase uses `anyhow` for error propagation.\n\nSee also [[architecture]] for the overall design.",
                "page_type": "topic",
                "source_files": ["src/error.rs"]
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "created");
        assert_eq!(content["page_id"], "error-handling");
        assert_eq!(content["title"], "Error Handling Patterns");
        assert_eq!(content["type"], "topic");
        assert_eq!(
            content["total_wiki_pages"].as_u64().unwrap(),
            (initial_count + 1) as u64
        );
        // Check wiki links were extracted
        let links = content["links_to"].as_array().unwrap();
        assert!(links.iter().any(|l| l == "architecture"));

        // Page should be findable in store
        let page = store.get_page("error-handling").unwrap();
        assert_eq!(page.frontmatter.title, "Error Handling Patterns");
        assert_eq!(page.frontmatter.source_files, vec!["src/error.rs"]);
    }

    #[test]
    fn test_wiki_contribute_update_existing_page() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "mod-mcp",
                "content": "# MCP Server (Updated)\n\nNow with wiki_contribute support.\n\nSee [[mod-parser]] for parser details.",
                "source_files": ["src/mcp/wiki.rs"]
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "updated");
        assert_eq!(content["page_id"], "mod-mcp");

        let page = store.get_page("mod-mcp").unwrap();
        assert!(page.content.contains("wiki_contribute support"));
        // Original source files should be preserved + new one merged
        assert!(
            page.frontmatter
                .source_files
                .contains(&"src/mcp/mod.rs".to_string())
        );
        assert!(
            page.frontmatter
                .source_files
                .contains(&"src/mcp/tools.rs".to_string())
        );
        assert!(
            page.frontmatter
                .source_files
                .contains(&"src/mcp/wiki.rs".to_string())
        );
        // Links should reflect new content
        assert!(
            page.frontmatter
                .links_to
                .contains(&"mod-parser".to_string())
        );
        // page_type should be preserved
        assert_eq!(page.frontmatter.page_type, PageType::Module);
    }

    #[test]
    fn test_wiki_contribute_missing_page_param() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_contribute(&mut store, &json!({"content": "test"}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter: page"));
    }

    #[test]
    fn test_wiki_contribute_missing_content_param() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_contribute(&mut store, &json!({"page": "test"}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter: content"));
    }

    #[test]
    fn test_wiki_contribute_new_page_requires_title() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_contribute(
            &mut store,
            &json!({"page": "new-page", "content": "some content"}),
        );
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter: title"));
    }

    #[test]
    fn test_wiki_contribute_invalid_page_id() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_contribute(
            &mut store,
            &json!({"page": "../../etc", "title": "Bad", "content": "test"}),
        );
        let text = result["content"][0]["text"].as_str().unwrap();
        // sanitize_id("../../etc") = "etc" which is valid, so this should succeed
        let content: Value = serde_json::from_str(text).unwrap();
        assert_eq!(content["page_id"], "etc");
    }

    #[test]
    fn test_wiki_contribute_default_page_type() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "my-analysis",
                "title": "My Analysis",
                "content": "Some analysis."
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["type"], "topic");
    }

    #[test]
    fn test_wiki_contribute_listed_in_tools() {
        let defs = tool_definitions(false, false, true);
        let tools = defs["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"wiki_contribute"));
    }

    #[test]
    fn test_wiki_generate_and_update_listed_in_tools() {
        let defs = tool_definitions(false, false, true);
        let tools = defs["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"wiki_generate"));
        assert!(names.contains(&"wiki_update"));
    }

    /// Build a workspace rooted in a temp dir (for tests that write to disk).
    fn make_workspace_in(root: &std::path::Path) -> WorkspaceIndex {
        let mut index = make_test_index();
        index.root = root.to_path_buf();
        index.root_name = root.file_name().unwrap().to_string_lossy().to_string();
        wrap_workspace(index)
    }

    #[test]
    fn test_wiki_generate_returns_context() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = make_workspace_in(tmp.path());
        let result = tool_wiki_generate(&ws, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        let content: Value = serde_json::from_str(text).unwrap();
        assert_eq!(content["action"], "initialized");
        assert!(
            content["context"]
                .as_str()
                .unwrap()
                .contains("Codebase Structural Index")
        );
        assert!(
            content["instructions"]
                .as_str()
                .unwrap()
                .contains("wiki_contribute")
        );
    }

    #[test]
    fn test_wiki_generate_blocks_when_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = make_workspace_in(tmp.path());

        // First call should succeed
        let result = tool_wiki_generate(&ws, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        let content: Value = serde_json::from_str(text).unwrap();
        assert_eq!(content["action"], "initialized");

        // Verify manifest was created
        let manifest_path = tmp.path().join(".indxr").join("wiki").join("manifest.yaml");
        assert!(
            manifest_path.exists(),
            "manifest.yaml should exist after wiki_generate"
        );

        // Second call without force should fail
        let result2 = tool_wiki_generate(&ws, &json!({}));
        let text2 = result2["content"][0]["text"].as_str().unwrap();
        assert!(
            text2.contains("Wiki already exists"),
            "Expected 'Wiki already exists' but got: {}",
            text2
        );

        // With force=true should succeed again
        let result3 = tool_wiki_generate(&ws, &json!({"force": true}));
        let text3 = result3["content"][0]["text"].as_str().unwrap();
        let content3: Value = serde_json::from_str(text3).unwrap();
        assert_eq!(content3["action"], "initialized");
    }

    #[test]
    fn test_wiki_update_no_wiki_dispatch() {
        // Verify that wiki_update through handle_tools_call returns an error when no wiki exists
        let index = make_test_index();
        let mut ws = wrap_workspace(index);
        let config = WorkspaceConfig {
            workspace: crate::workspace::single_root_workspace(&ws.root),
            template: IndexConfig {
                root: ws.root.clone(),
                cache_dir: ws.root.join(".cache"),
                max_file_size: 512,
                max_depth: None,
                exclude: vec![],
                no_gitignore: false,
            },
        };
        let registry = ParserRegistry::new();
        let mut wiki_store: WikiStoreOption = None;

        let resp = crate::mcp::handle_tools_call(
            json!(1),
            &mut ws,
            &config,
            &registry,
            &json!({"name": "wiki_update", "arguments": {}}),
            &mut wiki_store,
        );
        let text = resp.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("No wiki found"));
    }

    #[test]
    fn test_wiki_update_no_changes() {
        // Use the real repo root so git commands work
        let root = std::env::current_dir().unwrap();
        let mut store = make_test_wiki_store();
        store.root = root.join(".indxr").join("wiki");
        let mut index = make_test_index();
        index.root = root.clone();
        let ws = wrap_workspace(index);
        // Use HEAD so there are no changes
        let registry = ParserRegistry::new();
        let result = tool_wiki_update(&store, &ws, &registry, &json!({"since": "HEAD"}));
        let text = result["content"][0]["text"].as_str().unwrap();
        let content: Value = serde_json::from_str(text).unwrap();
        assert_eq!(content["action"], "no_changes");
    }

    #[test]
    fn test_wiki_update_empty_ref() {
        let mut store = make_test_wiki_store();
        store.manifest.generated_at_ref = String::new();
        let index = make_test_index();
        let ws = wrap_workspace(index);
        let registry = ParserRegistry::new();
        let result = tool_wiki_update(&store, &ws, &registry, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("No git ref to diff against"));
    }

    #[test]
    fn test_wiki_suggest_contribution_update_source_page() {
        let store = make_test_wiki_store();
        let result = tool_wiki_suggest_contribution(
            &store,
            &json!({
                "synthesis": "The MCP server handles JSON-RPC tool dispatch with structural analysis.",
                "source_pages": ["mod-mcp"]
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["suggestion"], "update");
        assert_eq!(content["target_page"], "mod-mcp");
        // Source page boost (50) + word overlaps should give high confidence
        assert!(content["confidence"].as_u64().unwrap() >= 50);
    }

    #[test]
    fn test_wiki_suggest_contribution_create_no_match() {
        let store = make_test_wiki_store();
        let result = tool_wiki_suggest_contribution(
            &store,
            &json!({
                "synthesis": "Kubernetes deployment orchestration with Helm charts."
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["suggestion"], "create");
        assert!(
            content["suggested_id"]
                .as_str()
                .unwrap()
                .starts_with("topic-")
        );
    }

    #[test]
    fn test_wiki_suggest_contribution_empty_store() {
        let store = WikiStore {
            root: PathBuf::from("/tmp/test-wiki"),
            manifest: WikiManifest {
                version: 1,
                generated_at_ref: "abc1234".to_string(),
                generated_at: "2026-04-05T10:00:00Z".to_string(),
                pages: vec![],
            },
            pages: vec![],
        };
        let result = tool_wiki_suggest_contribution(
            &store,
            &json!({
                "synthesis": "Some analysis about error handling patterns."
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["suggestion"], "create");
        assert_eq!(content["confidence"].as_u64().unwrap(), 0);
    }

    #[test]
    fn test_wiki_suggest_contribution_missing_synthesis() {
        let store = make_test_wiki_store();
        let result = tool_wiki_suggest_contribution(&store, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter: synthesis"));
    }

    #[test]
    fn test_wiki_suggest_contribution_short_words_only() {
        let store = make_test_wiki_store();
        let result =
            tool_wiki_suggest_contribution(&store, &json!({ "synthesis": "a b c do it go on" }));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["suggestion"], "create");
        // All words < 4 chars, so suggested_id should be the fallback
        assert_eq!(content["suggested_id"], "topic-new");
    }

    #[test]
    fn test_wiki_contribute_resolve_contradictions_roundtrip() {
        let mut store = make_test_wiki_store();

        // Step 1: Add a contradiction to a page
        tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "mod-mcp",
                "content": "# MCP Server\n\nUpdated content.",
                "contradictions": [
                    { "description": "Wiki stated sync but code is async", "source": "src/mcp/mod.rs:383" }
                ]
            }),
        );

        // Verify the contradiction exists and is unresolved
        let page = store
            .pages
            .iter()
            .find(|p| p.frontmatter.id == "mod-mcp")
            .unwrap();
        assert_eq!(page.frontmatter.contradictions.len(), 1);
        assert!(page.frontmatter.contradictions[0].resolved_at.is_none());

        // Step 2: Resolve contradictions
        tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "mod-mcp",
                "content": "# MCP Server\n\nFully updated content with async.",
                "resolve_contradictions": true
            }),
        );

        // Verify the contradiction is now resolved
        let page = store
            .pages
            .iter()
            .find(|p| p.frontmatter.id == "mod-mcp")
            .unwrap();
        assert_eq!(page.frontmatter.contradictions.len(), 1);
        assert!(page.frontmatter.contradictions[0].resolved_at.is_some());
    }

    /// Helper: create a test wiki store backed by a real temp directory (for tests that write).
    fn make_test_wiki_store_on_disk(tmp: &std::path::Path) -> WikiStore {
        let wiki_dir = tmp.join("wiki");
        std::fs::create_dir_all(&wiki_dir).unwrap();
        let mut store = make_test_wiki_store();
        store.root = wiki_dir;
        // Save the initial state so save_incremental can work
        store.save().unwrap();
        store
    }

    #[test]
    fn test_wiki_compound_appends_to_existing_page() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_test_wiki_store_on_disk(tmp.path());
        let initial_count = store.pages.len();

        // "mod-mcp" is a source page (score += 50), plus word overlap with "server" and "tools"
        let result = tool_wiki_compound(
            &mut store,
            &json!({
                "synthesis": "The MCP server dispatches tools via JSON-RPC protocol handlers.",
                "source_pages": ["mod-mcp"]
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "compounded");
        assert_eq!(content["page_id"], "mod-mcp");
        assert_eq!(
            content["total_wiki_pages"].as_u64().unwrap(),
            initial_count as u64
        );

        // Verify the synthesis was appended to the page content
        let page = store.get_page("mod-mcp").unwrap();
        assert!(page.content.contains("Compounded insight"));
        assert!(page.content.contains("JSON-RPC protocol handlers"));
    }

    #[test]
    fn test_wiki_compound_creates_new_page() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_test_wiki_store_on_disk(tmp.path());
        let initial_count = store.pages.len();

        // No matching pages for this synthesis
        let result = tool_wiki_compound(
            &mut store,
            &json!({
                "synthesis": "Kubernetes orchestration patterns with Helm charts for deployment.",
                "title": "Kubernetes Deployment"
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "created");
        assert_eq!(content["page_id"], "kubernetesdeployment");
        assert_eq!(content["title"], "Kubernetes Deployment");
        assert_eq!(
            content["total_wiki_pages"].as_u64().unwrap(),
            (initial_count + 1) as u64
        );

        // Page should exist in store
        let page = store.get_page("kubernetesdeployment").unwrap();
        assert!(page.content.contains("Kubernetes orchestration"));
    }

    #[test]
    fn test_wiki_compound_score_threshold_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_test_wiki_store_on_disk(tmp.path());
        let initial_count = store.pages.len();

        // Only has a few word overlaps but no source_page boost — should score below 20
        // "parser" overlaps with title "Parser Module" (+10), "languages" overlaps content (+2) = 12
        let result = tool_wiki_compound(
            &mut store,
            &json!({
                "synthesis": "The parser handles multiple languages.",
                "title": "Parser Languages"
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        // Score 12 < 20 threshold, so it should create a new page
        assert_eq!(content["action"], "created");
        assert_eq!(
            content["total_wiki_pages"].as_u64().unwrap(),
            (initial_count + 1) as u64
        );
    }

    #[test]
    fn test_wiki_compound_page_id_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_test_wiki_store_on_disk(tmp.path());

        // Create a page that will collide with derived ID
        let result1 = tool_wiki_compound(
            &mut store,
            &json!({
                "synthesis": "Kubernetes deployment strategies for microservices.",
                "title": "K8s Strategies"
            }),
        );
        let content1: Value =
            serde_json::from_str(result1["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content1["action"], "created");
        assert_eq!(content1["page_id"], "k8sstrategies");

        // Create another page with the same title — should get a suffixed ID
        let result2 = tool_wiki_compound(
            &mut store,
            &json!({
                "synthesis": "Advanced Kubernetes deployment strategies.",
                "title": "K8s Strategies"
            }),
        );
        let content2: Value =
            serde_json::from_str(result2["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content2["action"], "created");
        assert_eq!(content2["page_id"], "k8sstrategies-2");

        // Both pages should exist
        assert!(store.get_page("k8sstrategies").is_some());
        assert!(store.get_page("k8sstrategies-2").is_some());
    }

    #[test]
    fn test_wiki_compound_missing_synthesis() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_test_wiki_store_on_disk(tmp.path());
        let result = tool_wiki_compound(&mut store, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter: synthesis"));
    }

    #[test]
    fn test_wiki_compound_empty_synthesis() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_test_wiki_store_on_disk(tmp.path());
        let result = tool_wiki_compound(&mut store, &json!({"synthesis": ""}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Missing required parameter: synthesis"));
    }

    #[test]
    fn test_wiki_compound_extracts_wiki_links() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_test_wiki_store_on_disk(tmp.path());

        let result = tool_wiki_compound(
            &mut store,
            &json!({
                "synthesis": "The MCP server works closely with the parser. See [[mod-parser]] for details.",
                "source_pages": ["mod-mcp"]
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "compounded");

        // The page should now link to mod-parser (extracted from [[mod-parser]])
        let page = store.get_page("mod-mcp").unwrap();
        assert!(
            page.frontmatter
                .links_to
                .contains(&"mod-parser".to_string())
        );
    }

    #[test]
    fn test_wiki_status_failure_stats() {
        let mut store = make_test_wiki_store();
        let ws = wrap_workspace(make_test_index());

        // Baseline: no failures
        let result = tool_wiki_status(&store, &ws);
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["failures"]["total"], 0);
        assert_eq!(content["failures"]["unresolved"], 0);
        assert!(
            content["failures"]["pages_with_failures"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        // Add an unresolved failure
        tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Tool dispatch fails",
                "attempted_fix": "Added fallback",
                "diagnosis": "Wrong error path",
                "page": "mod-mcp"
            }),
        );

        let result = tool_wiki_status(&store, &ws);
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["failures"]["total"], 1);
        assert_eq!(content["failures"]["unresolved"], 1);
        assert!(
            content["failures"]["pages_with_failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "mod-mcp")
        );

        // Add a resolved failure (has actual_fix)
        tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Parser panics on empty input",
                "attempted_fix": "Added nil check",
                "diagnosis": "Wrong layer",
                "actual_fix": "Added early return in tokenizer",
                "page": "mod-parser"
            }),
        );

        let result = tool_wiki_status(&store, &ws);
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["failures"]["total"], 2);
        assert_eq!(content["failures"]["unresolved"], 1);
        // Only mod-mcp has unresolved failures
        let pages_with = content["failures"]["pages_with_failures"]
            .as_array()
            .unwrap();
        assert_eq!(pages_with.len(), 1);
        assert_eq!(pages_with[0], "mod-mcp");
    }

    // -----------------------------------------------------------------------
    // wiki_record_failure tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_wiki_record_failure_auto_routes() {
        let mut store = make_test_wiki_store();
        // Use words that overlap with page title "MCP Server Module" and content
        // "Handles JSON-RPC protocol for tool dispatch"
        let result = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Server module crashes on JSON-RPC protocol dispatch",
                "attempted_fix": "Added a default match arm in handle_tool_call",
                "diagnosis": "The panic was in tool_definitions, not handle_tool_call dispatch"
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "recorded_on_existing");
        // Should route to mod-mcp because symptom+diagnosis overlap with title and content
        assert_eq!(content["page_id"], "mod-mcp");
        assert_eq!(content["failure_index"], 0);

        let page = store.get_page("mod-mcp").unwrap();
        assert_eq!(page.frontmatter.failures.len(), 1);
        assert!(
            page.frontmatter.failures[0]
                .symptom
                .contains("Server module crashes")
        );
        assert!(page.frontmatter.failures[0].resolved_at.is_none());
    }

    #[test]
    fn test_wiki_record_failure_explicit_page() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Parsing fails for nested generics",
                "attempted_fix": "Increased recursion limit",
                "diagnosis": "The grammar rule was wrong, not the limit",
                "page": "mod-parser",
                "source_files": ["src/parser/mod.rs"]
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "recorded_on_existing");
        assert_eq!(content["page_id"], "mod-parser");

        let page = store.get_page("mod-parser").unwrap();
        assert_eq!(page.frontmatter.failures.len(), 1);
        assert_eq!(
            page.frontmatter.failures[0].source_files,
            vec!["src/parser/mod.rs"]
        );
    }

    #[test]
    fn test_wiki_record_failure_creates_new_page() {
        let mut store = make_test_wiki_store();
        let initial_count = store.pages.len();
        let result = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Database connection pool exhausted under load",
                "attempted_fix": "Increased pool size to 200",
                "diagnosis": "Connections were leaked by unclosed transactions",
                "actual_fix": "Added drop guard for transactions"
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "recorded_on_new");
        assert_eq!(content["failure_index"], 0);
        assert_eq!(
            content["total_wiki_pages"].as_u64().unwrap(),
            (initial_count + 1) as u64
        );

        // The failure should have actual_fix and resolved_at set
        let page_id = content["page_id"].as_str().unwrap();
        let page = store.get_page(page_id).unwrap();
        assert_eq!(page.frontmatter.failures.len(), 1);
        assert_eq!(
            page.frontmatter.failures[0].actual_fix.as_deref(),
            Some("Added drop guard for transactions")
        );
        assert!(page.frontmatter.failures[0].resolved_at.is_some());
    }

    #[test]
    fn test_wiki_record_failure_missing_params() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "something broke"
                // missing attempted_fix and diagnosis
            }),
        );
        assert!(result.get("isError").is_some());
    }

    #[test]
    fn test_wiki_contribute_with_failures() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "mod-mcp",
                "content": "# MCP Server\n\nUpdated with failure tracking.",
                "failures": [{
                    "symptom": "Tool dispatch hangs on large payloads",
                    "attempted_fix": "Added timeout to tool handler",
                    "diagnosis": "The hang was in serialization, not dispatch"
                }]
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "updated");

        let page = store.get_page("mod-mcp").unwrap();
        assert_eq!(page.frontmatter.failures.len(), 1);
        assert!(
            page.frontmatter.failures[0]
                .symptom
                .contains("Tool dispatch hangs")
        );
    }

    #[test]
    fn test_wiki_contribute_resolve_failures() {
        let mut store = make_test_wiki_store();

        // First, add a failure
        tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "MCP server crashes on startup",
                "attempted_fix": "Checked port binding",
                "diagnosis": "Config file was malformed",
                "page": "mod-mcp"
            }),
        );
        let page = store.get_page("mod-mcp").unwrap();
        assert!(page.frontmatter.failures[0].resolved_at.is_none());

        // Now resolve via wiki_contribute
        tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "mod-mcp",
                "content": "# MCP Server\n\nFixed startup crash.",
                "resolve_failures": true
            }),
        );
        let page = store.get_page("mod-mcp").unwrap();
        assert!(page.frontmatter.failures[0].resolved_at.is_some());
    }

    #[test]
    fn test_wiki_search_failure_counts() {
        let mut store = make_test_wiki_store();
        // Add a failure to mod-mcp
        tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "MCP tool returns empty results",
                "attempted_fix": "Fixed query parsing",
                "diagnosis": "Index was stale",
                "page": "mod-mcp"
            }),
        );

        let result = tool_wiki_search(&store, &json!({"query": "MCP Server"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        let mcp_result = content["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "mod-mcp")
            .unwrap();
        assert_eq!(mcp_result["failure_count"], 1);
        assert_eq!(mcp_result["unresolved_failures"], 1);
    }

    #[test]
    fn test_wiki_search_matches_symptoms() {
        let mut store = make_test_wiki_store();
        // Add a failure with a specific error message
        tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "ECONNREFUSED when calling external API",
                "attempted_fix": "Added retry logic",
                "diagnosis": "DNS resolution was failing",
                "page": "mod-mcp"
            }),
        );

        // Search by the error message — should find the page via failure symptom
        let result = tool_wiki_search(&store, &json!({"query": "ECONNREFUSED"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(content["matches"].as_u64().unwrap() > 0);
        assert_eq!(content["results"][0]["id"], "mod-mcp");
    }

    #[test]
    fn test_wiki_search_include_failures_flag() {
        let mut store = make_test_wiki_store();
        tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "MCP tool timeout",
                "attempted_fix": "Increased timeout",
                "diagnosis": "Deadlock in tool handler",
                "page": "mod-mcp"
            }),
        );

        // Without flag — no failure details
        let result = tool_wiki_search(&store, &json!({"query": "MCP"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        let mcp_result = content["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "mod-mcp")
            .unwrap();
        assert!(mcp_result.get("failures").is_none());

        // With flag — failure details included
        let result = tool_wiki_search(&store, &json!({"query": "MCP", "include_failures": true}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        let mcp_result = content["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "mod-mcp")
            .unwrap();
        let failures = mcp_result["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0]["symptom"], "MCP tool timeout");
        assert_eq!(failures[0]["diagnosis"], "Deadlock in tool handler");
    }

    #[test]
    fn test_wiki_read_shows_failures() {
        let mut store = make_test_wiki_store();
        tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "MCP server returns malformed JSON",
                "attempted_fix": "Fixed serialization in response builder",
                "diagnosis": "The issue was in the content encoding, not serialization",
                "source_files": ["src/mcp/mod.rs"],
                "page": "mod-mcp"
            }),
        );

        let result = tool_wiki_read(&store, &json!({"page": "mod-mcp"}));
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        let failures = content["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0]["index"], 0);
        assert_eq!(failures[0]["symptom"], "MCP server returns malformed JSON");
        assert!(failures[0].get("source_files").is_some());
    }

    #[test]
    fn test_wiki_record_failure_dedup_suffix() {
        let mut store = make_test_wiki_store();
        // First failure creates a new page with a derived topic ID
        let result1 = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Quantum flux capacitor overload in warp core",
                "attempted_fix": "Reversed polarity",
                "diagnosis": "Wrong subspace frequency"
            }),
        );
        let content1: Value =
            serde_json::from_str(result1["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content1["action"], "recorded_on_new");
        let page_id1 = content1["page_id"].as_str().unwrap().to_string();

        // Second failure with similar symptom should create a page with suffix
        let result2 = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Quantum flux capacitor overload in warp core again",
                "attempted_fix": "Replaced capacitor",
                "diagnosis": "Capacitor was defective"
            }),
        );
        let content2: Value =
            serde_json::from_str(result2["content"][0]["text"].as_str().unwrap()).unwrap();
        // Should either route to the first page (if score >= 30) or create a new suffixed page
        let page_id2 = content2["page_id"].as_str().unwrap().to_string();
        if content2["action"] == "recorded_on_new" {
            // If it created a new page, the ID should have a -2 suffix
            assert!(
                page_id2.ends_with("-2"),
                "Expected suffixed page ID, got: {}",
                page_id2
            );
        } else {
            // If it routed to existing, that's also valid
            assert_eq!(page_id2, page_id1);
        }
    }

    #[test]
    fn test_wiki_record_failure_empty_page_id() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Something broke",
                "attempted_fix": "Tried something",
                "diagnosis": "Did not work",
                "page": "///..."
            }),
        );
        assert!(result.get("isError").is_some());
    }

    #[test]
    fn test_wiki_record_failure_yaml_special_chars() {
        let mut store = make_test_wiki_store();
        let result = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "Error: \"unexpected token\" at line 42:\n  expected: '}'\n  got: ':'",
                "attempted_fix": "Added escape for `:` in YAML output — didn't help",
                "diagnosis": "The colon was inside a quoted string; real issue was\nmultiline value not being block-scalar formatted",
                "page": "mod-parser"
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(content["action"], "recorded_on_existing");
        assert_eq!(content["page_id"], "mod-parser");

        // Verify roundtrip through page serialization
        let page = store.get_page("mod-parser").unwrap();
        assert_eq!(page.frontmatter.failures.len(), 1);
        assert!(
            page.frontmatter.failures[0]
                .symptom
                .contains("unexpected token")
        );
        assert!(page.frontmatter.failures[0].diagnosis.contains("multiline"));
    }

    #[test]
    fn test_wiki_record_failure_topic_failure_fallback() {
        let mut store = make_test_wiki_store();
        // Create a minimal page to simulate that the symptom words are all short
        // (< 4 chars), so derive_topic_id returns "topic-new" and sanitize_id keeps it.
        // We need a symptom with only short words so the derive path is exercised.
        let result = tool_wiki_record_failure(
            &mut store,
            &json!({
                "symptom": "it is an odd bug",
                "attempted_fix": "no fix yet",
                "diagnosis": "no clue"
            }),
        );
        let content: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        // Should create a new page since short words won't match existing pages
        assert_eq!(content["action"], "recorded_on_new");
        // The page_id should be "topic-new" since all words < 4 chars
        // (derive_topic_id filters >= 4 chars)
        let page_id = content["page_id"].as_str().unwrap();
        assert!(
            page_id.starts_with("topic-"),
            "Expected topic- prefix, got: {}",
            page_id
        );
    }

    #[test]
    fn test_wiki_from_json_with_actual_fix_sets_resolved() {
        let now = "2026-04-07T00:00:00Z";
        let val = serde_json::json!({
            "symptom": "Build fails on CI",
            "attempted_fix": "Pinned dependency version",
            "diagnosis": "Wrong lockfile committed",
            "actual_fix": "Regenerated lockfile from scratch"
        });
        let fp = crate::wiki::page::FailurePattern::from_json(&val, now).unwrap();
        assert_eq!(
            fp.actual_fix.as_deref(),
            Some("Regenerated lockfile from scratch")
        );
        assert_eq!(fp.resolved_at.as_deref(), Some(now));
    }

    #[test]
    fn test_wiki_from_json_without_actual_fix_not_resolved() {
        let now = "2026-04-07T00:00:00Z";
        let val = serde_json::json!({
            "symptom": "Build fails on CI",
            "attempted_fix": "Pinned dependency version",
            "diagnosis": "Wrong lockfile committed"
        });
        let fp = crate::wiki::page::FailurePattern::from_json(&val, now).unwrap();
        assert!(fp.actual_fix.is_none());
        assert!(fp.resolved_at.is_none());
    }

    #[test]
    fn test_wiki_contribute_with_actual_fix_sets_resolved() {
        let mut store = make_test_wiki_store();
        tool_wiki_contribute(
            &mut store,
            &json!({
                "page": "mod-mcp",
                "content": "# MCP Server\n\nUpdated.",
                "failures": [{
                    "symptom": "Server hangs on large payload",
                    "attempted_fix": "Added timeout",
                    "diagnosis": "Hang was in serialization",
                    "actual_fix": "Switched to streaming serializer"
                }]
            }),
        );
        let page = store.get_page("mod-mcp").unwrap();
        assert_eq!(page.frontmatter.failures.len(), 1);
        assert_eq!(
            page.frontmatter.failures[0].actual_fix.as_deref(),
            Some("Switched to streaming serializer")
        );
        assert!(
            page.frontmatter.failures[0].resolved_at.is_some(),
            "Failure with actual_fix via wiki_contribute should be marked resolved"
        );
    }
}
