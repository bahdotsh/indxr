use tree_sitter::Node;

use crate::model::Import;
use crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility};

use super::DeclExtractor;

pub struct PythonExtractor;

impl DeclExtractor for PythonExtractor {
    fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>) {
        let mut imports = Vec::new();
        let mut declarations = Vec::new();

        for i in 0..root.child_count() {
            let Some(child) = root.child(i) else {
                continue;
            };
            match child.kind() {
                "import_statement" | "import_from_statement" => {
                    if let Some(import) = extract_import(child, source) {
                        imports.push(import);
                    }
                }
                "function_definition" => {
                    if let Some(decl) = extract_function(child, source, DeclKind::Function) {
                        declarations.push(decl);
                    }
                }
                "class_definition" => {
                    if let Some(decl) = extract_class(child, source) {
                        declarations.push(decl);
                    }
                }
                "decorated_definition" => {
                    let mut decls = extract_decorated(child, source);
                    declarations.append(&mut decls);
                }
                "expression_statement" => {
                    if let Some(decl) = extract_assignment(child, source) {
                        declarations.push(decl);
                    }
                }
                _ => {}
            }
        }

        (imports, declarations)
    }
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.start_byte()..node.end_byte()]
}

fn extract_visibility(name: &str) -> Visibility {
    if name.starts_with('_') {
        Visibility::Private
    } else {
        Visibility::Public
    }
}

fn extract_docstring(body: Node<'_>, source: &str) -> Option<String> {
    // The first child of the body block; if it's an expression_statement
    // containing a string, it's a docstring.
    for i in 0..body.child_count() {
        let Some(child) = body.child(i) else {
            continue;
        };
        if child.kind() == "expression_statement" {
            for j in 0..child.child_count() {
                let Some(inner) = child.child(j) else {
                    continue;
                };
                if inner.kind() == "string" {
                    let text = node_text(inner, source);
                    // Strip triple-quote delimiters
                    let cleaned = text
                        .trim_start_matches("\"\"\"")
                        .trim_start_matches("'''")
                        .trim_end_matches("\"\"\"")
                        .trim_end_matches("'''")
                        .trim();
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_string());
                    }
                }
            }
            // Only check the first expression_statement
            break;
        } else {
            // If the first child is not an expression_statement, there's no docstring
            break;
        }
    }
    None
}

fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String> {
    // Python uses preceding # comments as doc comments (less common, but
    // follows the same prev_sibling pattern as Rust for consistency).
    let mut comments = Vec::new();
    let mut prev = node.prev_sibling();

    while let Some(sibling) = prev {
        if sibling.kind() == "comment" {
            let text = node_text(sibling, source);
            let cleaned = text.trim_start_matches('#').trim().to_string();
            comments.push(cleaned);
        } else {
            break;
        }
        prev = sibling.prev_sibling();
    }

    if comments.is_empty() {
        None
    } else {
        comments.reverse();
        Some(comments.join(" "))
    }
}

fn extract_function_signature(node: Node<'_>, source: &str) -> String {
    // Build signature from "def name(params) -> return_type"
    let mut sig = String::from("def ");
    if let Some(name_node) = node.child_by_field_name("name") {
        sig.push_str(node_text(name_node, source));
    }
    if let Some(params) = node.child_by_field_name("parameters") {
        sig.push_str(node_text(params, source));
    }
    if let Some(ret) = node.child_by_field_name("return_type") {
        sig.push_str(" -> ");
        sig.push_str(node_text(ret, source));
    }
    sig
}

fn extract_import(node: Node<'_>, source: &str) -> Option<Import> {
    let text = node_text(node, source).trim().to_string();
    Some(Import { text })
}

fn body_lines(node: Node<'_>) -> Option<usize> {
    let start = node.start_position().row;
    let end = node.end_position().row;
    Some(end.saturating_sub(start))
}

fn detect_is_test_function(name: &str) -> bool {
    name.starts_with("test_")
}

fn detect_is_test_class(name: &str) -> bool {
    name.starts_with("Test")
}

fn detect_is_async(signature: &str) -> bool {
    signature.contains("async def")
}

fn has_deprecated_decorator(decorators: &[String]) -> bool {
    decorators.iter().any(|d| d.contains("deprecated"))
}

/// Extract base class names from a class node's superclasses field.
fn extract_base_classes(node: Node<'_>, source: &str) -> Vec<String> {
    let mut bases = Vec::new();
    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        // superclasses is an argument_list like (Base1, Base2)
        for i in 0..superclasses.child_count() {
            if let Some(child) = superclasses.child(i) {
                match child.kind() {
                    "identifier" => {
                        bases.push(node_text(child, source).to_string());
                    }
                    "attribute" => {
                        bases.push(node_text(child, source).to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    bases
}

fn extract_function(node: Node<'_>, source: &str, kind: DeclKind) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(&name);
    let signature = extract_function_signature(node, source);
    let line = node.start_position().row + 1;

    // Get docstring from function body
    let doc_comment = node
        .child_by_field_name("body")
        .and_then(|body| extract_docstring(body, source))
        .or_else(|| extract_doc_comment(node, source));

    let mut decl = Declaration::new(kind, name.clone(), signature.clone(), visibility, line);
    decl.doc_comment = doc_comment;
    decl.is_test = detect_is_test_function(&name);
    decl.is_async = detect_is_async(&signature);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_class(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(&name);
    let line = node.start_position().row + 1;

    // Build signature: "class Name(bases)"
    let mut sig = format!("class {}", name);
    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        sig.push_str(node_text(superclasses, source));
    }
    let signature = sig;

    // Get docstring from class body
    let doc_comment = node
        .child_by_field_name("body")
        .and_then(|body| extract_docstring(body, source))
        .or_else(|| extract_doc_comment(node, source));

    // Extract methods from class body
    let mut methods = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.child_count() {
            let Some(child) = body.child(i) else {
                continue;
            };
            match child.kind() {
                "function_definition" => {
                    if let Some(method) = extract_function(child, source, DeclKind::Method) {
                        methods.push(method);
                    }
                }
                "decorated_definition" => {
                    // A decorated method inside a class
                    let inner = find_inner_definition(child);
                    if let Some(inner_node) = inner
                        && inner_node.kind() == "function_definition"
                    {
                        let decorators = extract_decorators(child, source);
                        if let Some(mut method) =
                            extract_function(inner_node, source, DeclKind::Method)
                        {
                            if !decorators.is_empty() {
                                let prefix = decorators.join(" ");
                                method.signature = format!("{} {}", prefix, method.signature);
                            }
                            // Check for @deprecated on decorated methods
                            if has_deprecated_decorator(&decorators) {
                                method.is_deprecated = true;
                            }
                            methods.push(method);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Build relationships from base classes
    let base_classes = extract_base_classes(node, source);
    let relationships: Vec<Relationship> = base_classes
        .into_iter()
        .map(|base| Relationship {
            kind: RelKind::Extends,
            target: base,
        })
        .collect();

    let mut decl = Declaration::new(DeclKind::Struct, name.clone(), signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.children = methods;
    decl.is_test = detect_is_test_class(&name);
    decl.body_lines = body_lines(node);
    decl.relationships = relationships;

    Some(decl)
}

fn find_inner_definition(decorated: Node<'_>) -> Option<Node<'_>> {
    for i in 0..decorated.child_count() {
        let Some(child) = decorated.child(i) else {
            continue;
        };
        match child.kind() {
            "function_definition" | "class_definition" => return Some(child),
            _ => {}
        }
    }
    None
}

fn extract_decorators(decorated: Node<'_>, source: &str) -> Vec<String> {
    let mut decorators = Vec::new();
    for i in 0..decorated.child_count() {
        let Some(child) = decorated.child(i) else {
            continue;
        };
        if child.kind() == "decorator" {
            let text = node_text(child, source).trim().to_string();
            decorators.push(text);
        }
    }
    decorators
}

fn extract_decorated(node: Node<'_>, source: &str) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    let decorators = extract_decorators(node, source);

    let inner = find_inner_definition(node);
    let Some(inner_node) = inner else {
        return declarations;
    };

    match inner_node.kind() {
        "function_definition" => {
            if let Some(mut decl) = extract_function(inner_node, source, DeclKind::Function) {
                decl.line = node.start_position().row + 1;
                if !decorators.is_empty() {
                    let prefix = decorators.join(" ");
                    decl.signature = format!("{} {}", prefix, decl.signature);
                }
                // Check for @deprecated decorator
                if has_deprecated_decorator(&decorators) {
                    decl.is_deprecated = true;
                }
                // Check for async in the full decorated signature
                if detect_is_async(&decl.signature) {
                    decl.is_async = true;
                }
                declarations.push(decl);
            }
        }
        "class_definition" => {
            if let Some(mut decl) = extract_class(inner_node, source) {
                decl.line = node.start_position().row + 1;
                if !decorators.is_empty() {
                    let prefix = decorators.join(" ");
                    decl.signature = format!("{} {}", prefix, decl.signature);
                }
                // Check for @deprecated decorator on class
                if has_deprecated_decorator(&decorators) {
                    decl.is_deprecated = true;
                }
                declarations.push(decl);
            }
        }
        _ => {}
    }

    declarations
}

fn extract_assignment(node: Node<'_>, source: &str) -> Option<Declaration> {
    // Look for an assignment child inside this expression_statement
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else {
            continue;
        };
        if child.kind() == "assignment" {
            let left = child.child_by_field_name("left")?;
            let name = node_text(left, source).to_string();
            // Only handle simple identifier assignments (module-level constants)
            if left.kind() != "identifier" {
                return None;
            }
            let visibility = extract_visibility(&name);
            let line = node.start_position().row + 1;
            let signature = node_text(node, source)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let doc_comment = extract_doc_comment(node, source);

            let mut decl = Declaration::new(DeclKind::Constant, name, signature, visibility, line);
            decl.doc_comment = doc_comment;
            decl.body_lines = body_lines(node);

            return Some(decl);
        }
    }
    None
}
