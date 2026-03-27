pub mod complexity;
pub mod queries;
pub mod regex_parser;
pub mod tree_sitter_parser;

use std::path::Path;

use anyhow::Result;

use crate::languages::Language;
use crate::model::FileIndex;

pub trait LanguageParser: Send + Sync {
    fn language(&self) -> Language;
    fn parse_file(&self, path: &Path, content: &str) -> Result<FileIndex>;
}

pub struct ParserRegistry {
    parsers: Vec<Box<dyn LanguageParser>>,
}

impl ParserRegistry {
    pub fn new() -> Self {
        use regex_parser::RegexParser;
        use tree_sitter_parser::TreeSitterParser;

        let mut parsers: Vec<Box<dyn LanguageParser>> = Vec::new();

        // Tree-sitter based parsers
        let ts_languages = [
            Language::Rust,
            Language::Python,
            Language::TypeScript,
            Language::JavaScript,
            Language::Go,
            Language::Java,
            Language::C,
            Language::Cpp,
        ];

        for lang in ts_languages {
            parsers.push(Box::new(TreeSitterParser::new(lang)));
        }

        // Regex-based parsers
        let regex_languages = [
            Language::Shell,
            Language::Toml,
            Language::Yaml,
            Language::Json,
            Language::Sql,
            Language::Markdown,
            Language::Protobuf,
            Language::GraphQL,
            Language::Ruby,
            Language::Kotlin,
            Language::Swift,
            Language::CSharp,
            Language::ObjectiveC,
            Language::Xml,
            Language::Html,
            Language::Css,
            Language::Gradle,
            Language::Cmake,
            Language::Properties,
        ];

        for lang in regex_languages {
            parsers.push(Box::new(RegexParser::new(lang)));
        }

        ParserRegistry { parsers }
    }

    pub fn get_parser(&self, language: &Language) -> Option<&dyn LanguageParser> {
        self.parsers
            .iter()
            .find(|p| &p.language() == language)
            .map(|p| p.as_ref())
    }
}
