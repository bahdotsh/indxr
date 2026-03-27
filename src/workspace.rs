use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The kind of workspace detected at a root directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceKind {
    /// Cargo workspace (`Cargo.toml` with `[workspace]` section).
    Cargo,
    /// npm/pnpm/yarn workspace (`package.json` with `"workspaces"` field).
    Npm,
    /// Go workspace (`go.work` file).
    Go,
    /// No workspace detected; the root is treated as a single member.
    None,
}

impl WorkspaceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Go => "go",
            Self::None => "none",
        }
    }
}

/// A single member of a workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// Short name (e.g. package name from Cargo.toml or package.json).
    pub name: String,
    /// Absolute path to the member directory.
    pub path: PathBuf,
    /// Path relative to the workspace root.
    pub relative_path: PathBuf,
}

/// A detected workspace with its members.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Absolute path to the workspace root.
    pub root: PathBuf,
    /// What kind of workspace was detected.
    pub kind: WorkspaceKind,
    /// Resolved members.
    pub members: Vec<WorkspaceMember>,
}

/// Detect a workspace at the given root directory.
///
/// Checks in priority order: Cargo.toml > package.json > go.work.
/// Returns a workspace with `WorkspaceKind::None` (single synthetic member) if
/// no workspace manifest is found.
pub fn detect_workspace(root: &Path) -> Result<Workspace> {
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot resolve workspace root: {}", root.display()))?;

    // Try Cargo workspace
    let cargo_toml = root.join("Cargo.toml");
    if cargo_toml.is_file() {
        if let Some(ws) = detect_cargo_workspace(&root, &cargo_toml)? {
            return Ok(ws);
        }
    }

    // Try npm workspace
    let package_json = root.join("package.json");
    if package_json.is_file() {
        if let Some(ws) = detect_npm_workspace(&root, &package_json)? {
            return Ok(ws);
        }
    }

    // Try Go workspace
    let go_work = root.join("go.work");
    if go_work.is_file() {
        if let Some(ws) = detect_go_workspace(&root, &go_work)? {
            return Ok(ws);
        }
    }

    // No workspace found — single-member fallback
    Ok(single_root_workspace(&root))
}

/// Wrap a non-workspace root as a single-member workspace.
pub fn single_root_workspace(root: &Path) -> Workspace {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    Workspace {
        root: root.to_path_buf(),
        kind: WorkspaceKind::None,
        members: vec![WorkspaceMember {
            name,
            path: root.to_path_buf(),
            relative_path: PathBuf::from("."),
        }],
    }
}

// ---------------------------------------------------------------------------
// Cargo workspace detection
// ---------------------------------------------------------------------------

fn detect_cargo_workspace(root: &Path, cargo_toml: &Path) -> Result<Option<Workspace>> {
    let content = fs::read_to_string(cargo_toml)
        .with_context(|| format!("failed to read {}", cargo_toml.display()))?;

    let doc: toml::Value = toml::from_str(&content)
        .with_context(|| format!("invalid TOML: {}", cargo_toml.display()))?;

    let workspace_table = match doc.get("workspace") {
        Some(w) => w,
        None => return Ok(None), // Has Cargo.toml but no [workspace] section
    };

    let member_globs = match workspace_table.get("members").and_then(|m| m.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>(),
        None => return Ok(None),
    };

    let mut members = Vec::new();
    for pattern in &member_globs {
        let expanded = expand_glob(root, pattern);
        for member_path in expanded {
            if !member_path.is_dir() {
                continue;
            }
            let name = cargo_package_name(&member_path).unwrap_or_else(|| {
                member_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            });
            let relative_path = member_path
                .strip_prefix(root)
                .unwrap_or(&member_path)
                .to_path_buf();
            members.push(WorkspaceMember {
                name,
                path: member_path,
                relative_path,
            });
        }
    }

    if members.is_empty() {
        return Ok(None);
    }

    members.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(Some(Workspace {
        root: root.to_path_buf(),
        kind: WorkspaceKind::Cargo,
        members,
    }))
}

/// Read [package].name from a member's Cargo.toml, if it exists.
fn cargo_package_name(member_dir: &Path) -> Option<String> {
    let toml_path = member_dir.join("Cargo.toml");
    let content = fs::read_to_string(toml_path).ok()?;
    let doc: toml::Value = toml::from_str(&content).ok()?;
    doc.get("package")?.get("name")?.as_str().map(String::from)
}

// ---------------------------------------------------------------------------
// npm workspace detection
// ---------------------------------------------------------------------------

fn detect_npm_workspace(root: &Path, package_json: &Path) -> Result<Option<Workspace>> {
    let content = fs::read_to_string(package_json)
        .with_context(|| format!("failed to read {}", package_json.display()))?;

    let doc: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("invalid JSON: {}", package_json.display()))?;

    // "workspaces" can be an array or { "packages": [...] }
    let patterns = match doc.get("workspaces") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>(),
        Some(serde_json::Value::Object(obj)) => {
            match obj.get("packages").and_then(|p| p.as_array()) {
                Some(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                None => return Ok(None),
            }
        }
        _ => return Ok(None),
    };

    if patterns.is_empty() {
        return Ok(None);
    }

    let mut members = Vec::new();
    for pattern in &patterns {
        let expanded = expand_glob(root, pattern);
        for member_path in expanded {
            if !member_path.is_dir() {
                continue;
            }
            let name = npm_package_name(&member_path).unwrap_or_else(|| {
                member_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            });
            let relative_path = member_path
                .strip_prefix(root)
                .unwrap_or(&member_path)
                .to_path_buf();
            members.push(WorkspaceMember {
                name,
                path: member_path,
                relative_path,
            });
        }
    }

    if members.is_empty() {
        return Ok(None);
    }

    members.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(Some(Workspace {
        root: root.to_path_buf(),
        kind: WorkspaceKind::Npm,
        members,
    }))
}

/// Read "name" from a member's package.json, if it exists.
fn npm_package_name(member_dir: &Path) -> Option<String> {
    let path = member_dir.join("package.json");
    let content = fs::read_to_string(path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&content).ok()?;
    doc.get("name")?.as_str().map(String::from)
}

// ---------------------------------------------------------------------------
// Go workspace detection
// ---------------------------------------------------------------------------

fn detect_go_workspace(root: &Path, go_work: &Path) -> Result<Option<Workspace>> {
    let content = fs::read_to_string(go_work)
        .with_context(|| format!("failed to read {}", go_work.display()))?;

    let mut members = Vec::new();
    let mut in_use_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Single-line: use ./path
        if trimmed.starts_with("use ") && !trimmed.contains('(') {
            if let Some(path_str) = parse_go_work_path(trimmed.strip_prefix("use ").unwrap()) {
                if let Some(member) = resolve_go_member(root, &path_str) {
                    members.push(member);
                }
            }
            continue;
        }

        // Multi-line use block
        if trimmed == "use (" {
            in_use_block = true;
            continue;
        }
        if trimmed == ")" && in_use_block {
            in_use_block = false;
            continue;
        }
        if in_use_block {
            if let Some(path_str) = parse_go_work_path(trimmed) {
                if let Some(member) = resolve_go_member(root, &path_str) {
                    members.push(member);
                }
            }
        }
    }

    if members.is_empty() {
        return Ok(None);
    }

    members.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(Some(Workspace {
        root: root.to_path_buf(),
        kind: WorkspaceKind::Go,
        members,
    }))
}

/// Parse a path from a go.work use directive, stripping comments and quotes.
fn parse_go_work_path(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s.starts_with("//") {
        return None;
    }
    // Strip inline comments
    let s = s.split("//").next().unwrap_or(s).trim();
    // Strip optional quotes
    let s = s.trim_matches('"');
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

/// Resolve a go.work member path into a WorkspaceMember.
fn resolve_go_member(root: &Path, path_str: &str) -> Option<WorkspaceMember> {
    let member_path = root.join(path_str).canonicalize().ok()?;
    if !member_path.is_dir() {
        return None;
    }
    let relative_path = member_path
        .strip_prefix(root)
        .unwrap_or(&member_path)
        .to_path_buf();

    // Try to get module name from go.mod
    let name = go_module_name(&member_path).unwrap_or_else(|| {
        member_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    });

    Some(WorkspaceMember {
        name,
        path: member_path,
        relative_path,
    })
}

/// Read the module name from go.mod (first line: `module <name>`).
fn go_module_name(member_dir: &Path) -> Option<String> {
    let path = member_dir.join("go.mod");
    let content = fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?.trim();
    first_line
        .strip_prefix("module ")
        .map(|s| s.trim().to_string())
}

// ---------------------------------------------------------------------------
// Glob expansion helper
// ---------------------------------------------------------------------------

/// Expand a glob pattern relative to a root directory.
/// Returns sorted absolute paths of matching directories.
fn expand_glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    // If the pattern has no glob metacharacters, just return the literal path
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        let path = root.join(pattern);
        if path.is_dir() {
            return vec![path];
        }
        return Vec::new();
    }

    // Use std::fs to do a simple one-level glob expansion.
    // Workspace globs are typically like "crates/*" or "packages/*" — a literal
    // prefix with a single trailing wildcard. We handle this common case
    // efficiently without pulling in another crate.
    let parts: Vec<&str> = pattern.split('/').collect();

    let mut candidates = vec![root.to_path_buf()];

    for part in &parts {
        let mut next = Vec::new();
        for base in &candidates {
            if part.contains('*') || part.contains('?') || part.contains('[') {
                // This segment is a glob — list directory entries and filter
                if let Ok(entries) = fs::read_dir(base) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if glob_match_segment(part, &name_str) && entry.path().is_dir() {
                            next.push(entry.path());
                        }
                    }
                }
            } else {
                // Literal segment
                let child = base.join(part);
                if child.exists() {
                    next.push(child);
                }
            }
        }
        candidates = next;
    }

    candidates.sort();
    candidates
}

/// Simple glob matching for a single path segment (supports `*` and `?`).
fn glob_match_segment(pattern: &str, name: &str) -> bool {
    // Handle the common case of bare `*`
    if pattern == "*" {
        return true;
    }

    let pat: Vec<char> = pattern.chars().collect();
    let nam: Vec<char> = name.chars().collect();
    glob_match_chars(&pat, &nam)
}

fn glob_match_chars(pat: &[char], nam: &[char]) -> bool {
    let (mut pi, mut ni) = (0, 0);
    let (mut star_pi, mut star_ni) = (usize::MAX, 0);

    while ni < nam.len() {
        if pi < pat.len() && (pat[pi] == '?' || pat[pi] == nam[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star_pi = pi;
            star_ni = ni;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ni += 1;
            ni = star_ni;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }

    pi == pat.len()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_glob_match_segment() {
        assert!(glob_match_segment("*", "anything"));
        assert!(glob_match_segment("foo*", "foobar"));
        assert!(glob_match_segment("*bar", "foobar"));
        assert!(glob_match_segment("f?o", "foo"));
        assert!(!glob_match_segment("f?o", "fooo"));
        assert!(glob_match_segment("*-*", "my-crate"));
        assert!(!glob_match_segment("abc", "def"));
    }

    #[test]
    fn test_single_root_workspace() {
        let dir = TempDir::new().unwrap();
        let ws = single_root_workspace(dir.path());
        assert_eq!(ws.kind, WorkspaceKind::None);
        assert_eq!(ws.members.len(), 1);
        assert_eq!(ws.members[0].relative_path, PathBuf::from("."));
    }

    #[test]
    fn test_detect_cargo_workspace() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create workspace Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"
[workspace]
members = ["crates/*"]
"#,
        )
        .unwrap();

        // Create two member crates
        let crate_a = root.join("crates/alpha");
        let crate_b = root.join("crates/beta");
        fs::create_dir_all(&crate_a).unwrap();
        fs::create_dir_all(&crate_b).unwrap();

        fs::write(
            crate_a.join("Cargo.toml"),
            r#"
[package]
name = "alpha"
version = "0.1.0"
"#,
        )
        .unwrap();

        fs::write(
            crate_b.join("Cargo.toml"),
            r#"
[package]
name = "beta"
version = "0.1.0"
"#,
        )
        .unwrap();

        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::Cargo);
        assert_eq!(ws.members.len(), 2);

        let names: Vec<&str> = ws.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn test_detect_cargo_workspace_literal_members() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("Cargo.toml"),
            r#"
[workspace]
members = ["core", "cli"]
"#,
        )
        .unwrap();

        let core = root.join("core");
        let cli = root.join("cli");
        fs::create_dir_all(&core).unwrap();
        fs::create_dir_all(&cli).unwrap();

        fs::write(
            core.join("Cargo.toml"),
            r#"[package]
name = "my-core"
"#,
        )
        .unwrap();
        // cli has no Cargo.toml — should fall back to dir name
        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::Cargo);
        assert_eq!(ws.members.len(), 2);

        let names: Vec<&str> = ws.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"my-core"));
        assert!(names.contains(&"cli"));
    }

    #[test]
    fn test_detect_npm_workspace() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("package.json"),
            r#"{
  "name": "monorepo",
  "workspaces": ["packages/*"]
}"#,
        )
        .unwrap();

        let pkg_a = root.join("packages/web");
        let pkg_b = root.join("packages/api");
        fs::create_dir_all(&pkg_a).unwrap();
        fs::create_dir_all(&pkg_b).unwrap();

        fs::write(pkg_a.join("package.json"), r#"{ "name": "@mono/web" }"#).unwrap();

        fs::write(pkg_b.join("package.json"), r#"{ "name": "@mono/api" }"#).unwrap();

        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::Npm);
        assert_eq!(ws.members.len(), 2);

        let names: Vec<&str> = ws.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"@mono/web"));
        assert!(names.contains(&"@mono/api"));
    }

    #[test]
    fn test_detect_npm_workspace_packages_field() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // pnpm/yarn style: { "workspaces": { "packages": [...] } }
        fs::write(
            root.join("package.json"),
            r#"{
  "name": "monorepo",
  "workspaces": { "packages": ["libs/core"] }
}"#,
        )
        .unwrap();

        let member = root.join("libs/core");
        fs::create_dir_all(&member).unwrap();
        fs::write(member.join("package.json"), r#"{ "name": "core-lib" }"#).unwrap();

        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::Npm);
        assert_eq!(ws.members.len(), 1);
        assert_eq!(ws.members[0].name, "core-lib");
    }

    #[test]
    fn test_detect_go_workspace() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let svc = root.join("services/auth");
        let lib = root.join("libs/common");
        fs::create_dir_all(&svc).unwrap();
        fs::create_dir_all(&lib).unwrap();

        fs::write(
            root.join("go.work"),
            r#"go 1.21

use (
    ./services/auth
    ./libs/common
)
"#,
        )
        .unwrap();

        fs::write(
            svc.join("go.mod"),
            "module github.com/org/auth\n\ngo 1.21\n",
        )
        .unwrap();

        fs::write(
            lib.join("go.mod"),
            "module github.com/org/common\n\ngo 1.21\n",
        )
        .unwrap();

        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::Go);
        assert_eq!(ws.members.len(), 2);

        let names: Vec<&str> = ws.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"github.com/org/auth"));
        assert!(names.contains(&"github.com/org/common"));
    }

    #[test]
    fn test_detect_go_workspace_single_use() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let mod_dir = root.join("mymod");
        fs::create_dir_all(&mod_dir).unwrap();

        fs::write(root.join("go.work"), "go 1.21\n\nuse ./mymod\n").unwrap();
        // No go.mod — falls back to dir name
        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::Go);
        assert_eq!(ws.members.len(), 1);
        assert_eq!(ws.members[0].name, "mymod");
    }

    #[test]
    fn test_no_workspace_detected() {
        let dir = TempDir::new().unwrap();
        let ws = detect_workspace(dir.path()).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::None);
        assert_eq!(ws.members.len(), 1);
    }

    #[test]
    fn test_cargo_toml_without_workspace_section() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Regular Cargo.toml (no [workspace])
        fs::write(
            root.join("Cargo.toml"),
            r#"
[package]
name = "standalone"
version = "0.1.0"
"#,
        )
        .unwrap();

        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::None);
    }

    #[test]
    fn test_package_json_without_workspaces() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("package.json"),
            r#"{ "name": "standalone", "version": "1.0.0" }"#,
        )
        .unwrap();

        let ws = detect_workspace(root).unwrap();
        assert_eq!(ws.kind, WorkspaceKind::None);
    }

    #[test]
    fn test_workspace_kind_as_str() {
        assert_eq!(WorkspaceKind::Cargo.as_str(), "cargo");
        assert_eq!(WorkspaceKind::Npm.as_str(), "npm");
        assert_eq!(WorkspaceKind::Go.as_str(), "go");
        assert_eq!(WorkspaceKind::None.as_str(), "none");
    }

    #[test]
    fn test_expand_glob_literal() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let sub = root.join("mydir");
        fs::create_dir_all(&sub).unwrap();

        let result = expand_glob(root, "mydir");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], sub);
    }

    #[test]
    fn test_expand_glob_wildcard() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("pkgs/aaa")).unwrap();
        fs::create_dir_all(root.join("pkgs/bbb")).unwrap();
        // A file, not a dir — should be excluded
        fs::write(root.join("pkgs/file.txt"), "hello").unwrap();

        let mut result = expand_glob(root, "pkgs/*");
        result.sort();
        assert_eq!(result.len(), 2);
    }
}
