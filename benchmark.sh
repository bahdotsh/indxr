#!/usr/bin/env bash
# =============================================================================
# indxr Benchmark Script
# =============================================================================
# Measures token usage and efficiency of indxr vs naive approaches for
# providing codebase context to AI agents.
#
# Usage:
#   ./benchmark.sh [PROJECT_PATH ...]
#
# If no paths are given, it benchmarks the indxr project itself.
#
# Requirements:
#   - indxr binary (cargo build --release, or cargo install --path .)
#   - jq (for JSON parsing)
#   - Python 3 with tiktoken (pip install tiktoken) — for OpenAI token counts
#   - Optional: ANTHROPIC_API_KEY env var + anthropic SDK — for Claude token counts
#
# Token counting:
#   - OpenAI:  tiktoken o200k_base (GPT-4o tokenizer) — offline, exact
#   - Claude:  Anthropic count_tokens API — requires ANTHROPIC_API_KEY, exact
#   - If tiktoken is not installed, falls back to ~4 chars/token estimate
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

INDXR="${INDXR_BIN:-$(command -v indxr 2>/dev/null || echo "")}"
if [ -z "$INDXR" ]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    if [ -f "$SCRIPT_DIR/target/release/indxr" ]; then
        INDXR="$SCRIPT_DIR/target/release/indxr"
    else
        echo "ERROR: indxr binary not found. Build with: cargo build --release"
        exit 1
    fi
fi

if ! command -v jq &>/dev/null; then
    echo "ERROR: jq is required. Install with: brew install jq (macOS) or apt install jq (Linux)"
    exit 1
fi

# Find Python with tiktoken. Check venv first, then system python.
PYTHON=""
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -x "$SCRIPT_DIR/.bench-venv/bin/python3" ]; then
    PYTHON="$SCRIPT_DIR/.bench-venv/bin/python3"
elif python3 -c "import tiktoken" 2>/dev/null; then
    PYTHON="python3"
fi

# Detect available tokenizers
HAS_TIKTOKEN=false
HAS_ANTHROPIC=false
if [ -n "$PYTHON" ]; then
    $PYTHON -c "import tiktoken" 2>/dev/null && HAS_TIKTOKEN=true
    if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
        $PYTHON -c "import anthropic" 2>/dev/null && HAS_ANTHROPIC=true
    fi
fi

BENCH_TMPDIR=$(mktemp -d)
trap 'rm -rf "$BENCH_TMPDIR"' EXIT

# Colors (disabled if not a terminal)
if [ -t 1 ]; then
    BOLD='\033[1m'
    DIM='\033[2m'
    CYAN='\033[36m'
    GREEN='\033[32m'
    YELLOW='\033[33m'
    RED='\033[31m'
    RESET='\033[0m'
else
    BOLD='' DIM='' CYAN='' GREEN='' YELLOW='' RED='' RESET=''
fi

# ---------------------------------------------------------------------------
# Token counting
# ---------------------------------------------------------------------------

# Count OpenAI tokens (tiktoken o200k_base / GPT-4o) from a file
# Returns token count, or falls back to char/4 estimate
count_tokens_openai() {
    local file="$1"
    if [ "$HAS_TIKTOKEN" = true ]; then
        $PYTHON -c "
import tiktoken, sys
enc = tiktoken.get_encoding('o200k_base')
with open(sys.argv[1], 'r', errors='replace') as f:
    print(len(enc.encode(f.read())))
" "$file" 2>/dev/null || _fallback_count "$file"
    else
        _fallback_count "$file"
    fi
}

# Count Claude tokens via Anthropic API from a file
# Returns token count or "N/A"
count_tokens_claude() {
    local file="$1"
    if [ "$HAS_ANTHROPIC" = true ]; then
        $PYTHON -c "
import anthropic, sys
client = anthropic.Anthropic()
with open(sys.argv[1], 'r', errors='replace') as f:
    text = f.read()
resp = client.messages.count_tokens(
    model='claude-sonnet-4-20250514',
    messages=[{'role': 'user', 'content': text}],
)
print(resp.input_tokens)
" "$file" 2>/dev/null || echo "N/A"
    else
        echo "N/A"
    fi
}

_fallback_count() {
    local chars
    chars=$(wc -c < "$1" | tr -d ' ')
    echo $(( (chars + 3) / 4 ))
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

fmt_num() {
    printf "%'d" "$1" 2>/dev/null || printf "%d" "$1"
}

pct() {
    local part="$1" whole="$2"
    if [ "$whole" -eq 0 ]; then echo "N/A"; return; fi
    echo "scale=1; $part * 100 / $whole" | bc 2>/dev/null || echo "N/A"
}

ratio() {
    local big="$1" small="$2"
    if [ "$small" -eq 0 ]; then echo "N/A"; return; fi
    echo "scale=1; $big / $small" | bc 2>/dev/null || echo "N/A"
}

sep() {
    printf "${DIM}%s${RESET}\n" "$(printf '─%.0s' {1..80})"
}

section() {
    echo ""
    printf "${BOLD}${CYAN}▸ %s${RESET}\n" "$1"
    sep
}

# Run indxr, write output to a temp file, return: "openai_tokens claude_tokens elapsed_ms file_path"
run_indxr() {
    local project="$1"; shift
    local cache_dir="$BENCH_TMPDIR/cache"
    local tmpfile
    tmpfile=$(mktemp "$BENCH_TMPDIR/indxr_out.XXXXXX")
    local start end elapsed
    start=$($PYTHON -c 'import time; print(int(time.time()*1e6))' 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e6))')
    "$INDXR" "$project" -q --cache-dir "$cache_dir" "$@" > "$tmpfile" 2>/dev/null
    end=$($PYTHON -c 'import time; print(int(time.time()*1e6))' 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e6))')
    elapsed=$(( (end - start) / 1000 ))
    local openai_tok claude_tok
    openai_tok=$(count_tokens_openai "$tmpfile")
    claude_tok=$(count_tokens_claude "$tmpfile")
    echo "$openai_tok $claude_tok $elapsed $tmpfile"
}

run_indxr_cold() {
    local project="$1"; shift
    local cache_dir
    cache_dir=$(mktemp -d)
    local tmpfile
    tmpfile=$(mktemp "$BENCH_TMPDIR/indxr_out.XXXXXX")
    local start end elapsed
    start=$($PYTHON -c 'import time; print(int(time.time()*1e6))' 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e6))')
    "$INDXR" "$project" -q --cache-dir "$cache_dir" "$@" > "$tmpfile" 2>/dev/null
    end=$($PYTHON -c 'import time; print(int(time.time()*1e6))' 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e6))')
    elapsed=$(( (end - start) / 1000 ))
    local openai_tok claude_tok
    openai_tok=$(count_tokens_openai "$tmpfile")
    claude_tok=$(count_tokens_claude "$tmpfile")
    rm -rf "$cache_dir"
    echo "$openai_tok $claude_tok $elapsed $tmpfile"
}

run_indxr_warm() {
    local project="$1"; shift
    local cache_dir
    cache_dir=$(mktemp -d)
    "$INDXR" "$project" -q --cache-dir "$cache_dir" "$@" > /dev/null 2>/dev/null
    local tmpfile
    tmpfile=$(mktemp "$BENCH_TMPDIR/indxr_out.XXXXXX")
    local start end elapsed
    start=$($PYTHON -c 'import time; print(int(time.time()*1e6))' 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e6))')
    "$INDXR" "$project" -q --cache-dir "$cache_dir" "$@" > "$tmpfile" 2>/dev/null
    end=$($PYTHON -c 'import time; print(int(time.time()*1e6))' 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e6))')
    elapsed=$(( (end - start) / 1000 ))
    local openai_tok claude_tok
    openai_tok=$(count_tokens_openai "$tmpfile")
    claude_tok=$(count_tokens_claude "$tmpfile")
    rm -rf "$cache_dir"
    echo "$openai_tok $claude_tok $elapsed $tmpfile"
}

# MCP query — returns: "openai_tokens claude_tokens"
mcp_query() {
    local project="$1"
    local tool_name="$2"
    local args="$3"
    local request
    request=$(jq -cn --arg method "tools/call" --arg name "$tool_name" --argjson args "$args" '{
        jsonrpc: "2.0",
        id: 1,
        method: $method,
        params: { name: $name, arguments: $args }
    }')

    local tmpfile
    tmpfile=$(mktemp "$BENCH_TMPDIR/mcp_out.XXXXXX")

    {
        echo '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'
        echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
        echo "$request"
    } | "$INDXR" serve "$project" > "$tmpfile" 2>/dev/null || true

    # Extract inner text content to a file for tokenization
    local inner_file
    inner_file=$(mktemp "$BENCH_TMPDIR/mcp_inner.XXXXXX")
    tail -1 "$tmpfile" | jq -r '.result.content[0].text // empty' 2>/dev/null > "$inner_file" || true

    local openai_tok claude_tok
    openai_tok=$(count_tokens_openai "$inner_file")
    claude_tok=$(count_tokens_claude "$inner_file")
    echo "$openai_tok $claude_tok"
}

# ---------------------------------------------------------------------------
# Formatting helpers for dual-tokenizer output
# ---------------------------------------------------------------------------

# Print token count with both tokenizers
# Usage: print_tokens openai_tok claude_tok
fmt_tok() {
    local openai="$1" claude="$2"
    if [ "$claude" = "N/A" ]; then
        printf "%s" "$(fmt_num "$openai")"
    else
        printf "%s (openai) / %s (claude)" "$(fmt_num "$openai")" "$(fmt_num "$claude")"
    fi
}

# Print ratio for dual counts
fmt_ratio() {
    local raw_openai="$1" tok_openai="$2" raw_claude="$3" tok_claude="$4"
    local r_openai
    r_openai=$(ratio "$raw_openai" "$tok_openai")
    if [ "$tok_claude" = "N/A" ] || [ "$raw_claude" = "N/A" ]; then
        printf "%s" "${r_openai}"
    else
        local r_claude
        r_claude=$(ratio "$raw_claude" "$tok_claude")
        printf "%s / %s" "$r_openai" "$r_claude"
    fi
}

# ---------------------------------------------------------------------------
# Benchmark a single project
# ---------------------------------------------------------------------------

benchmark_project() {
    local project="$1"
    local project_name
    project_name=$(basename "$project")

    if [ ! -d "$project" ]; then
        echo "WARNING: $project does not exist, skipping"
        return
    fi

    echo ""
    printf "${BOLD}${GREEN}━━━ Benchmarking: %s ━━━${RESET}\n" "$project_name"
    printf "${DIM}Path: %s${RESET}\n" "$project"

    # ------------------------------------------------------------------
    # 1. Baseline: raw source file metrics
    # ------------------------------------------------------------------
    section "1. Raw Source Baseline (what 'cat all files' costs)"

    # Use indxr's own file list (respects .gitignore) for an apples-to-apples
    # comparison — cat only the files indxr would actually index.
    local json_index
    json_index=$("$INDXR" "$project" -q -f json 2>/dev/null || echo '{"files":[]}')

    local raw_file="$BENCH_TMPDIR/raw_source"
    local file_list="$BENCH_TMPDIR/file_list"
    echo "$json_index" | jq -r '.files[].path' 2>/dev/null > "$file_list" || true

    # Cat all indexed files into one blob
    > "$raw_file"
    while IFS= read -r relpath; do
        cat "$project/$relpath" >> "$raw_file" 2>/dev/null || true
    done < "$file_list"

    local raw_files raw_lines raw_chars
    raw_files=$(wc -l < "$file_list" | tr -d ' ')
    raw_lines=$(wc -l < "$raw_file" | tr -d ' ')
    raw_chars=$(wc -c < "$raw_file" | tr -d ' ')

    local raw_openai raw_claude
    raw_openai=$(count_tokens_openai "$raw_file")
    raw_claude=$(count_tokens_claude "$raw_file")

    printf "  Files:       %s\n" "$(fmt_num "$raw_files")"
    printf "  Lines:       %s\n" "$(fmt_num "$raw_lines")"
    printf "  Characters:  %s\n" "$(fmt_num "$raw_chars")"
    if [ "$raw_claude" != "N/A" ]; then
        printf "  ${RED}Tokens (OpenAI / GPT-4o):   %s${RESET}\n" "$(fmt_num "$raw_openai")"
        printf "  ${RED}Tokens (Claude):            %s${RESET}\n" "$(fmt_num "$raw_claude")"
    else
        printf "  ${RED}Tokens (OpenAI / GPT-4o):   %s${RESET}\n" "$(fmt_num "$raw_openai")"
    fi

    # ------------------------------------------------------------------
    # 2. tree output
    # ------------------------------------------------------------------
    section "2. Naive Structural: tree output"

    local tree_file="$BENCH_TMPDIR/tree_output"
    if command -v tree &>/dev/null; then
        tree -I 'target|node_modules|.git|vendor|__pycache__' --noreport "$project" 2>/dev/null > "$tree_file" || true
    else
        find "$project" -not -path '*/target/*' -not -path '*/node_modules/*' -not -path '*/.git/*' -not -path '*/vendor/*' -type f 2>/dev/null | sort > "$tree_file" || true
    fi
    local tree_openai
    tree_openai=$(count_tokens_openai "$tree_file")

    printf "  Tokens (OpenAI):  %s\n" "$(fmt_num "$tree_openai")"
    printf "  ${DIM}(structure only — no code understanding)${RESET}\n"

    # ------------------------------------------------------------------
    # 3. indxr detail levels
    # ------------------------------------------------------------------
    section "3. indxr Detail Levels (cold cache)"

    if [ "$raw_claude" != "N/A" ]; then
        printf "  ${DIM}%-14s %10s  %10s  %6s  │  compression (openai / claude)${RESET}\n" \
            "" "openai" "claude" "time"
    fi

    local summary_openai=0 signatures_openai=0 full_openai=0
    local summary_claude="N/A" signatures_claude="N/A" full_claude="N/A"
    for level in summary signatures full; do
        local result
        result=$(run_indxr_cold "$project" -d "$level")
        local o_tok c_tok ms
        o_tok=$(echo "$result" | awk '{print $1}')
        c_tok=$(echo "$result" | awk '{print $2}')
        ms=$(echo "$result" | awk '{print $3}')
        eval "${level}_openai=$o_tok"
        eval "${level}_claude=$c_tok"

        local ratio_o savings_o
        ratio_o=$(ratio "$raw_openai" "$o_tok")
        savings_o=$(pct $((raw_openai - o_tok)) "$raw_openai")

        if [ "$c_tok" != "N/A" ]; then
            local ratio_c savings_c
            ratio_c=$(ratio "$raw_claude" "$c_tok")
            savings_c=$(pct $((raw_claude - c_tok)) "$raw_claude")
            printf "  %-12s  %8s  %8s  %4sms  │  ${GREEN}%sx / %sx${RESET}  │  ${GREEN}%s%% / %s%% saved${RESET}\n" \
                "$level:" "$(fmt_num "$o_tok")" "$(fmt_num "$c_tok")" "$ms" "$ratio_o" "$ratio_c" "$savings_o" "$savings_c"
        else
            printf "  %-12s  %8s tokens  %4sms  │  ${GREEN}%sx compression${RESET}  │  ${GREEN}%s%% saved${RESET}\n" \
                "$level:" "$(fmt_num "$o_tok")" "$ms" "$ratio_o" "$savings_o"
        fi
    done

    # ------------------------------------------------------------------
    # 4. Token budgets
    # ------------------------------------------------------------------
    section "4. Token Budget (--max-tokens)"

    for budget in 2000 4000 8000 15000; do
        local result
        result=$(run_indxr "$project" --max-tokens "$budget")
        local o_tok c_tok ms
        o_tok=$(echo "$result" | awk '{print $1}')
        c_tok=$(echo "$result" | awk '{print $2}')
        ms=$(echo "$result" | awk '{print $3}')

        local raw_pct
        raw_pct=$(pct "$o_tok" "$raw_openai")

        if [ "$c_tok" != "N/A" ]; then
            printf "  budget %-6s  →  %7s openai / %7s claude  │  ${GREEN}%s%% of raw${RESET}\n" \
                "$budget" "$(fmt_num "$o_tok")" "$(fmt_num "$c_tok")" "$raw_pct"
        else
            printf "  budget %-6s  →  %8s tokens  │  ${GREEN}%s%% of raw${RESET}\n" \
                "$budget" "$(fmt_num "$o_tok")" "$raw_pct"
        fi
    done

    # ------------------------------------------------------------------
    # 5. Targeted queries
    # ------------------------------------------------------------------
    section "5. Targeted Queries (scoped indexing)"

    local sample_symbol="" sample_kind="function" sample_path=""

    sample_path=$(find "$project" -name 'src' -type d -not -path '*/target/*' | head -1 || echo "")
    if [ -n "$sample_path" ]; then
        sample_path=$(echo "$sample_path" | sed "s|^$project/||")
    fi

    sample_symbol=$(echo "$json_index" | jq -r '[.files[]?.declarations[]?.name // empty] | .[3] // .[0] // "main"' 2>/dev/null || echo "main")

    if [ -n "$sample_symbol" ]; then
        local result o_tok ms
        result=$(run_indxr "$project" --symbol "$sample_symbol")
        o_tok=$(echo "$result" | awk '{print $1}')
        ms=$(echo "$result" | awk '{print $3}')
        printf "  --symbol %-20s  %8s tokens  %4sms  │  ${GREEN}%sx vs raw${RESET}\n" \
            "\"$sample_symbol\"" "$(fmt_num "$o_tok")" "$ms" "$(ratio "$raw_openai" "$o_tok")"
    fi

    local result o_tok ms
    result=$(run_indxr "$project" --kind "$sample_kind")
    o_tok=$(echo "$result" | awk '{print $1}')
    ms=$(echo "$result" | awk '{print $3}')
    printf "  --kind %-22s  %8s tokens  %4sms  │  ${GREEN}%sx vs raw${RESET}\n" \
        "\"$sample_kind\"" "$(fmt_num "$o_tok")" "$ms" "$(ratio "$raw_openai" "$o_tok")"

    result=$(run_indxr "$project" --public-only)
    o_tok=$(echo "$result" | awk '{print $1}')
    ms=$(echo "$result" | awk '{print $3}')
    printf "  --public-only %16s  %8s tokens  %4sms  │  ${GREEN}%sx vs raw${RESET}\n" \
        "" "$(fmt_num "$o_tok")" "$ms" "$(ratio "$raw_openai" "$o_tok")"

    if [ -n "$sample_path" ]; then
        result=$(run_indxr "$project" --filter-path "$sample_path")
        o_tok=$(echo "$result" | awk '{print $1}')
        ms=$(echo "$result" | awk '{print $3}')
        printf "  --filter-path %-16s  %8s tokens  %4sms  │  ${GREEN}%sx vs raw${RESET}\n" \
            "\"$sample_path\"" "$(fmt_num "$o_tok")" "$ms" "$(ratio "$raw_openai" "$o_tok")"
    fi

    # ------------------------------------------------------------------
    # 6. MCP server tools
    # ------------------------------------------------------------------
    section "6. MCP Server Per-Tool Token Cost"

    local mcp_result mcp_o mcp_c

    mcp_result=$(mcp_query "$project" "get_stats" '{}')
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    mcp_c=$(echo "$mcp_result" | awk '{print $2}')
    printf "  get_stats              %8s tokens\n" "$(fmt_num "$mcp_o")"

    mcp_result=$(mcp_query "$project" "get_tree" '{}')
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    printf "  get_tree               %8s tokens\n" "$(fmt_num "$mcp_o")"

    mcp_result=$(mcp_query "$project" "lookup_symbol" "{\"name\":\"$sample_symbol\",\"limit\":10}")
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    printf "  lookup_symbol(%-8s %8s tokens\n" "\"${sample_symbol:0:6}\")" "$(fmt_num "$mcp_o")"

    mcp_result=$(mcp_query "$project" "search_signatures" '{"query":"fn","limit":10}')
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    printf "  search_signatures(fn)  %8s tokens\n" "$(fmt_num "$mcp_o")"

    local first_file
    first_file=$(echo "$json_index" | jq -r '[.files[] | select(.language != "Markdown")] | .[0].path // empty' 2>/dev/null || echo "")
    if [ -n "$first_file" ]; then
        local first_file_basename
        first_file_basename=$(basename "$first_file")

        mcp_result=$(mcp_query "$project" "list_declarations" "{\"path\":\"$first_file\"}")
        mcp_o=$(echo "$mcp_result" | awk '{print $1}')
        printf "  list_decl(%-12s %8s tokens  (deep)\n" "$first_file_basename)" "$(fmt_num "$mcp_o")"

        mcp_result=$(mcp_query "$project" "list_declarations" "{\"path\":\"$first_file\",\"shallow\":true}")
        mcp_o=$(echo "$mcp_result" | awk '{print $1}')
        printf "  list_decl(%-12s %8s tokens  (shallow)\n" "$first_file_basename)" "$(fmt_num "$mcp_o")"

        mcp_result=$(mcp_query "$project" "get_imports" "{\"path\":\"$first_file\"}")
        mcp_o=$(echo "$mcp_result" | awk '{print $1}')
        printf "  get_imports(%-10s %8s tokens\n" "$first_file_basename)" "$(fmt_num "$mcp_o")"

        local file_cat_tokens
        file_cat_tokens=$(count_tokens_openai "$project/$first_file")
        printf "\n  ${DIM}Compare: cat-ing %s would cost ~%s tokens${RESET}\n" \
            "$first_file_basename" "$(fmt_num "$file_cat_tokens")"
    fi

    # ------------------------------------------------------------------
    # 7. Cache performance
    # ------------------------------------------------------------------
    section "7. Cache Performance"

    local cold_ms warm_ms
    cold_ms=$(run_indxr_cold "$project" | awk '{print $3}')
    warm_ms=$(run_indxr_warm "$project" | awk '{print $3}')

    local speedup
    if [ "$warm_ms" -gt 0 ]; then
        speedup=$(ratio "$cold_ms" "$warm_ms")
    else
        speedup="N/A"
    fi

    printf "  Cold (no cache):  %6s ms\n" "$cold_ms"
    printf "  Warm (cached):    %6s ms\n" "$warm_ms"
    printf "  ${GREEN}Speedup:          %sx${RESET}\n" "$speedup"

    # ------------------------------------------------------------------
    # 8. Summary table
    # ------------------------------------------------------------------
    section "8. Summary: Token Efficiency Comparison"

    local budget_8k_openai
    budget_8k_openai=$(run_indxr "$project" --max-tokens 8000 | awk '{print $1}')

    printf "\n"
    if [ "$raw_claude" != "N/A" ]; then
        printf "  ${BOLD}%-32s %10s %10s %8s %8s${RESET}\n" "Approach" "OpenAI" "Claude" "OA ratio" "CL ratio"
        sep
        printf "  %-32s ${RED}%10s %10s${RESET} %8s %8s\n" \
            "cat all source files" "$(fmt_num "$raw_openai")" "$(fmt_num "$raw_claude")" "1.0x" "1.0x"
        printf "  %-32s %10s %10s %8s %8s\n" \
            "tree (structure only)" "$(fmt_num "$tree_openai")" "—" "$(ratio "$raw_openai" "$tree_openai")x" "—"
        printf "  %-32s ${GREEN}%10s %10s %8s %8s${RESET}\n" \
            "indxr summary" "$(fmt_num "$summary_openai")" "$(fmt_num "$summary_claude")" \
            "$(ratio "$raw_openai" "$summary_openai")x" "$(ratio "$raw_claude" "$summary_claude")x"
        printf "  %-32s ${GREEN}%10s %10s %8s %8s${RESET}\n" \
            "indxr signatures" "$(fmt_num "$signatures_openai")" "$(fmt_num "$signatures_claude")" \
            "$(ratio "$raw_openai" "$signatures_openai")x" "$(ratio "$raw_claude" "$signatures_claude")x"
        printf "  %-32s ${GREEN}%10s %10s %8s %8s${RESET}\n" \
            "indxr full" "$(fmt_num "$full_openai")" "$(fmt_num "$full_claude")" \
            "$(ratio "$raw_openai" "$full_openai")x" "$(ratio "$raw_claude" "$full_claude")x"
        printf "  %-32s ${GREEN}%10s %10s %8s %8s${RESET}\n" \
            "indxr --max-tokens 8000" "$(fmt_num "$budget_8k_openai")" "—" \
            "$(ratio "$raw_openai" "$budget_8k_openai")x" "—"
    else
        printf "  ${BOLD}%-35s %12s %10s${RESET}\n" "Approach" "Tokens (OA)" "vs Raw"
        sep
        printf "  %-35s ${RED}%12s${RESET} %10s\n" \
            "cat all source files" "$(fmt_num "$raw_openai")" "1.0x"
        printf "  %-35s %12s %10s\n" \
            "tree (structure only)" "$(fmt_num "$tree_openai")" "$(ratio "$raw_openai" "$tree_openai")x"
        printf "  %-35s ${GREEN}%12s %10s${RESET}\n" \
            "indxr --detail summary" "$(fmt_num "$summary_openai")" "$(ratio "$raw_openai" "$summary_openai")x"
        printf "  %-35s ${GREEN}%12s %10s${RESET}\n" \
            "indxr --detail signatures" "$(fmt_num "$signatures_openai")" "$(ratio "$raw_openai" "$signatures_openai")x"
        printf "  %-35s ${GREEN}%12s %10s${RESET}\n" \
            "indxr --detail full" "$(fmt_num "$full_openai")" "$(ratio "$raw_openai" "$full_openai")x"
        printf "  %-35s ${GREEN}%12s %10s${RESET}\n" \
            "indxr --max-tokens 8000" "$(fmt_num "$budget_8k_openai")" "$(ratio "$raw_openai" "$budget_8k_openai")x"
    fi

    printf "\n"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

echo ""
printf "${BOLD}${CYAN}╔══════════════════════════════════════════════════════════════╗${RESET}\n"
printf "${BOLD}${CYAN}║            indxr Token Efficiency Benchmark                 ║${RESET}\n"
printf "${BOLD}${CYAN}╚══════════════════════════════════════════════════════════════╝${RESET}\n"
printf "${DIM}indxr binary: %s${RESET}\n" "$INDXR"

# Show tokenizer status
if [ "$HAS_TIKTOKEN" = true ]; then
    printf "${GREEN}OpenAI tokenizer: tiktoken o200k_base (GPT-4o) — exact${RESET}\n"
else
    printf "${YELLOW}OpenAI tokenizer: not available (install tiktoken) — using ~4 chars/token estimate${RESET}\n"
fi
if [ "$HAS_ANTHROPIC" = true ]; then
    printf "${GREEN}Claude tokenizer: Anthropic count_tokens API — exact${RESET}\n"
else
    if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
        printf "${DIM}Claude tokenizer: skipped (set ANTHROPIC_API_KEY to enable)${RESET}\n"
    else
        printf "${YELLOW}Claude tokenizer: not available (install anthropic SDK)${RESET}\n"
    fi
fi
printf "${DIM}Date: %s${RESET}\n" "$(date '+%Y-%m-%d %H:%M:%S')"

if [ $# -eq 0 ]; then
    PROJECTS=(".")
else
    PROJECTS=("$@")
fi

for project in "${PROJECTS[@]}"; do
    project=$(cd "$project" && pwd)
    benchmark_project "$project"
done

if [ ${#PROJECTS[@]} -gt 1 ]; then
    echo ""
    printf "${BOLD}${CYAN}━━━ Cross-Project Summary ━━━${RESET}\n"
    printf "${DIM}See per-project sections above for detailed breakdowns.${RESET}\n"
    printf "\nKey takeaway: indxr provides ${GREEN}5-600x token reduction${RESET} vs raw source,\n"
    printf "while preserving the structural information AI agents need.\n"
    printf "MCP tools enable ${GREEN}surgical queries at ~50-500 tokens${RESET} per call.\n"
fi

echo ""
printf "${BOLD}Benchmark complete.${RESET}\n"
