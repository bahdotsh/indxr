---
id: mod-cli
title: CLI Interface
page_type: module
source_files:
- src/cli.rs
- src/main.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:28:06Z
links_to: []
covers: []
---

# CLI Interface

The CLI (`src/cli.rs`) defines all command-line arguments using `clap` with derive macros. The main entry point (`src/main.rs`) matches on the parsed CLI to dispatch to the appropriate subsystem.

## Top-Level Structure

### `Cli` struct
The root argument struct with 26 fields covering:
- **Input/output**: root path, `-o` output file, `-f` format (markdown/json/yaml)
- **Detail level**: `-d` (summary, signatures, full)
- **Filtering**: `--filter-path`, `--public-only`, `--symbol`, `--kind`, `-l` languages
- **Budget**: `--max-tokens`
- **Git diffing**: `--since` ref
- **Graphs**: `--graph` format, `--graph-level`, `--graph-depth`
- **Hotspots**: `--hotspots` flag
- **Caching**: `--no-cache`, `--cache-dir`
- **Traversal**: `--max-depth`, `--max-file-size`, `-e` exclude patterns, `--no-gitignore`
- **Output control**: `--omit-imports`, `--omit-tree`, `--quiet`, `--stats`
- **Workspace**: `--member`, `--no-workspace`

### `Command` enum (subcommands)
- **`Serve`** — start MCP server. Options: `--all-tools`, `--watch`, `--debounce-ms`, `--http`, `--wiki-auto-update`, `--wiki-debounce-ms`, plus shared `IndexOpts`.
- **`Watch`** — watch mode. Options: output path, `--debounce-ms`, `--quiet`, plus shared `IndexOpts`.
- **`Diff`** — structural diff. Options: `--since`, `--pr`, `-f` format, plus shared `IndexOpts`.
- **`Wiki`** — wiki subcommand with `WikiAction`.
- **`Init`** — agent config setup. Options: `--claude`, `--cursor`, `--windsurf`, `--codex`, `--all`, `--global`, `--force`, `--no-index`, `--no-hooks`, `--no-rtk`, `--max-file-size`.
- **`Members`** — list workspace members.

### `WikiAction` enum
- `Generate` — generate wiki (with `--dry-run`, `--model`, `--exec` options)
- `Update` — update wiki from changes (`--since`, `--model`, `--exec`)
- `Status` — check wiki health
- `Compound` — compound knowledge from file or stdin

### `IndexOpts` struct
Shared indexing options reused across subcommands: `--max-depth`, `--max-file-size`, `-e`, `--no-gitignore`, `--cache-dir`, `--member`, `--no-workspace`.

## Dispatch (`src/main.rs`)

`main()` parses the CLI, then branches on whether a subcommand was used:
- Subcommands dispatch to their respective modules (MCP server, watch, diff, wiki, init, members)
- No subcommand: runs the default indexing pipeline (walk → parse → filter → budget → format → output)

