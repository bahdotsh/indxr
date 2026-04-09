pub mod c;
pub mod cpp;
pub mod go;
pub mod java;
pub mod javascript;
pub mod python;
pub mod qml;
pub mod rust;
pub mod typescript;

use crate::languages::Language;
use crate::model::Import;
use crate::model::declarations::Declaration;

pub trait DeclExtractor: Send + Sync {
    fn extract(&self, root: tree_sitter::Node<'_>, source: &str)
    -> (Vec<Import>, Vec<Declaration>);
}

pub fn get_extractor(language: &Language) -> Box<dyn DeclExtractor> {
    match language {
        Language::Rust => Box::new(rust::RustExtractor),
        Language::Python => Box::new(python::PythonExtractor),
        Language::Go => Box::new(go::GoExtractor),
        Language::TypeScript => Box::new(typescript::TypeScriptExtractor),
        Language::JavaScript => Box::new(javascript::JavaScriptExtractor),
        Language::Java => Box::new(java::JavaExtractor),
        Language::C => Box::new(c::CExtractor),
        Language::Cpp => Box::new(cpp::CppExtractor),
        Language::Qml => Box::new(qml::QmlExtractor),
        _ => unreachable!("get_extractor should only be called for tree-sitter languages"),
    }
}
