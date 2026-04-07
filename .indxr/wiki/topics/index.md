---
id: index
title: Wiki Index
page_type: topic
source_files: []
generated_at_ref: ''
generated_at: 2026-04-07T13:28:42Z
links_to:
- architecture
- mod-indexer
- mod-model
- mod-parser
- mod-cache
- mod-mcp
- mod-wiki
- mod-diff
- mod-dep-graph
- mod-filter-budget
- mod-output
- mod-workspace
- mod-watch
- mod-cli
- mod-llm
- languages
covers: []
---

# indxr Wiki

A knowledge wiki for indxr — a fast Rust codebase indexer for AI agents.

## Architecture

- [[architecture]] — Overall architecture, pipeline stages, data flow, and key design decisions

## Core Modules

- [[mod-indexer]] — Core indexing orchestration: parallel parsing, caching, workspace building
- [[mod-model]] — Data model: WorkspaceIndex, CodebaseIndex, FileIndex, Declaration hierarchy
- [[mod-parser]] — Dual parser system: tree-sitter (8 languages) + regex (19 languages)
- [[mod-cache]] — Incremental binary caching with mtime + xxh3 fingerprinting

## Server & Tools

- [[mod-mcp]] — MCP server: JSON-RPC protocol, stdio/HTTP transports, 26+ tool implementations
- [[mod-wiki]] — Wiki system: persistent knowledge base, generation, compounding, failure recording

## Analysis & Output

- [[mod-diff]] — Git structural diffing: declaration-level changes between git refs and PRs
- [[mod-dep-graph]] — Dependency graph generation: file-level and symbol-level, DOT/Mermaid/JSON output
- [[mod-filter-budget]] — Filtering (path, kind, visibility, symbol) and progressive token budget truncation
- [[mod-output]] — Output formatting: Markdown, YAML, JSON renderers

## Infrastructure

- [[mod-workspace]] — Workspace/monorepo detection: Cargo, npm, Go workspaces
- [[mod-watch]] — File watching: debounced re-indexing with notify crate
- [[mod-cli]] — CLI argument parsing and dispatch via clap
- [[mod-llm]] — LLM client abstraction: Claude, OpenAI, external command backends

## Reference

- [[languages]] — 27 supported languages: extensions, parsing strategy, classification

