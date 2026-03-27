use std::path::Path;

use anyhow::Result;

use crate::languages::Language;
use crate::model::FileIndex;

use super::LanguageParser;
use super::complexity;
use super::queries;

pub struct TreeSitterParser {
    language: Language,
}

impl TreeSitterParser {
    pub fn new(language: Language) -> Self {
        Self { language }
    }

    fn get_ts_language(&self, path: &Path) -> tree_sitter::Language {
        match self.language {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::TypeScript => {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "tsx" {
                    tree_sitter_typescript::LANGUAGE_TSX.into()
                } else {
                    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
                }
            }
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            _ => unreachable!("TreeSitterParser should only be used with tree-sitter languages"),
        }
    }
}

impl LanguageParser for TreeSitterParser {
    fn language(&self) -> Language {
        self.language.clone()
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<FileIndex> {
        let mut parser = tree_sitter::Parser::new();
        let ts_lang = self.get_ts_language(path);
        parser
            .set_language(&ts_lang)
            .map_err(|e| anyhow::anyhow!("Failed to set tree-sitter language: {:?}", e))?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse file"))?;

        let root = tree.root_node();
        let extractor = queries::get_extractor(&self.language);
        let (imports, mut declarations) = extractor.extract(root, content);

        complexity::annotate_complexity(&mut declarations, root, content, &self.language);

        let lines = content.lines().count();

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
