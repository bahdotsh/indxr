use std::collections::HashSet;
use std::fmt::Write;

use anyhow::Result;

use crate::model::CodebaseIndex;
use crate::model::DetailLevel;
use crate::model::declarations::{DeclKind, Declaration, Visibility};

use super::OutputFormatter;

/// Options that control what sections appear in markdown output.
#[derive(Debug, Clone, Default)]
pub struct MarkdownOptions {
    pub omit_imports: bool,
    pub omit_tree: bool,
}

pub struct MarkdownFormatter {
    pub options: MarkdownOptions,
}

impl MarkdownFormatter {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            options: MarkdownOptions::default(),
        }
    }

    pub fn with_options(options: MarkdownOptions) -> Self {
        Self { options }
    }
}

impl OutputFormatter for MarkdownFormatter {
    fn format(&self, index: &CodebaseIndex, detail: DetailLevel) -> Result<String> {
        let mut out = String::new();

        // Header
        writeln!(out, "# Codebase Index: {}", index.root_name)?;
        writeln!(out)?;

        // Stats
        let mut lang_summary: Vec<String> = index
            .stats
            .languages
            .iter()
            .map(|(lang, count)| format!("{} ({})", lang, count))
            .collect();
        lang_summary.sort();
        writeln!(
            out,
            "> Generated: {} | Files: {} | Lines: {}",
            index.generated_at, index.stats.total_files, index.stats.total_lines
        )?;
        writeln!(out, "> Languages: {}", lang_summary.join(", "))?;
        writeln!(out)?;

        // Directory tree
        if !self.options.omit_tree {
            writeln!(out, "## Directory Structure")?;
            writeln!(out)?;
            writeln!(out, "```")?;
            writeln!(out, "{}/", index.root_name)?;
            for entry in &index.tree {
                let indent = "  ".repeat(entry.depth);
                if entry.is_dir {
                    writeln!(out, "{}{}/", indent, entry.path)?;
                } else {
                    writeln!(out, "{}{}", indent, entry.path)?;
                }
            }
            writeln!(out, "```")?;
            writeln!(out)?;
        }

        // Summary mode stops here
        if detail == DetailLevel::Summary {
            return Ok(out);
        }

        // Public API surface section — collect which declarations we show here
        // so we can avoid duplicating them in per-file sections
        let mut shown_in_api: HashSet<(String, usize)> = HashSet::new();

        let has_public = index.files.iter().any(|f| {
            f.declarations
                .iter()
                .any(|d| matches!(d.visibility, Visibility::Public))
        });

        if has_public {
            writeln!(out, "---")?;
            writeln!(out)?;
            writeln!(out, "## Public API Surface")?;
            writeln!(out)?;
            for file in &index.files {
                let public_decls: Vec<&Declaration> = file
                    .declarations
                    .iter()
                    .filter(|d| {
                        matches!(d.visibility, Visibility::Public)
                            && !matches!(d.kind, DeclKind::Impl)
                    })
                    .collect();
                if public_decls.is_empty() {
                    continue;
                }
                let file_path = file.path.display().to_string();
                writeln!(out, "**{}**", file_path)?;
                for decl in &public_decls {
                    write!(out, "- `{}`", decl.signature)?;
                    if detail == DetailLevel::Full {
                        write_badges(&mut out, decl)?;
                    }
                    writeln!(out)?;
                    // Track that we showed this declaration in the API surface
                    shown_in_api.insert((file_path.clone(), decl.line));
                }
                writeln!(out)?;
            }
        }

        // File sections
        for file in &index.files {
            let file_path = file.path.display().to_string();

            writeln!(out, "---")?;
            writeln!(out)?;
            writeln!(out, "## {}", file_path)?;
            writeln!(out)?;
            writeln!(
                out,
                "**Language:** {} | **Size:** {} | **Lines:** {}",
                file.language,
                format_size(file.size),
                file.lines
            )?;
            writeln!(out)?;

            // Imports (with summarization for large import lists)
            if !self.options.omit_imports && !file.imports.is_empty() {
                writeln!(out, "**Imports:**")?;
                let max_shown = 10;
                let total = file.imports.len();
                for import in file.imports.iter().take(max_shown) {
                    writeln!(out, "- `{}`", import.text)?;
                }
                if total > max_shown {
                    writeln!(out, "- *... and {} more imports*", total - max_shown)?;
                }
                writeln!(out)?;
            }

            // Declarations
            if !file.declarations.is_empty() {
                writeln!(out, "**Declarations:**")?;
                writeln!(out)?;
                for decl in &file.declarations {
                    format_declaration(&mut out, decl, 0, detail, &shown_in_api, &file_path)?;
                }
            }
        }

        Ok(out)
    }
}

/// Write metadata badges for a declaration (test, async, deprecated)
fn write_badges(out: &mut String, decl: &Declaration) -> std::fmt::Result {
    let mut badges = Vec::new();
    if decl.is_test {
        badges.push("test");
    }
    if decl.is_async {
        badges.push("async");
    }
    if decl.is_deprecated {
        badges.push("deprecated");
    }
    if !badges.is_empty() {
        write!(out, " [{}]", badges.join(", "))?;
    }
    Ok(())
}

fn format_declaration(
    out: &mut String,
    decl: &Declaration,
    depth: usize,
    detail: DetailLevel,
    shown_in_api: &HashSet<(String, usize)>,
    file_path: &str,
) -> std::fmt::Result {
    let indent = "  ".repeat(depth);

    // Skip top-level public declarations that were already shown in the API surface
    // (but always show impl blocks and declarations with children, e.g. QML root components)
    if depth == 0
        && matches!(decl.visibility, Visibility::Public)
        && !matches!(decl.kind, DeclKind::Impl)
        && decl.children.is_empty()
        && shown_in_api.contains(&(file_path.to_string(), decl.line))
    {
        return Ok(());
    }

    match decl.kind {
        DeclKind::Impl => {
            writeln!(out, "{}**`{}`**", indent, decl.signature)?;
        }
        DeclKind::Field | DeclKind::Variant => {
            writeln!(out, "{}- `{}`", indent, decl.signature)?;
            return Ok(());
        }
        _ => {
            let vis = match &decl.visibility {
                Visibility::Public => "pub ",
                Visibility::PublicCrate => "pub(crate) ",
                Visibility::Private => "",
            };

            // Avoid duplicating visibility in signature
            let sig = if decl.signature.starts_with("pub ")
                || decl.signature.starts_with("pub(")
                || decl.signature.starts_with("export ")
            {
                decl.signature.clone()
            } else {
                format!("{}{}", vis, decl.signature)
            };

            write!(out, "{}`{}`", indent, sig)?;
            if detail == DetailLevel::Full {
                write_badges(out, decl)?;
            }
            writeln!(out)?;
        }
    }

    // Full-level metadata: doc comments, line numbers, body size, relationships
    if detail == DetailLevel::Full {
        if let Some(doc) = &decl.doc_comment {
            writeln!(out, "{}> {}", indent, doc)?;
        }

        if decl.kind != DeclKind::Impl && decl.line > 0 {
            write!(out, "{}> Line {}", indent, decl.line)?;
            if let Some(body) = decl.body_lines {
                write!(out, " ({} lines)", body)?;
            }
            writeln!(out)?;
        }

        if !decl.relationships.is_empty() {
            let rels: Vec<String> = decl
                .relationships
                .iter()
                .map(|r| format!("{} `{}`", r.kind, r.target))
                .collect();
            writeln!(out, "{}> {}", indent, rels.join(", "))?;
        }
    }

    // Children
    if !decl.children.is_empty() {
        match decl.kind {
            DeclKind::Struct | DeclKind::Class => {
                let fields: Vec<String> = decl
                    .children
                    .iter()
                    .filter(|c| c.kind == DeclKind::Field)
                    .map(|f| format!("`{}`", f.signature))
                    .collect();
                if !fields.is_empty() {
                    writeln!(out, "{}> Fields: {}", indent, fields.join(", "))?;
                }
                // Methods, functions, nested classes/enums inside class/struct
                for child in &decl.children {
                    if !matches!(child.kind, DeclKind::Field | DeclKind::Variant) {
                        format_declaration(out, child, depth + 1, detail, shown_in_api, file_path)?;
                    }
                }
            }
            DeclKind::Enum => {
                let variants: Vec<String> = decl
                    .children
                    .iter()
                    .filter(|c| c.kind == DeclKind::Variant)
                    .map(|v| format!("`{}`", v.name))
                    .collect();
                if !variants.is_empty() {
                    writeln!(out, "{}> Variants: {}", indent, variants.join(", "))?;
                }
            }
            DeclKind::Impl | DeclKind::Trait | DeclKind::Module => {
                for child in &decl.children {
                    format_declaration(out, child, depth + 1, detail, shown_in_api, file_path)?;
                }
            }
            DeclKind::Message | DeclKind::Service | DeclKind::SchemaType | DeclKind::Interface => {
                for child in &decl.children {
                    format_declaration(out, child, depth + 1, detail, shown_in_api, file_path)?;
                }
            }
            DeclKind::Heading => {
                for child in &decl.children {
                    format_declaration(out, child, depth + 1, detail, shown_in_api, file_path)?;
                }
            }
            DeclKind::ConfigKey => {
                for child in &decl.children {
                    format_declaration(out, child, depth + 1, detail, shown_in_api, file_path)?;
                }
            }
            DeclKind::TableDef => {
                let cols: Vec<String> = decl
                    .children
                    .iter()
                    .filter(|c| c.kind == DeclKind::Field)
                    .map(|f| format!("`{}`", f.signature))
                    .collect();
                if !cols.is_empty() {
                    writeln!(out, "{}> Columns: {}", indent, cols.join(", "))?;
                }
            }
            _ => {}
        }
    }

    writeln!(out)?;
    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

impl std::fmt::Display for crate::model::declarations::RelKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            crate::model::declarations::RelKind::Implements => write!(f, "implements"),
            crate::model::declarations::RelKind::Extends => write!(f, "extends"),
        }
    }
}
