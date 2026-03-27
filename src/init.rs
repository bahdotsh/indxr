use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::Result;

use crate::indexer::{self, IndexConfig};
use crate::model::DetailLevel;
use crate::output::OutputFormatter;
use crate::output::markdown::{MarkdownFormatter, MarkdownOptions};

pub struct InitOptions {
    pub path: PathBuf,
    pub claude: bool,
    pub cursor: bool,
    pub windsurf: bool,
    pub generate_index: bool,
    pub force: bool,
    pub include_hooks: bool,
    pub include_rtk: bool,
    pub max_file_size: u64,
}

enum WriteResult {
    Created(PathBuf),
    Skipped(PathBuf, &'static str),
    Appended(PathBuf),
}

pub fn run_init(opts: InitOptions) -> Result<()> {
    let root = fs::canonicalize(&opts.path)
        .map_err(|e| anyhow::anyhow!("cannot resolve path '{}': {}", opts.path.display(), e))?;

    // Determine which agents to set up
    let agents: Vec<&str> = [
        opts.claude.then_some("Claude Code"),
        opts.cursor.then_some("Cursor"),
        opts.windsurf.then_some("Windsurf"),
    ]
    .into_iter()
    .flatten()
    .collect();

    eprintln!("indxr init: setting up for {}", agents.join(", "));

    // Detect RTK if not disabled
    let rtk_detected = opts.include_rtk && detect_rtk();
    if rtk_detected {
        eprintln!("  RTK detected — will configure command compression hooks");
    }
    eprintln!();

    let mut results = Vec::new();

    if opts.claude {
        results.extend(setup_claude(
            &root,
            opts.force,
            opts.include_hooks,
            rtk_detected,
        )?);
    }
    if opts.cursor {
        results.extend(setup_cursor(&root, opts.force, rtk_detected)?);
    }
    if opts.windsurf {
        results.extend(setup_windsurf(&root, opts.force, rtk_detected)?);
    }

    results.push(setup_gitignore(&root)?);

    if opts.generate_index {
        results.push(generate_index(&root, opts.max_file_size)?);
    }

    // Print summary
    for result in &results {
        match result {
            WriteResult::Created(path) => {
                eprintln!("  Created  {}", display_relative(path, &root));
            }
            WriteResult::Skipped(path, reason) => {
                eprintln!("  Skipped  {} ({})", display_relative(path, &root), reason);
            }
            WriteResult::Appended(path) => {
                eprintln!("  Appended {}", display_relative(path, &root));
            }
        }
    }

    eprintln!();
    eprintln!("Done! indxr is ready.");

    Ok(())
}

fn display_relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn write_file_safe(path: &Path, content: &str, force: bool) -> Result<WriteResult> {
    if path.exists() && !force {
        return Ok(WriteResult::Skipped(
            path.to_path_buf(),
            "already exists, use --force to overwrite",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(WriteResult::Created(path.to_path_buf()))
}

fn setup_claude(
    root: &Path,
    force: bool,
    include_hooks: bool,
    include_rtk: bool,
) -> Result<Vec<WriteResult>> {
    let mut results = Vec::new();
    results.push(write_file_safe(
        &root.join(".mcp.json"),
        &mcp_json_content(),
        force,
    )?);
    results.push(write_file_safe(
        &root.join("CLAUDE.md"),
        &claude_md_content(root, include_rtk),
        force,
    )?);
    if include_hooks {
        results.push(write_file_safe(
            &root.join(".claude/settings.json"),
            &claude_settings_content(include_rtk),
            force,
        )?);
    }
    if include_rtk && include_hooks {
        results.extend(setup_rtk_claude(root, force)?);
    }
    Ok(results)
}

fn setup_cursor(root: &Path, force: bool, include_rtk: bool) -> Result<Vec<WriteResult>> {
    let results = vec![
        write_file_safe(&root.join(".cursor/mcp.json"), &mcp_json_content(), force)?,
        write_file_safe(
            &root.join(".cursorrules"),
            &cursorrules_content(include_rtk),
            force,
        )?,
    ];
    Ok(results)
}

fn setup_windsurf(root: &Path, force: bool, include_rtk: bool) -> Result<Vec<WriteResult>> {
    let results = vec![
        write_file_safe(&root.join(".windsurf/mcp.json"), &mcp_json_content(), force)?,
        write_file_safe(
            &root.join(".windsurfrules"),
            &windsurfrules_content(include_rtk),
            force,
        )?,
    ];
    Ok(results)
}

fn detect_rtk() -> bool {
    ProcessCommand::new("rtk")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn setup_rtk_claude(root: &Path, force: bool) -> Result<Vec<WriteResult>> {
    let hook_path = root.join(".claude/hooks/rtk-rewrite.sh");
    let result = write_file_safe(&hook_path, RTK_HOOK_SCRIPT, force)?;

    // Make executable on Unix
    #[cfg(unix)]
    if matches!(result, WriteResult::Created(_)) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms)?;
    }

    Ok(vec![result])
}

const RTK_HOOK_SCRIPT: &str = r#"#!/bin/bash
# RTK rewrite hook for Claude Code — installed by indxr init
# Intercepts Bash commands and rewrites them through rtk for token compression

# Skip silently if rtk or jq is not installed
command -v rtk >/dev/null 2>&1 || exit 0
command -v jq >/dev/null 2>&1 || exit 0

# Extract the command from tool input
COMMAND=$(printf '%s' "$TOOL_INPUT" | jq -r '.command // empty')
[ -z "$COMMAND" ] && exit 0

# Ask rtk to rewrite the command
REWRITTEN=$(rtk rewrite "$COMMAND" 2>/dev/null)
EXIT_CODE=$?

case $EXIT_CODE in
  0)
    # Rewrite successful — auto-allow with rewritten command
    ESCAPED=$(printf '%s' "$REWRITTEN" | jq -Rs .)
    echo "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\",\"updatedInput\":{\"command\":$ESCAPED}}}"
    ;;
  2)
    # Deny rule matched
    echo "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"deny\"}}"
    ;;
  3)
    # Ask rule matched
    echo "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\"}}"
    ;;
  *)
    # No rewrite available or error — pass through unchanged
    exit 0
    ;;
esac
"#;

fn setup_gitignore(root: &Path) -> Result<WriteResult> {
    let gitignore_path = root.join(".gitignore");
    let entry = ".indxr-cache/";

    if gitignore_path.exists() {
        let content = fs::read_to_string(&gitignore_path)?;
        if content.lines().any(|line| line.trim() == entry) {
            return Ok(WriteResult::Skipped(
                gitignore_path,
                "already contains .indxr-cache/",
            ));
        }
        let separator = if content.ends_with('\n') { "" } else { "\n" };
        fs::write(&gitignore_path, format!("{content}{separator}{entry}\n"))?;
        Ok(WriteResult::Appended(gitignore_path))
    } else {
        fs::write(&gitignore_path, format!("{entry}\n"))?;
        Ok(WriteResult::Created(gitignore_path))
    }
}

fn generate_index(root: &Path, max_file_size: u64) -> Result<WriteResult> {
    let config = IndexConfig {
        root: root.to_path_buf(),
        cache_dir: root.join(".indxr-cache"),
        max_file_size,
        max_depth: None,
        exclude: Vec::new(),
        no_gitignore: false,
    };

    let index = indexer::build_index(&config)?;

    let formatter = MarkdownFormatter::with_options(MarkdownOptions {
        omit_imports: false,
        omit_tree: false,
    });
    let output = formatter.format(&index, DetailLevel::Signatures)?;

    let index_path = root.join("INDEX.md");
    fs::write(&index_path, output)?;

    Ok(WriteResult::Created(index_path))
}

// --- Template content ---

fn mcp_json_content() -> String {
    r#"{
  "mcpServers": {
    "indxr": {
      "command": "indxr",
      "args": ["serve", "."]
    }
  }
}
"#
    .to_string()
}

fn claude_md_content(root: &Path, include_rtk: bool) -> String {
    let project_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Project".to_string());

    let mut content = format!(
        r#"# {project_name}

## Codebase Navigation — MUST USE indxr MCP tools

An MCP server called `indxr` is available. **Always use indxr tools before the Read tool.** Do NOT read full source files as a first step — use the MCP tools to explore, then read only what you need.

### Token savings reference

| Action | Approx tokens | When to use |
|--------|--------------|-------------|
| `get_tree` | ~200-400 | First: understand directory layout |
| `get_file_summary` | ~200-400 | Understand a file without reading it |
| `batch_file_summaries` | ~400-1200 | Summarize multiple files in one call |
| `get_file_context` | ~400-600 | Understand dependencies and reverse deps |
| `lookup_symbol` | ~100-200 | Find a specific function/type across codebase |
| `search_signatures` | ~100-300 | Find functions by signature pattern |
| `search_relevant` | ~200-400 | Find files/symbols by concept or partial name (supports `kind` filter) |
| `explain_symbol` | ~100-300 | Everything to USE a symbol without reading its body |
| `get_public_api` | ~200-500 | Public API surface of a file or module |
| `get_callers` | ~100-300 | Who references this symbol (imports + signatures) |
| `get_related_tests` | ~100-200 | Find tests for a symbol by naming convention |
| `get_diff_summary` | ~200-500 | Structural changes since a git ref (vs reading raw diffs) |
| `get_hotspots` | ~200-500 | Most complex functions ranked by composite score |
| `get_health` | ~200-400 | Codebase health summary with aggregate complexity metrics |
| `get_type_flow` | ~200-500 | Track which functions produce/consume a type across the codebase |
| `read_source` (symbol) | ~50-300 | Read one function/struct. Supports `symbols` array and `collapse`. |
| `get_token_estimate` | ~100 | Check cost before reading. Supports `directory`/`glob`. |
| `Read` (full file) | **500-10000+** | ONLY when editing or need exact formatting |

### Exploration workflow (follow this order)

1. `search_relevant` — find files/symbols related to your task by concept, partial name, or type pattern. **Start here when you know what you're looking for but not where it is.**
2. `get_tree` — see directory/file layout. Use `path` param to scope to a subtree.
3. `get_file_summary` — get a complete overview of any file without reading it. Use `batch_file_summaries` for multiple files.
4. `get_file_context` — understand a file's reverse dependencies and related files.
5. `lookup_symbol` — find declarations by name across all indexed files.
6. `explain_symbol` — get full interface details for a symbol without reading its body.
7. `search_signatures` — find functions/methods by signature substring.
8. `get_callers` — find who references a symbol.
9. `get_token_estimate` — before deciding to `Read` a file, check how many tokens it costs.
10. `read_source` — read source code by symbol name or line range. Use `symbols` array to read multiple in one call.
11. `get_public_api` — get only public declarations with signatures for a file or directory.
12. `get_related_tests` — find test functions for a symbol.
13. `list_declarations` — list all declarations in a file.
14. `get_imports` — get import statements for a file.
15. `get_stats` — codebase stats: file count, line count, language breakdown.
16. `get_diff_summary` — get structural changes since a git ref.
17. `get_hotspots` — get the most complex functions ranked by composite score.
18. `get_health` — get codebase health summary: aggregate complexity, documentation coverage, test ratio.
19. `get_type_flow` — track where a type flows across function boundaries. Shows producers and consumers.
20. `regenerate_index` — re-index after code changes.

### When to use the Read tool instead
- You need to **edit** a file (Read is required before Edit)
- You need exact formatting/whitespace that `read_source` doesn't preserve
- The file is not a source file (e.g., config files, documentation)

### DO NOT
- Read full source files just to understand what's in them — use `get_file_summary`
- Read full source files to review code — use `get_file_summary` to triage, then `read_source` on specific symbols
- Dump all files into context — use MCP tools to be surgical
- Read a file without first checking `get_token_estimate` if you're unsure about its size
- Use `git diff` to understand changes — use `get_diff_summary` instead

### After making code changes
Run `regenerate_index` to keep INDEX.md current.
"#
    );

    if include_rtk {
        content.push_str(
            r#"
## Command output compression — RTK

RTK is configured to automatically compress shell command outputs (git, cargo, npm, etc.) before they reach your context window. This happens transparently via a PreToolUse hook — no manual prefixing needed. Commands like `git status`, `cargo test`, and `npm test` are rewritten through `rtk` for 60-90% token savings on terminal output.
"#,
        );
    }

    content
}

fn claude_settings_content(include_rtk: bool) -> String {
    use serde_json::json;

    let read_hook = json!({
        "matcher": "Read",
        "hooks": [{
            "type": "command",
            "command": "echo 'IMPORTANT: Before reading full source files, use indxr MCP tools to minimize token usage:\n- get_file_summary: understand a file without reading it (~300 tokens vs ~3000+)\n- lookup_symbol / search_signatures: find specific functions/types\n- read_source: read only the exact function/symbol you need (~100 tokens vs full file)\nOnly use Read when you need to EDIT a file, need exact formatting, or the file is not source code (e.g., CLAUDE.md, Cargo.toml).'"
        }]
    });

    let rtk_hook = json!({
        "matcher": "Bash",
        "hooks": [{
            "type": "command",
            "command": ".claude/hooks/rtk-rewrite.sh"
        }]
    });

    let git_diff_hook = json!({
        "matcher": "Bash",
        "hooks": [{
            "type": "command",
            "command": "if printf '%s' \"$TOOL_INPUT\" | grep -qE 'git\\s+diff'; then echo 'IMPORTANT: Use indxr get_diff_summary MCP tool instead of git diff. It shows structural changes (added/removed/modified declarations) at ~200-500 tokens vs thousands for raw diffs. Example: get_diff_summary(since_ref: \"main\")'; fi"
        }]
    });

    let mut hooks = vec![read_hook];
    if include_rtk {
        hooks.push(rtk_hook);
    }
    hooks.push(git_diff_hook);

    let settings = json!({
        "hooks": {
            "PreToolUse": hooks
        }
    });

    serde_json::to_string_pretty(&settings).unwrap() + "\n"
}

fn cursorrules_content(include_rtk: bool) -> String {
    let mut content = r#"# Codebase Navigation — Use indxr MCP tools

An MCP server called `indxr` is available. Always use indxr tools before reading full files.

## Exploration workflow
1. `search_relevant` — find files/symbols by concept or partial name
2. `get_tree` — see directory/file layout
3. `get_file_summary` / `batch_file_summaries` — understand files without reading them
4. `explain_symbol` — get signature, docs, and relationships for a symbol
5. `get_public_api` — public API surface of a file or module
6. `get_callers` / `get_related_tests` — find references and tests
7. `get_token_estimate` — check cost before deciding to read a full file
8. `read_source` — read just one function/struct by name
9. Read (full file) — ONLY when editing or need exact formatting

## When to read full files instead
- You need to edit a file
- You need exact formatting/whitespace
- The file is not source code (e.g., config files, documentation)

## Do NOT
- Read full source files just to understand what's in them
- Dump all files into context
- Use `git diff` when `get_diff_summary` would suffice

## After making code changes
Run `regenerate_index` to keep the index current.
"#
    .to_string();

    if include_rtk {
        content.push_str(
            r#"
## Command output compression — RTK

RTK is installed and compresses shell command outputs (git, cargo, npm, etc.) by 60-90%, saving context window tokens.
"#,
        );
    }

    content
}

fn windsurfrules_content(include_rtk: bool) -> String {
    // Same content as cursorrules — both are concise agent instruction files
    cursorrules_content(include_rtk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_mcp_json_is_valid() {
        let content = mcp_json_content();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["mcpServers"]["indxr"]["command"].is_string());
        assert_eq!(parsed["mcpServers"]["indxr"]["command"], "indxr");
    }

    #[test]
    fn test_claude_settings_is_valid_json() {
        let content = claude_settings_content(false);
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["hooks"]["PreToolUse"].is_array());
    }

    #[test]
    fn test_claude_settings_with_rtk_is_valid_json() {
        let content = claude_settings_content(true);
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let hooks = parsed["hooks"]["PreToolUse"].as_array().unwrap();
        // Should have 3 entries: Read, Bash (rtk), Bash (indxr git diff)
        assert_eq!(hooks.len(), 3);
        assert_eq!(hooks[0]["matcher"], "Read");
        assert_eq!(hooks[1]["matcher"], "Bash");
        assert!(
            hooks[1]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("rtk-rewrite")
        );
        assert_eq!(hooks[2]["matcher"], "Bash");
    }

    #[test]
    fn test_claude_settings_without_rtk_has_two_hooks() {
        let content = claude_settings_content(false);
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let hooks = parsed["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(hooks.len(), 2);
    }

    #[test]
    fn test_claude_md_contains_key_sections() {
        let dir = TempDir::new().unwrap();
        let content = claude_md_content(dir.path(), false);
        assert!(content.contains("MUST USE indxr MCP tools"));
        assert!(content.contains("Token savings reference"));
        assert!(content.contains("Exploration workflow"));
        assert!(content.contains("When to use the Read tool instead"));
        assert!(content.contains("DO NOT"));
    }

    #[test]
    fn test_claude_md_with_rtk_contains_rtk_section() {
        let dir = TempDir::new().unwrap();
        let content = claude_md_content(dir.path(), true);
        assert!(content.contains("Command output compression — RTK"));
    }

    #[test]
    fn test_claude_md_without_rtk_no_rtk_section() {
        let dir = TempDir::new().unwrap();
        let content = claude_md_content(dir.path(), false);
        assert!(!content.contains("RTK"));
    }

    #[test]
    fn test_claude_md_uses_directory_name() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("my-project");
        fs::create_dir(&subdir).unwrap();
        let content = claude_md_content(&subdir, false);
        assert!(content.starts_with("# my-project"));
    }

    #[test]
    fn test_write_file_safe_creates_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        let result = write_file_safe(&path, "hello", false).unwrap();
        assert!(matches!(result, WriteResult::Created(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn test_write_file_safe_skips_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "original").unwrap();
        let result = write_file_safe(&path, "new", false).unwrap();
        assert!(matches!(result, WriteResult::Skipped(_, _)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    }

    #[test]
    fn test_write_file_safe_force_overwrites() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "original").unwrap();
        let result = write_file_safe(&path, "new", true).unwrap();
        assert!(matches!(result, WriteResult::Created(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn test_write_file_safe_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sub/dir/test.txt");
        let result = write_file_safe(&path, "hello", false).unwrap();
        assert!(matches!(result, WriteResult::Created(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn test_setup_gitignore_creates_new() {
        let dir = TempDir::new().unwrap();
        let result = setup_gitignore(dir.path()).unwrap();
        assert!(matches!(result, WriteResult::Created(_)));
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(content, ".indxr-cache/\n");
    }

    #[test]
    fn test_setup_gitignore_appends() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();
        let result = setup_gitignore(dir.path()).unwrap();
        assert!(matches!(result, WriteResult::Appended(_)));
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(content, "node_modules/\n.indxr-cache/\n");
    }

    #[test]
    fn test_setup_gitignore_appends_with_missing_newline() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "node_modules/").unwrap();
        let result = setup_gitignore(dir.path()).unwrap();
        assert!(matches!(result, WriteResult::Appended(_)));
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(content, "node_modules/\n.indxr-cache/\n");
    }

    #[test]
    fn test_setup_gitignore_skips_if_present() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(".gitignore"),
            "node_modules/\n.indxr-cache/\n",
        )
        .unwrap();
        let result = setup_gitignore(dir.path()).unwrap();
        assert!(matches!(result, WriteResult::Skipped(_, _)));
    }

    #[test]
    fn test_setup_claude_creates_files() {
        let dir = TempDir::new().unwrap();
        let results = setup_claude(dir.path(), false, true, false).unwrap();
        assert_eq!(results.len(), 3);
        assert!(dir.path().join(".mcp.json").exists());
        assert!(dir.path().join("CLAUDE.md").exists());
        assert!(dir.path().join(".claude/settings.json").exists());
    }

    #[test]
    fn test_setup_claude_with_rtk_creates_hook() {
        let dir = TempDir::new().unwrap();
        let results = setup_claude(dir.path(), false, true, true).unwrap();
        // 3 base files + 1 rtk hook script
        assert_eq!(results.len(), 4);
        assert!(dir.path().join(".claude/hooks/rtk-rewrite.sh").exists());

        // Verify hook script is executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(dir.path().join(".claude/hooks/rtk-rewrite.sh"))
                .unwrap()
                .permissions();
            assert_eq!(perms.mode() & 0o111, 0o111);
        }

        // Verify settings.json includes rtk hook
        let settings = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        assert!(settings.contains("rtk-rewrite"));
    }

    #[test]
    fn test_setup_claude_without_hooks() {
        let dir = TempDir::new().unwrap();
        let results = setup_claude(dir.path(), false, false, false).unwrap();
        assert_eq!(results.len(), 2);
        assert!(dir.path().join(".mcp.json").exists());
        assert!(dir.path().join("CLAUDE.md").exists());
        assert!(!dir.path().join(".claude/settings.json").exists());
    }

    #[test]
    fn test_setup_claude_rtk_without_hooks_skips_rtk() {
        let dir = TempDir::new().unwrap();
        // include_rtk=true but include_hooks=false — rtk hook should not be created
        let results = setup_claude(dir.path(), false, false, true).unwrap();
        assert_eq!(results.len(), 2);
        assert!(!dir.path().join(".claude/hooks/rtk-rewrite.sh").exists());
    }

    #[test]
    fn test_setup_cursor_creates_files() {
        let dir = TempDir::new().unwrap();
        let results = setup_cursor(dir.path(), false, false).unwrap();
        assert_eq!(results.len(), 2);
        assert!(dir.path().join(".cursor/mcp.json").exists());
        assert!(dir.path().join(".cursorrules").exists());
    }

    #[test]
    fn test_setup_windsurf_creates_files() {
        let dir = TempDir::new().unwrap();
        let results = setup_windsurf(dir.path(), false, false).unwrap();
        assert_eq!(results.len(), 2);
        assert!(dir.path().join(".windsurf/mcp.json").exists());
        assert!(dir.path().join(".windsurfrules").exists());
    }

    #[test]
    fn test_rtk_hook_script_is_valid_bash() {
        assert!(RTK_HOOK_SCRIPT.starts_with("#!/bin/bash"));
        assert!(RTK_HOOK_SCRIPT.contains("rtk rewrite"));
        assert!(RTK_HOOK_SCRIPT.contains("hookSpecificOutput"));
        // Must use printf, not echo, for piping $TOOL_INPUT (echo mishandles flags/backslashes)
        assert!(RTK_HOOK_SCRIPT.contains("printf '%s' \"$TOOL_INPUT\""));
        assert!(!RTK_HOOK_SCRIPT.contains("echo \"$TOOL_INPUT\""));
    }
}
