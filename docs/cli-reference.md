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
- `--cursor` — Set up for Cursor (`.cursor/mcp.json`, `.cursorrules`)
- `--windsurf` — Set up for Windsurf (`.windsurf/mcp.json`, `.windsurfrules`)
- `--all` — Set up for all supported agents (default when no agent flag is specified)
- `--no-index` — Skip generating INDEX.md
- `--no-hooks` — Skip PreToolUse hooks for Claude Code (`.claude/settings.json`)
- `--force` — Overwrite existing files (default: skip with warning)
- `--max-file-size <KB>` — Skip files larger than N KB when generating INDEX.md (default: 512)

**Behavior:**
- If no agent flag is specified, defaults to `--all`
- Existing files are skipped with a warning unless `--force` is used
- `.gitignore` is appended with `.indxr-cache/` if not already present (never overwritten)

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
# Set up for all agents (Claude Code, Cursor, Windsurf)
indxr init

# Claude Code only
indxr init --claude

# Cursor and Windsurf only
indxr init --cursor --windsurf

# Config files only, skip INDEX.md generation
indxr init --no-index

# Skip PreToolUse hooks
indxr init --claude --no-hooks

# Overwrite existing files
indxr init --force

# Re-run after initial setup (skips existing files)
indxr init
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
