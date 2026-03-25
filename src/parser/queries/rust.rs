use tree_sitter::Node;

use crate::model::Import;
use crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility};

use super::DeclExtractor;

pub struct RustExtractor;

impl DeclExtractor for RustExtractor {
    fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>) {
        let mut imports = Vec::new();
        let mut declarations = Vec::new();

        for i in 0..root.child_count() {
            let Some(child) = root.child(i) else {
                continue;
            };
            match child.kind() {
                "use_declaration" => {
                    if let Some(import) = extract_import(child, source) {
                        imports.push(import);
                    }
                }
                "function_item" => {
                    if let Some(decl) = extract_function(child, source, DeclKind::Function) {
                        declarations.push(decl);
                    }
                }
                "struct_item" => {
                    if let Some(decl) = extract_struct(child, source) {
                        declarations.push(decl);
                    }
                }
                "enum_item" => {
                    if let Some(decl) = extract_enum(child, source) {
                        declarations.push(decl);
                    }
                }
                "trait_item" => {
                    if let Some(decl) = extract_trait(child, source) {
                        declarations.push(decl);
                    }
                }
                "impl_item" => {
                    if let Some(decl) = extract_impl(child, source) {
                        declarations.push(decl);
                    }
                }
                "const_item" => {
                    if let Some(decl) = extract_const_or_static(child, source, DeclKind::Constant) {
                        declarations.push(decl);
                    }
                }
                "static_item" => {
                    if let Some(decl) = extract_const_or_static(child, source, DeclKind::Static) {
                        declarations.push(decl);
                    }
                }
                "type_item" => {
                    if let Some(decl) = extract_type_alias(child, source) {
                        declarations.push(decl);
                    }
                }
                "mod_item" => {
                    if let Some(decl) = extract_module(child, source) {
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

fn extract_visibility(node: Node<'_>, source: &str) -> Visibility {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.kind() == "visibility_modifier"
        {
            let text = node_text(child, source);
            if text.contains("crate") {
                return Visibility::PublicCrate;
            }
            return Visibility::Public;
        }
    }
    Visibility::Private
}

fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut prev = node.prev_sibling();

    while let Some(sibling) = prev {
        match sibling.kind() {
            "line_comment" => {
                let text = node_text(sibling, source);
                if let Some(doc) = text.strip_prefix("///") {
                    comments.push(doc.trim().to_string());
                } else {
                    break;
                }
            }
            "block_comment" => {
                let text = node_text(sibling, source);
                if text.starts_with("/**") {
                    let cleaned = text
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .trim()
                        .to_string();
                    comments.push(cleaned);
                }
                break;
            }
            // Skip over attribute items (e.g., #[derive(...)])
            "attribute_item" => {}
            _ => break,
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

fn extract_signature(node: Node<'_>, source: &str) -> String {
    let text = node_text(node, source);
    // Take text up to the opening brace or semicolon
    let end = text
        .find('{')
        .or_else(|| text.find(';'))
        .unwrap_or(text.len());
    let sig = text[..end].trim();
    // Normalize whitespace
    sig.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_import(node: Node<'_>, source: &str) -> Option<Import> {
    let text = node_text(node, source).trim();
    let clean = text.trim_end_matches(';').trim();
    let clean = clean.strip_prefix("use ").unwrap_or(clean);
    Some(Import {
        text: clean.to_string(),
    })
}

fn body_lines(node: Node<'_>) -> Option<usize> {
    let start = node.start_position().row;
    let end = node.end_position().row;
    Some(end.saturating_sub(start))
}

/// Check if a node has a specific attribute by scanning its previous siblings.
/// Returns true if any preceding `attribute_item` contains the given text.
fn has_attribute(node: Node<'_>, source: &str, attr_text: &str) -> bool {
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "attribute_item" {
            let text = node_text(sibling, source);
            if text.contains(attr_text) {
                return true;
            }
        } else if sibling.kind() != "line_comment" && sibling.kind() != "block_comment" {
            break;
        }
        prev = sibling.prev_sibling();
    }
    false
}

fn detect_is_test(node: Node<'_>, source: &str) -> bool {
    has_attribute(node, source, "test")
}

fn detect_is_async(signature: &str) -> bool {
    signature.contains("async fn")
}

fn detect_is_deprecated(node: Node<'_>, source: &str) -> bool {
    has_attribute(node, source, "deprecated")
}

fn extract_function(node: Node<'_>, source: &str, kind: DeclKind) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let signature = extract_signature(node, source);
    let line = node.start_position().row + 1;

    let mut decl = Declaration::new(kind, name, signature.clone(), visibility, line);
    decl.doc_comment = doc_comment;
    decl.is_test = detect_is_test(node, source);
    decl.is_async = detect_is_async(&signature);
    decl.is_deprecated = detect_is_deprecated(node, source);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_struct(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let signature = extract_signature(node, source);
    let line = node.start_position().row + 1;

    let mut fields = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.child_count() {
            if let Some(child) = body.child(i)
                && child.kind() == "field_declaration"
                && let Some(field) = extract_field(child, source)
            {
                fields.push(field);
            }
        }
    }

    let mut decl = Declaration::new(DeclKind::Struct, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.children = fields;
    decl.is_deprecated = detect_is_deprecated(node, source);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_field(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let line = node.start_position().row + 1;

    let type_text = node
        .child_by_field_name("type")
        .map(|t| node_text(t, source))
        .unwrap_or("");

    let signature = format!("{}: {}", name, type_text);

    let mut decl = Declaration::new(DeclKind::Field, name, signature, visibility, line);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_enum(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let signature = extract_signature(node, source);
    let line = node.start_position().row + 1;

    let mut variants = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.child_count() {
            if let Some(child) = body.child(i)
                && child.kind() == "enum_variant"
                && let Some(variant) = extract_variant(child, source)
            {
                variants.push(variant);
            }
        }
    }

    let mut decl = Declaration::new(DeclKind::Enum, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.children = variants;
    decl.is_deprecated = detect_is_deprecated(node, source);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_variant(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let line = node.start_position().row + 1;
    let text = node_text(node, source).trim().trim_end_matches(',');
    let signature = text.split_whitespace().collect::<Vec<_>>().join(" ");

    let mut decl = Declaration::new(DeclKind::Variant, name, signature, Visibility::Public, line);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_trait(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let signature = extract_signature(node, source);
    let line = node.start_position().row + 1;

    let mut methods = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.child_count() {
            if let Some(child) = body.child(i) {
                match child.kind() {
                    "function_item" | "function_signature_item" => {
                        if let Some(method) = extract_function(child, source, DeclKind::Method) {
                            methods.push(method);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut decl = Declaration::new(DeclKind::Trait, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.children = methods;
    decl.is_deprecated = detect_is_deprecated(node, source);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_impl(node: Node<'_>, source: &str) -> Option<Declaration> {
    let type_node = node.child_by_field_name("type")?;
    let type_name = node_text(type_node, source).to_string();

    let trait_node = node.child_by_field_name("trait");
    let name = if let Some(trait_n) = trait_node {
        let trait_name = node_text(trait_n, source);
        format!("{} for {}", trait_name, type_name)
    } else {
        type_name
    };

    let signature = extract_signature(node, source);
    let line = node.start_position().row + 1;

    let mut methods = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.child_count() {
            if let Some(child) = body.child(i)
                && child.kind() == "function_item"
                && let Some(method) = extract_function(child, source, DeclKind::Method)
            {
                methods.push(method);
            }
        }
    }

    let mut decl = Declaration::new(DeclKind::Impl, name, signature, Visibility::Private, line);
    decl.children = methods;
    decl.body_lines = body_lines(node);

    // For `impl Trait for Type`, create an Implements relationship
    if let Some(trait_n) = trait_node {
        let trait_name = node_text(trait_n, source).to_string();
        decl.relationships.push(Relationship {
            kind: RelKind::Implements,
            target: trait_name,
        });
    }

    Some(decl)
}

fn extract_const_or_static(node: Node<'_>, source: &str, kind: DeclKind) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;
    let signature = extract_signature(node, source);

    let mut decl = Declaration::new(kind, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.is_deprecated = detect_is_deprecated(node, source);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_type_alias(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;
    let signature = extract_signature(node, source);

    let mut decl = Declaration::new(DeclKind::TypeAlias, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.is_deprecated = detect_is_deprecated(node, source);
    decl.body_lines = body_lines(node);

    Some(decl)
}

fn extract_module(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let line = node.start_position().row + 1;
    let signature = extract_signature(node, source);

    let mut decl = Declaration::new(DeclKind::Module, name, signature, visibility, line);
    decl.body_lines = body_lines(node);

    Some(decl)
}
