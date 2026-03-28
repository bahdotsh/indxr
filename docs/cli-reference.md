# CLI Reference

Complete command-line reference for indxr.

## Synopsis

```
indxr [OPTIONS] [PATH] [COMMAND]
```

## Commands

### `init`

Initialize indxr configuration files for AI agent integration. Creates MCP configs, agent instruction files, PreToolUse hooks, and an initial INDEX.md in one command.

```bash
indxr init [PATH] [OPTIONS]
```

**Options:**
- `--claude` — Set up for Claude Code (`.mcp.json`, `CLAUDE.md`, `.claude/settings.json`)
- `--cursor` — Set up for Cursor (`.cursor/mcp.json`, `.cursor/rules/indxr.mdc`)
- `--windsurf` — Set up for Windsurf (`.windsurf/mcp.json`, `.windsurf/rules/indxr.md`)
- `--codex` — Set up for OpenAI Codex CLI (`.codex/config.toml`, `AGENTS.md`)
- `--all` — Set up for all supported agents (default when no agent flag is specified)
- `--global` — Install to global/user-level config so indxr is available for all projects
- `--no-index` — Skip generating INDEX.md
- `--no-hooks` — Skip PreToolUse hooks for Claude Code (`.claude/settings.json`)
- `--force` — Overwrite existing files (default: skip with warning)
- `--max-file-size <KB>` — Skip files larger than N KB when generating INDEX.md (default: 512)

**Behavior:**
- If no agent flag is specified, defaults to `--all`
- Existing files are skipped with a warning unless `--force` is used
- `.gitignore` is appended with `.indxr-cache/` if not already present (never overwritten)
- `--global` writes to user-level config directories:
  - Claude Code: `~/.claude.json` (MCP), `~/.claude/CLAUDE.md` (instructions)
  - Cursor: `~/.cursor/mcp.json` (MCP)
  - Windsurf: `~/.codeium/windsurf/mcp_config.json` (MCP), `~/.codeium/windsurf/memories/global_rules.md` (rules)
  - Codex CLI: `~/.codex/config.toml` (MCP), `~/.codex/AGENTS.md` (instructions)
- `--global` merges MCP/TOML server entries into existing config files (preserves other servers)
- Detects deprecated `.cursorrules` and `.windsurfrules` files and suggests removal (rules have moved to `.cursor/rules/indxr.mdc` and `.windsurf/rules/indxr.md`)

### `watch`

Watch for file changes and keep INDEX.md continuously up to date. Performs an initial index, then re-indexes on each debounced source file change.

```bash
indxr watch [PATH] [OPTIONS]
```

**Options:**
- `-o, --output <FILE>` — Output file path (default: `INDEX.md` in root directory)
- `--cache-dir <DIR>` — Cache directory (default: `.indxr-cache`)
- `--max-file-size <KB>` — Skip files larger than N KB (default: 512)
- `--max-depth <N>` — Maximum directory depth
- `-e, --exclude <PATTERNS>` — Glob patterns to exclude
- `--no-gitignore` — Don't respect .gitignore
- `--debounce-ms <MS>` — Debounce timeout in milliseconds (default: 300)
- `-q, --quiet` — Suppress progress output

**Behavior:**
- Filters out non-source files, hidden directories (`.git`), cache directories, and the output file itself
- Only triggers re-index for files with recognized language extensions
- Blocks indefinitely until Ctrl+C

### `diff`

Show structural changes for a GitHub PR or git ref. Requires either `--pr` or `--since` (not both).

```bash
indxr diff --pr <NUMBER> [PATH] [OPTIONS]
indxr diff --since <REF> [PATH] [OPTIONS]
```

**Options:**
- `--pr <NUMBER>` — GitHub PR number to diff against its base branch (resolves via GitHub API)
- `--since <REF>` — Git ref to diff against (branch, tag, or commit)
- `-f, --format <FORMAT>` — Output format: `markdown` (default) or `json`

**Authentication (for `--pr`):**

The PR option requires a GitHub token. Looks for (in order):
1. `GITHUB_TOKEN` environment variable
2. `GH_TOKEN` environment variable
3. `gh auth token` (GitHub CLI)

The base branch must be available locally. Run `git fetch origin <base>` if needed.

**Examples:**
```bash
# Structural diff for PR #42
indxr diff --pr 42

# JSON output
indxr diff --pr 42 -f json

# Diff against a git ref (same as --since flag)
indxr diff --since main

# Diff a specific project
indxr diff /path/to/project --since v1.0
```

### `serve`

Start an MCP server for AI agent integration. See [MCP Server](mcp-server.md) for full details.

```bash
indxr serve [PATH] [OPTIONS]
```

**Options:**
- `--cache-dir <DIR>` — Cache directory (default: `.indxr-cache`)
- `--max-file-size <KB>` — Skip files larger than N KB (default: 512)
- `--max-depth <N>` — Maximum directory depth
- `-e, --exclude <PATTERNS>` — Glob patterns to exclude
- `--no-gitignore` — Don't respect .gitignore
- `--watch` — Watch for file changes and auto-reindex the in-memory index
- `--debounce-ms <MS>` — Debounce timeout in milliseconds, requires `--watch` (default: 300)

## Arguments

### `[PATH]`

Root directory to index. Defaults to the current directory (`.`).

```bash
indxr ./my-project
indxr /home/user/repos/backend
```

## Options

### Output

| Flag | Description | Default |
|------|-------------|---------|
| `-o, --output <FILE>` | Write output to a file instead of stdout | stdout |
| `-f, --format <FORMAT>` | Output format: `markdown`, `json`, `yaml` | `markdown` |
| `-d, --detail <LEVEL>` | Detail level: `summary`, `signatures`, `full` | `signatures` |
| `--omit-imports` | Omit import listings from output | off |
| `--omit-tree` | Omit directory tree from output | off |

### Filtering

| Flag | Description |
|------|-------------|
| `-l, --languages <LANGS>` | Filter by language (comma-separated). Example: `rust,python` |
| `--filter-path <SUBPATH>` | Only include files under this subdirectory |
| `--symbol <SYMBOL>` | Search for a symbol by name (case-insensitive substring) |
| `--kind <KIND>` | Filter by declaration kind: `function`, `struct`, `class`, `trait`, `enum`, `interface`, `module`, `method`, `constant`, `impl`, `type`, `namespace`, `macro`, `table`, `service`, `message`, `rpc`, and more |
| `--public-only` | Only show public declarations |

### File Discovery

| Flag | Description | Default |
|------|-------------|---------|
| `--max-depth <N>` | Maximum directory traversal depth | unlimited |
| `--max-file-size <KB>` | Skip files larger than N kilobytes | 512 |
| `-e, --exclude <PATTERNS>` | Glob patterns to exclude (repeatable) | none |
| `--no-gitignore` | Don't respect .gitignore rules | off |

### Caching

| Flag | Description | Default |
|------|-------------|---------|
| `--no-cache` | Disable incremental caching | off |
| `--cache-dir <DIR>` | Custom cache directory | `.indxr-cache` |

### Complexity Hotspots

| Flag | Description |
|------|-------------|
| `--hotspots` | Show the top 30 most complex functions and exit |

Outputs a table with composite score, cyclomatic complexity, max nesting depth, parameter count, and body lines for each function. Only tree-sitter parsed languages (Rust, Python, TypeScript, JavaScript, Go, Java, C, C++) are analyzed. Combine with `--filter-path` to scope to a directory.

### Dependency Graph

| Flag | Description |
|------|-------------|
| `--graph <FORMAT>` | Output dependency graph instead of index: `dot`, `mermaid`, or `json` |
| `--graph-level <LEVEL>` | Graph granularity: `file` (default) or `symbol`. Requires `--graph` |
| `--graph-depth <N>` | Max edge hops from scoped files. Requires `--graph` |

### Advanced

| Flag | Description |
|------|-------------|
| `--since <REF>` | Structural diff since a git ref (branch, tag, commit) |
| `--max-tokens <N>` | Token budget for output (approximate, ~4 chars/token) |
| `-q, --quiet` | Suppress progress output to stderr |
| `--stats` | Print indexing statistics to stderr |

## Examples

### Basic Usage

```bash
# Index current directory
indxr

# Index a specific project
indxr ./my-project

# Write to file
indxr -o INDEX.md
```

### Output Formats

```bash
# JSON for programmatic use
indxr -f json -o index.json

# YAML for human-readable structured output
indxr -f yaml -o index.yaml

# Summary only (no declarations)
indxr -d summary

# Full detail with metadata
indxr -d full
```

### Filtering

```bash
# Only Rust and Python files
indxr -l rust,python

# Only files under src/parser
indxr --filter-path src/parser

# Only public functions
indxr --kind function --public-only

# Find all "parse" symbols
indxr --symbol parse

# Combined: public structs in src/model
indxr --filter-path src/model --kind struct --public-only
```

### Git Diffing

```bash
# Changes since main branch
indxr --since main

# Changes since a tag
indxr --since v1.0.0

# Changes in last 5 commits
indxr --since HEAD~5

# Changes since a commit hash
indxr --since abc1234

# JSON diff output
indxr --since main -f json

# PR-aware structural diff (via diff subcommand)
indxr diff --pr 42
indxr diff --pr 42 -f json
indxr diff --since main
```

### Token Budget

```bash
# Fit in 4000 tokens
indxr --max-tokens 4000

# Compact public API within budget
indxr --public-only --max-tokens 3000

# Budget with JSON output
indxr --max-tokens 8000 -f json
```

### File Control

```bash
# Limit depth
indxr --max-depth 3

# Exclude test directories
indxr -e "*/tests/*" -e "*/test/*"

# Include gitignored files
indxr --no-gitignore

# Skip large files
indxr --max-file-size 256
```

### File Watching

```bash
# Watch current directory, keep INDEX.md updated
indxr watch

# Watch a specific project
indxr watch ./my-project

# Custom output path
indxr watch -o custom-index.md

# Slower debounce for high-frequency saves
indxr watch --debounce-ms 500

# Quiet mode (no progress output)
indxr watch --quiet

# MCP server with auto-reindex
indxr serve --watch
indxr serve --watch --debounce-ms 500
```

### Agent Setup

```bash
# Set up for all agents (Claude Code, Cursor, Windsurf, Codex CLI)
indxr init

# Claude Code only
indxr init --claude

# Cursor and Windsurf only
indxr init --cursor --windsurf

# OpenAI Codex CLI only
indxr init --codex

# Install globally for all projects
indxr init --global

# Global Cursor only
indxr init --global --cursor

# Config files only, skip INDEX.md generation
indxr init --no-index

# Skip PreToolUse hooks
indxr init --claude --no-hooks

# Overwrite existing files
indxr init --force

# Re-run after initial setup (skips existing files)
indxr init
```

### Complexity Hotspots

```bash
# Show top 30 most complex functions
indxr --hotspots

# Scoped to a directory
indxr --hotspots --filter-path src/parser

# Example output:
#   Score   CC  Nest Params  Lines  Function
# ------------------------------------------------------------------------------
#    18.5   12     4      1     20  src/parser.rs:10  parse_file
#     5.3    2     1      1      5  src/utils.rs:35   internal_helper
```

### Dependency Graph

```bash
# File-level DOT graph (for Graphviz)
indxr --graph dot

# File-level Mermaid diagram
indxr --graph mermaid

# JSON graph for programmatic use
indxr --graph json -o deps.json

# Symbol-level graph (trait impls, method relationships)
indxr --graph dot --graph-level symbol

# Scoped to a directory
indxr --graph mermaid --filter-path src/parser

# Limit to 2 hops from scoped files
indxr --graph dot --filter-path src/mcp --graph-depth 2

# Write to file
indxr --graph dot -o deps.dot
```

### Combining Options

```bash
# Compact public API index for an agent
indxr --public-only --max-tokens 8000 --omit-imports -o API.md

# Quick structural diff of backend changes
indxr --since main --filter-path src/backend -l rust

# Full JSON index without cache
indxr -f json -d full --no-cache -o full-index.json
```
