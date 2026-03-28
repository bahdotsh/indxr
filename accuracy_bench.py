#!/usr/bin/env python3
"""
indxr Accuracy Benchmark
=========================
Measures LLM answer quality with full-file context vs indxr structural context.

Proves: same or better accuracy with 5-20x fewer tokens.

Usage:
    python accuracy_bench.py                      # run all questions
    python accuracy_bench.py --dry-run             # show contexts, don't call API
    python accuracy_bench.py --filter symbol_lookup # run one category
    python accuracy_bench.py --repo /path/to/project
    python accuracy_bench.py --output results.json

Requirements:
    - ANTHROPIC_API_KEY env var
    - anthropic SDK (pip install anthropic)
    - indxr binary on PATH or INDXR_BIN env var
"""

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path


# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

DEFAULT_MODEL = "claude-sonnet-4-6"
DEFAULT_QUESTIONS = "accuracy_questions.json"
DEFAULT_OUTPUT = "benchmark_results.json"
API_DELAY = 0.5  # seconds between API calls to avoid rate limiting

SYSTEM_PROMPT = (
    "You are answering a question about a codebase. "
    "Answer concisely and precisely. Be specific — include file paths, "
    "function names, types, and other concrete details. "
    "Do not hedge or speculate. If the provided context does not contain "
    "enough information to answer, say so."
)


# ---------------------------------------------------------------------------
# indxr MCP client
# ---------------------------------------------------------------------------

class IndxrMCP:
    """Communicate with indxr MCP server via subprocess stdin/stdout."""

    def __init__(self, project_path: str):
        indxr_bin = os.environ.get("INDXR_BIN") or _find_indxr()
        self.proc = subprocess.Popen(
            [indxr_bin, "serve", project_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        self._msg_id = 0
        self._initialize()

    def _initialize(self):
        self._send({"jsonrpc": "2.0", "id": self._next_id(), "method": "initialize", "params": {}})
        self._read_response()  # read initialize result
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _next_id(self) -> int:
        self._msg_id += 1
        return self._msg_id

    def _send(self, msg: dict):
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def _read_response(self) -> dict:
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("indxr MCP server closed unexpectedly")
            line = line.strip()
            if not line:
                continue
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue

    def call_tool(self, name: str, arguments: dict) -> str:
        """Call an MCP tool and return the text content."""
        msg_id = self._next_id()
        self._send({
            "jsonrpc": "2.0",
            "id": msg_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        })
        resp = self._read_response()
        # Extract text from content array
        content = resp.get("result", {}).get("content", [])
        texts = [c.get("text", "") for c in content if c.get("type") == "text"]
        return "\n".join(texts)

    def close(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            self.proc.wait(timeout=5)


def _find_indxr() -> str:
    """Find the indxr binary."""
    # Check common locations
    for candidate in [
        Path(__file__).parent / "target" / "release" / "indxr",
        Path(__file__).parent / "target" / "debug" / "indxr",
    ]:
        if candidate.exists():
            return str(candidate)
    # Fall back to PATH
    result = subprocess.run(["which", "indxr"], capture_output=True, text=True)
    if result.returncode == 0:
        return result.stdout.strip()
    print("ERROR: indxr binary not found. Build with: cargo build --release")
    sys.exit(1)


# ---------------------------------------------------------------------------
# Context assembly
# ---------------------------------------------------------------------------

def build_baseline_context(question: dict, repo_path: Path) -> str:
    """Build full-file-read context for baseline condition."""
    parts = ["Here are the relevant source files:\n"]
    for rel_path in question["baseline_files"]:
        full_path = repo_path / rel_path
        if full_path.exists():
            content = full_path.read_text(errors="replace")
            parts.append(f"=== File: {rel_path} ===\n{content}\n")
        else:
            parts.append(f"=== File: {rel_path} ===\n[file not found]\n")
    return "\n".join(parts)


def build_indxr_context(question: dict, mcp: IndxrMCP) -> str:
    """Build indxr structural context by executing tool calls."""
    parts = ["Here is structural information from a codebase indexer:\n"]
    for tc in question["indxr_tool_calls"]:
        tool_name = tc["tool"]
        tool_args = tc.get("args", {})
        try:
            result = mcp.call_tool(tool_name, tool_args)
            args_str = ", ".join(f'{k}="{v}"' for k, v in tool_args.items())
            parts.append(f"=== {tool_name}({args_str}) ===\n{result}\n")
        except Exception as e:
            parts.append(f"=== {tool_name} ===\n[error: {e}]\n")
    return "\n".join(parts)


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

def score_answer(question: dict, answer: str) -> float:
    """Score an answer against ground truth. Returns 0.0-1.0."""
    method = question["scoring"]
    targets = question["scoring_targets"]
    answer_lower = answer.lower()

    if method == "substring":
        # Any target found = 1.0
        return 1.0 if any(t.lower() in answer_lower for t in targets) else 0.0

    elif method == "all_of":
        # All targets must be found
        found = sum(1 for t in targets if t.lower() in answer_lower)
        return found / len(targets)

    elif method == "set_match":
        # Fraction of targets found (partial credit)
        found = sum(1 for t in targets if t.lower() in answer_lower)
        return found / len(targets)

    elif method == "number_range":
        # Extract numbers from answer, check if any fall in range
        import re
        nums = [int(n) for n in re.findall(r'\b(\d+)\b', answer)]
        lo, hi = int(targets[0]), int(targets[1])
        return 1.0 if any(lo <= n <= hi for n in nums) else 0.0

    else:
        print(f"  WARNING: Unknown scoring method '{method}', defaulting to 0.0")
        return 0.0


# ---------------------------------------------------------------------------
# Claude API
# ---------------------------------------------------------------------------

def ask_claude(client, model: str, context: str, question: str) -> tuple:
    """Ask Claude a question with given context. Returns (answer, usage_dict)."""
    response = client.messages.create(
        model=model,
        max_tokens=1024,
        temperature=0,
        system=SYSTEM_PROMPT,
        messages=[{
            "role": "user",
            "content": f"{context}\n\nQuestion: {question}",
        }],
    )
    answer = response.content[0].text
    usage = {
        "input_tokens": response.usage.input_tokens,
        "output_tokens": response.usage.output_tokens,
    }
    return answer, usage


# ---------------------------------------------------------------------------
# Result types
# ---------------------------------------------------------------------------

@dataclass
class QuestionResult:
    id: str
    category: str
    difficulty: str
    question: str
    baseline_score: float = 0.0
    indxr_score: float = 0.0
    baseline_input_tokens: int = 0
    indxr_input_tokens: int = 0
    baseline_output_tokens: int = 0
    indxr_output_tokens: int = 0
    baseline_answer: str = ""
    indxr_answer: str = ""
    token_reduction: float = 0.0


# ---------------------------------------------------------------------------
# Output formatting
# ---------------------------------------------------------------------------

def print_results(results: list, model: str):
    """Print a formatted results table to stdout."""
    print()
    print("=" * 90)
    print("indxr Accuracy Benchmark Results")
    print("=" * 90)
    print(f"Model:     {model}")
    print(f"Questions: {len(results)}")
    print(f"Date:      {time.strftime('%Y-%m-%d %H:%M:%S')}")
    print()

    # Header
    print(f"{'ID':<12} {'Category':<20} {'BL Score':>8} {'IX Score':>8} {'BL Toks':>8} {'IX Toks':>8} {'Reduction':>10}")
    print("-" * 90)

    total_bl_score = 0.0
    total_ix_score = 0.0
    total_bl_tokens = 0
    total_ix_tokens = 0

    for r in results:
        reduction_str = f"{r.token_reduction:.1f}x" if r.token_reduction > 0 else "N/A"
        print(
            f"{r.id:<12} {r.category:<20} {r.baseline_score:>8.2f} {r.indxr_score:>8.2f} "
            f"{r.baseline_input_tokens:>8,} {r.indxr_input_tokens:>8,} {reduction_str:>10}"
        )
        total_bl_score += r.baseline_score
        total_ix_score += r.indxr_score
        total_bl_tokens += r.baseline_input_tokens
        total_ix_tokens += r.indxr_input_tokens

    print("-" * 90)

    n = len(results)
    avg_bl = total_bl_score / n if n else 0
    avg_ix = total_ix_score / n if n else 0
    avg_reduction = total_bl_tokens / total_ix_tokens if total_ix_tokens else 0
    delta = avg_ix - avg_bl

    print()
    print("Summary:")
    print(f"  Baseline accuracy:    {avg_bl:.2f}  ({int(total_bl_score)}/{n} questions)")
    print(f"  indxr accuracy:       {avg_ix:.2f}  ({int(total_ix_score)}/{n} questions)")
    print(f"  Accuracy delta:       {'+' if delta >= 0 else ''}{delta * 100:.1f}%")
    print(f"  Total baseline tokens: {total_bl_tokens:,}")
    print(f"  Total indxr tokens:    {total_ix_tokens:,}")
    print(f"  Avg token reduction:   {avg_reduction:.1f}x")
    print()

    if avg_ix >= avg_bl:
        print(f"  => indxr achieves {'better' if avg_ix > avg_bl else 'equal'} accuracy with {avg_reduction:.1f}x fewer tokens.")
    else:
        print(f"  => indxr uses {avg_reduction:.1f}x fewer tokens with {abs(delta) * 100:.1f}% accuracy tradeoff.")

    # Per-category breakdown
    categories = sorted(set(r.category for r in results))
    if len(categories) > 1:
        print()
        print("Per-category breakdown:")
        print(f"  {'Category':<20} {'BL Acc':>8} {'IX Acc':>8} {'Avg Reduction':>14}")
        print("  " + "-" * 56)
        for cat in categories:
            cat_results = [r for r in results if r.category == cat]
            cat_bl = sum(r.baseline_score for r in cat_results) / len(cat_results)
            cat_ix = sum(r.indxr_score for r in cat_results) / len(cat_results)
            cat_bl_tok = sum(r.baseline_input_tokens for r in cat_results)
            cat_ix_tok = sum(r.indxr_input_tokens for r in cat_results)
            cat_red = cat_bl_tok / cat_ix_tok if cat_ix_tok else 0
            print(f"  {cat:<20} {cat_bl:>8.2f} {cat_ix:>8.2f} {cat_red:>13.1f}x")

    print()


def write_json_results(results: list, metadata: dict, output_path: str):
    """Write results to a JSON file for programmatic consumption."""
    n = len(results)
    total_bl = sum(r.baseline_score for r in results)
    total_ix = sum(r.indxr_score for r in results)
    total_bl_tok = sum(r.baseline_input_tokens for r in results)
    total_ix_tok = sum(r.indxr_input_tokens for r in results)

    output = {
        "metadata": metadata,
        "results": [asdict(r) for r in results],
        "summary": {
            "question_count": n,
            "baseline_accuracy": total_bl / n if n else 0,
            "indxr_accuracy": total_ix / n if n else 0,
            "accuracy_delta": (total_ix - total_bl) / n if n else 0,
            "total_baseline_tokens": total_bl_tok,
            "total_indxr_tokens": total_ix_tok,
            "avg_token_reduction": total_bl_tok / total_ix_tok if total_ix_tok else 0,
        },
    }

    with open(output_path, "w") as f:
        json.dump(output, f, indent=2)
    print(f"Results written to {output_path}")


# ---------------------------------------------------------------------------
# Dry run
# ---------------------------------------------------------------------------

def dry_run(questions: list, repo_path: Path, mcp: IndxrMCP):
    """Show contexts without calling the API."""
    for q in questions:
        print(f"\n{'='*80}")
        print(f"Question {q['id']}: {q['question']}")
        print(f"Category: {q['category']} | Scoring: {q['scoring']}")
        print(f"Ground truth: {q['ground_truth']}")

        print(f"\n--- BASELINE CONTEXT ---")
        bl_ctx = build_baseline_context(q, repo_path)
        bl_chars = len(bl_ctx)
        print(f"[{bl_chars} chars, ~{bl_chars // 4} estimated tokens]")
        print(f"Files: {', '.join(q['baseline_files'])}")

        print(f"\n--- INDXR CONTEXT ---")
        ix_ctx = build_indxr_context(q, mcp)
        ix_chars = len(ix_ctx)
        print(f"[{ix_chars} chars, ~{ix_chars // 4} estimated tokens]")
        print(ix_ctx[:500] + ("..." if len(ix_ctx) > 500 else ""))

        est_reduction = bl_chars / ix_chars if ix_chars else 0
        print(f"\nEstimated reduction: {est_reduction:.1f}x")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="indxr Accuracy Benchmark")
    parser.add_argument("--questions", default=DEFAULT_QUESTIONS, help="Path to questions JSON")
    parser.add_argument("--repo", default=".", help="Path to project root")
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="Output JSON path")
    parser.add_argument("--model", default=DEFAULT_MODEL, help="Claude model to use")
    parser.add_argument("--filter", default=None, help="Run only this category")
    parser.add_argument("--dry-run", action="store_true", help="Show contexts without calling API")
    parser.add_argument("--verbose", "-v", action="store_true", help="Show answers")
    args = parser.parse_args()

    repo_path = Path(args.repo).resolve()
    questions_path = Path(args.questions)
    if not questions_path.is_absolute():
        questions_path = Path(__file__).parent / questions_path

    # Load questions
    with open(questions_path) as f:
        data = json.load(f)
    questions = data["questions"]

    if args.filter:
        questions = [q for q in questions if q["category"] == args.filter]
        if not questions:
            print(f"No questions matching category '{args.filter}'")
            sys.exit(1)

    print(f"Loaded {len(questions)} questions from {questions_path.name}")
    print(f"Repository: {repo_path}")

    # Start indxr MCP server
    print("Starting indxr MCP server...")
    mcp = IndxrMCP(str(repo_path))
    print("MCP server ready.")

    if args.dry_run:
        dry_run(questions, repo_path, mcp)
        mcp.close()
        return

    # Verify API key
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("ERROR: ANTHROPIC_API_KEY not set")
        mcp.close()
        sys.exit(1)

    import anthropic
    client = anthropic.Anthropic()

    print(f"Model: {args.model}")
    print(f"Running {len(questions)} questions x 2 conditions = {len(questions) * 2} API calls")
    print()

    results = []

    for i, q in enumerate(questions):
        qid = q["id"]
        print(f"[{i+1}/{len(questions)}] {qid}: {q['question'][:60]}...")

        # --- Condition A: Baseline (full file reads) ---
        bl_context = build_baseline_context(q, repo_path)
        bl_answer, bl_usage = ask_claude(client, args.model, bl_context, q["question"])
        bl_score = score_answer(q, bl_answer)
        time.sleep(API_DELAY)

        # --- Condition B: indxr (structural context) ---
        ix_context = build_indxr_context(q, mcp)
        ix_answer, ix_usage = ask_claude(client, args.model, ix_context, q["question"])
        ix_score = score_answer(q, ix_answer)
        time.sleep(API_DELAY)

        # Token reduction
        bl_in = bl_usage["input_tokens"]
        ix_in = ix_usage["input_tokens"]
        reduction = bl_in / ix_in if ix_in > 0 else 0

        r = QuestionResult(
            id=qid,
            category=q["category"],
            difficulty=q.get("difficulty", "medium"),
            question=q["question"],
            baseline_score=bl_score,
            indxr_score=ix_score,
            baseline_input_tokens=bl_in,
            indxr_input_tokens=ix_in,
            baseline_output_tokens=bl_usage["output_tokens"],
            indxr_output_tokens=ix_usage["output_tokens"],
            baseline_answer=bl_answer,
            indxr_answer=ix_answer,
            token_reduction=reduction,
        )
        results.append(r)

        # Progress
        score_indicator = lambda s: "OK" if s >= 0.8 else ("~" if s >= 0.5 else "X")
        print(f"  BL: {score_indicator(bl_score)} ({bl_score:.2f}, {bl_in:,} tok)  IX: {score_indicator(ix_score)} ({ix_score:.2f}, {ix_in:,} tok)  {reduction:.1f}x reduction")

        if args.verbose:
            print(f"  BL answer: {bl_answer[:120]}...")
            print(f"  IX answer: {ix_answer[:120]}...")

    # Cleanup
    mcp.close()

    # Output
    print_results(results, args.model)

    metadata = {
        "model": args.model,
        "date": time.strftime("%Y-%m-%d %H:%M:%S"),
        "repo": str(repo_path),
        "question_count": len(results),
        "questions_file": str(questions_path),
        "version": data.get("version", 1),
    }
    write_json_results(results, metadata, args.output)


if __name__ == "__main__":
    main()
