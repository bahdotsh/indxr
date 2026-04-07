---
id: topic-init
title: Agent Config Scaffolding (init)
page_type: topic
source_files:
- src/init.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:25:12Z
links_to: []
covers: []
---

# Agent Config Scaffolding (init)

The init module (`src/init.rs`) generates configuration files for multiple AI coding agents, making it easy to set up indxr as an MCP server in any project.

## Supported Agents

- **Claude Code** — `.mcp.json`, `CLAUDE.md`, `.claude/settings.json`
- **Cursor** — `.cursor/mcp.json`, `.cursor/rules/indxr.mdc`
- **Windsurf** — `.windsurf/mcp.json`, `.windsurfrules`
- **Codex CLI** — `codex.toml`, `AGENTS.md`

## InitOptions

```rust
struct InitOptions {
    root: PathBuf,
    agents: Vec<String>,    // Which agents to configure ("claude", "cursor", etc.)
    force: bool,            // Overwrite existing files
    global: bool,           // Install globally for all projects
    no_index: bool,         // Skip INDEX.md generation
    no_hooks: bool,         // Skip PreToolUse hook setup
    no_rtk: bool,           // Skip RTK hook setup
    max_file_size: u64,
    all_tools: bool,        // Use --all-tools in MCP config
    is_workspace: bool,     // Detected workspace mode
}
```

## What Gets Generated

### Per-project (local)
1. **MCP server config** — JSON or TOML pointing to `indxr serve .` with appropriate args
2. **Instruction file** — Agent-specific rules file (CLAUDE.md, .mdc, etc.) with indxr MCP usage instructions
3. **Settings** — Agent-specific settings (Claude's `settings.json` with PreToolUse hooks)
4. **INDEX.md** — Initial structural index (unless `--no-index`)
5. **.gitignore** — Adds `.indxr/` to gitignore

### Global (`--global`)
Installs to user home directory so indxr is available for all projects without per-project setup.

## Smart File Writing

- `write_file_safe()` — Won't overwrite existing files unless `--force`
- `merge_mcp_server()` — Merges the `indxr` server entry into existing `.mcp.json` without destroying other servers
- `merge_mcp_server_toml()` — Same for TOML format (Codex)
- `append_or_create_instructions()` — Appends indxr instructions to existing rule files with a marker to avoid duplicates

## RTK Integration

If [RTK](https://github.com/rtk-rs/rtk) is detected on the system (`detect_rtk()`), init can set up a PreToolUse hook that rewrites Bash commands through RTK for token compression. The hook script is defined in `RTK_HOOK_SCRIPT`.

## CLI Usage

```bash
indxr init                    # Auto-detect and set up all agents
indxr init --claude           # Claude Code only
indxr init --cursor --windsurf # Specific agents
indxr init --global           # Install globally
indxr init --force            # Overwrite existing
indxr init --no-index         # Skip INDEX.md generation
```

