use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Declaration {
    pub kind: DeclKind,
    pub name: String,
    pub signature: String,
    pub visibility: Visibility,
    pub line: usize,
    pub doc_comment: Option<String>,
    pub children: Vec<Declaration>,

    // Contextual metadata
    #[serde(default)]
    pub is_test: bool,
    #[serde(default)]
    pub is_async: bool,
    #[serde(default)]
    pub is_deprecated: bool,
    #[serde(default)]
    pub body_lines: Option<usize>,

    // Complexity metrics (tree-sitter languages only)
    #[serde(default)]
    pub complexity: Option<ComplexityMetrics>,

    // Cross-references
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComplexityMetrics {
    pub cyclomatic: u16,
    pub max_nesting: u16,
    pub param_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub kind: RelKind,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RelKind {
    Implements,
    Extends,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DeclKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Constant,
    Static,
    TypeAlias,
    Module,
    Class,
    Field,
    Variant,
    Method,
    // New kinds for expanded language support
    Interface,
    Namespace,
    Macro,
    ConfigKey,
    Heading,
    TableDef,
    Service,
    Message,
    RpcMethod,
    ShellFunction,
    SchemaType,
    Route,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    PublicCrate,
    Private,
}

impl Declaration {
    /// Create a new Declaration with default metadata fields.
    pub fn new(
        kind: DeclKind,
        name: String,
        signature: String,
        visibility: Visibility,
        line: usize,
    ) -> Self {
        Self {
            kind,
            name,
            signature,
            visibility,
            line,
            doc_comment: None,
            children: Vec::new(),
            is_test: false,
            is_async: false,
            is_deprecated: false,
            body_lines: None,
            complexity: None,
            relationships: Vec::new(),
        }
    }
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Visibility::Public => write!(f, "pub"),
            Visibility::PublicCrate => write!(f, "pub(crate)"),
            Visibility::Private => Ok(()),
        }
    }
}

impl fmt::Display for DeclKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeclKind::Function | DeclKind::Method => write!(f, "fn"),
            DeclKind::Struct => write!(f, "struct"),
            DeclKind::Enum => write!(f, "enum"),
            DeclKind::Trait => write!(f, "trait"),
            DeclKind::Impl => write!(f, "impl"),
            DeclKind::Constant => write!(f, "const"),
            DeclKind::Static => write!(f, "static"),
            DeclKind::TypeAlias => write!(f, "type"),
            DeclKind::Module => write!(f, "mod"),
            DeclKind::Class => write!(f, "class"),
            DeclKind::Field => write!(f, "field"),
            DeclKind::Variant => write!(f, "variant"),
            DeclKind::Interface => write!(f, "interface"),
            DeclKind::Namespace => write!(f, "namespace"),
            DeclKind::Macro => write!(f, "macro"),
            DeclKind::ConfigKey => write!(f, "key"),
            DeclKind::Heading => write!(f, "heading"),
            DeclKind::TableDef => write!(f, "table"),
            DeclKind::Service => write!(f, "service"),
            DeclKind::Message => write!(f, "message"),
            DeclKind::RpcMethod => write!(f, "rpc"),
            DeclKind::ShellFunction => write!(f, "function"),
            DeclKind::SchemaType => write!(f, "type"),
            DeclKind::Route => write!(f, "route"),
        }
    }
}

impl DeclKind {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "fn" | "function" => Some(DeclKind::Function),
            "struct" => Some(DeclKind::Struct),
            "enum" => Some(DeclKind::Enum),
            "trait" => Some(DeclKind::Trait),
            "impl" => Some(DeclKind::Impl),
            "const" | "constant" => Some(DeclKind::Constant),
            "static" => Some(DeclKind::Static),
            "type" | "type_alias" => Some(DeclKind::TypeAlias),
            "mod" | "module" => Some(DeclKind::Module),
            "class" => Some(DeclKind::Class),
            "field" => Some(DeclKind::Field),
            "variant" => Some(DeclKind::Variant),
            "method" => Some(DeclKind::Method),
            "interface" => Some(DeclKind::Interface),
            "namespace" => Some(DeclKind::Namespace),
            "macro" => Some(DeclKind::Macro),
            "key" | "config_key" => Some(DeclKind::ConfigKey),
            "heading" => Some(DeclKind::Heading),
            "table" | "table_def" => Some(DeclKind::TableDef),
            "service" => Some(DeclKind::Service),
            "message" => Some(DeclKind::Message),
            "rpc" | "rpc_method" => Some(DeclKind::RpcMethod),
            "shell_function" => Some(DeclKind::ShellFunction),
            "schema_type" => Some(DeclKind::SchemaType),
            "route" => Some(DeclKind::Route),
            _ => None,
        }
    }
}
