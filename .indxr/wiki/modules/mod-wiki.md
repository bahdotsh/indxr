---
id: mod-wiki
title: Wiki System
page_type: module
source_files:
- src/wiki/mod.rs
- src/wiki/page.rs
- src/wiki/store.rs
- src/wiki/generate.rs
- src/wiki/prompts.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:26:06Z
links_to:
- page-id
covers: []
---

# Wiki System

The wiki system (`src/wiki/`) provides a persistent, agent-writable knowledge base that grows alongside the codebase. It's feature-gated behind `--features wiki`.

## Purpose

The wiki serves as a bridge between code structure and higher-level understanding. While the index tells you *what* exists, the wiki explains *why* things are the way they are — architecture decisions, module responsibilities, design patterns, and failure patterns.

## Module Structure

### Page Model (`src/wiki/page.rs`)

`WikiPage` represents a single wiki page:
- `frontmatter` — `Frontmatter` struct with metadata:
  - `id` — URL-safe slug (e.g., "mod-parser")
  - `title` — human-readable title
  - `page_type` — `PageType` enum: Architecture, Module, Entity, Topic
  - `source_files` — related source file paths
  - `created`, `updated` — timestamps
  - `git_ref` — git ref at time of last update
  - `contradictions` — `Vec<Contradiction>` for tracking inconsistencies
  - `failure_patterns` — `Vec<FailurePattern>` for recording failed fix attempts
- `content` — Markdown body with `[[page-id]]` wiki links

Pages are stored as Markdown files with YAML frontmatter under `.indxr/wiki/pages/`.

### Wiki Store (`src/wiki/store.rs`)

`WikiStore` manages the on-disk wiki:
- `load(wiki_dir)` — reads the manifest and all pages from disk
- `save_page(page)` — writes a page to disk and updates the manifest
- `get_page(id)` — retrieve a page by ID
- `list_pages()` — list all page IDs
- `search(query)` — keyword search across page titles, IDs, and content

The `WikiManifest` tracks all pages with their metadata for fast lookups.

### Wiki Generation (`src/wiki/generate.rs`)

`WikiGenerator` handles LLM-driven wiki creation and updates:
- `generate()` — creates a complete wiki from scratch. Builds a planning context from the codebase index, sends it to the LLM to plan pages, then generates each page.
- `update(since)` — incremental update. Detects code changes since a git ref, identifies affected wiki pages, and regenerates only those pages.
- `build_planning_context()` — creates a condensed representation of the codebase for the LLM planning prompt.

### Prompts (`src/wiki/prompts.rs`)

System prompts for LLM-driven generation:
- `plan_system_prompt()` — instructs the LLM to plan wiki pages from codebase context
- `page_system_prompt()` — instructs the LLM to write a single page
- `index_system_prompt()` — instructs the LLM to write the index page
- `update_system_prompt()` — instructs the LLM to update pages based on code changes
- `incremental_plan_system_prompt()` — for planning incremental updates

## Knowledge Compounding (`src/wiki/mod.rs`)

`compound_into_wiki()` automatically routes new knowledge to the right wiki page:
1. `score_pages()` scores existing pages against the synthesis text using keyword overlap
2. If a good match is found, the synthesis is appended to that page
3. If no match, a new topic page is created with an auto-derived ID via `derive_topic_id()`

This makes the wiki grow richer with every agent interaction.

## MCP Integration

When the wiki feature is enabled and a wiki exists, 9 additional MCP tools become available:
- **`wiki_search`** — keyword search with excerpts
- **`wiki_read`** — read a page by ID
- **`wiki_status`** — health report (page count, staleness, coverage)
- **`wiki_contribute`** — create or update a page
- **`wiki_generate`** — initialize wiki and return planning context
- **`wiki_update`** — analyze changes and return affected pages
- **`wiki_suggest_contribution`** — suggest where to route new knowledge
- **`wiki_compound`** — auto-route knowledge to the best page
- **`wiki_record_failure`** — record failed fix attempts for future agents

## Agent-Driven vs CLI Generation

The wiki can be generated two ways:
1. **CLI** (`indxr wiki generate`) — requires an LLM provider (API key or `--exec` command). The CLI orchestrates the full generation pipeline.
2. **MCP** (`wiki_generate` tool) — the agent IS the LLM. The tool returns codebase context, the agent plans pages, then calls `wiki_contribute` for each page. No API key needed.

