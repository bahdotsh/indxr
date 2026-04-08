use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
    Java,
    C,
    Cpp,
    Shell,
    Toml,
    Yaml,
    Json,
    Sql,
    Markdown,
    Protobuf,
    GraphQL,
    // Expanded language support
    Ruby,
    Kotlin,
    Swift,
    CSharp,
    ObjectiveC,
    Xml,
    Html,
    Css,
    Gradle,
    Cmake,
    Properties,
    Qml,
}

impl Language {
    pub fn detect(path: &Path) -> Option<Self> {
        // Check for known filenames first
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            match filename {
                "Dockerfile" | "Makefile" | "Justfile" => return None, // skip these
                "Cargo.toml" | "pyproject.toml" | "Pipfile" => return Some(Language::Toml),
                "package.json" | "tsconfig.json" | "composer.json" => return Some(Language::Json),
                "docker-compose.yml" | "docker-compose.yaml" => return Some(Language::Yaml),
                ".bashrc" | ".zshrc" | ".bash_profile" | ".profile" => {
                    return Some(Language::Shell);
                }
                "Gemfile" | "Rakefile" => return Some(Language::Ruby),
                "CMakeLists.txt" => return Some(Language::Cmake),
                "build.gradle" | "settings.gradle" | "build.gradle.kts" | "settings.gradle.kts" => {
                    return Some(Language::Gradle);
                }
                _ => {}
            }
        }

        let ext = path.extension()?.to_str()?;
        match ext {
            "rs" => Some(Language::Rust),
            "py" | "pyi" => Some(Language::Python),
            "ts" | "tsx" | "mts" | "cts" => Some(Language::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" | "flow" => Some(Language::JavaScript),
            "go" => Some(Language::Go),
            "java" => Some(Language::Java),
            "c" | "h" => Some(Language::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Language::Cpp),
            "sh" | "bash" | "zsh" => Some(Language::Shell),
            "toml" => Some(Language::Toml),
            "yml" | "yaml" => Some(Language::Yaml),
            "json" | "jsonc" => Some(Language::Json),
            "sql" => Some(Language::Sql),
            "md" | "markdown" => Some(Language::Markdown),
            "proto" => Some(Language::Protobuf),
            "graphql" | "gql" => Some(Language::GraphQL),
            "rb" | "rake" | "gemspec" | "podspec" => Some(Language::Ruby),
            "kt" | "kts" => Some(Language::Kotlin),
            "swift" => Some(Language::Swift),
            "cs" => Some(Language::CSharp),
            "m" | "mm" => Some(Language::ObjectiveC),
            "xml" | "plist" | "svg" | "xib" | "storyboard" => Some(Language::Xml),
            "html" | "htm" => Some(Language::Html),
            "css" => Some(Language::Css),
            "gradle" => Some(Language::Gradle),
            "cmake" => Some(Language::Cmake),
            "properties" => Some(Language::Properties),
            "qml" => Some(Language::Qml),
            _ => None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::TypeScript => "TypeScript",
            Language::JavaScript => "JavaScript",
            Language::Go => "Go",
            Language::Java => "Java",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::Shell => "Shell",
            Language::Toml => "TOML",
            Language::Yaml => "YAML",
            Language::Json => "JSON",
            Language::Sql => "SQL",
            Language::Markdown => "Markdown",
            Language::Protobuf => "Protobuf",
            Language::GraphQL => "GraphQL",
            Language::Ruby => "Ruby",
            Language::Kotlin => "Kotlin",
            Language::Swift => "Swift",
            Language::CSharp => "C#",
            Language::ObjectiveC => "Objective-C",
            Language::Xml => "XML",
            Language::Html => "HTML",
            Language::Css => "CSS",
            Language::Gradle => "Gradle",
            Language::Cmake => "CMake",
            Language::Properties => "Properties",
            Language::Qml => "QML",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "rust" | "rs" => Some(Language::Rust),
            "python" | "py" => Some(Language::Python),
            "typescript" | "ts" => Some(Language::TypeScript),
            "javascript" | "js" => Some(Language::JavaScript),
            "go" => Some(Language::Go),
            "java" => Some(Language::Java),
            "c" => Some(Language::C),
            "cpp" | "c++" | "cxx" => Some(Language::Cpp),
            "shell" | "sh" | "bash" | "zsh" => Some(Language::Shell),
            "toml" => Some(Language::Toml),
            "yaml" | "yml" => Some(Language::Yaml),
            "json" => Some(Language::Json),
            "sql" => Some(Language::Sql),
            "markdown" | "md" => Some(Language::Markdown),
            "protobuf" | "proto" => Some(Language::Protobuf),
            "graphql" | "gql" => Some(Language::GraphQL),
            "ruby" | "rb" => Some(Language::Ruby),
            "kotlin" | "kt" => Some(Language::Kotlin),
            "swift" => Some(Language::Swift),
            "csharp" | "c#" | "cs" => Some(Language::CSharp),
            "objectivec" | "objective-c" | "objc" => Some(Language::ObjectiveC),
            "xml" => Some(Language::Xml),
            "html" => Some(Language::Html),
            "css" => Some(Language::Css),
            "gradle" => Some(Language::Gradle),
            "cmake" => Some(Language::Cmake),
            "properties" => Some(Language::Properties),
            "qml" => Some(Language::Qml),
            _ => None,
        }
    }

    /// Whether this language uses tree-sitter for parsing.
    #[allow(dead_code)]
    pub fn uses_tree_sitter(&self) -> bool {
        matches!(
            self,
            Language::Rust
                | Language::Python
                | Language::TypeScript
                | Language::JavaScript
                | Language::Go
                | Language::Java
                | Language::C
                | Language::Cpp
                | Language::Qml
        )
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
