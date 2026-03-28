# Benchmarks

indxr ships two benchmarks that measure different things:

1. **Token Efficiency Benchmark** (`benchmark.sh`) — measures how many tokens indxr outputs vs reading raw source files. Answers: "how much smaller is indxr's output?"
2. **Accuracy Benchmark** (`accuracy_bench.py`) — measures whether an LLM answers questions correctly with indxr context vs full-file context. Answers: "does the token reduction hurt answer quality?"

Together they prove the core claim: **indxr gives agents the same (or better) understanding of a codebase with 5-20x fewer tokens.**

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

# Benchmark multiple well-known repos
./benchmark.sh ~/projects/fastapi ~/projects/tokio ~/projects/express

# With Claude token counts (set your key first)
export ANTHROPIC_API_KEY=sk-ant-...
./benchmark.sh
```

If no paths are given, it benchmarks the indxr project itself.

### Output

Prints a formatted, color-coded table to the terminal. Example summary for the indxr codebase:

```
▸ 8. Summary: Token Efficiency Comparison

  Approach                             Tokens (OA)    vs Raw
  ──────────────────────────────────────────────────────────────
  cat all source files                     211,667      1.0x
  tree (structure only)                      1,200    176.4x
  indxr --detail summary                    2,800     75.6x
  indxr --detail signatures                 5,400     39.2x
  indxr --detail full                       8,100     26.1x
  indxr --max-tokens 8000                   7,800     27.1x
```

Each section also shows per-tool breakdowns and timing. When run against multiple projects, it prints per-project results followed by a cross-project summary.

### How it works internally

The script:
1. Builds a JSON index of the project with `indxr -f json`
2. Concatenates all indexed source files to measure the raw baseline
3. Runs `indxr` with different flags, capturing output to temp files
4. Counts tokens in each temp file using tiktoken / Anthropic API / fallback
5. Starts an MCP server subprocess and sends JSON-RPC tool calls, measuring the token cost of each response
6. Computes compression ratios and prints everything in a formatted table

---

## Accuracy Benchmark (`accuracy_bench.py`)

### Why it exists

The token efficiency benchmark proves indxr outputs fewer tokens. But fewer tokens only matter if the LLM can still answer correctly. A skeptic can reasonably ask: "sure it's smaller, but does the model lose information it needs?"

This benchmark answers that directly. It sends the same questions to Claude under two conditions — full file reads vs indxr structural context — and compares answer quality. If indxr context produces equal or better answers with fewer tokens, the claim is proven.

### What it measures

For each of 20 code comprehension questions:

1. **Baseline condition**: reads the full contents of relevant source files and sends them as context to Claude
2. **indxr condition**: queries indxr's MCP server (file summaries, symbol lookups, caller traces, etc.) and sends that structural output as context
3. **Scores both answers** against a known ground-truth answer using deterministic scoring
4. **Records exact token counts** from the Claude API response (`response.usage.input_tokens`) — these are real billing tokens, not estimates

### Question categories

The 20 questions cover 7 categories of code comprehension:

| Category | Count | What it tests | Example |
|----------|-------|--------------|---------|
| `symbol_lookup` | 4 | Find where a symbol is defined | "In which file is `estimate_tokens` defined?" |
| `cross_file` | 3 | Trace imports/dependencies across files | "Which files import from `src/mcp/helpers.rs`?" |
| `function_behavior` | 3 | Understand what a function does | "What does the `tool_error` function do?" |
| `structural_counting` | 3 | Count/enumerate declarations | "How many public functions are in `tools.rs`?" |
| `caller_tracing` | 2 | Find who calls/uses a symbol | "Which functions reference `estimate_tokens`?" |
| `architecture` | 3 | Answer high-level structural questions | "What are the top-level modules in `src/`?" |
| `public_api` | 2 | Identify public API surface | "What are the public functions in `budget.rs`?" |

### Scoring

All scoring is deterministic — no LLM-as-judge, fully reproducible:

- **substring**: answer contains the expected string (score: 1.0 or 0.0)
- **all_of**: answer contains all expected strings (score: fraction found)
- **set_match**: fraction of expected items found in the answer (partial credit)
- **number_range**: a number in the answer falls within the expected range (score: 1.0 or 0.0)

### Token counting

Token counts come from the **Claude API response** — not from any estimation. When you call `client.messages.create(...)`, the response includes `response.usage.input_tokens` and `response.usage.output_tokens`. These are exact counts from Anthropic's tokenizer, the same numbers that appear on your billing.

### Requirements

- indxr binary (`cargo build --release`)
- Python 3 with anthropic SDK (`pip install anthropic`)
- `ANTHROPIC_API_KEY` environment variable

### Setup

```bash
cargo build --release

# Python environment (first time only)
python3 -m venv .bench-venv
.bench-venv/bin/pip install anthropic
```

### Usage

```bash
# Set your API key
export ANTHROPIC_API_KEY=sk-ant-...

# Run the full benchmark
.bench-venv/bin/python3 accuracy_bench.py

# Dry run — show contexts without calling the API (free, no key needed)
.bench-venv/bin/python3 accuracy_bench.py --dry-run

# Show the full LLM answers for both conditions
.bench-venv/bin/python3 accuracy_bench.py --verbose

# Run only one question category
.bench-venv/bin/python3 accuracy_bench.py --filter symbol_lookup

# Use a different model
.bench-venv/bin/python3 accuracy_bench.py --model claude-haiku-4-5

# Custom output path
.bench-venv/bin/python3 accuracy_bench.py --output my_results.json
```

**Cost**: ~$0.50–1.00 per run (40 API calls, ~370K total tokens). Takes about 2 minutes.

### Output

Prints a results table to the terminal:

```
==========================================================================================
indxr Accuracy Benchmark Results
==========================================================================================
Model:     claude-sonnet-4-6
Questions: 20

ID           Category             BL Score IX Score  BL Toks  IX Toks  Reduction
------------------------------------------------------------------------------------------
sym-001      symbol_lookup            1.00     1.00   27,633      203     136.1x
sym-002      symbol_lookup            1.00     1.00   21,744      228      95.4x
behav-001    function_behavior        1.00     1.00   29,290      243     120.5x
caller-001   caller_tracing           1.00     1.00   27,728      165     168.0x
...
------------------------------------------------------------------------------------------

Summary:
  Baseline accuracy:    0.97  (19/20 questions)
  indxr accuracy:       0.99  (19/20 questions)
  Accuracy delta:       +2.3%
  Total baseline tokens: 351,349
  Total indxr tokens:    16,124
  Avg token reduction:   21.8x

  => indxr achieves better accuracy with 21.8x fewer tokens.
```

Also writes `benchmark_results.json` with full data for every question: both LLM answers, exact token counts, scores, and aggregate summary.

### How it works internally

The script:
1. Loads questions from `accuracy_questions.json`
2. Starts an indxr MCP server as a subprocess (communicates via stdin/stdout JSON-RPC)
3. For each question:
   - **Baseline**: reads the specified source files from disk, concatenates them as context, sends to Claude API with `temperature=0`
   - **indxr**: calls the specified MCP tools (e.g. `lookup_symbol`, `get_file_summary`), concatenates their outputs as context, sends to Claude API with `temperature=0`
   - Scores both answers against ground truth using deterministic string matching
   - Records exact token counts from `response.usage`
4. Prints the results table and writes `benchmark_results.json`

### Files

| File | Purpose |
|------|---------|
| `accuracy_bench.py` | Benchmark runner — MCP communication, API calls, scoring, output |
| `accuracy_questions.json` | 20 questions with ground truth, scoring methods, baseline files, and indxr tool calls |
| `benchmark_results.json` | Output — full results from the last run |

### Adding questions

Add entries to `accuracy_questions.json`:

```json
{
  "id": "my-001",
  "category": "symbol_lookup",
  "question": "Where is function X defined?",
  "ground_truth": "src/foo.rs",
  "scoring": "substring",
  "scoring_targets": ["src/foo.rs"],
  "baseline_files": ["src/foo.rs", "src/bar.rs"],
  "indxr_tool_calls": [
    {"tool": "lookup_symbol", "args": {"name": "X"}}
  ],
  "difficulty": "easy"
}
```

- **`baseline_files`**: the files an agent would typically read to answer the question. Include the files containing the answer plus a few plausible extras an agent might open while searching.
- **`indxr_tool_calls`**: the MCP tool calls indxr would make instead. Each entry specifies a tool name and arguments.
- **`scoring_targets`**: strings to look for in the LLM's answer. The scoring method determines how they're checked.
- **`scoring`**: one of `substring`, `all_of`, `set_match`, or `number_range`.

---

## Running both benchmarks

```bash
# One-time setup
cargo build --release
python3 -m venv .bench-venv
.bench-venv/bin/pip install tiktoken anthropic
export ANTHROPIC_API_KEY=sk-ant-...

# Token efficiency (no API key needed if tiktoken is installed)
./benchmark.sh

# Accuracy (requires API key, ~$0.50-1.00)
.bench-venv/bin/python3 accuracy_bench.py

# Benchmark other projects (token efficiency only)
./benchmark.sh ~/projects/fastapi ~/projects/tokio
```
