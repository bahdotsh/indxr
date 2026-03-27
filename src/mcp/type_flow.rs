use serde::Serialize;

use crate::languages::Language;
use crate::model::CodebaseIndex;
use crate::model::declarations::{DeclKind, Declaration};

// ---------------------------------------------------------------------------
// Type flow helpers
// ---------------------------------------------------------------------------

/// Extracted type names from a function/method signature.
pub(super) struct TypeInfo {
    pub param_types: Vec<String>,
    pub return_types: Vec<String>,
}

/// A function/field that produces or consumes a given type.
#[derive(Serialize)]
pub(super) struct TypeFlowEntry {
    pub file: String,
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub line: usize,
    pub role: String,
}

/// Primitives and builtins to skip when extracting type names.
const PRIMITIVE_TYPES: &[&str] = &[
    "str",
    "string",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "f32",
    "f64",
    "bool",
    "char",
    "int",
    "float",
    "double",
    "long",
    "short",
    "byte",
    "void",
    "undefined",
    "null",
    "none",
    "any",
    "object",
    "number",
    "boolean",
    "self",
    "error",
];

fn is_primitive(name: &str) -> bool {
    PRIMITIVE_TYPES.iter().any(|p| p.eq_ignore_ascii_case(name))
}

/// Extract all type names from a raw type string like `Result<FileIndex, Error>` or `&mut Vec<String>`.
/// Returns individual type names with primitives filtered out.
fn normalize_type_names(raw: &str) -> Vec<String> {
    let mut names = Vec::new();
    // Strip reference/pointer markers
    let cleaned = raw
        .replace("&mut ", "")
        .replace("&'_ ", "")
        .replace('&', "")
        .replace("*const ", "")
        .replace("*mut ", "")
        .replace('*', "");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return names;
    }
    // Extract identifiers: sequences of alphanumeric + underscore that start with a letter
    // This naturally handles generics like Result<FileIndex, Error> → [Result, FileIndex, Error]
    let mut current = String::new();
    for ch in cleaned.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            if !current.is_empty()
                && current.chars().next().is_some_and(|c| c.is_alphabetic())
                && !is_primitive(&current)
            {
                names.push(current.clone());
            }
            current.clear();
        }
    }
    if !current.is_empty()
        && current.chars().next().is_some_and(|c| c.is_alphabetic())
        && !is_primitive(&current)
    {
        names.push(current);
    }
    names
}

/// Find the matching close delimiter index, handling nesting.
fn find_matching_close(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Extract parameter and return types from a declaration's signature,
/// using language-aware heuristics.
pub(super) fn extract_types_from_signature(signature: &str, language: &Language) -> TypeInfo {
    match language {
        Language::Rust | Language::C | Language::Cpp => extract_types_rust_c(signature),
        Language::Go => extract_types_go(signature),
        Language::TypeScript | Language::JavaScript => extract_types_ts(signature),
        Language::Python => extract_types_python(signature),
        Language::Kotlin => extract_types_kotlin(signature),
        Language::Java | Language::CSharp => extract_types_java_like(signature),
        Language::Swift => extract_types_swift(signature),
        Language::Ruby => extract_types_ruby(signature),
        _ => TypeInfo {
            param_types: vec![],
            return_types: vec![],
        },
    }
}

/// Rust/C/C++: `fn name(param: Type) -> ReturnType`
fn extract_types_rust_c(sig: &str) -> TypeInfo {
    let mut param_types = Vec::new();
    let mut return_types = Vec::new();

    // Find parameter list between first ( and matching )
    if let Some(paren_start) = sig.find('(') {
        let rest = &sig[paren_start..];
        if let Some(paren_end) = find_matching_close(rest, '(', ')') {
            let params_str = &rest[1..paren_end];
            // Split by comma (respecting nesting)
            for param in split_respecting_nesting(params_str, ',') {
                let param = param.trim();
                // Rust: look for `: Type` pattern
                if let Some(colon_pos) = param.rfind(':') {
                    let type_part = param[colon_pos + 1..].trim();
                    param_types.extend(normalize_type_names(type_part));
                }
                // C/C++: type comes before the name — "Type name"
                else if !param.is_empty()
                    && param != "self"
                    && param != "&self"
                    && param != "&mut self"
                {
                    let tokens: Vec<&str> = param.split_whitespace().collect();
                    if tokens.len() >= 2 {
                        let type_part = tokens[..tokens.len() - 1].join(" ");
                        param_types.extend(normalize_type_names(&type_part));
                    }
                }
            }

            // Return type: look for -> after the ) (Rust style)
            let after_params = &rest[paren_end + 1..];
            if let Some(arrow_pos) = after_params.find("->") {
                let ret_str = after_params[arrow_pos + 2..].trim();
                // Strip trailing { or where clause
                let ret_str = ret_str
                    .split('{')
                    .next()
                    .unwrap_or(ret_str)
                    .split(" where ")
                    .next()
                    .unwrap_or(ret_str)
                    .trim();
                return_types.extend(normalize_type_names(ret_str));
            }
            // C/C++ fallback: return type appears before the function name
            // (no -> arrow means this is C/C++ style, e.g. "int* create_buffer(size_t len)")
            else if !sig.contains("->") {
                let before_name = &sig[..paren_start];
                let tokens: Vec<&str> = before_name.split_whitespace().collect();
                if tokens.len() >= 2 {
                    let skip = [
                        "pub",
                        "fn",
                        "async",
                        "unsafe",
                        "extern",
                        "static",
                        "inline",
                        "virtual",
                        "const",
                        "constexpr",
                        "explicit",
                        "override",
                    ];
                    let type_tokens: Vec<&&str> = tokens[..tokens.len() - 1]
                        .iter()
                        .filter(|t| !skip.contains(&t.to_lowercase().as_str()))
                        .collect();
                    if !type_tokens.is_empty() {
                        let type_str = type_tokens
                            .iter()
                            .map(|t| **t)
                            .collect::<Vec<_>>()
                            .join(" ");
                        return_types.extend(normalize_type_names(&type_str));
                    }
                }
            }
        }
    }

    TypeInfo {
        param_types,
        return_types,
    }
}

/// Go: `func (recv) Name(name Type) (RetType, error)`
fn extract_types_go(sig: &str) -> TypeInfo {
    let mut param_types = Vec::new();
    let mut return_types = Vec::new();

    let sig_trimmed = sig.trim();

    // Find all parenthesized groups
    let mut paren_groups: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = sig_trimmed.chars().collect();
    while i < chars.len() {
        if chars[i] == '(' {
            if let Some(end) = find_matching_close(&sig_trimmed[i..], '(', ')') {
                paren_groups.push((i, i + end));
                i = i + end + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    // Determine which group is params and which is return types
    let (params_group, return_group) = match paren_groups.len() {
        0 => (None, None),
        1 => (Some(0), None),
        2 => {
            // Could be (receiver) Name(params) or (params)(returns)
            // If the text between groups contains a name, first is receiver
            let between = &sig_trimmed[paren_groups[0].1 + 1..paren_groups[1].0];
            if between.trim().chars().any(|c| c.is_alphabetic()) {
                (Some(1), None)
            } else {
                (Some(0), Some(1))
            }
        }
        _ => (Some(paren_groups.len() - 2), Some(paren_groups.len() - 1)),
    };

    // Extract param types
    if let Some(pi) = params_group {
        let (start, end) = paren_groups[pi];
        let params_str = &sig_trimmed[start + 1..end];
        for param in split_respecting_nesting(params_str, ',') {
            let param = param.trim();
            let tokens: Vec<&str> = param.split_whitespace().collect();
            if let Some(last) = tokens.last() {
                param_types.extend(normalize_type_names(last));
            }
        }
    }

    // Extract return types
    if let Some(ri) = return_group {
        let (start, end) = paren_groups[ri];
        let ret_str = &sig_trimmed[start + 1..end];
        for ret in split_respecting_nesting(ret_str, ',') {
            return_types.extend(normalize_type_names(ret.trim()));
        }
    } else if let Some(pi) = params_group {
        // Single return type after the param group (no parens)
        let after = &sig_trimmed[paren_groups[pi].1 + 1..];
        let after = after.split('{').next().unwrap_or(after).trim();
        if !after.is_empty() {
            return_types.extend(normalize_type_names(after));
        }
    }

    TypeInfo {
        param_types,
        return_types,
    }
}

/// TypeScript/JavaScript: `function name(param: Type): ReturnType`
fn extract_types_ts(sig: &str) -> TypeInfo {
    let mut param_types = Vec::new();
    let mut return_types = Vec::new();

    if let Some(paren_start) = sig.find('(') {
        let rest = &sig[paren_start..];
        if let Some(paren_end) = find_matching_close(rest, '(', ')') {
            let params_str = &rest[1..paren_end];
            for param in split_respecting_nesting(params_str, ',') {
                let param = param.trim();
                if let Some(colon_pos) = param.find(':') {
                    let type_part = param[colon_pos + 1..].trim();
                    param_types.extend(normalize_type_names(type_part));
                }
            }

            // Return type after ): Type
            let after_params = &rest[paren_end + 1..];
            let after_params = after_params.trim();
            if let Some(stripped) = after_params.strip_prefix(':') {
                let ret = stripped.trim();
                let ret = ret.split('{').next().unwrap_or(ret).trim();
                return_types.extend(normalize_type_names(ret));
            }
        }
    }

    TypeInfo {
        param_types,
        return_types,
    }
}

/// Python: `def name(param: Type) -> ReturnType`
fn extract_types_python(sig: &str) -> TypeInfo {
    let mut param_types = Vec::new();
    let mut return_types = Vec::new();

    if let Some(paren_start) = sig.find('(') {
        let rest = &sig[paren_start..];
        if let Some(paren_end) = find_matching_close(rest, '(', ')') {
            let params_str = &rest[1..paren_end];
            for param in split_respecting_nesting(params_str, ',') {
                let param = param.trim();
                // Skip *args, **kwargs, bare self
                if param.starts_with('*') || param == "self" || param == "cls" {
                    continue;
                }
                if let Some(colon_pos) = param.find(':') {
                    // Strip default value: `param: Type = default`
                    let type_part = param[colon_pos + 1..].trim();
                    let type_part = type_part.split('=').next().unwrap_or(type_part).trim();
                    param_types.extend(normalize_type_names(type_part));
                }
            }

            // Return type: -> Type after )
            let after_params = &rest[paren_end + 1..];
            if let Some(arrow_pos) = after_params.find("->") {
                let ret = after_params[arrow_pos + 2..].trim();
                let ret = ret.split(':').next().unwrap_or(ret).trim();
                return_types.extend(normalize_type_names(ret));
            }
        }
    }

    TypeInfo {
        param_types,
        return_types,
    }
}

/// Java/C#: `ReturnType name(Type param, Type param)`
fn extract_types_java_like(sig: &str) -> TypeInfo {
    let mut param_types = Vec::new();
    let mut return_types = Vec::new();

    if let Some(paren_start) = sig.find('(') {
        let rest = &sig[paren_start..];
        if let Some(paren_end) = find_matching_close(rest, '(', ')') {
            let params_str = &rest[1..paren_end];
            for param in split_respecting_nesting(params_str, ',') {
                let param = param.trim();
                if param.is_empty() {
                    continue;
                }
                // Kotlin: `name: Type`
                if param.contains(':') {
                    if let Some(colon_pos) = param.find(':') {
                        let type_part = param[colon_pos + 1..].trim();
                        param_types.extend(normalize_type_names(type_part));
                    }
                } else {
                    // Java/C#: `Type name` or `final Type name`
                    let tokens: Vec<&str> = param.split_whitespace().collect();
                    let filtered: Vec<&&str> = tokens
                        .iter()
                        .filter(|t| {
                            !["final", "var", "val", "params", "out", "ref", "readonly"]
                                .contains(&t.to_lowercase().as_str())
                        })
                        .collect();
                    if filtered.len() >= 2 {
                        // Everything except the last token is the type
                        let type_str = filtered[..filtered.len() - 1]
                            .iter()
                            .map(|t| **t)
                            .collect::<Vec<_>>()
                            .join(" ");
                        param_types.extend(normalize_type_names(&type_str));
                    }
                }
            }

            // Return type: before the function name (before '(')
            let before_paren = &sig[..paren_start];
            let tokens: Vec<&str> = before_paren.split_whitespace().collect();
            let skip = [
                "public",
                "private",
                "protected",
                "internal",
                "static",
                "abstract",
                "final",
                "override",
                "virtual",
                "async",
                "suspend",
                "fun",
                "def",
                "open",
                "sealed",
                "inline",
                "synchronized",
                "native",
                "transient",
                "volatile",
            ];
            let meaningful: Vec<&&str> = tokens
                .iter()
                .filter(|t| !skip.contains(&t.to_lowercase().as_str()))
                .collect();
            // In Java-like: `ReturnType methodName` → return type is everything except last
            if meaningful.len() >= 2 {
                let type_str = meaningful[..meaningful.len() - 1]
                    .iter()
                    .map(|t| **t)
                    .collect::<Vec<_>>()
                    .join(" ");
                return_types.extend(normalize_type_names(&type_str));
            }
        }
    }

    TypeInfo {
        param_types,
        return_types,
    }
}

/// Kotlin: `fun name(name: Type, name: Type): ReturnType`
fn extract_types_kotlin(sig: &str) -> TypeInfo {
    let mut param_types = Vec::new();
    let mut return_types = Vec::new();

    if let Some(paren_start) = sig.find('(') {
        let rest = &sig[paren_start..];
        if let Some(paren_end) = find_matching_close(rest, '(', ')') {
            let params_str = &rest[1..paren_end];
            for param in split_respecting_nesting(params_str, ',') {
                let param = param.trim();
                if param.is_empty() {
                    continue;
                }
                if let Some(colon_pos) = param.find(':') {
                    let type_part = param[colon_pos + 1..].trim();
                    // Strip default value: `param: Type = default`
                    let type_part = type_part.split('=').next().unwrap_or(type_part).trim();
                    param_types.extend(normalize_type_names(type_part));
                }
            }

            // Return type: `: Type` after the closing paren
            let after_params = &rest[paren_end + 1..];
            let after_params = after_params.trim();
            if let Some(stripped) = after_params.strip_prefix(':') {
                let ret = stripped.trim();
                let ret = ret.split('{').next().unwrap_or(ret).trim();
                let ret = ret.split(" where ").next().unwrap_or(ret).trim();
                return_types.extend(normalize_type_names(ret));
            }
        }
    }

    TypeInfo {
        param_types,
        return_types,
    }
}

/// Swift: `func name(label param: Type) -> ReturnType`
fn extract_types_swift(sig: &str) -> TypeInfo {
    let mut param_types = Vec::new();
    let mut return_types = Vec::new();

    if let Some(paren_start) = sig.find('(') {
        let rest = &sig[paren_start..];
        if let Some(paren_end) = find_matching_close(rest, '(', ')') {
            let params_str = &rest[1..paren_end];
            for param in split_respecting_nesting(params_str, ',') {
                let param = param.trim();
                // Swift params: `label name: Type` or `_ name: Type` or `name: Type`
                if let Some(colon_pos) = param.rfind(':') {
                    let type_part = param[colon_pos + 1..].trim();
                    let type_part = type_part.split('=').next().unwrap_or(type_part).trim();
                    param_types.extend(normalize_type_names(type_part));
                }
            }

            let after_params = &rest[paren_end + 1..];
            if let Some(arrow_pos) = after_params.find("->") {
                let ret = after_params[arrow_pos + 2..].trim();
                let ret = ret.split('{').next().unwrap_or(ret).trim();
                return_types.extend(normalize_type_names(ret));
            }
        }
    }

    TypeInfo {
        param_types,
        return_types,
    }
}

/// Ruby: no type annotations in signatures typically, return empty.
fn extract_types_ruby(_sig: &str) -> TypeInfo {
    TypeInfo {
        param_types: vec![],
        return_types: vec![],
    }
}

/// Split a string by a delimiter, respecting nested `<>`, `()`, `[]` pairs.
fn split_respecting_nesting(s: &str, delim: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth = (depth - 1).max(0),
            c if c == delim && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Build a type flow report for a given type name across the index.
pub(super) fn build_type_flow(
    index: &CodebaseIndex,
    type_name: &str,
    path_filter: Option<&str>,
    include_fields: bool,
) -> (Vec<TypeFlowEntry>, Vec<TypeFlowEntry>) {
    let type_lower = type_name.to_lowercase();
    let mut producers = Vec::new();
    let mut consumers = Vec::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();

        // Apply path filter
        if let Some(pf) = path_filter {
            if !file_path.contains(pf) && !file_path.ends_with(pf) {
                continue;
            }
        }

        scan_decls_for_type_flow(
            &file.declarations,
            &file_path,
            &file.language,
            &type_lower,
            include_fields,
            &mut producers,
            &mut consumers,
        );
    }

    (producers, consumers)
}

fn scan_decls_for_type_flow(
    decls: &[Declaration],
    file_path: &str,
    language: &Language,
    type_lower: &str,
    include_fields: bool,
    producers: &mut Vec<TypeFlowEntry>,
    consumers: &mut Vec<TypeFlowEntry>,
) {
    for decl in decls {
        match decl.kind {
            DeclKind::Function | DeclKind::Method | DeclKind::RpcMethod => {
                let info = extract_types_from_signature(&decl.signature, language);

                if info
                    .return_types
                    .iter()
                    .any(|t| t.to_lowercase() == type_lower)
                {
                    producers.push(TypeFlowEntry {
                        file: file_path.to_string(),
                        name: decl.name.clone(),
                        kind: format!("{}", decl.kind),
                        signature: decl.signature.clone(),
                        line: decl.line,
                        role: "producer".to_string(),
                    });
                }

                if info
                    .param_types
                    .iter()
                    .any(|t| t.to_lowercase() == type_lower)
                {
                    consumers.push(TypeFlowEntry {
                        file: file_path.to_string(),
                        name: decl.name.clone(),
                        kind: format!("{}", decl.kind),
                        signature: decl.signature.clone(),
                        line: decl.line,
                        role: "consumer".to_string(),
                    });
                }
            }
            DeclKind::Field if include_fields => {
                if normalize_type_names(&decl.signature)
                    .iter()
                    .any(|t| t.to_lowercase() == type_lower)
                {
                    consumers.push(TypeFlowEntry {
                        file: file_path.to_string(),
                        name: decl.name.clone(),
                        kind: format!("{}", decl.kind),
                        signature: decl.signature.clone(),
                        line: decl.line,
                        role: "consumer".to_string(),
                    });
                }
            }
            _ => {}
        }

        // Recurse into children
        scan_decls_for_type_flow(
            &decl.children,
            file_path,
            language,
            type_lower,
            include_fields,
            producers,
            consumers,
        );
    }
}
