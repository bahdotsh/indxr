use tree_sitter::Node;

use crate::model::Import;
use crate::model::declarations::{DeclKind, Declaration, Visibility};

use super::DeclExtractor;

pub struct QmlExtractor;

impl DeclExtractor for QmlExtractor {
    fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>) {
        let mut imports = Vec::new();
        let mut declarations = Vec::new();

        for i in 0..root.child_count() {
            let Some(child) = root.child(i) else {
                continue;
            };
            match child.kind() {
                "ui_import" => {
                    if let Some(import) = extract_import(child, source) {
                        imports.push(import);
                    }
                }
                "ui_object_definition" => {
                    if let Some(decl) = extract_object_definition(child, source) {
                        declarations.push(decl);
                    }
                }
                "ui_annotated_object" => {
                    if let Some(def) = child.child_by_field_name("definition") {
                        if let Some(decl) = extract_object_definition(def, source) {
                            declarations.push(decl);
                        }
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

fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String> {
    let sibling = node.prev_sibling()?;
    if sibling.kind() == "comment" {
        let text = node_text(sibling, source);
        if text.starts_with("/**") {
            let cleaned = text
                .trim_start_matches("/**")
                .trim_end_matches("*/")
                .trim()
                .to_string();
            return Some(cleaned);
        }
    }
    None
}

fn extract_import(node: Node<'_>, source: &str) -> Option<Import> {
    let text = node_text(node, source).trim();
    let clean = text.trim_end_matches(';').trim();
    Some(Import {
        text: clean.to_string(),
    })
}

fn extract_object_definition(node: Node<'_>, source: &str) -> Option<Declaration> {
    let type_name = node.child_by_field_name("type_name")?;
    let name = node_text(type_name, source).to_string();
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;
    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut children = Vec::new();
    if let Some(initializer) = node.child_by_field_name("initializer") {
        children = extract_object_members(initializer, source);
    }

    let mut decl = Declaration::new(
        DeclKind::Class,
        name.clone(),
        name,
        Visibility::Public,
        line,
    );
    decl.doc_comment = doc_comment;
    decl.children = children;
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_object_members(initializer: Node<'_>, source: &str) -> Vec<Declaration> {
    let mut members = Vec::new();

    for i in 0..initializer.child_count() {
        let Some(child) = initializer.child(i) else {
            continue;
        };
        if let Some(decl) = extract_member(child, source) {
            members.push(decl);
        }
    }

    members
}

fn extract_member(node: Node<'_>, source: &str) -> Option<Declaration> {
    match node.kind() {
        "ui_object_definition" => extract_object_definition(node, source),
        "ui_property" => extract_property(node, source),
        "ui_signal" => extract_signal(node, source),
        "ui_binding" => extract_binding(node, source),
        "function_declaration" => extract_function(node, source),
        "enum_declaration" => extract_enum(node, source),
        "ui_inline_component" => extract_inline_component(node, source),
        "ui_object_definition_binding" => extract_object_definition_binding(node, source),
        "ui_annotated_object_member" => {
            // Unwrap annotation wrapper, process inner member
            for i in 0..node.child_count() {
                let Some(child) = node.child(i) else {
                    continue;
                };
                if child.kind() != "ui_annotation" {
                    return extract_member(child, source);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_property(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;

    // Build signature from modifiers + "property" + type + name (omit value)
    let mut sig_parts = Vec::new();
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else {
            continue;
        };
        match child.kind() {
            "ui_property_modifier" => sig_parts.push(node_text(child, source).to_string()),
            "property" => sig_parts.push("property".to_string()),
            "type_identifier" | "nested_type_identifier" => {
                // Only include the type field, not the name or value
                if node
                    .child_by_field_name("type")
                    .is_some_and(|t| t.id() == child.id())
                {
                    sig_parts.push(node_text(child, source).to_string());
                }
            }
            "identifier" => {
                // Include only the name field
                if child.id() == name_node.id() {
                    sig_parts.push(name.clone());
                    break; // Stop before the `: value` part
                }
            }
            _ => {}
        }
    }
    let signature = sig_parts.join(" ");

    let mut decl = Declaration::new(DeclKind::Field, name, signature, Visibility::Public, line);
    decl.doc_comment = doc_comment;
    decl.body_lines = Some(0);
    Some(decl)
}

fn extract_signal(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;

    let text = node_text(node, source).trim();
    let signature = text
        .lines()
        .next()
        .unwrap_or(text)
        .trim_end_matches(';')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut decl = Declaration::new(DeclKind::Method, name, signature, Visibility::Public, line);
    decl.doc_comment = doc_comment;
    decl.body_lines = Some(0);
    Some(decl)
}

fn extract_binding(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();

    // Skip "id" bindings — they're identifiers, not structural declarations
    if name == "id" {
        return None;
    }

    let line = node.start_position().row + 1;

    let mut decl = Declaration::new(
        DeclKind::Field,
        name.clone(),
        name,
        Visibility::Public,
        line,
    );
    decl.body_lines = Some(0);
    Some(decl)
}

fn extract_function(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;

    let text = node_text(node, source);
    let end = text.find('{').unwrap_or(text.len());
    let signature = text[..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(
        DeclKind::Function,
        name,
        signature,
        Visibility::Public,
        line,
    );
    decl.doc_comment = doc_comment;
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_enum(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;
    let signature = format!("enum {name}");

    let mut children = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() {
            if let Some(member) = body.named_child(i) {
                let variant_name = node_text(member, source).trim_end_matches(',').to_string();
                if !variant_name.is_empty() {
                    let vline = member.start_position().row + 1;
                    children.push(Declaration::new(
                        DeclKind::Variant,
                        variant_name.clone(),
                        variant_name,
                        Visibility::Public,
                        vline,
                    ));
                }
            }
        }
    }

    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Enum, name, signature, Visibility::Public, line);
    decl.doc_comment = doc_comment;
    decl.children = children;
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_inline_component(node: Node<'_>, source: &str) -> Option<Declaration> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source).to_string();
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;

    let text = node_text(node, source);
    let end = text.find('{').unwrap_or(text.len());
    let signature = text[..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut children = Vec::new();
    // The component field points to the inner object definition
    if let Some(component) = node.child_by_field_name("component") {
        if let Some(initializer) = component.child_by_field_name("initializer") {
            children = extract_object_members(initializer, source);
        }
    }

    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Class, name, signature, Visibility::Public, line);
    decl.doc_comment = doc_comment;
    decl.children = children;
    decl.body_lines = body_lines;
    Some(decl)
}

fn extract_object_definition_binding(node: Node<'_>, source: &str) -> Option<Declaration> {
    // `Type on name { ... }` — e.g., `NumberAnimation on x { ... }`
    let type_name = node.child_by_field_name("type_name")?;
    let name = node_text(type_name, source).to_string();
    let doc_comment = extract_doc_comment(node, source);
    let line = node.start_position().row + 1;

    let text = node_text(node, source);
    let end = text.find('{').unwrap_or(text.len());
    let signature = text[..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut children = Vec::new();
    if let Some(initializer) = node.child_by_field_name("initializer") {
        children = extract_object_members(initializer, source);
    }

    let body_lines = Some(
        node.end_position()
            .row
            .saturating_sub(node.start_position().row),
    );

    let mut decl = Declaration::new(DeclKind::Class, name, signature, Visibility::Public, line);
    decl.doc_comment = doc_comment;
    decl.children = children;
    decl.body_lines = body_lines;
    Some(decl)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_qml(source: &str) -> (Vec<Import>, Vec<Declaration>) {
        let mut parser = tree_sitter::Parser::new();
        let lang: tree_sitter::Language = tree_sitter_qmljs::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let extractor = QmlExtractor;
        extractor.extract(tree.root_node(), source)
    }

    #[test]
    fn imports() {
        let src = r#"
import QtQuick 2.15
import QtQuick.Controls 2.15
"#;
        let (imports, _) = parse_qml(src);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].text, "import QtQuick 2.15");
        assert_eq!(imports[1].text, "import QtQuick.Controls 2.15");
    }

    #[test]
    fn root_component_with_nested() {
        let src = r#"
import QtQuick 2.15

Rectangle {
    width: 800

    Text {
        text: "Hello"
    }
}
"#;
        let (_, decls) = parse_qml(src);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "Rectangle");
        assert_eq!(decls[0].kind, DeclKind::Class);
        // Children: width binding + Text object
        let children = &decls[0].children;
        let width = children.iter().find(|d| d.name == "width").unwrap();
        assert_eq!(width.signature, "width"); // binding signature is just the name
        assert!(
            children
                .iter()
                .any(|d| d.name == "Text" && d.kind == DeclKind::Class)
        );
    }

    #[test]
    fn properties() {
        let src = r#"
Rectangle {
    property string title: "Hello"
    readonly property int count: 0
    default property alias myAlias: other.prop
}
"#;
        let (_, decls) = parse_qml(src);
        let children = &decls[0].children;
        let props: Vec<_> = children
            .iter()
            .filter(|d| d.kind == DeclKind::Field && d.signature.contains("property"))
            .collect();
        assert!(
            props.len() >= 2,
            "expected at least 2 properties, got {}",
            props.len()
        );

        let title = props.iter().find(|d| d.name == "title").unwrap();
        assert_eq!(title.signature, "property string title");

        let count = props.iter().find(|d| d.name == "count").unwrap();
        assert_eq!(count.signature, "readonly property int count");
    }

    #[test]
    fn signals() {
        let src = r#"
Rectangle {
    signal clicked()
    signal valueChanged(newValue: int, oldValue: int)
}
"#;
        let (_, decls) = parse_qml(src);
        let children = &decls[0].children;
        let signals: Vec<_> = children
            .iter()
            .filter(|d| d.kind == DeclKind::Method)
            .collect();
        assert_eq!(signals.len(), 2);
        assert!(signals.iter().any(|d| d.name == "clicked"));
        assert!(signals.iter().any(|d| d.name == "valueChanged"));
    }

    #[test]
    fn functions() {
        let src = r#"
Rectangle {
    function calculate(x, limit) {
        if (x > limit) {
            return x;
        }
        return 0;
    }
}
"#;
        let (_, decls) = parse_qml(src);
        let children = &decls[0].children;
        let funcs: Vec<_> = children
            .iter()
            .filter(|d| d.kind == DeclKind::Function)
            .collect();
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "calculate");
        assert!(funcs[0].signature.contains("function calculate(x, limit)"));
    }

    #[test]
    fn enums() {
        let src = r#"
Rectangle {
    enum Status {
        Active,
        Inactive,
        Pending
    }
}
"#;
        let (_, decls) = parse_qml(src);
        let children = &decls[0].children;
        let enums: Vec<_> = children
            .iter()
            .filter(|d| d.kind == DeclKind::Enum)
            .collect();
        assert_eq!(enums.len(), 1);
        assert_eq!(enums[0].name, "Status");
        assert!(
            enums[0].children.len() >= 3,
            "expected 3 variants, got {}",
            enums[0].children.len()
        );
    }

    #[test]
    fn inline_component() {
        let src = r#"
Rectangle {
    component MyButton: Button {
        text: "Click me"
    }
}
"#;
        let (_, decls) = parse_qml(src);
        let children = &decls[0].children;
        let comps: Vec<_> = children.iter().filter(|d| d.name == "MyButton").collect();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].kind, DeclKind::Class);
        assert!(comps[0].signature.contains("component MyButton"));
    }

    #[test]
    fn bindings_skip_id() {
        let src = r#"
Rectangle {
    id: root
    width: 800
    height: 600
}
"#;
        let (_, decls) = parse_qml(src);
        let children = &decls[0].children;
        // id binding should be skipped
        assert!(!children.iter().any(|d| d.name == "id"));
        assert!(children.iter().any(|d| d.name == "width"));
        assert!(children.iter().any(|d| d.name == "height"));
    }

    #[test]
    fn full_qml_file() {
        let src = r#"
import QtQuick 2.15
import QtQuick.Controls 2.15

Rectangle {
    id: root
    width: 800
    height: 600

    property string title: "Hello"

    signal clicked()

    function doSomething() {
        console.log("done");
    }

    Text {
        text: root.title
    }
}
"#;
        let (imports, decls) = parse_qml(src);
        assert_eq!(imports.len(), 2);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "Rectangle");

        let children = &decls[0].children;
        // width, height, title property, clicked signal, doSomething function, Text object
        assert!(
            children.len() >= 5,
            "expected >= 5 children, got {}",
            children.len()
        );
    }
}
