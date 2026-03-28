#!/usr/bin/env bash
# =============================================================================
# indxr Benchmark Script
# =============================================================================
# Measures token usage and efficiency of indxr vs naive approaches for
# providing codebase context to AI agents.
#
# Usage:
#   ./benchmark.sh [OPTIONS] [PROJECT_PATH ...]
#
# Options:
#   --json [FILE]     Output machine-readable JSON results (to FILE or stdout)
#   --runs N, -n N    Number of timing runs per measurement (default: 10)
#   --help, -h        Show this help
#
# If no paths are given, it benchmarks the indxr project itself.
#
# Requirements:
#   - indxr binary (cargo build --release, or cargo install --path .)
#   - jq (for JSON parsing)
#   - Python 3 with tiktoken >= 0.7 (pip install tiktoken) — for OpenAI token counts
#   - Optional: ANTHROPIC_API_KEY env var + anthropic SDK >= 0.40 (pip install anthropic) — for Claude token counts
#
# Token counting:
#   - OpenAI:  tiktoken o200k_base (GPT-4o/GPT-4.1/GPT-5/o3/o4-mini) — offline, exact
#   - Claude:  Anthropic count_tokens API (claude-sonnet-4-6) — requires ANTHROPIC_API_KEY, exact
#   - If tiktoken is not installed, falls back to ~4 chars/token estimate
#
# Cache semantics:
#   "cold" = indxr application cache is empty (fresh temp dir).
#     NOTE: OS filesystem cache (page cache) is NOT purged.
#     To get true cold-storage timings on macOS: sudo purge
#     On Linux: sync && echo 3 | sudo tee /proc/sys/vm/drop_caches
#   "warm" = indxr cache is primed from a previous run.
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

BENCH_RUNS=${BENCH_RUNS:-10}
JSON_OUTPUT=false
JSON_FILE=""
PROJECTS=()

show_help() {
    cat <<'HELP'
indxr Benchmark Script

Measures token usage and efficiency of indxr vs naive approaches for
providing codebase context to AI agents.

Usage:
  ./benchmark.sh [OPTIONS] [PROJECT_PATH ...]

Options:
  --json [FILE]     Output machine-readable JSON results (to FILE or stdout)
  --runs N, -n N    Number of timing runs per measurement (default: 10)
  --help, -h        Show this help

If no paths are given, it benchmarks the indxr project itself.

Requirements:
  - indxr binary (cargo build --release, or cargo install --path .)
  - jq (for JSON parsing)
  - Python 3 with tiktoken >= 0.7 (pip install tiktoken) — for OpenAI token counts
  - Optional: ANTHROPIC_API_KEY + anthropic SDK >= 0.40 — for Claude token counts
HELP
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --json)
            JSON_OUTPUT=true
            if [[ -n "${2:-}" && "${2:-}" != --* ]]; then
                JSON_FILE="$2"
                shift
            fi
            shift
            ;;
        --runs|-n)
            BENCH_RUNS="$2"
            shift 2
            ;;
        --help|-h)
            show_help
            ;;
        *)
            PROJECTS+=("$1")
            shift
            ;;
    esac
done

if [ ${#PROJECTS[@]} -eq 0 ]; then
    PROJECTS=(".")
fi

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
elif command -v python3 &>/dev/null; then
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

# JSON accumulator
JSON_DATA_FILE="$BENCH_TMPDIR/json_results.json"
echo '{"projects":[]}' > "$JSON_DATA_FILE"

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
    model='claude-sonnet-4-6',
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
# Statistics helpers (Python-backed)
# ---------------------------------------------------------------------------

# Compute statistics from a list of numbers
# Input: space-separated numbers as arguments
# Output: JSON object with mean, median, stddev, min, max, p50, p95, outliers
compute_stats() {
    local numbers="$*"
    $PYTHON -c "
import statistics, json, sys, math
data = [float(x) for x in sys.argv[1].split()]
n = len(data)
if n == 0:
    print('{}')
    sys.exit(0)
data_sorted = sorted(data)
mean = statistics.mean(data)
median = statistics.median(data)
stdev = statistics.stdev(data) if n > 1 else 0.0
mn, mx = min(data), max(data)
# Percentiles (nearest-rank)
p50_idx = max(0, min(int(n * 0.50), n - 1))
p95_idx = max(0, min(int(n * 0.95), n - 1))
p50 = data_sorted[p50_idx]
p95 = data_sorted[p95_idx]
# Outlier detection: Tukey's 1.5x IQR fences
q1_idx = max(0, int(n * 0.25))
q3_idx = max(0, min(int(n * 0.75), n - 1))
q1 = data_sorted[q1_idx]
q3 = data_sorted[q3_idx]
iqr = q3 - q1
lower_fence = q1 - 1.5 * iqr
upper_fence = q3 + 1.5 * iqr
outlier_count = sum(1 for v in data if v < lower_fence or v > upper_fence)
print(json.dumps({
    'mean': round(mean, 2),
    'median': round(median, 2),
    'stdev': round(stdev, 2),
    'min': round(mn, 2),
    'max': round(mx, 2),
    'p50': round(p50, 2),
    'p95': round(p95, 2),
    'n': n,
    'outlier_count': outlier_count,
}))
" "$numbers"
}

# Format stats JSON into human-readable string
# Usage: fmt_stats "$stats_json" [unit]
fmt_stats() {
    local stats_json="$1"
    local unit="${2:-ms}"
    local mean median stdev mn mx outlier_count
    mean=$(echo "$stats_json" | jq -r '.mean')
    median=$(echo "$stats_json" | jq -r '.median')
    stdev=$(echo "$stats_json" | jq -r '.stdev')
    mn=$(echo "$stats_json" | jq -r '.min')
    mx=$(echo "$stats_json" | jq -r '.max')
    outlier_count=$(echo "$stats_json" | jq -r '.outlier_count')
    local out
    out=$(printf "%.1f +/- %.1f %s  (median: %.1f, range: %.1f-%.1f)" \
        "$mean" "$stdev" "$unit" "$median" "$mn" "$mx")
    if [ "$outlier_count" -gt 0 ] 2>/dev/null; then
        printf "%s  ${YELLOW}[%d outlier(s)]${RESET}" "$out" "$outlier_count"
    else
        printf "%s" "$out"
    fi
}

# Extract a field from stats JSON
stat_field() {
    echo "$1" | jq -r ".$2"
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

fmt_num() {
    printf "%'d" "$1" 2>/dev/null || printf "%d" "$1"
}

pct() {
    local part="$1" whole="$2"
    if [ "$whole" -eq 0 ] 2>/dev/null; then echo "N/A"; return; fi
    echo "scale=1; $part * 100 / $whole" | bc 2>/dev/null || echo "N/A"
}

ratio() {
    local big="$1" small="$2"
    if [ "$small" = "0" ] || [ "$small" = "0.0" ] || [ "$small" = "0.00" ]; then echo "N/A"; return; fi
    echo "scale=1; $big / $small" | bc 2>/dev/null || echo "N/A"
}

sep() {
    printf "${DIM}%s${RESET}\n" "$(printf '%.0s─' {1..80})"
}

section() {
    echo ""
    printf "${BOLD}${CYAN}▸ %s${RESET}\n" "$1"
    sep
}

# Print a progress dot during multi-run loops
progress_dot() {
    if [ -t 1 ]; then
        printf "${DIM}.${RESET}" >&2
    fi
}

progress_done() {
    if [ -t 1 ]; then
        printf "\r\033[K" >&2
    fi
}

# ---------------------------------------------------------------------------
# Multi-run timing infrastructure
# ---------------------------------------------------------------------------

# Low-level: run indxr once, echo "elapsed_ms tmpfile_path"
_time_indxr() {
    local project="$1"; shift
    local cache_dir="$1"; shift
    local tmpfile
    tmpfile=$(mktemp "$BENCH_TMPDIR/indxr_out.XXXXXX")
    local start end
    start=$($PYTHON -c 'import time; print(int(time.time()*1e6))')
    "$INDXR" "$project" -q --cache-dir "$cache_dir" "$@" > "$tmpfile" 2>/dev/null
    end=$($PYTHON -c 'import time; print(int(time.time()*1e6))')
    echo "$(( (end - start) / 1000 )) $tmpfile"
}

# Globals set by multi-run functions
MRUN_OPENAI_TOK=""
MRUN_CLAUDE_TOK=""
MRUN_STATS_JSON=""
MRUN_TMPFILE=""

# Multi-run with shared cache (for general use)
run_indxr_multi() {
    local project="$1"; shift
    local cache_dir="$BENCH_TMPDIR/cache"
    local times=()
    local last_tmpfile=""
    local i
    for ((i=0; i<BENCH_RUNS; i++)); do
        local result ms tmpfile
        result=$(_time_indxr "$project" "$cache_dir" "$@")
        ms=$(echo "$result" | awk '{print $1}')
        tmpfile=$(echo "$result" | awk '{print $2}')
        times+=("$ms")
        last_tmpfile="$tmpfile"
        progress_dot
    done
    progress_done
    MRUN_STATS_JSON=$(compute_stats "${times[@]}")
    MRUN_OPENAI_TOK=$(count_tokens_openai "$last_tmpfile")
    MRUN_CLAUDE_TOK=$(count_tokens_claude "$last_tmpfile")
    MRUN_TMPFILE="$last_tmpfile"
}

# Multi-run cold: each run uses a fresh cache dir
run_indxr_cold_multi() {
    local project="$1"; shift
    local times=()
    local last_tmpfile=""
    local i
    for ((i=0; i<BENCH_RUNS; i++)); do
        local cache_dir
        cache_dir=$(mktemp -d)
        local result ms tmpfile
        result=$(_time_indxr "$project" "$cache_dir" "$@")
        ms=$(echo "$result" | awk '{print $1}')
        tmpfile=$(echo "$result" | awk '{print $2}')
        times+=("$ms")
        last_tmpfile="$tmpfile"
        rm -rf "$cache_dir"
        progress_dot
    done
    progress_done
    MRUN_STATS_JSON=$(compute_stats "${times[@]}")
    MRUN_OPENAI_TOK=$(count_tokens_openai "$last_tmpfile")
    MRUN_CLAUDE_TOK=$(count_tokens_claude "$last_tmpfile")
    MRUN_TMPFILE="$last_tmpfile"
}

# Multi-run warm: prime cache once, then run N times
run_indxr_warm_multi() {
    local project="$1"; shift
    local cache_dir
    cache_dir=$(mktemp -d)
    # Prime the cache
    "$INDXR" "$project" -q --cache-dir "$cache_dir" "$@" > /dev/null 2>/dev/null
    local times=()
    local last_tmpfile=""
    local i
    for ((i=0; i<BENCH_RUNS; i++)); do
        local result ms tmpfile
        result=$(_time_indxr "$project" "$cache_dir" "$@")
        ms=$(echo "$result" | awk '{print $1}')
        tmpfile=$(echo "$result" | awk '{print $2}')
        times+=("$ms")
        last_tmpfile="$tmpfile"
        progress_dot
    done
    progress_done
    MRUN_STATS_JSON=$(compute_stats "${times[@]}")
    MRUN_OPENAI_TOK=$(count_tokens_openai "$last_tmpfile")
    MRUN_CLAUDE_TOK=$(count_tokens_claude "$last_tmpfile")
    MRUN_TMPFILE="$last_tmpfile"
    rm -rf "$cache_dir"
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
fmt_tok() {
    local openai="$1" claude="$2"
    if [ "$claude" = "N/A" ]; then
        printf "%s" "$(fmt_num "$openai")"
    else
        printf "%s (openai) / %s (claude)" "$(fmt_num "$openai")" "$(fmt_num "$claude")"
    fi
}

# ---------------------------------------------------------------------------
# Environment detection
# ---------------------------------------------------------------------------

detect_environment() {
    local os_info cpu_info ram_gb disk_info

    os_info="$(uname -srm)"

    # CPU
    if [[ "$(uname)" == "Darwin" ]]; then
        cpu_info="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo 'unknown')"
    else
        cpu_info="$(lscpu 2>/dev/null | grep 'Model name' | sed 's/.*: *//' || echo 'unknown')"
    fi

    # RAM
    if [[ "$(uname)" == "Darwin" ]]; then
        ram_gb=$(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 ))
    else
        ram_gb=$(( $(grep MemTotal /proc/meminfo 2>/dev/null | awk '{print $2}' || echo 0) / 1048576 ))
    fi

    # Disk
    if [[ "$(uname)" == "Darwin" ]]; then
        local is_ssd
        is_ssd=$(diskutil info / 2>/dev/null | grep 'Solid State' | awk '{print $NF}' || echo "unknown")
        if [ "$is_ssd" = "Yes" ]; then
            disk_info="SSD"
        elif [ "$is_ssd" = "No" ]; then
            disk_info="HDD"
        else
            disk_info="unknown"
        fi
    else
        disk_info="$(cat /sys/block/sda/queue/rotational 2>/dev/null | grep -q 0 && echo 'SSD' || echo 'HDD/unknown')"
    fi

    ENV_OS="$os_info"
    ENV_CPU="$cpu_info"
    ENV_RAM_GB="$ram_gb"
    ENV_DISK="$disk_info"
}

# Globals set by detect_environment
ENV_OS=""
ENV_CPU=""
ENV_RAM_GB=""
ENV_DISK=""

# ---------------------------------------------------------------------------
# JSON helpers
# ---------------------------------------------------------------------------

json_add_project() {
    local project_json="$1"
    local tmp
    tmp=$(mktemp "$BENCH_TMPDIR/json_tmp.XXXXXX")
    jq --argjson proj "$project_json" '.projects += [$proj]' "$JSON_DATA_FILE" > "$tmp"
    mv "$tmp" "$JSON_DATA_FILE"
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
    printf "${DIM}Runs per measurement: %d${RESET}\n" "$BENCH_RUNS"

    # JSON accumulator for this project
    local PJ='{}'
    PJ=$(echo "$PJ" | jq --arg name "$project_name" --arg path "$project" \
        '.name = $name | .path = $path')

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

    PJ=$(echo "$PJ" | jq \
        --argjson files "$raw_files" \
        --argjson lines "$raw_lines" \
        --argjson chars "$raw_chars" \
        --argjson raw_openai "$raw_openai" \
        --arg raw_claude "$raw_claude" \
        '.baseline = {files: $files, lines: $lines, chars: $chars, openai_tokens: $raw_openai, claude_tokens: $raw_claude}')

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

    PJ=$(echo "$PJ" | jq --argjson v "$tree_openai" '.tree_openai_tokens = $v')

    # ------------------------------------------------------------------
    # 3. indxr detail levels (multi-run, cold cache)
    # ------------------------------------------------------------------
    section "3. indxr Detail Levels (cold cache, N=$BENCH_RUNS runs)"

    if [ "$raw_claude" != "N/A" ]; then
        printf "  ${DIM}%-14s %10s  %10s  %-40s  │  compression${RESET}\n" \
            "" "openai" "claude" "latency (mean +/- stdev ms)"
    else
        printf "  ${DIM}%-14s %10s  %-40s  │  compression${RESET}\n" \
            "" "openai" "latency (mean +/- stdev ms)"
    fi

    local summary_openai=0 signatures_openai=0 full_openai=0
    local summary_claude="N/A" signatures_claude="N/A" full_claude="N/A"
    local PJ_LEVELS='{}'
    for level in summary signatures full; do
        run_indxr_cold_multi "$project" -d "$level"
        local o_tok=$MRUN_OPENAI_TOK
        local c_tok=$MRUN_CLAUDE_TOK
        local stats=$MRUN_STATS_JSON
        local mean_ms
        mean_ms=$(stat_field "$stats" "mean")
        eval "${level}_openai=$o_tok"
        eval "${level}_claude=$c_tok"

        local ratio_o savings_o
        ratio_o=$(ratio "$raw_openai" "$o_tok")
        savings_o=$(pct $((raw_openai - o_tok)) "$raw_openai")

        local stats_display
        stats_display=$(fmt_stats "$stats")

        if [ "$c_tok" != "N/A" ]; then
            local ratio_c savings_c
            ratio_c=$(ratio "$raw_claude" "$c_tok")
            savings_c=$(pct $((raw_claude - c_tok)) "$raw_claude")
            printf "  %-12s  %8s  %8s  %-40s  │  ${GREEN}%sx / %sx${RESET} (%s%% / %s%%)\n" \
                "$level:" "$(fmt_num "$o_tok")" "$(fmt_num "$c_tok")" "$stats_display" \
                "$ratio_o" "$ratio_c" "$savings_o" "$savings_c"
        else
            printf "  %-12s  %8s  %-40s  │  ${GREEN}%sx${RESET} (%s%% saved)\n" \
                "$level:" "$(fmt_num "$o_tok")" "$stats_display" "$ratio_o" "$savings_o"
        fi

        PJ_LEVELS=$(echo "$PJ_LEVELS" | jq \
            --arg level "$level" \
            --argjson openai "$o_tok" \
            --arg claude "$c_tok" \
            --argjson stats "$stats" \
            --arg ratio "$ratio_o" \
            '.[$level] = {openai_tokens: $openai, claude_tokens: $claude, timing: $stats, compression_ratio: $ratio}')
    done
    PJ=$(echo "$PJ" | jq --argjson v "$PJ_LEVELS" '.detail_levels = $v')

    # ------------------------------------------------------------------
    # 4. Token budgets
    # ------------------------------------------------------------------
    section "4. Token Budget (--max-tokens)"

    local PJ_BUDGETS='[]'
    for budget in 2000 4000 8000 15000; do
        run_indxr_multi "$project" --max-tokens "$budget"
        local o_tok=$MRUN_OPENAI_TOK
        local c_tok=$MRUN_CLAUDE_TOK
        local stats=$MRUN_STATS_JSON

        local raw_pct
        raw_pct=$(pct "$o_tok" "$raw_openai")

        if [ "$c_tok" != "N/A" ]; then
            printf "  budget %-6s  →  %7s openai / %7s claude  │  ${GREEN}%s%% of raw${RESET}\n" \
                "$budget" "$(fmt_num "$o_tok")" "$(fmt_num "$c_tok")" "$raw_pct"
        else
            printf "  budget %-6s  →  %8s tokens  │  ${GREEN}%s%% of raw${RESET}\n" \
                "$budget" "$(fmt_num "$o_tok")" "$raw_pct"
        fi

        PJ_BUDGETS=$(echo "$PJ_BUDGETS" | jq \
            --argjson budget "$budget" \
            --argjson openai "$o_tok" \
            --arg claude "$c_tok" \
            --arg pct_of_raw "$raw_pct" \
            '. += [{budget: $budget, openai_tokens: $openai, claude_tokens: $claude, pct_of_raw: $pct_of_raw}]')
    done
    PJ=$(echo "$PJ" | jq --argjson v "$PJ_BUDGETS" '.token_budgets = $v')

    # ------------------------------------------------------------------
    # 5. Targeted queries (multi-run)
    # ------------------------------------------------------------------
    section "5. Targeted Queries (scoped indexing, N=$BENCH_RUNS runs)"

    local sample_symbol="" sample_kind="function" sample_path=""

    sample_path=$(find "$project" -name 'src' -type d -not -path '*/target/*' | head -1 || echo "")
    if [ -n "$sample_path" ]; then
        sample_path=$(echo "$sample_path" | sed "s|^$project/||")
    fi

    sample_symbol=$(echo "$json_index" | jq -r '[.files[]?.declarations[]?.name // empty] | .[3] // .[0] // "main"' 2>/dev/null || echo "main")

    local PJ_QUERIES='[]'

    if [ -n "$sample_symbol" ]; then
        run_indxr_multi "$project" --symbol "$sample_symbol"
        local o_tok=$MRUN_OPENAI_TOK
        local stats=$MRUN_STATS_JSON
        local mean_ms
        mean_ms=$(stat_field "$stats" "mean")
        local stats_display
        stats_display=$(fmt_stats "$stats")
        printf "  --symbol %-20s  %8s tokens  %s  │  ${GREEN}%sx vs raw${RESET}\n" \
            "\"$sample_symbol\"" "$(fmt_num "$o_tok")" "$stats_display" "$(ratio "$raw_openai" "$o_tok")"
        PJ_QUERIES=$(echo "$PJ_QUERIES" | jq \
            --arg query "--symbol $sample_symbol" \
            --argjson openai "$o_tok" \
            --argjson stats "$stats" \
            '. += [{query: $query, openai_tokens: $openai, timing: $stats}]')
    fi

    run_indxr_multi "$project" --kind "$sample_kind"
    local o_tok=$MRUN_OPENAI_TOK
    local stats=$MRUN_STATS_JSON
    local stats_display
    stats_display=$(fmt_stats "$stats")
    printf "  --kind %-22s  %8s tokens  %s  │  ${GREEN}%sx vs raw${RESET}\n" \
        "\"$sample_kind\"" "$(fmt_num "$o_tok")" "$stats_display" "$(ratio "$raw_openai" "$o_tok")"
    PJ_QUERIES=$(echo "$PJ_QUERIES" | jq \
        --arg query "--kind $sample_kind" \
        --argjson openai "$o_tok" \
        --argjson stats "$stats" \
        '. += [{query: $query, openai_tokens: $openai, timing: $stats}]')

    run_indxr_multi "$project" --public-only
    o_tok=$MRUN_OPENAI_TOK
    stats=$MRUN_STATS_JSON
    stats_display=$(fmt_stats "$stats")
    printf "  --public-only %16s  %8s tokens  %s  │  ${GREEN}%sx vs raw${RESET}\n" \
        "" "$(fmt_num "$o_tok")" "$stats_display" "$(ratio "$raw_openai" "$o_tok")"
    PJ_QUERIES=$(echo "$PJ_QUERIES" | jq \
        --arg query "--public-only" \
        --argjson openai "$o_tok" \
        --argjson stats "$stats" \
        '. += [{query: $query, openai_tokens: $openai, timing: $stats}]')

    if [ -n "$sample_path" ]; then
        run_indxr_multi "$project" --filter-path "$sample_path"
        o_tok=$MRUN_OPENAI_TOK
        stats=$MRUN_STATS_JSON
        stats_display=$(fmt_stats "$stats")
        printf "  --filter-path %-16s  %8s tokens  %s  │  ${GREEN}%sx vs raw${RESET}\n" \
            "\"$sample_path\"" "$(fmt_num "$o_tok")" "$stats_display" "$(ratio "$raw_openai" "$o_tok")"
        PJ_QUERIES=$(echo "$PJ_QUERIES" | jq \
            --arg query "--filter-path $sample_path" \
            --argjson openai "$o_tok" \
            --argjson stats "$stats" \
            '. += [{query: $query, openai_tokens: $openai, timing: $stats}]')
    fi

    PJ=$(echo "$PJ" | jq --argjson v "$PJ_QUERIES" '.targeted_queries = $v')

    # ------------------------------------------------------------------
    # 6. MCP server tools
    # ------------------------------------------------------------------
    section "6. MCP Server Per-Tool Token Cost"

    local mcp_result mcp_o mcp_c
    local PJ_MCP='[]'

    mcp_result=$(mcp_query "$project" "get_stats" '{}')
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    mcp_c=$(echo "$mcp_result" | awk '{print $2}')
    printf "  get_stats              %8s tokens\n" "$(fmt_num "$mcp_o")"
    PJ_MCP=$(echo "$PJ_MCP" | jq --arg tool "get_stats" --argjson tok "$mcp_o" '. += [{tool: $tool, openai_tokens: $tok}]')

    mcp_result=$(mcp_query "$project" "get_tree" '{}')
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    printf "  get_tree               %8s tokens\n" "$(fmt_num "$mcp_o")"
    PJ_MCP=$(echo "$PJ_MCP" | jq --arg tool "get_tree" --argjson tok "$mcp_o" '. += [{tool: $tool, openai_tokens: $tok}]')

    mcp_result=$(mcp_query "$project" "lookup_symbol" "{\"name\":\"$sample_symbol\",\"limit\":10}")
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    printf "  lookup_symbol(%-8s %8s tokens\n" "\"${sample_symbol:0:6}\")" "$(fmt_num "$mcp_o")"
    PJ_MCP=$(echo "$PJ_MCP" | jq --arg tool "lookup_symbol" --argjson tok "$mcp_o" '. += [{tool: $tool, openai_tokens: $tok}]')

    mcp_result=$(mcp_query "$project" "search_signatures" '{"query":"fn","limit":10}')
    mcp_o=$(echo "$mcp_result" | awk '{print $1}')
    printf "  search_signatures(fn)  %8s tokens\n" "$(fmt_num "$mcp_o")"
    PJ_MCP=$(echo "$PJ_MCP" | jq --arg tool "search_signatures" --argjson tok "$mcp_o" '. += [{tool: $tool, openai_tokens: $tok}]')

    local first_file
    first_file=$(echo "$json_index" | jq -r '[.files[] | select(.language != "Markdown")] | .[0].path // empty' 2>/dev/null || echo "")
    if [ -n "$first_file" ]; then
        local first_file_basename
        first_file_basename=$(basename "$first_file")

        mcp_result=$(mcp_query "$project" "list_declarations" "{\"path\":\"$first_file\"}")
        mcp_o=$(echo "$mcp_result" | awk '{print $1}')
        printf "  list_decl(%-12s %8s tokens  (deep)\n" "$first_file_basename)" "$(fmt_num "$mcp_o")"
        PJ_MCP=$(echo "$PJ_MCP" | jq --arg tool "list_declarations(deep)" --argjson tok "$mcp_o" '. += [{tool: $tool, openai_tokens: $tok}]')

        mcp_result=$(mcp_query "$project" "list_declarations" "{\"path\":\"$first_file\",\"shallow\":true}")
        mcp_o=$(echo "$mcp_result" | awk '{print $1}')
        printf "  list_decl(%-12s %8s tokens  (shallow)\n" "$first_file_basename)" "$(fmt_num "$mcp_o")"
        PJ_MCP=$(echo "$PJ_MCP" | jq --arg tool "list_declarations(shallow)" --argjson tok "$mcp_o" '. += [{tool: $tool, openai_tokens: $tok}]')

        mcp_result=$(mcp_query "$project" "get_imports" "{\"path\":\"$first_file\"}")
        mcp_o=$(echo "$mcp_result" | awk '{print $1}')
        printf "  get_imports(%-10s %8s tokens\n" "$first_file_basename)" "$(fmt_num "$mcp_o")"
        PJ_MCP=$(echo "$PJ_MCP" | jq --arg tool "get_imports" --argjson tok "$mcp_o" '. += [{tool: $tool, openai_tokens: $tok}]')

        local file_cat_tokens
        file_cat_tokens=$(count_tokens_openai "$project/$first_file")
        printf "\n  ${DIM}Compare: cat-ing %s would cost ~%s tokens${RESET}\n" \
            "$first_file_basename" "$(fmt_num "$file_cat_tokens")"
    fi

    PJ=$(echo "$PJ" | jq --argjson v "$PJ_MCP" '.mcp_tools = $v')

    # ------------------------------------------------------------------
    # 7. Cache performance (multi-run with full statistics)
    # ------------------------------------------------------------------
    section "7. Cache Performance (N=$BENCH_RUNS runs per mode)"

    run_indxr_cold_multi "$project"
    local cold_stats=$MRUN_STATS_JSON

    run_indxr_warm_multi "$project"
    local warm_stats=$MRUN_STATS_JSON

    local cold_mean warm_mean cold_median warm_median cold_p95 warm_p95
    cold_mean=$(stat_field "$cold_stats" "mean")
    warm_mean=$(stat_field "$warm_stats" "mean")
    cold_median=$(stat_field "$cold_stats" "median")
    warm_median=$(stat_field "$warm_stats" "median")
    cold_p95=$(stat_field "$cold_stats" "p95")
    warm_p95=$(stat_field "$warm_stats" "p95")

    local speedup_mean speedup_median
    speedup_mean=$(ratio "$cold_mean" "$warm_mean")
    speedup_median=$(ratio "$cold_median" "$warm_median")

    printf "  Cold (no cache):  %s\n" "$(fmt_stats "$cold_stats")"
    printf "  Warm (cached):    %s\n" "$(fmt_stats "$warm_stats")"
    echo ""
    printf "  ${BOLD}Percentiles:${RESET}\n"
    printf "    %-8s  %8s  %8s  %8s\n" "" "median" "p95" "mean"
    printf "    %-8s  %6s ms  %6s ms  %6s ms\n" "cold:" "$cold_median" "$cold_p95" "$cold_mean"
    printf "    %-8s  %6s ms  %6s ms  %6s ms\n" "warm:" "$warm_median" "$warm_p95" "$warm_mean"
    echo ""
    printf "  ${GREEN}Speedup (mean):   %sx${RESET}\n" "$speedup_mean"
    printf "  ${GREEN}Speedup (median): %sx${RESET}\n" "$speedup_median"
    echo ""
    printf "  ${DIM}Note: 'cold' = indxr cache cold; OS page cache still warm.${RESET}\n"
    printf "  ${DIM}For true cold-storage timing, run: sudo purge (macOS)${RESET}\n"

    PJ=$(echo "$PJ" | jq \
        --argjson cold "$cold_stats" \
        --argjson warm "$warm_stats" \
        --arg speedup_mean "$speedup_mean" \
        --arg speedup_median "$speedup_median" \
        '.cache_performance = {cold: $cold, warm: $warm, speedup_mean: $speedup_mean, speedup_median: $speedup_median}')

    # ------------------------------------------------------------------
    # 8. Summary table
    # ------------------------------------------------------------------
    section "8. Summary: Token Efficiency Comparison"

    local budget_8k_openai
    run_indxr_multi "$project" --max-tokens 8000
    budget_8k_openai=$MRUN_OPENAI_TOK

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

    # Save project JSON
    json_add_project "$PJ"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

detect_environment

echo ""
printf "${BOLD}${CYAN}╔══════════════════════════════════════════════════════════════╗${RESET}\n"
printf "${BOLD}${CYAN}║            indxr Token Efficiency Benchmark                 ║${RESET}\n"
printf "${BOLD}${CYAN}╚══════════════════════════════════════════════════════════════╝${RESET}\n"
printf "${DIM}indxr binary: %s${RESET}\n" "$INDXR"

# Show tokenizer status
if [ "$HAS_TIKTOKEN" = true ]; then
    printf "${GREEN}OpenAI tokenizer: tiktoken o200k_base (GPT-4o/4.1/5/o3/o4-mini) — exact${RESET}\n"
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

# Environment
section "Environment"
printf "  OS:       %s\n" "$ENV_OS"
printf "  CPU:      %s\n" "$ENV_CPU"
printf "  RAM:      %s GB\n" "$ENV_RAM_GB"
printf "  Disk:     %s\n" "$ENV_DISK"
printf "  Runs:     %d per measurement\n" "$BENCH_RUNS"
printf "${DIM}  Date:     %s${RESET}\n" "$(date '+%Y-%m-%d %H:%M:%S')"

for project in "${PROJECTS[@]}"; do
    project=$(cd "$project" && pwd)
    benchmark_project "$project"
done

if [ ${#PROJECTS[@]} -gt 1 ]; then
    echo ""
    printf "${BOLD}${CYAN}━━━ Cross-Project Aggregate Summary ━━━${RESET}\n"
    sep

    $PYTHON -c "
import json, statistics, sys
with open('$JSON_DATA_FILE') as f:
    data = json.load(f)
projects = data['projects']
n = len(projects)
print(f'  Projects benchmarked: {n}')
print()

# Compression ratios per detail level
for level in ['summary', 'signatures', 'full']:
    ratios = []
    for p in projects:
        r = p.get('detail_levels', {}).get(level, {}).get('compression_ratio', '')
        if r and r != 'N/A':
            try:
                ratios.append(float(r))
            except ValueError:
                pass
    if ratios:
        avg = statistics.mean(ratios)
        lo, hi = min(ratios), max(ratios)
        if n > 1:
            print(f'  {level:12s}  avg compression: {avg:.1f}x  (range: {lo:.1f}x - {hi:.1f}x)')
        else:
            print(f'  {level:12s}  compression: {avg:.1f}x')

print()

# Cache performance
cold_means = [p.get('cache_performance', {}).get('cold', {}).get('mean', 0) for p in projects]
warm_means = [p.get('cache_performance', {}).get('warm', {}).get('mean', 0) for p in projects]
cold_means = [x for x in cold_means if x > 0]
warm_means = [x for x in warm_means if x > 0]
if cold_means:
    print(f'  Avg cold time:  {statistics.mean(cold_means):.1f} ms')
if warm_means:
    print(f'  Avg warm time:  {statistics.mean(warm_means):.1f} ms')
if cold_means and warm_means:
    print(f'  Avg speedup:    {statistics.mean(cold_means) / statistics.mean(warm_means):.1f}x')
print()
"
fi

echo ""
printf "${BOLD}Benchmark complete.${RESET}\n"

# ---------------------------------------------------------------------------
# JSON export
# ---------------------------------------------------------------------------

if [ "$JSON_OUTPUT" = true ]; then
    local_json=$(jq \
        --arg os "$ENV_OS" \
        --arg cpu "$ENV_CPU" \
        --argjson ram "$ENV_RAM_GB" \
        --arg disk "$ENV_DISK" \
        --arg date "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
        --argjson runs "$BENCH_RUNS" \
        --arg indxr_bin "$INDXR" \
        --argjson has_tiktoken "$([ "$HAS_TIKTOKEN" = true ] && echo true || echo false)" \
        --argjson has_anthropic "$([ "$HAS_ANTHROPIC" = true ] && echo true || echo false)" \
        '. + {
            environment: {os: $os, cpu: $cpu, ram_gb: $ram, disk: $disk},
            metadata: {date: $date, runs_per_measurement: $runs, indxr_binary: $indxr_bin,
                       has_tiktoken: $has_tiktoken, has_anthropic: $has_anthropic}
        }' "$JSON_DATA_FILE")

    if [ -n "$JSON_FILE" ]; then
        echo "$local_json" | jq . > "$JSON_FILE"
        printf "${DIM}JSON results written to: %s${RESET}\n" "$JSON_FILE"
    else
        echo ""
        echo "$local_json" | jq .
    fi
fi

echo ""
