# Benchmarks

indxr ships three benchmarks:

1. **Token Efficiency Benchmark** (`benchmark.sh`) — measures how many tokens indxr outputs vs reading raw source files. Answers: "how much smaller is indxr's output?"
2. **Accuracy Benchmark v2** (`bench/`) — agent-loop benchmark where both conditions (baseline and indxr) use an LLM that chooses its own tools. Answers: "does the token reduction hurt answer quality when the agent picks its own tools?"
3. **Accuracy Benchmark v1** (`accuracy_bench.py`) — original benchmark with pre-specified tool calls. Kept for reference.

Together they prove the core claim: **indxr gives agents the same (or better) understanding of a codebase with fewer tokens.**

---

## Token Efficiency Benchmark (`benchmark.sh`)

### Why it exists

When an AI agent needs to understand a codebase, the naive approach is to read source files. A 500-line file costs ~3,000+ tokens. Multiply that across multiple files, multiple steps, and multiple retries — agents routinely burn 10,000-30,000 tokens just figuring out where things are.

indxr provides structural alternatives: file summaries, symbol lookups, targeted reads. But how much do they actually save? This benchmark measures that with real tokenizers, not estimates.

### What it measures

The benchmark runs 8 sections against each project:

1. **Raw source baseline** — `cat` all indexed source files together and count the tokens. This is the cost of "read everything", the worst case for an agent without indxr.

2. **Naive structural: `tree` output** — what you get from just running `tree`. Structure only, no code understanding.

3. **indxr detail levels** (cold cache) — token output at three levels:
   - `summary`: directory tree + file list only
   - `signatures`: + function/struct signatures (the default)
   - `full`: + doc comments, line numbers, body line counts

4. **Token budgets** — output size when using `--max-tokens 2000/4000/8000/15000`. indxr progressively truncates: first doc comments, then private declarations, then children, then least-important files.

5. **Targeted queries** — token cost of scoped indexing:
   - `--symbol "name"`: search for a specific symbol
   - `--kind function`: filter by declaration kind
   - `--public-only`: only public API surface
   - `--filter-path src/`: scope to a subtree

6. **MCP server per-tool costs** — actually starts the MCP server and calls individual tools (`get_stats`, `get_tree`, `lookup_symbol`, `search_signatures`, `list_declarations`, `get_imports`), measuring the token cost of each response. Also shows the cost of `cat`-ing the same file for comparison.

7. **Cache performance** — cold vs warm indexing time and speedup ratio.

8. **Summary table** — side-by-side comparison of all approaches with compression ratios.

### Token counting

Uses real tokenizers, not estimates:

- **OpenAI**: tiktoken `o200k_base` (GPT-4o / GPT-4.1 / GPT-5 / o3 / o4-mini) — offline, exact
- **Claude**: Anthropic `count_tokens` API (claude-sonnet-4-6) — requires `ANTHROPIC_API_KEY`, exact
- **Fallback**: `~4 chars/token` estimate if neither tokenizer is installed

When both tokenizers are available, the benchmark shows both counts side by side so you can compare.

### Requirements

- indxr binary (`cargo build --release`)
- jq (`brew install jq` on macOS, `apt install jq` on Linux)
- Python 3 with tiktoken (`pip install tiktoken`)
- Optional: `ANTHROPIC_API_KEY` + anthropic SDK (`pip install anthropic`) for Claude token counts

### Setup

```bash
cargo build --release

# Python environment (first time only)
python3 -m venv .bench-venv
.bench-venv/bin/pip install tiktoken anthropic
```

### Usage

```bash
# Benchmark the current project (defaults to cwd)
./benchmark.sh

# Benchmark specific projects
./benchmark.sh /path/to/project1 /path/to/project2

# With Claude token counts (set your key first)
export ANTHROPIC_API_KEY=sk-ant-...
./benchmark.sh
```

If no paths are given, it benchmarks the indxr project itself.

---

## Accuracy Benchmark v2 (`bench/`)

### Why it exists

The v1 accuracy benchmark had several methodological limitations:
- **Pre-specified tool calls** — the indxr tools to call were hard-coded per question, assuming perfect tool selection
- **Strawman baseline** — the baseline concatenated full files, rather than letting an agent search
- **Self-benchmarking only** — tested only on indxr's own codebase
- **Only structural questions** — no behavioral or reasoning questions
- **No statistical rigor** — single run, no confidence intervals

v2 addresses all of these with a proper **agent loop** design.

### Design

Both conditions use the same agent loop — the LLM decides which tools to call:

| Condition | Available tools | What it simulates |
|-----------|----------------|-------------------|
| **Baseline** | `grep_codebase`, `read_file`, `list_directory` | Agent with standard code tools (grep + read) |
| **indxr** | MCP tools (dynamically fetched from server; 3 compound default, 26 with `--all-tools`) | Agent with indxr structural tools |

Same model, same system prompt, same question, same `temperature=0`. The only difference is the toolbox.

### Question categories (40 questions)

| Category | Count | What it tests | Example |
|----------|-------|--------------|---------|
| `structural` | 17 | Symbol lookup, cross-file imports, counting, architecture | "Where is `estimate_tokens` defined?" |
| `behavioral` | 13 | What functions do, edge cases, error handling | "What happens with an unknown tool name?" |
| `reasoning` | 10 | Why code is designed a certain way | "Why are doc comments stripped before private decls?" |

### Scoring

All scoring is deterministic — no LLM-as-judge:

- **substring**: answer contains the expected string (1.0 or 0.0)
- **all_of**: fraction of required strings found
- **set_match**: fraction of expected items found (partial credit)
- **number_range**: extracted number falls within expected range

**Anti-hallucination**: questions can specify `scoring_anti_targets` — strings that indicate wrong information. Each anti-target found applies a penalty of `0.25 × (found / total)`.

### Statistical rigor

- Runs each question K times (default K=3)
- Reports mean accuracy with bootstrap 95% confidence intervals
- Per-category breakdown
- Records total input tokens across all agent rounds (real API billing tokens)
- Tracks tool-call count and rounds per condition

### Requirements

- indxr binary (`cargo build --release`)
- Python 3 with anthropic SDK (`pip install anthropic`)
- `ANTHROPIC_API_KEY` environment variable

### Usage

```bash
# Set your API key
export ANTHROPIC_API_KEY=sk-ant-...

# Full benchmark (3 runs, ~$15-25)
python -m bench

# Quick single run (~$5-8)
python -m bench --runs 1

# Dry run — show tools and questions without API calls (free)
python -m bench --dry-run

# Run one category
python -m bench --filter behavioral

# Run one question
python -m bench --question behav-004

# Verbose — show agent tool traces and answers
python -m bench --verbose

# External repo
python -m bench --repo /path/to/project --questions my_questions.json

# Different model
python -m bench --model claude-haiku-4-5
```

### Output

Prints a results table with per-question and per-category breakdown, then writes `benchmark_results_v2.json` with full per-run data including agent traces.

**Note on token measurement:** The benchmark measures total API billing tokens (`input_tokens`) across all agent rounds. This includes system prompt, tool definitions, and growing conversation history — not just tool output. Because tool definitions are re-sent every round, the number of tools directly impacts total cost. indxr's default 3 compound tools add minimal schema overhead vs the 3 baseline tools. On small codebases where grep+read is already cheap, this makes indxr's structural tools competitive even on simple questions. indxr's compound tools show the biggest wins on questions that require reading large files or multi-step exploration (e.g., counting public functions, tracing type flow).

### Adding questions for external repos

Create a questions file following this format:

```json
{
  "version": 2,
  "repo": "self",
  "questions": [
    {
      "id": "my-001",
      "category": "structural",
      "difficulty": "easy",
      "question": "Where is function X defined?",
      "ground_truth": "src/foo.rs",
      "scoring": "substring",
      "scoring_targets": ["src/foo.rs"],
      "scoring_anti_targets": []
    }
  ]
}
```

Categories: `structural`, `behavioral`, `reasoning`. Scoring methods: `substring`, `all_of`, `set_match`, `number_range`.

### Files

| File | Purpose |
|------|---------|
| `bench/__init__.py` | Package marker |
| `bench/__main__.py` | Entry point for `python -m bench` |
| `bench/agent.py` | Agent loop (shared by both conditions) |
| `bench/tools_baseline.py` | Baseline tools: grep, read, list_directory |
| `bench/tools_indxr.py` | indxr MCP tools (dynamically loaded from server) |
| `bench/scoring.py` | Deterministic scoring with anti-hallucination |
| `bench/stats.py` | Multi-run aggregation, bootstrap confidence intervals |
| `bench/output.py` | Terminal tables and JSON output |
| `bench/runner.py` | CLI, orchestration, main loop |
| `bench_questions/indxr.json` | 40 questions across 3 categories |

---

## Accuracy Benchmark v1 (`accuracy_bench.py`) — Reference

The original accuracy benchmark, kept for historical reference. Uses pre-specified tool calls and file selections rather than an agent loop.

| File | Purpose |
|------|---------|
| `accuracy_bench.py` | v1 benchmark runner |
| `accuracy_questions.json` | v1 questions (20, with hard-coded tool calls) |
| `benchmark_results.json` | v1 results from the last run |

See the v1 section in git history for full documentation.

---

## Running all benchmarks

```bash
# One-time setup
cargo build --release
python3 -m venv .bench-venv
.bench-venv/bin/pip install tiktoken anthropic
export ANTHROPIC_API_KEY=sk-ant-...

# Token efficiency (no API key needed if tiktoken is installed)
./benchmark.sh

# Accuracy v2 — quick single run (~$5-8)
.bench-venv/bin/python -m bench --runs 1

# Accuracy v2 — full with statistical rigor (~$15-25)
.bench-venv/bin/python -m bench --runs 3
```
