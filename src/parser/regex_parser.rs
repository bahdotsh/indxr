use std::path::Path;

use anyhow::Result;
use regex::Regex;

use crate::languages::Language;
use crate::model::declarations::{DeclKind, Declaration, Visibility};
use crate::model::{FileIndex, Import};

use super::LanguageParser;

pub struct RegexParser {
    language: Language,
}

impl RegexParser {
    pub fn new(language: Language) -> Self {
        Self { language }
    }
}

impl LanguageParser for RegexParser {
    fn language(&self) -> Language {
        self.language.clone()
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<FileIndex> {
        let lines = content.lines().count();
        let (imports, declarations) = match self.language {
            Language::Shell => parse_shell(content),
            Language::Toml => parse_toml(path, content),
            Language::Yaml => parse_yaml(path, content),
            Language::Json => parse_json(path, content),
            Language::Sql => parse_sql(content),
            Language::Markdown => parse_markdown(content),
            Language::Protobuf => parse_protobuf(content),
            Language::GraphQL => parse_graphql(content),
            Language::Ruby => parse_ruby(content),
            Language::Kotlin => parse_kotlin(content),
            Language::Swift => parse_swift(content),
            Language::CSharp => parse_csharp(content),
            Language::ObjectiveC => parse_objc(content),
            Language::Xml => parse_xml(content),
            Language::Html => parse_html(content),
            Language::Css => parse_css(content),
            Language::Gradle => parse_gradle(content),
            Language::Cmake => parse_cmake(content),
            Language::Properties => parse_properties(content),
            _ => (Vec::new(), Vec::new()),
        };

        Ok(FileIndex {
            path: path.to_path_buf(),
            language: self.language.clone(),
            size: content.len() as u64,
            lines,
            imports,
            declarations,
        })
    }
}

// ---------------------------------------------------------------------------
// Shell parser
// ---------------------------------------------------------------------------

fn parse_shell(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_func1 = Regex::new(r"^function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*\)").unwrap();
    let re_func2 = Regex::new(r"^function\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*\{|\s*$)").unwrap();
    let re_func3 = Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*\)\s*\{?").unwrap();
    let re_export = Regex::new(r"^export\s+([A-Za-z_][A-Za-z0-9_]*)=(.*)").unwrap();
    let re_alias = Regex::new(r"^alias\s+([A-Za-z_][A-Za-z0-9_\-]*)=").unwrap();
    let re_source = Regex::new(r"^(?:source\s+|\.\s+)(.+)$").unwrap();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Source / dot-import
        if let Some(caps) = re_source.captures(trimmed) {
            imports.push(Import {
                text: caps[1]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            });
            continue;
        }

        // function name() { ... }
        if let Some(caps) = re_func1.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::ShellFunction,
                name.clone(),
                format!("function {}()", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // function name { ... }
        if let Some(caps) = re_func2.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::ShellFunction,
                name.clone(),
                format!("function {}", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // name() { ... }  — but not things like echo() which are unlikely, still parse them
        if let Some(caps) = re_func3.captures(trimmed) {
            let name = caps[1].to_string();
            // Skip common keywords that look like function calls
            if matches!(
                name.as_str(),
                "if" | "for"
                    | "while"
                    | "until"
                    | "case"
                    | "do"
                    | "then"
                    | "else"
                    | "elif"
                    | "fi"
                    | "done"
                    | "esac"
                    | "export"
                    | "alias"
                    | "source"
                    | "echo"
                    | "return"
                    | "exit"
                    | "set"
                    | "unset"
                    | "local"
                    | "readonly"
            ) {
                continue;
            }
            declarations.push(Declaration::new(
                DeclKind::ShellFunction,
                name.clone(),
                format!("{}()", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // export VAR=value
        if let Some(caps) = re_export.captures(trimmed) {
            let name = caps[1].to_string();
            let value = caps[2].to_string();
            declarations.push(Declaration::new(
                DeclKind::Constant,
                name.clone(),
                format!("export {}={}", name, truncate_value(&value, 60)),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // alias name=...
        if let Some(caps) = re_alias.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Constant,
                name.clone(),
                format!("alias {}", name),
                Visibility::Public,
                line_num + 1,
            ));
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// TOML parser
// ---------------------------------------------------------------------------

fn parse_toml(path: &Path, content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_section = Regex::new(r"^\[([^\]]+)\]").unwrap();
    let re_array_section = Regex::new(r"^\[\[([^\]]+)\]\]").unwrap();
    let re_kv = Regex::new(r#"^([A-Za-z_][A-Za-z0-9_\-]*)\s*="#).unwrap();

    let is_cargo = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == "Cargo.toml")
        .unwrap_or(false);

    let mut current_section: Option<(String, usize)> = None; // (name, decl_index)
    let mut in_dependencies = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // [[array.section]]
        if let Some(caps) = re_array_section.captures(trimmed) {
            let name = caps[1].to_string();
            in_dependencies = false;
            let decl = Declaration::new(
                DeclKind::ConfigKey,
                name.clone(),
                format!("[[{}]]", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            current_section = Some((name, idx));
            continue;
        }

        // [section]
        if let Some(caps) = re_section.captures(trimmed) {
            let name = caps[1].to_string();
            in_dependencies = is_cargo
                && (name == "dependencies"
                    || name == "dev-dependencies"
                    || name == "build-dependencies"
                    || name.starts_with("dependencies.")
                    || name.starts_with("dev-dependencies.")
                    || name.starts_with("build-dependencies."));
            let decl = Declaration::new(
                DeclKind::ConfigKey,
                name.clone(),
                format!("[{}]", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            current_section = Some((name, idx));
            continue;
        }

        // key = value
        if let Some(caps) = re_kv.captures(trimmed) {
            let key = caps[1].to_string();

            if is_cargo && in_dependencies {
                imports.push(Import { text: key.clone() });
            }

            let child = Declaration::new(
                DeclKind::ConfigKey,
                key.clone(),
                trimmed.to_string(),
                Visibility::Private,
                line_num + 1,
            );

            if let Some((_, parent_idx)) = &current_section {
                declarations[*parent_idx].children.push(child);
            } else {
                // Top-level key with no section
                declarations.push(child);
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// YAML parser
// ---------------------------------------------------------------------------

fn parse_yaml(path: &Path, content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let imports = Vec::new();
    let mut declarations = Vec::new();

    let is_docker_compose = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("docker-compose"))
        .unwrap_or(false);

    let re_top_key = Regex::new(r"^([A-Za-z_][A-Za-z0-9_\-]*)\s*:").unwrap();
    let re_child_key = Regex::new(r"^(\s{2,})([A-Za-z_][A-Za-z0-9_\-]*)\s*:").unwrap();

    let mut current_top: Option<usize> = None;
    let mut in_services = false;

    for (line_num, line) in content.lines().enumerate() {
        // Skip comments and blank lines
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Top-level key (no leading whitespace)
        if let Some(caps) = re_top_key.captures(line) {
            let name = caps[1].to_string();
            in_services = is_docker_compose && name == "services";

            let decl = Declaration::new(
                DeclKind::ConfigKey,
                name.clone(),
                format!("{}:", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            current_top = Some(idx);
            continue;
        }

        // Indented child key (services children in docker-compose, or general children)
        if is_docker_compose
            && in_services
            && let Some(caps) = re_child_key.captures(line)
        {
            let indent: &str = &caps[1];
            let name = caps[2].to_string();

            // 2-space indent = direct child of services
            if indent.len() <= 4
                && let Some(parent_idx) = current_top
            {
                let child = Declaration::new(
                    DeclKind::ConfigKey,
                    name.clone(),
                    format!("service: {}", name),
                    Visibility::Private,
                    line_num + 1,
                );
                declarations[parent_idx].children.push(child);
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// JSON parser
// ---------------------------------------------------------------------------

fn parse_json(path: &Path, content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let is_package_json = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == "package.json")
        .unwrap_or(false);

    // Simple line-by-line JSON parsing: detect top-level keys by tracking brace depth.
    let re_key = Regex::new(r#"^\s*"([^"]+)"\s*:"#).unwrap();

    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut current_top: Option<(String, usize)> = None;
    let mut in_deps = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Count brace/bracket depth changes before this line's key
        // We need the depth at the start of this line, so count chars on previous lines.
        // Actually, let's track depth as we go.

        if let Some(caps) = re_key.captures(trimmed) {
            let key = caps[1].to_string();

            if brace_depth == 1 && bracket_depth == 0 {
                // Top-level key
                in_deps = is_package_json
                    && (key == "dependencies"
                        || key == "devDependencies"
                        || key == "peerDependencies"
                        || key == "optionalDependencies");

                let decl = Declaration::new(
                    DeclKind::ConfigKey,
                    key.clone(),
                    trimmed.trim_end_matches(',').to_string(),
                    Visibility::Public,
                    line_num + 1,
                );
                let idx = declarations.len();
                declarations.push(decl);
                current_top = Some((key, idx));
            } else if brace_depth == 2 && bracket_depth == 0 {
                // Depth-2 key: child of current top-level
                if is_package_json && in_deps {
                    imports.push(Import { text: key.clone() });
                }

                let child = Declaration::new(
                    DeclKind::ConfigKey,
                    key.clone(),
                    trimmed.trim_end_matches(',').to_string(),
                    Visibility::Private,
                    line_num + 1,
                );

                if let Some((_, parent_idx)) = &current_top {
                    declarations[*parent_idx].children.push(child);
                }
            }
        }

        // Update depth tracking after processing the line.
        // Skip over string contents to avoid counting braces inside strings.
        let mut chars = trimmed.chars();
        while let Some(ch) = chars.next() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if brace_depth <= 1 {
                        in_deps = false;
                    }
                }
                '[' => bracket_depth += 1,
                ']' => bracket_depth -= 1,
                '"' => {
                    // Skip until closing quote (handling escaped quotes)
                    let mut escaped = false;
                    for inner in chars.by_ref() {
                        if escaped {
                            escaped = false;
                        } else if inner == '\\' {
                            escaped = true;
                        } else if inner == '"' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// SQL parser
// ---------------------------------------------------------------------------

fn parse_sql(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let imports = Vec::new();
    let mut declarations = Vec::new();

    let re_create_table =
        Regex::new(r#"(?i)^CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?[`"]?(\w+)[`"]?"#).unwrap();
    let re_create_view = Regex::new(
        r#"(?i)^CREATE\s+(?:OR\s+REPLACE\s+)?(?:MATERIALIZED\s+)?VIEW\s+[`"]?(\w+)[`"]?"#,
    )
    .unwrap();
    let re_create_index = Regex::new(
        r#"(?i)^CREATE\s+(?:UNIQUE\s+)?INDEX\s+(?:IF\s+NOT\s+EXISTS\s+)?[`"]?(\w+)[`"]?"#,
    )
    .unwrap();
    let re_create_func =
        Regex::new(r#"(?i)^CREATE\s+(?:OR\s+REPLACE\s+)?(?:FUNCTION|PROCEDURE)\s+[`"]?(\w+)[`"]?"#)
            .unwrap();
    let re_create_type = Regex::new(r#"(?i)^CREATE\s+TYPE\s+[`"]?(\w+)[`"]?"#).unwrap();
    let re_column =
        Regex::new(r"(?i)^\s+(\w+)\s+(SERIAL|BIGSERIAL|SMALLSERIAL|INTEGER|INT|BIGINT|SMALLINT|TEXT|VARCHAR|CHAR|BOOLEAN|BOOL|FLOAT|DOUBLE|REAL|DECIMAL|NUMERIC|DATE|TIME|TIMESTAMP|TIMESTAMPTZ|UUID|JSONB?|BYTEA|BLOB|CLOB|XML|ARRAY)")
            .unwrap();

    let mut current_table: Option<usize> = None;
    let mut in_create_block = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("--")
            || trimmed.starts_with("/*")
            || trimmed.starts_with("*")
        {
            continue;
        }

        // CREATE TABLE
        if let Some(caps) = re_create_table.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::TableDef,
                name.clone(),
                format!("CREATE TABLE {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            current_table = Some(idx);
            in_create_block = true;
            continue;
        }

        // CREATE VIEW
        if let Some(caps) = re_create_view.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::TableDef,
                name.clone(),
                format!("CREATE VIEW {}", name),
                Visibility::Public,
                line_num + 1,
            ));
            current_table = None;
            in_create_block = false;
            continue;
        }

        // CREATE INDEX
        if let Some(caps) = re_create_index.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                name.clone(),
                format!("CREATE INDEX {}", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // CREATE FUNCTION / PROCEDURE
        if let Some(caps) = re_create_func.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Function,
                name.clone(),
                trimmed.to_string(),
                Visibility::Public,
                line_num + 1,
            ));
            current_table = None;
            in_create_block = false;
            continue;
        }

        // CREATE TYPE
        if let Some(caps) = re_create_type.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::SchemaType,
                name.clone(),
                format!("CREATE TYPE {}", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // Column definitions inside CREATE TABLE
        if in_create_block {
            if trimmed.starts_with(')') || trimmed.starts_with(");") {
                in_create_block = false;
                current_table = None;
                continue;
            }

            if let Some(caps) = re_column.captures(line) {
                let col_name = caps[1].to_string();
                let col_type = caps[2].to_string();

                // Skip SQL keywords that look like column names
                let upper = col_name.to_uppercase();
                if matches!(
                    upper.as_str(),
                    "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK" | "CONSTRAINT" | "INDEX" | "KEY"
                ) {
                    continue;
                }

                let child = Declaration::new(
                    DeclKind::Field,
                    col_name.clone(),
                    format!("{} {}", col_name, col_type),
                    Visibility::Private,
                    line_num + 1,
                );

                if let Some(parent_idx) = current_table {
                    declarations[parent_idx].children.push(child);
                }
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Markdown parser
// ---------------------------------------------------------------------------

fn parse_markdown(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let imports: Vec<Import> = Vec::new();
    let mut declarations: Vec<Declaration> = Vec::new();

    let re_heading = Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();

    // Stack tracks (heading_level, path) where path is the index path to
    // navigate from declarations root to this heading's Declaration node.
    // e.g., path [0, 2] means declarations[0].children[2]
    let mut heading_stack: Vec<(usize, Vec<usize>)> = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        if let Some(caps) = re_heading.captures(line) {
            let level = caps[1].len();
            let text = caps[2].trim().to_string();

            let decl = Declaration::new(
                DeclKind::Heading,
                text.clone(),
                format!("{} {}", &caps[1], text),
                Visibility::Public,
                line_num + 1,
            );

            // Pop stack entries that are at the same or deeper level
            while heading_stack
                .last()
                .map(|(l, _)| *l >= level)
                .unwrap_or(false)
            {
                heading_stack.pop();
            }

            if let Some((_, parent_path)) = heading_stack.last() {
                // Navigate to the parent declaration using the path
                let parent = get_decl_by_path_mut(&mut declarations, parent_path);
                let child_idx = parent.children.len();
                parent.children.push(decl);
                // Build path for this child
                let mut path = parent_path.clone();
                path.push(child_idx);
                heading_stack.push((level, path));
            } else {
                // No parent — this is a top-level heading.
                let idx = declarations.len();
                declarations.push(decl);
                heading_stack.push((level, vec![idx]));
            }
        }
    }

    (imports, declarations)
}

/// Navigate to a Declaration by path. Path[0] indexes into the top-level
/// declarations vec, subsequent indices index into children.
fn get_decl_by_path_mut<'a>(
    declarations: &'a mut [Declaration],
    path: &[usize],
) -> &'a mut Declaration {
    let mut current = &mut declarations[path[0]];
    for &idx in &path[1..] {
        current = &mut current.children[idx];
    }
    current
}

// ---------------------------------------------------------------------------
// Protobuf parser
// ---------------------------------------------------------------------------

fn parse_protobuf(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_syntax = Regex::new(r#"^syntax\s*=\s*"([^"]+)""#).unwrap();
    let re_package = Regex::new(r"^package\s+([\w.]+)\s*;").unwrap();
    let re_import = Regex::new(r#"^import\s+(?:public\s+)?"([^"]+)"\s*;"#).unwrap();
    let re_message = Regex::new(r"^message\s+(\w+)\s*\{?").unwrap();
    let re_service = Regex::new(r"^service\s+(\w+)\s*\{?").unwrap();
    let re_enum = Regex::new(r"^enum\s+(\w+)\s*\{?").unwrap();
    let re_rpc =
        Regex::new(r"^\s*rpc\s+(\w+)\s*\(\s*(?:stream\s+)?(\w+)\s*\)\s*returns\s*\(\s*(?:stream\s+)?(\w+)\s*\)")
            .unwrap();
    let re_field =
        Regex::new(r"^\s+(?:repeated\s+|optional\s+|required\s+|map<[^>]+>\s+)?(\w+(?:\.\w+)*)\s+(\w+)\s*=\s*(\d+)")
            .unwrap();
    let re_enum_variant = Regex::new(r"^\s+(\w+)\s*=\s*(\d+)\s*;").unwrap();

    enum BlockKind {
        Message(usize),
        Service(usize),
        Enum(usize),
    }

    let mut block_stack: Vec<BlockKind> = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut block_start_depth: Vec<i32> = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with("//") {
            // Still count braces in comments? No, skip.
            continue;
        }

        // Syntax declaration
        if let Some(caps) = re_syntax.captures(trimmed) {
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                "syntax".to_string(),
                format!("syntax = \"{}\"", &caps[1]),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // Package
        if let Some(caps) = re_package.captures(trimmed) {
            imports.push(Import {
                text: format!("package {}", &caps[1]),
            });
            continue;
        }

        // Import
        if let Some(caps) = re_import.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // Message (only at top level or nested)
        if let Some(caps) = re_message.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Message,
                name.clone(),
                format!("message {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(BlockKind::Message(idx));
            block_start_depth.push(brace_depth);
            // Count braces on this line
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // Service
        if let Some(caps) = re_service.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Service,
                name.clone(),
                format!("service {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(BlockKind::Service(idx));
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // Enum
        if let Some(caps) = re_enum.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Enum,
                name.clone(),
                format!("enum {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(BlockKind::Enum(idx));
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // RPC inside a service
        if let Some(caps) = re_rpc.captures(trimmed) {
            let name = caps[1].to_string();
            let req = caps[2].to_string();
            let resp = caps[3].to_string();
            let child = Declaration::new(
                DeclKind::RpcMethod,
                name.clone(),
                format!("rpc {}({}) returns ({})", name, req, resp),
                Visibility::Public,
                line_num + 1,
            );
            if let Some(BlockKind::Service(idx)) = block_stack.last() {
                declarations[*idx].children.push(child);
            } else {
                declarations.push(child);
            }
            // Count braces
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // Field inside a message
        if let Some(caps) = re_field.captures(line) {
            let field_type = caps[1].to_string();
            let field_name = caps[2].to_string();

            let child = Declaration::new(
                DeclKind::Field,
                field_name.clone(),
                format!("{} {}", field_type, field_name),
                Visibility::Private,
                line_num + 1,
            );
            if let Some(BlockKind::Message(idx)) = block_stack.last() {
                declarations[*idx].children.push(child);
            }
            // Count braces
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // Enum variant inside an enum
        if let Some(caps) = re_enum_variant.captures(line) {
            let variant_name = caps[1].to_string();
            // Skip if it looks like a field (has a type before it) -- enum variants are just NAME = NUMBER
            let child = Declaration::new(
                DeclKind::Variant,
                variant_name.clone(),
                variant_name.clone(),
                Visibility::Public,
                line_num + 1,
            );
            if let Some(BlockKind::Enum(idx)) = block_stack.last() {
                declarations[*idx].children.push(child);
            }
        }

        // Count braces for all other lines
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
        }

        // Pop blocks if brace depth has returned
        while let Some(start_depth) = block_start_depth.last() {
            if brace_depth <= *start_depth {
                block_start_depth.pop();
                block_stack.pop();
            } else {
                break;
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// GraphQL parser
// ---------------------------------------------------------------------------

fn parse_graphql(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let imports = Vec::new();
    let mut declarations = Vec::new();

    let re_type = Regex::new(r"^type\s+(\w+)(?:\s+implements\s+[\w\s&]+)?\s*\{?").unwrap();
    let re_input = Regex::new(r"^input\s+(\w+)\s*\{?").unwrap();
    let re_interface = Regex::new(r"^interface\s+(\w+)\s*\{?").unwrap();
    let re_enum = Regex::new(r"^enum\s+(\w+)\s*\{?").unwrap();
    let re_schema = Regex::new(r"^schema\s*\{?").unwrap();
    let re_query = Regex::new(r"^(?:query|mutation|subscription)\s+(\w+)").unwrap();
    let re_field = Regex::new(r"^\s+(\w+)(?:\([^)]*\))?\s*:\s*(.+)$").unwrap();

    enum GqlBlock {
        Type(usize),
        Input(usize),
        Interface(usize),
        Enum(usize),
        Schema(usize),
        Other,
    }

    let mut block_stack: Vec<GqlBlock> = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut block_start_depth: Vec<i32> = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // schema { ... }
        if re_schema.is_match(trimmed) {
            let decl = Declaration::new(
                DeclKind::ConfigKey,
                "schema".to_string(),
                "schema".to_string(),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(GqlBlock::Schema(idx));
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // query/mutation/subscription Name
        if let Some(caps) = re_query.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Function,
                name.clone(),
                trimmed.to_string(),
                Visibility::Public,
                line_num + 1,
            ));
            block_stack.push(GqlBlock::Other);
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // type Name { ... }
        if let Some(caps) = re_type.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::SchemaType,
                name.clone(),
                trimmed.trim_end_matches('{').trim().to_string(),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(GqlBlock::Type(idx));
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // input Name { ... }
        if let Some(caps) = re_input.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::SchemaType,
                name.clone(),
                format!("input {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(GqlBlock::Input(idx));
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // interface Name { ... }
        if let Some(caps) = re_interface.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Interface,
                name.clone(),
                format!("interface {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(GqlBlock::Interface(idx));
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // enum Name { ... }
        if let Some(caps) = re_enum.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Enum,
                name.clone(),
                format!("enum {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            block_stack.push(GqlBlock::Enum(idx));
            block_start_depth.push(brace_depth);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }

        // Fields inside type/input/interface blocks
        if let Some(caps) = re_field.captures(line) {
            let field_name = caps[1].to_string();
            let field_type = caps[2].trim().trim_end_matches(',').to_string();

            // Skip GraphQL keywords
            if matches!(
                field_name.as_str(),
                "query"
                    | "mutation"
                    | "subscription"
                    | "type"
                    | "input"
                    | "interface"
                    | "enum"
                    | "schema"
                    | "extend"
                    | "directive"
                    | "scalar"
                    | "union"
                    | "fragment"
            ) {
                // count braces and continue
                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
                continue;
            }

            let child = Declaration::new(
                DeclKind::Field,
                field_name.clone(),
                format!("{}: {}", field_name, field_type),
                Visibility::Public,
                line_num + 1,
            );

            match block_stack.last() {
                Some(GqlBlock::Type(idx))
                | Some(GqlBlock::Input(idx))
                | Some(GqlBlock::Interface(idx)) => {
                    declarations[*idx].children.push(child);
                }
                Some(GqlBlock::Schema(idx)) => {
                    declarations[*idx].children.push(child);
                }
                _ => {
                    declarations.push(child);
                }
            }
        } else if brace_depth > 0 {
            // Inside an enum block, lines might just be variant names
            if let Some(GqlBlock::Enum(idx)) = block_stack.last() {
                let variant = trimmed.trim_end_matches(',').trim();
                if !variant.is_empty()
                    && !variant.starts_with('{')
                    && !variant.starts_with('}')
                    && !variant.starts_with('#')
                    && variant
                        .chars()
                        .next()
                        .map(|c| c.is_alphabetic())
                        .unwrap_or(false)
                {
                    let child = Declaration::new(
                        DeclKind::Variant,
                        variant.to_string(),
                        variant.to_string(),
                        Visibility::Public,
                        line_num + 1,
                    );
                    declarations[*idx].children.push(child);
                }
            }
        }

        // Count braces
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
        }

        // Pop blocks
        while let Some(start_depth) = block_start_depth.last() {
            if brace_depth <= *start_depth {
                block_start_depth.pop();
                block_stack.pop();
            } else {
                break;
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Ruby parser
// ---------------------------------------------------------------------------

fn parse_ruby(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations: Vec<Declaration> = Vec::new();

    let re_require =
        Regex::new(r#"^(?:require|require_relative|load)\s+['"]([^'"]+)['"]"#).unwrap();
    let re_gem = Regex::new(r#"^\s*gem\s+['"]([^'"]+)['"]"#).unwrap();
    let re_source = Regex::new(r#"^\s*source\s+['"]([^'"]+)['"]"#).unwrap();
    let re_class = Regex::new(r"^(\s*)class\s+([A-Z]\w*)(?:\s*<\s*(\S+))?").unwrap();
    let re_module = Regex::new(r"^(\s*)module\s+([A-Z]\w*)").unwrap();
    let re_def = Regex::new(r"^(\s*)def\s+(self\.)?([A-Za-z_]\w*[?!=]?)(?:\(([^)]*)\))?").unwrap();
    let re_constant = Regex::new(r"^(\s*)([A-Z][A-Z0-9_]+)\s*=").unwrap();
    let re_attr = Regex::new(r"^\s*(?:attr_accessor|attr_reader|attr_writer)\s+(.+)$").unwrap();
    let re_include = Regex::new(r"^\s*(?:include|extend|prepend)\s+(\S+)").unwrap();

    // Track class/module nesting for parent-child
    let mut container_stack: Vec<(usize, usize)> = Vec::new(); // (indent_level, decl_index)

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // require / require_relative
        if let Some(caps) = re_require.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // gem 'name' (Gemfile)
        if let Some(caps) = re_gem.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // source 'url' (Gemfile)
        if let Some(caps) = re_source.captures(trimmed) {
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                "source".to_string(),
                format!("source '{}'", &caps[1]),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Pop containers that are at the same or deeper indent
        while container_stack
            .last()
            .map(|(i, _)| *i >= indent)
            .unwrap_or(false)
        {
            container_stack.pop();
        }

        // class ClassName < SuperClass
        if let Some(caps) = re_class.captures(line) {
            let name = caps[2].to_string();
            let sig = if let Some(parent) = caps.get(3) {
                format!("class {} < {}", name, parent.as_str())
            } else {
                format!("class {}", name)
            };
            let decl =
                Declaration::new(DeclKind::Class, name, sig, Visibility::Public, line_num + 1);
            let idx = if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
                // Return the index within parent's children (we won't nest deeper from children)
                // For simplicity, push to top-level and track there
                declarations.len() // won't be used for deeper nesting from children
            } else {
                let idx = declarations.len();
                declarations.push(decl);
                idx
            };
            // Only track top-level containers for nesting
            if container_stack.is_empty() {
                container_stack.push((indent, idx));
            }
            continue;
        }

        // module ModuleName
        if let Some(caps) = re_module.captures(line) {
            let name = caps[2].to_string();
            let decl = Declaration::new(
                DeclKind::Module,
                name.clone(),
                format!("module {}", name),
                Visibility::Public,
                line_num + 1,
            );
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                let idx = declarations.len();
                declarations.push(decl);
                container_stack.push((indent, idx));
            }
            continue;
        }

        // def method_name(args) / def self.method_name(args)
        if let Some(caps) = re_def.captures(line) {
            let is_class_method = caps.get(2).is_some();
            let name = caps[3].to_string();
            let params = caps.get(4).map(|m| m.as_str()).unwrap_or("");
            let prefix = if is_class_method { "def self." } else { "def " };
            let sig = if params.is_empty() {
                format!("{}{}", prefix, name)
            } else {
                format!("{}{}({})", prefix, name, params)
            };
            let vis = if name.starts_with('_') {
                Visibility::Private
            } else {
                Visibility::Public
            };
            let decl = Declaration::new(DeclKind::Method, name, sig, vis, line_num + 1);
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                // Top-level function
                declarations.push(Declaration::new(
                    decl.kind,
                    decl.name,
                    decl.signature,
                    decl.visibility,
                    decl.line,
                ));
            }
            continue;
        }

        // CONSTANT = value
        if let Some(caps) = re_constant.captures(line) {
            let name = caps[2].to_string();
            let decl = Declaration::new(
                DeclKind::Constant,
                name.clone(),
                format!("{} = ...", name),
                Visibility::Public,
                line_num + 1,
            );
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
            continue;
        }

        // attr_accessor :name, :email
        if let Some(caps) = re_attr.captures(trimmed) {
            let attrs_str = caps[1].to_string();
            for attr in attrs_str.split(',') {
                let attr = attr.trim().trim_start_matches(':');
                if !attr.is_empty() {
                    let decl = Declaration::new(
                        DeclKind::Field,
                        attr.to_string(),
                        format!("attr {}", attr),
                        Visibility::Public,
                        line_num + 1,
                    );
                    if let Some((_, parent_idx)) = container_stack.last() {
                        declarations[*parent_idx].children.push(decl);
                    }
                }
            }
            continue;
        }

        // include/extend/prepend Module
        if let Some(caps) = re_include.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Kotlin parser
// ---------------------------------------------------------------------------

fn parse_kotlin(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_import = Regex::new(r"^import\s+(.+)$").unwrap();
    let re_package = Regex::new(r"^package\s+(.+)$").unwrap();
    let re_class = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|abstract|open|sealed|data|inner|value|annotation|enum)\s+)*(?:class|object)\s+(\w+)",
    )
    .unwrap();
    let re_interface = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|sealed|fun)\s+)*interface\s+(\w+)",
    )
    .unwrap();
    let re_fun = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|override|open|abstract|suspend|inline|tailrec|operator|infix|external)\s+)*fun\s+(?:<[^>]+>\s+)?(?:(\w+)\.)?(\w+)\s*\(([^)]*)\)",
    )
    .unwrap();
    let re_val = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|override|const|lateinit)\s+)*(?:val|var)\s+(\w+)\s*(?::\s*(\S+))?",
    )
    .unwrap();
    let re_typealias = Regex::new(r"^\s*typealias\s+(\w+)\s*=\s*(.+)$").unwrap();
    let re_companion = Regex::new(r"^\s*companion\s+object").unwrap();

    let mut brace_depth: i32 = 0;
    let mut container_stack: Vec<(i32, usize)> = Vec::new(); // (brace_depth_at_open, decl_index)

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with("*")
        {
            // Count braces even in blank/comment lines for tracking
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            // Pop containers
            while container_stack
                .last()
                .map(|(d, _)| brace_depth <= *d)
                .unwrap_or(false)
            {
                container_stack.pop();
            }
            continue;
        }

        // package
        if let Some(caps) = re_package.captures(trimmed) {
            imports.push(Import {
                text: format!("package {}", caps[1].trim()),
            });
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // import
        if let Some(caps) = re_import.captures(trimmed) {
            imports.push(Import {
                text: caps[1].trim().to_string(),
            });
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // Skip companion object (don't treat as a named container)
        if re_companion.is_match(trimmed) {
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // interface
        if let Some(caps) = re_interface.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Interface,
                name.clone(),
                format!("interface {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // class / object / data class / enum class / sealed class
        if let Some(caps) = re_class.captures(line) {
            let name = caps[1].to_string();
            let kind = if trimmed.contains("enum ") {
                DeclKind::Enum
            } else if trimmed.contains("object ") && !trimmed.contains("companion") {
                DeclKind::Module
            } else {
                DeclKind::Class
            };
            let sig_prefix = if trimmed.contains("data class") {
                "data class"
            } else if trimmed.contains("sealed class") {
                "sealed class"
            } else if trimmed.contains("enum class") {
                "enum class"
            } else if trimmed.contains("abstract class") {
                "abstract class"
            } else if trimmed.contains("object ") {
                "object"
            } else {
                "class"
            };
            let decl = Declaration::new(
                kind,
                name.clone(),
                format!("{} {}", sig_prefix, name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
                declarations[*parent_idx].children.len() - 1 // not usable as top-level index
            } else {
                let idx = declarations.len();
                declarations.push(decl);
                idx
            };
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth && container_stack.is_empty() {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // fun
        if let Some(caps) = re_fun.captures(line) {
            let name = caps[2].to_string();
            let params = caps[3].to_string();
            let sig = format!("fun {}({})", name, truncate_value(&params, 60));
            let vis = if trimmed.starts_with("private") {
                Visibility::Private
            } else {
                Visibility::Public
            };
            let decl = Declaration::new(DeclKind::Function, name, sig, vis, line_num + 1);
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // typealias
        if let Some(caps) = re_typealias.captures(trimmed) {
            let name = caps[1].to_string();
            let target = caps[2].to_string();
            declarations.push(Declaration::new(
                DeclKind::TypeAlias,
                name.clone(),
                format!("typealias {} = {}", name, truncate_value(&target, 40)),
                Visibility::Public,
                line_num + 1,
            ));
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // val / var (only at top-level or class-level, skip local variables)
        if brace_depth <= 1
            && let Some(caps) = re_val.captures(line)
        {
            let name = caps[1].to_string();
            let type_ann = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let keyword = if trimmed.contains("const ")
                || trimmed.starts_with("val ")
                || trimmed.contains(" val ")
            {
                "val"
            } else {
                "var"
            };
            let sig = if type_ann.is_empty() {
                format!("{} {}", keyword, name)
            } else {
                format!("{} {}: {}", keyword, name, type_ann)
            };
            let decl =
                Declaration::new(DeclKind::Field, name, sig, Visibility::Public, line_num + 1);
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
        }

        count_braces(trimmed, &mut brace_depth);
        pop_containers(&mut container_stack, brace_depth);
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Swift parser
// ---------------------------------------------------------------------------

fn parse_swift(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_import = Regex::new(r"^import\s+(\S+)").unwrap();
    let re_class = Regex::new(
        r"^(?:\s*)(?:(?:public|private|internal|fileprivate|open|final)\s+)*class\s+(\w+)",
    )
    .unwrap();
    let re_struct =
        Regex::new(r"^(?:\s*)(?:(?:public|private|internal|fileprivate)\s+)*struct\s+(\w+)")
            .unwrap();
    let re_protocol =
        Regex::new(r"^(?:\s*)(?:(?:public|private|internal|fileprivate)\s+)*protocol\s+(\w+)")
            .unwrap();
    let re_enum =
        Regex::new(r"^(?:\s*)(?:(?:public|private|internal|fileprivate|indirect)\s+)*enum\s+(\w+)")
            .unwrap();
    let re_extension =
        Regex::new(r"^(?:\s*)(?:(?:public|private|internal|fileprivate)\s+)*extension\s+(\w+)")
            .unwrap();
    let re_func = Regex::new(
        r"^(?:\s*)(?:(?:public|private|internal|fileprivate|open|override|static|class|mutating|@\w+)\s+)*func\s+(\w+)\s*(?:<[^>]+>)?\s*\(([^)]*)\)",
    )
    .unwrap();
    let re_typealias = Regex::new(
        r"^(?:\s*)(?:(?:public|private|internal|fileprivate)\s+)*typealias\s+(\w+)\s*=\s*(.+)$",
    )
    .unwrap();
    let re_let = Regex::new(
        r"^(?:\s*)(?:(?:public|private|internal|fileprivate|static|class|lazy)\s+)*(?:let|var)\s+(\w+)\s*(?::\s*(\S+))?",
    )
    .unwrap();

    let mut brace_depth: i32 = 0;
    let mut container_stack: Vec<(i32, usize)> = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with("*")
        {
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // import
        if let Some(caps) = re_import.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // protocol
        if let Some(caps) = re_protocol.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Interface,
                name.clone(),
                format!("protocol {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // class
        if let Some(caps) = re_class.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Class,
                name.clone(),
                format!("class {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // struct
        if let Some(caps) = re_struct.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Struct,
                name.clone(),
                format!("struct {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // enum
        if let Some(caps) = re_enum.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Enum,
                name.clone(),
                format!("enum {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // extension
        if let Some(caps) = re_extension.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Impl,
                name.clone(),
                format!("extension {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // func
        if let Some(caps) = re_func.captures(line) {
            let name = caps[1].to_string();
            let params = caps[2].to_string();
            let sig = format!("func {}({})", name, truncate_value(&params, 60));
            let vis = if trimmed.starts_with("private") || trimmed.starts_with("fileprivate") {
                Visibility::Private
            } else {
                Visibility::Public
            };
            let decl = Declaration::new(DeclKind::Method, name, sig, vis, line_num + 1);
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // typealias
        if let Some(caps) = re_typealias.captures(line) {
            let name = caps[1].to_string();
            let target = caps[2].trim().to_string();
            declarations.push(Declaration::new(
                DeclKind::TypeAlias,
                name.clone(),
                format!("typealias {} = {}", name, truncate_value(&target, 40)),
                Visibility::Public,
                line_num + 1,
            ));
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // top-level let/var (only outside function bodies)
        if brace_depth <= 1
            && let Some(caps) = re_let.captures(line)
        {
            let name = caps[1].to_string();
            // Skip common keywords that look like var names
            if matches!(
                name.as_str(),
                "self" | "super" | "return" | "guard" | "if" | "else" | "switch" | "case"
            ) {
                count_braces(trimmed, &mut brace_depth);
                pop_containers(&mut container_stack, brace_depth);
                continue;
            }
            let type_ann = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let keyword = if trimmed.contains("let ") {
                "let"
            } else {
                "var"
            };
            let sig = if type_ann.is_empty() {
                format!("{} {}", keyword, name)
            } else {
                format!("{} {}: {}", keyword, name, type_ann)
            };
            let decl =
                Declaration::new(DeclKind::Field, name, sig, Visibility::Public, line_num + 1);
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
        }

        count_braces(trimmed, &mut brace_depth);
        pop_containers(&mut container_stack, brace_depth);
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// C# parser
// ---------------------------------------------------------------------------

fn parse_csharp(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_using = Regex::new(r"^using\s+(?:static\s+)?([A-Za-z][\w.]*)\s*;").unwrap();
    let re_namespace = Regex::new(r"^\s*namespace\s+([\w.]+)").unwrap();
    let re_class = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|abstract|sealed|static|partial)\s+)*class\s+(\w+)",
    )
    .unwrap();
    let re_interface = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|partial)\s+)*interface\s+(\w+)",
    )
    .unwrap();
    let re_struct = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|readonly|partial)\s+)*struct\s+(\w+)",
    )
    .unwrap();
    let re_enum =
        Regex::new(r"^(?:\s*)(?:(?:public|private|protected|internal)\s+)*enum\s+(\w+)").unwrap();
    let re_method = Regex::new(
        r"^(?:\s*)(?:(?:public|private|protected|internal|static|virtual|override|abstract|async|new|sealed|extern)\s+)*(?:[\w<>\[\]?,\s]+)\s+(\w+)\s*(?:<[^>]+>)?\s*\(([^)]*)\)",
    )
    .unwrap();
    let mut brace_depth: i32 = 0;
    let mut container_stack: Vec<(i32, usize)> = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with("*")
        {
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // using
        if let Some(caps) = re_using.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // namespace
        if let Some(caps) = re_namespace.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Namespace,
                name.clone(),
                format!("namespace {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // interface
        if let Some(caps) = re_interface.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Interface,
                name.clone(),
                format!("interface {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
                declarations[*parent_idx].children.len() - 1
            } else {
                let idx = declarations.len();
                declarations.push(decl);
                idx
            };
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth && container_stack.is_empty() {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // class
        if let Some(caps) = re_class.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Class,
                name.clone(),
                format!("class {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
                declarations[*parent_idx].children.len() - 1
            } else {
                let idx = declarations.len();
                declarations.push(decl);
                idx
            };
            let open_depth = brace_depth;
            count_braces(trimmed, &mut brace_depth);
            if brace_depth > open_depth && container_stack.is_empty() {
                container_stack.push((open_depth, idx));
            }
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // struct
        if let Some(caps) = re_struct.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Struct,
                name.clone(),
                format!("struct {}", name),
                Visibility::Public,
                line_num + 1,
            );
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // enum
        if let Some(caps) = re_enum.captures(line) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Enum,
                name.clone(),
                format!("enum {}", name),
                Visibility::Public,
                line_num + 1,
            );
            if let Some((_, parent_idx)) = container_stack.last() {
                declarations[*parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
            count_braces(trimmed, &mut brace_depth);
            pop_containers(&mut container_stack, brace_depth);
            continue;
        }

        // method (only inside containers)
        if !container_stack.is_empty()
            && let Some(caps) = re_method.captures(line)
        {
            let name = caps[1].to_string();
            // Skip keywords that look like method names
            if !matches!(
                name.as_str(),
                "if" | "for"
                    | "while"
                    | "switch"
                    | "catch"
                    | "using"
                    | "lock"
                    | "return"
                    | "new"
                    | "throw"
                    | "typeof"
                    | "sizeof"
                    | "nameof"
                    | "class"
                    | "struct"
            ) {
                let params = caps[2].to_string();
                let sig = format!("{}({})", name, truncate_value(&params, 60));
                let vis = if trimmed.starts_with("private") {
                    Visibility::Private
                } else if trimmed.starts_with("protected") {
                    Visibility::PublicCrate
                } else {
                    Visibility::Public
                };
                let decl = Declaration::new(DeclKind::Method, name, sig, vis, line_num + 1);
                if let Some((_, parent_idx)) = container_stack.last() {
                    declarations[*parent_idx].children.push(decl);
                }
            }
        }

        count_braces(trimmed, &mut brace_depth);
        pop_containers(&mut container_stack, brace_depth);
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Objective-C parser
// ---------------------------------------------------------------------------

fn parse_objc(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_import = Regex::new(r#"^#import\s+[<"]([^>"]+)[>"]"#).unwrap();
    let re_include = Regex::new(r#"^#include\s+[<"]([^>"]+)[>"]"#).unwrap();
    let re_interface = Regex::new(r"^@interface\s+(\w+)\s*(?::\s*(\w+))?").unwrap();
    let re_implementation = Regex::new(r"^@implementation\s+(\w+)").unwrap();
    let re_protocol_decl = Regex::new(r"^@protocol\s+(\w+)\s*(?:<|$)").unwrap();
    let re_method = Regex::new(r"^([+-])\s*\(([^)]+)\)\s*(\w+)").unwrap();
    let re_property = Regex::new(r"^@property\s*(?:\([^)]*\)\s*)?(\S+)\s*\*?\s*(\w+)\s*;").unwrap();

    let mut current_container: Option<usize> = None;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with("*")
        {
            continue;
        }

        // #import / #include
        if let Some(caps) = re_import.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }
        if let Some(caps) = re_include.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // @interface ClassName : SuperClass
        if let Some(caps) = re_interface.captures(trimmed) {
            let name = caps[1].to_string();
            let sig = if let Some(parent) = caps.get(2) {
                format!("@interface {} : {}", name, parent.as_str())
            } else {
                format!("@interface {}", name)
            };
            let decl =
                Declaration::new(DeclKind::Class, name, sig, Visibility::Public, line_num + 1);
            let idx = declarations.len();
            declarations.push(decl);
            current_container = Some(idx);
            continue;
        }

        // @implementation ClassName
        if let Some(caps) = re_implementation.captures(trimmed) {
            let name = caps[1].to_string();
            // Try to find existing @interface for this class
            let existing = declarations
                .iter()
                .position(|d| d.name == name && d.kind == DeclKind::Class);
            if let Some(idx) = existing {
                current_container = Some(idx);
            } else {
                let decl = Declaration::new(
                    DeclKind::Class,
                    name.clone(),
                    format!("@implementation {}", name),
                    Visibility::Public,
                    line_num + 1,
                );
                let idx = declarations.len();
                declarations.push(decl);
                current_container = Some(idx);
            }
            continue;
        }

        // @protocol ProtocolName
        if let Some(caps) = re_protocol_decl.captures(trimmed) {
            let name = caps[1].to_string();
            let decl = Declaration::new(
                DeclKind::Interface,
                name.clone(),
                format!("@protocol {}", name),
                Visibility::Public,
                line_num + 1,
            );
            let idx = declarations.len();
            declarations.push(decl);
            current_container = Some(idx);
            continue;
        }

        // @end
        if trimmed == "@end" {
            current_container = None;
            continue;
        }

        // Method: - (ReturnType)methodName or + (ReturnType)methodName
        if let Some(caps) = re_method.captures(trimmed) {
            let method_type = &caps[1]; // + or -
            let return_type = caps[2].to_string();
            let name = caps[3].to_string();
            let sig = format!("{} ({}){}", method_type, return_type, name);
            let decl = Declaration::new(
                DeclKind::Method,
                name,
                sig,
                Visibility::Public,
                line_num + 1,
            );
            if let Some(parent_idx) = current_container {
                declarations[parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
            continue;
        }

        // @property
        if let Some(caps) = re_property.captures(trimmed) {
            let prop_type = caps[1].to_string();
            let name = caps[2].to_string();
            let decl = Declaration::new(
                DeclKind::Field,
                name.clone(),
                format!("{} {}", prop_type, name),
                Visibility::Public,
                line_num + 1,
            );
            if let Some(parent_idx) = current_container {
                declarations[parent_idx].children.push(decl);
            } else {
                declarations.push(decl);
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// XML parser
// ---------------------------------------------------------------------------

fn parse_xml(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let imports = Vec::new();
    let mut declarations = Vec::new();

    let re_open_tag = Regex::new(r"<(\w[\w\-.]*)(?:[\s/>]|$)").unwrap();
    let re_close_tag = Regex::new(r"</(\w[\w\-.]*)(?:[\s>]|$)").unwrap();

    let mut seen_elements = std::collections::HashSet::new();
    let mut depth: i32 = 0;
    let mut in_comment = false;
    // Track multiline opening tags: tag name and whether it's self-closing
    let mut pending_open: Option<(String, usize)> = None; // (tag_name, line_num)

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Track comment state
        if in_comment {
            if trimmed.contains("-->") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.contains("<!--") {
            if !trimmed.contains("-->") {
                in_comment = true;
            }
            continue;
        }

        // Skip XML declaration and processing instructions
        if trimmed.starts_with("<?") {
            continue;
        }

        // If we're inside a multiline opening tag, check if this line closes it
        if let Some((ref tag_name, tag_line)) = pending_open {
            if trimmed.contains("/>") {
                // Self-closing multiline tag — don't change depth
                // But still extract the element if at appropriate depth
                if depth <= 1 && !seen_elements.contains(tag_name) {
                    seen_elements.insert(tag_name.clone());
                    declarations.push(Declaration::new(
                        DeclKind::ConfigKey,
                        tag_name.clone(),
                        format!("<{}>", tag_name),
                        Visibility::Public,
                        tag_line + 1,
                    ));
                }
                pending_open = None;
                continue;
            } else if trimmed.contains('>') {
                // Closing the opening tag — element opens, depth increments
                if depth <= 1 && !seen_elements.contains(tag_name) {
                    seen_elements.insert(tag_name.clone());
                    declarations.push(Declaration::new(
                        DeclKind::ConfigKey,
                        tag_name.clone(),
                        format!("<{}>", tag_name),
                        Visibility::Public,
                        tag_line + 1,
                    ));
                }
                depth += 1;
                pending_open = None;
                // This line might also contain more tags after the >, fall through
                // But for simplicity in XML, we'll continue
                continue;
            }
            // Still inside multiline tag attributes
            continue;
        }

        // Count opening and closing tags on this line
        // Process closing tags
        for _ in re_close_tag.captures_iter(trimmed) {
            depth -= 1;
        }

        // Process opening tags
        if !trimmed.starts_with("</")
            && let Some(caps) = re_open_tag.captures(trimmed)
        {
            let tag = caps[1].to_string();

            // Check if this is a self-closing tag
            let is_self_closing = trimmed.contains("/>");

            // Check if the tag's > is on this line
            let has_closing_bracket = trimmed.contains('>');

            if !is_self_closing && has_closing_bracket {
                // Normal single-line opening tag
                if depth <= 1 && !seen_elements.contains(&tag) {
                    seen_elements.insert(tag.clone());
                    declarations.push(Declaration::new(
                        DeclKind::ConfigKey,
                        tag.clone(),
                        format!("<{}>", tag),
                        Visibility::Public,
                        line_num + 1,
                    ));
                }
                depth += 1;
            } else if is_self_closing {
                // Self-closing tag — extract but don't change depth
                if depth <= 1 && !seen_elements.contains(&tag) {
                    seen_elements.insert(tag.clone());
                    declarations.push(Declaration::new(
                        DeclKind::ConfigKey,
                        tag.clone(),
                        format!("<{}>", tag),
                        Visibility::Public,
                        line_num + 1,
                    ));
                }
            } else {
                // Multiline opening tag — no > on this line
                pending_open = Some((tag, line_num));
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// HTML parser
// ---------------------------------------------------------------------------

fn parse_html(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_tag = Regex::new(r"<(head|body|header|nav|main|section|article|aside|footer|form|table|script|style|template|slot|dialog)[\s>]").unwrap();
    let re_id = Regex::new(r#"id\s*=\s*["']([^"']+)["']"#).unwrap();
    let re_title = Regex::new(r"<title[^>]*>([^<]+)</title>").unwrap();
    let re_link_rel =
        Regex::new(r#"<link\s+rel\s*=\s*["']stylesheet["'][^>]*href\s*=\s*["']([^"']+)["']"#)
            .unwrap();
    let re_script_src = Regex::new(r#"<script[^>]*src\s*=\s*["']([^"']+)["']"#).unwrap();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Semantic section tags
        if let Some(caps) = re_tag.captures(trimmed) {
            let tag = caps[1].to_string();
            let mut name = tag.clone();
            // If it has an id, use that
            if let Some(id_caps) = re_id.captures(trimmed) {
                name = format!("{}#{}", tag, &id_caps[1]);
            }
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                name.clone(),
                format!("<{}>", name),
                Visibility::Public,
                line_num + 1,
            ));
        }

        // <title>
        if let Some(caps) = re_title.captures(trimmed) {
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                "title".to_string(),
                format!("title: {}", truncate_value(&caps[1], 60)),
                Visibility::Public,
                line_num + 1,
            ));
        }

        // External resources as imports
        if let Some(caps) = re_link_rel.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
        }
        if let Some(caps) = re_script_src.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// CSS parser
// ---------------------------------------------------------------------------

fn parse_css(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_import = Regex::new(r#"^@import\s+(?:url\()?['"]?([^'")]+)['"]?\)?\s*;"#).unwrap();
    let re_at_rule =
        Regex::new(r"^@(media|keyframes|font-face|supports|layer|container)\s*(.*)$").unwrap();
    let re_selector = Regex::new(r"^([.#]?[A-Za-z_\-][\w\-.*#:>\s,\[\]=~|^$]+)\s*\{").unwrap();
    let re_css_var = Regex::new(r"^\s*--([A-Za-z][\w-]*)\s*:").unwrap();

    let mut brace_depth: i32 = 0;
    let mut in_comment = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Track block comments
        if in_comment {
            if trimmed.contains("*/") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") {
            if !trimmed.contains("*/") {
                in_comment = true;
            }
            continue;
        }

        // @import
        if let Some(caps) = re_import.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // @-rules (only at top level)
        if brace_depth == 0 {
            if let Some(caps) = re_at_rule.captures(trimmed) {
                let rule = caps[1].to_string();
                let detail = caps[2].trim_end_matches('{').trim().to_string();
                let name = if detail.is_empty() {
                    format!("@{}", rule)
                } else {
                    format!("@{} {}", rule, truncate_value(&detail, 50))
                };
                declarations.push(Declaration::new(
                    DeclKind::ConfigKey,
                    name.clone(),
                    name,
                    Visibility::Public,
                    line_num + 1,
                ));
                count_braces(trimmed, &mut brace_depth);
                continue;
            }

            // Selectors at top level
            if let Some(caps) = re_selector.captures(trimmed) {
                let selector = caps[1].trim().to_string();
                declarations.push(Declaration::new(
                    DeclKind::Class,
                    selector.clone(),
                    selector,
                    Visibility::Public,
                    line_num + 1,
                ));
            }
        }

        // CSS custom properties (variables)
        if let Some(caps) = re_css_var.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Constant,
                format!("--{}", name),
                trimmed.trim_end_matches(';').to_string(),
                Visibility::Public,
                line_num + 1,
            ));
        }

        count_braces(trimmed, &mut brace_depth);
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Gradle parser
// ---------------------------------------------------------------------------

fn parse_gradle(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let imports = Vec::new();
    let mut declarations = Vec::new();

    let re_plugin_id = Regex::new(r#"^\s*id\s*(?:[('"]\s*)*([^'"()\s]+)"#).unwrap();
    let re_plugin_id_inline = Regex::new(r#"id\s*(?:[('"]\s*)*([^'"()\s]+)"#).unwrap();
    let re_plugin_apply = Regex::new(r#"^\s*apply\s+plugin:\s*['"]([^'"]+)['"]"#).unwrap();
    let re_top_block = Regex::new(r"^(\w+)\s*(?:\([^)]*\)\s*)?\{").unwrap();
    let re_task = Regex::new(r#"^\s*(?:task\s+|tasks\.register\s*\(\s*['"])(\w+)"#).unwrap();
    let re_def = Regex::new(r"^\s*(?:def|val|var)\s+(\w+)\s*=").unwrap();

    let mut brace_depth: i32 = 0;
    let mut in_plugins = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with("*")
        {
            count_braces(trimmed, &mut brace_depth);
            continue;
        }

        // Top-level blocks
        if brace_depth == 0
            && let Some(caps) = re_top_block.captures(trimmed)
        {
            let name = caps[1].to_string();
            in_plugins = name == "plugins";
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                name.clone(),
                format!("{} {{ }}", name),
                Visibility::Public,
                line_num + 1,
            ));
            // Extract plugin IDs from the same line (single-line plugins { id("...") })
            if in_plugins {
                for pcaps in re_plugin_id_inline.captures_iter(trimmed) {
                    let id = pcaps[1].to_string();
                    declarations.push(Declaration::new(
                        DeclKind::ConfigKey,
                        id.clone(),
                        format!("plugin: {}", id),
                        Visibility::Public,
                        line_num + 1,
                    ));
                }
            }
            count_braces(trimmed, &mut brace_depth);
            continue;
        }

        // Plugin ids (multi-line plugins block)
        if in_plugins && let Some(caps) = re_plugin_id.captures(trimmed) {
            let id = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                id.clone(),
                format!("plugin: {}", id),
                Visibility::Public,
                line_num + 1,
            ));
        }

        // apply plugin:
        if let Some(caps) = re_plugin_apply.captures(trimmed) {
            let id = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                id.clone(),
                format!("plugin: {}", id),
                Visibility::Public,
                line_num + 1,
            ));
            count_braces(trimmed, &mut brace_depth);
            continue;
        }

        // task
        if let Some(caps) = re_task.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Function,
                name.clone(),
                format!("task {}", name),
                Visibility::Public,
                line_num + 1,
            ));
            count_braces(trimmed, &mut brace_depth);
            continue;
        }

        // def / val / var
        if brace_depth <= 1
            && let Some(caps) = re_def.captures(trimmed)
        {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Constant,
                name.clone(),
                truncate_value(trimmed, 80),
                Visibility::Public,
                line_num + 1,
            ));
            count_braces(trimmed, &mut brace_depth);
            continue;
        }

        count_braces(trimmed, &mut brace_depth);

        // Track when we leave top-level blocks
        if brace_depth == 0 {
            in_plugins = false;
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// CMake parser
// ---------------------------------------------------------------------------

fn parse_cmake(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    let re_project = Regex::new(r"(?i)^\s*project\s*\(\s*(\w+)").unwrap();
    let re_add_exe = Regex::new(r"(?i)^\s*add_executable\s*\(\s*(\w+)").unwrap();
    let re_add_lib = Regex::new(r"(?i)^\s*add_library\s*\(\s*(\w+)").unwrap();
    let re_function = Regex::new(r"(?i)^\s*function\s*\(\s*(\w+)").unwrap();
    let re_macro = Regex::new(r"(?i)^\s*macro\s*\(\s*(\w+)").unwrap();
    let re_option = Regex::new(r#"(?i)^\s*option\s*\(\s*(\w+)\s+"([^"]*)"#).unwrap();
    let re_set = Regex::new(r"(?i)^\s*set\s*\(\s*(\w+)").unwrap();
    let re_find = Regex::new(r"(?i)^\s*find_package\s*\(\s*(\w+)").unwrap();
    let re_add_subdir = Regex::new(r"(?i)^\s*add_subdirectory\s*\(\s*(\S+)").unwrap();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // project()
        if let Some(caps) = re_project.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Module,
                name.clone(),
                format!("project({})", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // add_executable()
        if let Some(caps) = re_add_exe.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Function,
                name.clone(),
                format!("add_executable({})", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // add_library()
        if let Some(caps) = re_add_lib.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Module,
                name.clone(),
                format!("add_library({})", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // function()
        if let Some(caps) = re_function.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Function,
                name.clone(),
                format!("function({})", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // macro()
        if let Some(caps) = re_macro.captures(trimmed) {
            let name = caps[1].to_string();
            declarations.push(Declaration::new(
                DeclKind::Macro,
                name.clone(),
                format!("macro({})", name),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // option()
        if let Some(caps) = re_option.captures(trimmed) {
            let name = caps[1].to_string();
            let desc = caps[2].to_string();
            declarations.push(Declaration::new(
                DeclKind::ConfigKey,
                name.clone(),
                format!("option({} \"{}\")", name, truncate_value(&desc, 50)),
                Visibility::Public,
                line_num + 1,
            ));
            continue;
        }

        // set() - only for uppercase variables (conventions for cache/options)
        if let Some(caps) = re_set.captures(trimmed) {
            let name = caps[1].to_string();
            if name
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                declarations.push(Declaration::new(
                    DeclKind::Constant,
                    name.clone(),
                    format!("set({})", name),
                    Visibility::Public,
                    line_num + 1,
                ));
            }
            continue;
        }

        // find_package()
        if let Some(caps) = re_find.captures(trimmed) {
            imports.push(Import {
                text: caps[1].to_string(),
            });
            continue;
        }

        // add_subdirectory()
        if let Some(caps) = re_add_subdir.captures(trimmed) {
            imports.push(Import {
                text: caps[1].trim_end_matches(')').to_string(),
            });
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Properties parser (.properties files)
// ---------------------------------------------------------------------------

fn parse_properties(content: &str) -> (Vec<Import>, Vec<Declaration>) {
    let imports = Vec::new();
    let mut declarations = Vec::new();

    let re_kv = Regex::new(r"^([A-Za-z_][\w.\-]*)\s*[=:](.*)$").unwrap();

    // Group by prefix (first segment before dot)
    let mut sections: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }

        if let Some(caps) = re_kv.captures(trimmed) {
            let key = caps[1].to_string();
            let value = caps[2].trim().to_string();

            // Extract prefix for grouping
            if let Some(dot_pos) = key.find('.') {
                let prefix = &key[..dot_pos];
                if !sections.contains_key(prefix) {
                    let decl = Declaration::new(
                        DeclKind::ConfigKey,
                        prefix.to_string(),
                        format!("{}.*", prefix),
                        Visibility::Public,
                        line_num + 1,
                    );
                    let idx = declarations.len();
                    declarations.push(decl);
                    sections.insert(prefix.to_string(), idx);
                }
                let parent_idx = sections[prefix];
                declarations[parent_idx].children.push(Declaration::new(
                    DeclKind::ConfigKey,
                    key.clone(),
                    format!("{} = {}", key, truncate_value(&value, 50)),
                    Visibility::Private,
                    line_num + 1,
                ));
            } else {
                declarations.push(Declaration::new(
                    DeclKind::ConfigKey,
                    key.clone(),
                    format!("{} = {}", key, truncate_value(&value, 50)),
                    Visibility::Public,
                    line_num + 1,
                ));
            }
        }
    }

    (imports, declarations)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn count_braces(line: &str, depth: &mut i32) {
    for ch in line.chars() {
        match ch {
            '{' => *depth += 1,
            '}' => *depth -= 1,
            _ => {}
        }
    }
}

fn pop_containers(stack: &mut Vec<(i32, usize)>, brace_depth: i32) {
    while stack
        .last()
        .map(|(d, _)| brace_depth <= *d)
        .unwrap_or(false)
    {
        stack.pop();
    }
}

fn truncate_value(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parser(lang: Language) -> RegexParser {
        RegexParser::new(lang)
    }

    #[test]
    fn test_shell_functions() {
        let content = r#"#!/bin/bash
source ./utils.sh
. /etc/profile

function greet() {
    echo "hello"
}

function setup {
    echo "setup"
}

cleanup() {
    echo "done"
}

export PATH="/usr/local/bin:$PATH"
alias ll="ls -la"
"#;
        let parser = make_parser(Language::Shell);
        let result = parser.parse_file(Path::new("test.sh"), content).unwrap();
        assert_eq!(result.imports.len(), 2);
        assert_eq!(result.imports[0].text, "./utils.sh");
        assert!(result.declarations.iter().any(|d| d.name == "greet"));
        assert!(result.declarations.iter().any(|d| d.name == "setup"));
        assert!(result.declarations.iter().any(|d| d.name == "cleanup"));
        assert!(result.declarations.iter().any(|d| d.name == "PATH"));
        assert!(result.declarations.iter().any(|d| d.name == "ll"));
    }

    #[test]
    fn test_toml_cargo() {
        let content = r#"[package]
name = "my-crate"
version = "0.1.0"

[dependencies]
serde = "1"
anyhow = "1"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
"#;
        let parser = make_parser(Language::Toml);
        let result = parser.parse_file(Path::new("Cargo.toml"), content).unwrap();
        // Should have sections and imports for deps
        assert!(result.imports.iter().any(|i| i.text == "serde"));
        assert!(result.imports.iter().any(|i| i.text == "anyhow"));
        assert!(result.imports.iter().any(|i| i.text == "tokio"));
        assert!(result.declarations.iter().any(|d| d.name == "package"));
        assert!(result.declarations.iter().any(|d| d.name == "dependencies"));
    }

    #[test]
    fn test_sql_create_table() {
        let content = r#"CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email TEXT UNIQUE
);

CREATE INDEX idx_users_email ON users(email);

CREATE FUNCTION get_user(user_id INTEGER) RETURNS users AS $$
BEGIN
    RETURN QUERY SELECT * FROM users WHERE id = user_id;
END;
$$ LANGUAGE plpgsql;

CREATE VIEW active_users AS
SELECT * FROM users WHERE active = true;

CREATE TYPE status AS ENUM ('active', 'inactive');
"#;
        let parser = make_parser(Language::Sql);
        let result = parser.parse_file(Path::new("schema.sql"), content).unwrap();
        let table = result
            .declarations
            .iter()
            .find(|d| d.name == "users" && d.kind == DeclKind::TableDef)
            .unwrap();
        assert!(table.children.iter().any(|c| c.name == "id"));
        assert!(table.children.iter().any(|c| c.name == "name"));
        assert!(table.children.iter().any(|c| c.name == "email"));
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "idx_users_email")
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "get_user" && d.kind == DeclKind::Function)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "active_users" && d.kind == DeclKind::TableDef)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "status" && d.kind == DeclKind::SchemaType)
        );
    }

    #[test]
    fn test_markdown_headings() {
        let content = r#"# Introduction

Some text here.

## Getting Started

### Installation

### Configuration

## Usage

# API Reference

## Methods
"#;
        let parser = make_parser(Language::Markdown);
        let result = parser.parse_file(Path::new("README.md"), content).unwrap();
        // Two H1 headings at top level
        let top_level: Vec<_> = result
            .declarations
            .iter()
            .filter(|d| d.signature.starts_with("# "))
            .collect();
        assert_eq!(top_level.len(), 2);
        // H2 headings as children of H1
        let intro = result
            .declarations
            .iter()
            .find(|d| d.name == "Introduction")
            .unwrap();
        assert!(intro.children.iter().any(|c| c.name == "Getting Started"));
        assert!(intro.children.iter().any(|c| c.name == "Usage"));
    }

    #[test]
    fn test_protobuf() {
        let content = r#"syntax = "proto3";

package myservice.v1;

import "google/protobuf/timestamp.proto";

message User {
    string id = 1;
    string name = 2;
    string email = 3;
}

enum Status {
    UNKNOWN = 0;
    ACTIVE = 1;
    INACTIVE = 2;
}

service UserService {
    rpc GetUser(GetUserRequest) returns (User);
    rpc ListUsers(ListUsersRequest) returns (ListUsersResponse);
}
"#;
        let parser = make_parser(Language::Protobuf);
        let result = parser.parse_file(Path::new("user.proto"), content).unwrap();
        assert!(result.imports.iter().any(|i| i.text.contains("myservice")));
        assert!(result.imports.iter().any(|i| i.text.contains("timestamp")));
        let msg = result
            .declarations
            .iter()
            .find(|d| d.name == "User" && d.kind == DeclKind::Message)
            .unwrap();
        assert_eq!(msg.children.len(), 3);
        let enm = result
            .declarations
            .iter()
            .find(|d| d.name == "Status" && d.kind == DeclKind::Enum)
            .unwrap();
        assert_eq!(enm.children.len(), 3);
        let svc = result
            .declarations
            .iter()
            .find(|d| d.name == "UserService" && d.kind == DeclKind::Service)
            .unwrap();
        assert_eq!(svc.children.len(), 2);
        assert!(svc.children[0].kind == DeclKind::RpcMethod);
    }

    #[test]
    fn test_graphql() {
        let content = r#"type User {
    id: ID!
    name: String!
    email: String
    posts: [Post!]!
}

input CreateUserInput {
    name: String!
    email: String!
}

interface Node {
    id: ID!
}

enum Role {
    ADMIN
    USER
    GUEST
}

query GetUser($id: ID!) {
    user(id: $id) {
        id
        name
    }
}

schema {
    query: Query
    mutation: Mutation
}
"#;
        let parser = make_parser(Language::GraphQL);
        let result = parser
            .parse_file(Path::new("schema.graphql"), content)
            .unwrap();
        let user_type = result
            .declarations
            .iter()
            .find(|d| d.name == "User" && d.kind == DeclKind::SchemaType)
            .unwrap();
        assert_eq!(user_type.children.len(), 4);
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "CreateUserInput" && d.kind == DeclKind::SchemaType)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "Node" && d.kind == DeclKind::Interface)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "Role" && d.kind == DeclKind::Enum)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "GetUser" && d.kind == DeclKind::Function)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "schema" && d.kind == DeclKind::ConfigKey)
        );
    }

    #[test]
    fn test_ruby() {
        let content = r#"require 'json'
require_relative 'helper'

module MyModule
  class MyClass < Base
    TIMEOUT = 30

    attr_accessor :name, :email

    def initialize(name)
      @name = name
    end

    def self.create(args)
    end

    def greet
    end
  end
end
"#;
        let parser = make_parser(Language::Ruby);
        let result = parser.parse_file(Path::new("app.rb"), content).unwrap();
        assert_eq!(result.imports.len(), 2);
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "MyModule" && d.kind == DeclKind::Module)
        );
    }

    #[test]
    fn test_kotlin() {
        let content = r#"package com.example

import com.example.Base

class MyClass : Base() {
    override fun getName(): String = "test"

    fun doSomething(x: Int, y: String) {
    }
}

data class Point(val x: Int, val y: Int)

interface Drawable {
    fun draw()
}
"#;
        let parser = make_parser(Language::Kotlin);
        let result = parser.parse_file(Path::new("Test.kt"), content).unwrap();
        assert!(
            result
                .imports
                .iter()
                .any(|i| i.text.contains("com.example"))
        );
        let cls = result
            .declarations
            .iter()
            .find(|d| d.name == "MyClass" && d.kind == DeclKind::Class);
        assert!(cls.is_some());
        assert!(cls.unwrap().children.iter().any(|c| c.name == "getName"));
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "Point" && d.kind == DeclKind::Class)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "Drawable" && d.kind == DeclKind::Interface)
        );
    }

    #[test]
    fn test_swift() {
        let content = r#"import UIKit
import Foundation

class AppDelegate: UIResponder {
    func application(_ app: UIApplication) -> Bool {
        return true
    }
}

protocol Drawable {
    func draw()
}

struct Point {
    let x: Double
    let y: Double
}

enum Direction {
    case north
    case south
}
"#;
        let parser = make_parser(Language::Swift);
        let result = parser.parse_file(Path::new("App.swift"), content).unwrap();
        assert_eq!(result.imports.len(), 2);
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "AppDelegate" && d.kind == DeclKind::Class)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "Drawable" && d.kind == DeclKind::Interface)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "Point" && d.kind == DeclKind::Struct)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "Direction" && d.kind == DeclKind::Enum)
        );
    }

    #[test]
    fn test_csharp() {
        let content = r#"using System;
using System.Collections.Generic;

namespace MyApp {
    public class MyClass : BaseClass {
        public void DoSomething(string name) {
        }
    }

    public interface IDrawable {
        void Draw();
    }

    public enum Color {
        Red,
        Green,
        Blue
    }
}
"#;
        let parser = make_parser(Language::CSharp);
        let result = parser.parse_file(Path::new("App.cs"), content).unwrap();
        assert_eq!(result.imports.len(), 2);
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "MyApp" && d.kind == DeclKind::Namespace)
        );
    }

    #[test]
    fn test_objc() {
        let content = r#"#import <Foundation/Foundation.h>
#import "MyHeader.h"

@interface MyClass : NSObject

- (void)doSomething;
+ (instancetype)create;

@end

@implementation MyClass

- (void)doSomething {
}

@end
"#;
        let parser = make_parser(Language::ObjectiveC);
        let result = parser.parse_file(Path::new("MyClass.m"), content).unwrap();
        assert_eq!(result.imports.len(), 2);
        let cls = result
            .declarations
            .iter()
            .find(|d| d.name == "MyClass" && d.kind == DeclKind::Class)
            .unwrap();
        assert!(cls.children.iter().any(|c| c.name == "doSomething"));
        assert!(cls.children.iter().any(|c| c.name == "create"));
    }

    #[test]
    fn test_xml() {
        let content = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.INTERNET" />
    <application android:name=".MainApplication">
        <activity android:name=".MainActivity" />
    </application>
</manifest>
"#;
        let parser = make_parser(Language::Xml);
        let result = parser
            .parse_file(Path::new("AndroidManifest.xml"), content)
            .unwrap();
        assert!(
            !result.declarations.is_empty(),
            "XML should have declarations"
        );
        assert!(result.declarations.iter().any(|d| d.name == "manifest"));
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "uses-permission")
        );
        assert!(result.declarations.iter().any(|d| d.name == "application"));
        // activity is depth 2, should NOT be extracted
        assert!(!result.declarations.iter().any(|d| d.name == "activity"));
    }

    #[test]
    fn test_xml_multiline_tags() {
        let content = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.INTERNET" />
    <application
        android:name=".MainApplication"
        android:label="@string/app_name"
        android:icon="@mipmap/ic_launcher">
        <activity
            android:name=".MainActivity"
            android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#;
        let parser = make_parser(Language::Xml);
        let result = parser
            .parse_file(Path::new("AndroidManifest.xml"), content)
            .unwrap();
        assert!(
            result.declarations.iter().any(|d| d.name == "manifest"),
            "should find manifest"
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "uses-permission"),
            "should find uses-permission"
        );
        assert!(
            result.declarations.iter().any(|d| d.name == "application"),
            "should find application (multiline)"
        );
        // Deeper elements should NOT be extracted
        assert!(
            !result.declarations.iter().any(|d| d.name == "activity"),
            "activity is depth 2, skip"
        );
        assert!(
            !result
                .declarations
                .iter()
                .any(|d| d.name == "intent-filter"),
            "intent-filter is depth 3, skip"
        );
        assert!(
            !result.declarations.iter().any(|d| d.name == "action"),
            "action is depth 4, skip"
        );
    }

    #[test]
    fn test_css() {
        let content = r#"@import url('fonts.css');

:root {
    --primary-color: #333;
}

.container {
    display: flex;
}

#header {
    background: white;
}

@media (max-width: 768px) {
    .container {
        flex-direction: column;
    }
}
"#;
        let parser = make_parser(Language::Css);
        let result = parser.parse_file(Path::new("style.css"), content).unwrap();
        assert_eq!(result.imports.len(), 1);
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name.contains("container"))
        );
    }

    #[test]
    fn test_gradle_single_line_plugins() {
        let content = r#"pluginManagement { includeBuild("../node_modules/@react-native/gradle-plugin") }
plugins { id("com.facebook.react.settings") }
rootProject.name = 'fernweh_v2'
include ':app'
"#;
        let parser = make_parser(Language::Gradle);
        let result = parser
            .parse_file(Path::new("settings.gradle"), content)
            .unwrap();
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "pluginManagement"),
            "should find pluginManagement block"
        );
        assert!(
            result.declarations.iter().any(|d| d.name == "plugins"),
            "should find plugins block"
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "com.facebook.react.settings"),
            "should find plugin ID from single-line plugins block"
        );
    }

    #[test]
    fn test_ruby_gemfile() {
        let content = r#"source 'https://rubygems.org'

ruby ">= 2.6.10"

gem 'cocoapods', '>= 1.13'
gem 'activesupport', '>= 6.1'
gem 'bigdecimal'
"#;
        let parser = make_parser(Language::Ruby);
        let result = parser.parse_file(Path::new("Gemfile"), content).unwrap();
        assert_eq!(result.imports.len(), 3);
        assert!(result.imports.iter().any(|i| i.text == "cocoapods"));
        assert!(result.imports.iter().any(|i| i.text == "activesupport"));
        assert!(result.imports.iter().any(|i| i.text == "bigdecimal"));
        assert!(result.declarations.iter().any(|d| d.name == "source"));
    }

    #[test]
    fn test_cmake() {
        let content = r#"cmake_minimum_required(VERSION 3.20)
project(MyProject)

add_executable(myapp main.cpp)
add_library(mylib STATIC lib.cpp)

find_package(Boost REQUIRED)

function(my_helper)
    message("helper")
endfunction()
"#;
        let parser = make_parser(Language::Cmake);
        let result = parser
            .parse_file(Path::new("CMakeLists.txt"), content)
            .unwrap();
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "MyProject" && d.kind == DeclKind::Module)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "myapp" && d.kind == DeclKind::Function)
        );
        assert!(
            result
                .declarations
                .iter()
                .any(|d| d.name == "my_helper" && d.kind == DeclKind::Function)
        );
        assert!(result.imports.iter().any(|i| i.text == "Boost"));
    }

    #[test]
    fn test_properties() {
        let content = r#"# Database config
db.host=localhost
db.port=5432
db.name=mydb
app.name=MyApp
version=1.0
"#;
        let parser = make_parser(Language::Properties);
        let result = parser
            .parse_file(Path::new("config.properties"), content)
            .unwrap();
        assert!(result.declarations.iter().any(|d| d.name == "db"));
        let db = result.declarations.iter().find(|d| d.name == "db").unwrap();
        assert_eq!(db.children.len(), 3);
        assert!(result.declarations.iter().any(|d| d.name == "version"));
    }
}
