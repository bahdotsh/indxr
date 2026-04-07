---
id: mod-llm
title: LLM Client Abstraction
page_type: module
source_files:
- src/llm/mod.rs
- src/llm/claude.rs
- src/llm/openai.rs
- src/llm/command.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:28:26Z
links_to: []
covers: []
---

# LLM Client Abstraction

The LLM module (`src/llm/`) provides a unified interface for communicating with language models, used by the wiki generation system.

## Architecture

### LlmClient trait (`src/llm/mod.rs`)
A common interface for all LLM backends:
- `generate(system_prompt, user_prompt) -> Result<String>` — send a prompt and get a response

### Backends

**Claude** (`src/llm/claude.rs`):
- Uses the Anthropic API via `reqwest`
- Activated when `ANTHROPIC_API_KEY` is set
- Supports model override via `--model`

**OpenAI-compatible** (`src/llm/openai.rs`):
- Uses the OpenAI chat completions API via `ureq`
- Activated when `OPENAI_API_KEY` is set
- Works with any OpenAI-compatible endpoint

**External command** (`src/llm/command.rs`):
- Shells out to an arbitrary command via `INDXR_LLM_COMMAND` or `--exec`
- System prompt passed as first argument, user prompt on stdin
- Response read from stdout
- Ideal for coding agents that want to use themselves as the LLM

### Provider Resolution (`build_llm_client()` in `src/wiki/mod.rs`)

Priority order:
1. `--exec <CMD>` or `INDXR_LLM_COMMAND` → `CommandLlm`
2. `ANTHROPIC_API_KEY` → `ClaudeLlm`
3. `OPENAI_API_KEY` → `OpenAiLlm`
4. None → error with setup instructions

## MCP Alternative

When using the MCP server, the wiki tools (`wiki_generate`, `wiki_update`) don't need an LLM provider at all — the calling agent IS the LLM. The tools return context and the agent generates content directly, calling `wiki_contribute` to save pages.

