use tree_sitter::Node;

use crate::model::Import;
use crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility};

use super::DeclExtractor;

pub struct JavaExtractor;

impl DeclExtractor for JavaExtractor {
    fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>) {
        let mut imports = Vec::new();
        let mut declarations = Vec::new();

        for i in 0..root.child_count() {
            let Some(child) = root.child(i) else {
                continue;
            };
            match child.kind() {
                "import_declaration" => {
                    if let Some(import) = extract_import(child, source) {
                        imports.push(import);
                    }
                }
                "class_declaration" => {
                    if let Some(decl) = extract_class(child, source) {
                        declarations.push(decl);
                    }
                }
                "interface_declaration" => {
                    if let Some(decl) = extract_interface(child, source) {
                        declarations.push(decl);
                    }
                }
                "enum_declaration" => {
                    if let Some(decl) = extract_enum(child, source) {
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

fn extract_modifiers_text(node: Node<'_>, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.kind() == "modifiers"
        {
            return Some(node_text(child, source).to_string());
        }
    }
    None
}

fn extract_visibility(node: Node<'_>, source: &str) -> Visibility {
    // Check modifier child nodes directly instead of substring matching on text,
    // which could false-match on annotation names containing "public"/"private".
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.kind() == "modifiers"
        {
            for j in 0..child.child_count() {
                if let Some(modifier) = child.child(j) {
                    let text = node_text(modifier, source);
                    match text {
                        "public" => return Visibility::Public,
                        "private" | "protected" => return Visibility::Private,
                        _ => {}
                    }
                }
            }
        }
    }
    Visibility::Private
}

#[allow(dead_code)]
fn has_modifier(node: Node<'_>, source: &str, keyword: &str) -> bool {
    if let Some(mods) = extract_modifiers_text(node, source) {
        mods.contains(keyword)
    } else {
        false
    }
}

fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String> {
    let mut prev = node.prev_sibling();

    while let Some(sibling) = prev {
        match sibling.kind() {
            "block_comment" => {
                let text = node_text(sibling, source);
                if text.starts_with("/**") {
                    let cleaned = text
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|l| l.trim().trim_start_matches('*').trim())
                        .filter(|l| !l.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ");
                    return Some(cleaned);
                }
                return None;
            }
            "line_comment" => {
                // Regular line comment — skip past it, there may be a Javadoc above
                prev = sibling.prev_sibling();
                continue;
            }
            "marker_annotation" | "annotation" => {
                // Skip annotations like @Override, @Deprecated
                prev = sibling.prev_sibling();
                continue;
            }
            _ => return None,
        }
    }

    None
}

fn extract_signature(node: Node<'_>, source: &str) -> String {
    let text = node_text(node, source);
    let end = text
        .find('{')
        .or_else(|| text.find(';'))
        .unwrap_or(text.len());
    let sig = text[..end].trim();
    sig.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_import(node: Node<'_>, source: &str) -> Option<Import> {
    let text = node_text(node, source).trim();
    let clean = text.trim_end_matches(';').trim();
    let clean = clean.strip_prefix("import ").unwrap_or(clean);
    let clean = clean.strip_prefix("static ").unwrap_or(clean);
    Some(Import {
        text: clean.to_string(),
    })
}

/// Check if a node has a specific annotation by looking at its previous siblings.
fn has_annotation(node: Node<'_>, source: &str, annotation_name: &str) -> bool {
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        match sibling.kind() {
            "marker_annotation" | "annotation" => {
                let text = node_text(sibling, source);
                // Annotation text is like "@Test" or "@Deprecated"
                if text.contains(annotation_name) {
                    return true;
                }
                prev = sibling.prev_sibling();
                continue;
            }
            _ => break,
        }
    }

    // Also check inside modifiers node
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.kind() == "modifiers"
        {
            for j in 0..child.child_count() {
                if let Some(mod_child) = child.child(j)
                    && (mod_child.kind() == "marker_annotation" || mod_child.kind() == "annotation")
                {
                    let text = node_text(mod_child, source);
                    if text.contains(annotation_name) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Extract class relationships (extends and implements) from Java class declarations.
fn extract_class_relationships(node: Node<'_>, source: &str) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    // Check for superclass field
    if let Some(superclass) = node.child_by_field_name("superclass") {
        // superclass node text is typically like "extends Foo"
        let text = node_text(superclass, source).trim();
        let target = text.strip_prefix("extends ").unwrap_or(text).trim();
        if !target.is_empty() {
            relationships.push(Relationship {
                kind: RelKind::Extends,
                target: target.to_string(),
            });
        }
    }

    // Check for interfaces field
    if let Some(interfaces) = node.child_by_field_name("interfaces") {
        // interfaces node text is typically like "implements Foo, Bar"
        let text = node_text(interfaces, source).trim();
        let text = text.strip_prefix("implements ").unwrap_or(text).trim();
        for target in text.split(',') {
            let target = target.trim();
            if !target.is_empty() {
                relationships.push(Relationship {
                    kind: RelKind::Implements,
                    target: target.to_string(),
                });
            }
        }
    }

    // Fallback: parse from signature if fields not found
    if relationships.is_empty() {
        let sig = extract_signature(node, source);
        if let Some(pos) = sig.find("extends ") {
            let after = &sig[pos + 8..];
            let end = after
                .find("implements")
                .or_else(|| after.find('{'))
                .unwrap_or(after.len());
            let target = after[..end].trim().trim_end_matches('{').trim();
            if !target.is_empty() {
                relationships.push(Relationship {
                    kind: RelKind::Extends,
                    target: target.to_string(),
                });
            }
        }
        if let Some(pos) = sig.find("implements ") {
            let after = &sig[pos + 11..];
            let end = after.find('{').unwrap_or(after.len());
            let implements_str = after[..end].trim();
            for target in implements_str.split(',') {
                let target = target.trim();
                if !target.is_empty() {
                    relationships.push(Relationship {
                        kind: RelKind::Implements,
                        target: target.to_string(),
                    });
                }
            }
        }
    }

    relationships
}

fn extract_class(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let signature = extract_signature(node, source);
    let line = node.start_position().row + 1;

    let mut children = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.child_count() {
            let Some(child) = body.child(i) else {
                continue;
            };
            match child.kind() {
                "method_declaration" => {
                    if let Some(decl) = extract_method(child, source) {
                        children.push(decl);
                    }
                }
                "constructor_declaration" => {
                    if let Some(decl) = extract_constructor(child, source) {
                        children.push(decl);
                    }
                }
                "field_declaration" => {
                    if let Some(decl) = extract_field(child, source) {
                        children.push(decl);
                    }
                }
                "class_declaration" => {
                    if let Some(decl) = extract_class(child, source) {
                        children.push(decl);
                    }
                }
                "enum_declaration" => {
                    if let Some(decl) = extract_enum(child, source) {
                        children.push(decl);
                    }
                }
                "interface_declaration" => {
                    if let Some(decl) = extract_interface(child, source) {
                        children.push(decl);
                    }
                }
                _ => {}
            }
        }
    }

    let is_deprecated = has_annotation(node, source, "Deprecated");
    let is_test = has_annotation(node, source, "Test");
    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );
    let relationships = extract_class_relationships(node, source);

    let mut decl = Declaration::new(DeclKind::Struct, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.children = children;
    decl.is_test = is_test;
    decl.is_deprecated = is_deprecated;
    decl.body_lines = body_lines;
    decl.relationships = relationships;
    Some(decl)
}

fn extract_interface(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let signature = extract_signature(node, source);
    let line = node.start_position().row + 1;

    let mut methods = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.child_count() {
            let Some(child) = body.child(i) else {
                continue;
            };
            if child.kind() == "method_declaration"
                && let Some(decl) = extract_method(child, source)
            {
                methods.push(decl);
            }
        }
    }

    let is_deprecated = has_annotation(node, source, "Deprecated");
    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    // Interfaces can extend other interfaces
    let mut relationships = Vec::new();
    let sig = extract_signature(node, source);
    if let Some(pos) = sig.find("extends ") {
        let after = &sig[pos + 8..];
        let end = after.find('{').unwrap_or(after.len());
        let extends_str = after[..end].trim();
        for target in extends_str.split(',') {
            let target = target.trim();
            if !target.is_empty() {
                relationships.push(Relationship {
                    kind: RelKind::Extends,
                    target: target.to_string(),
                });
            }
        }
    }

    let mut decl = Declaration::new(DeclKind::Trait, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.children = methods;
    decl.is_deprecated = is_deprecated;
    decl.body_lines = body_lines;
    decl.relationships = relationships;
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
            let Some(child) = body.child(i) else {
                continue;
            };
            if child.kind() == "enum_constant"
                && let Some(variant) = extract_enum_constant(child, source)
            {
                variants.push(variant);
            }
        }
    }

    let is_deprecated = has_annotation(node, source, "Deprecated");
    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Enum, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.children = variants;
    decl.is_deprecated = is_deprecated;
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_enum_constant(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let line = node.start_position().row + 1;
    let text = node_text(node, source).trim().trim_end_matches(',');
    let signature = text.split_whitespace().collect::<Vec<_>>().join(" ");

    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Variant, name, signature, Visibility::Public, line);
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_method(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;

    let mut sig_parts = Vec::new();
    if let Some(mods) = extract_modifiers_text(node, source) {
        sig_parts.push(mods);
    }
    if let Some(ret_type) = node.child_by_field_name("type") {
        sig_parts.push(node_text(ret_type, source).to_string());
    }
    sig_parts.push(name.clone());
    if let Some(params) = node.child_by_field_name("parameters") {
        sig_parts.push(node_text(params, source).to_string());
    }
    let signature = sig_parts.join(" ");
    let signature = signature.split_whitespace().collect::<Vec<_>>().join(" ");

    let is_test = has_annotation(node, source, "Test");
    let is_deprecated = has_annotation(node, source, "Deprecated");
    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Method, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.is_test = is_test;
    decl.is_deprecated = is_deprecated;
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_constructor(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let visibility = extract_visibility(node, source);
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;

    let mut sig_parts = Vec::new();
    if let Some(mods) = extract_modifiers_text(node, source) {
        sig_parts.push(mods);
    }
    sig_parts.push(name.clone());
    if let Some(params) = node.child_by_field_name("parameters") {
        sig_parts.push(node_text(params, source).to_string());
    }
    let signature = sig_parts.join(" ");
    let signature = signature.split_whitespace().collect::<Vec<_>>().join(" ");

    let is_deprecated = has_annotation(node, source, "Deprecated");
    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Method, name, signature, visibility, line);
    decl.doc_comment = doc_comment;
    decl.is_deprecated = is_deprecated;
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_field(node: Node<'_>, source: &str) -> Option<Declaration> {
    let type_node = node.child_by_field_name("type");
    let type_text = type_node.map(|t| node_text(t, source)).unwrap_or("");

    // The field name is in the declarator child
    let declarator = node.child_by_field_name("declarator")?;
    let name_node = declarator.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();

    let visibility = extract_visibility(node, source);
    let line = node.start_position().row + 1;

    let mut sig_parts = Vec::new();
    if let Some(mods) = extract_modifiers_text(node, source) {
        sig_parts.push(mods);
    }
    sig_parts.push(type_text.to_string());
    sig_parts.push(name.clone());
    let signature = sig_parts.join(" ");
    let signature = signature.split_whitespace().collect::<Vec<_>>().join(" ");

    let is_deprecated = has_annotation(node, source, "Deprecated");
    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Field, name, signature, visibility, line);
    decl.is_deprecated = is_deprecated;
    decl.body_lines = body_lines;
    Some(decl)
}
