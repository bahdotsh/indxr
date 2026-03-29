"""
indxr Accuracy Benchmark v2 — Agent Loop Edition

Both conditions use an agent loop where the LLM chooses which tools to call.
Baseline agent gets grep + read + ls.  indxr agent gets MCP tools.

Usage:
    python -m bench                               # run all, 3 runs
    python -m bench --runs 1                       # quick single run
    python -m bench --filter behavioral            # one category
    python -m bench --question behav-004           # one question
    python -m bench --dry-run                      # show setup, no API calls
    python -m bench --repo /path/to/project        # external repo
    python -m bench --verbose                      # show agent traces

Requirements:
    - ANTHROPIC_API_KEY env var
    - anthropic SDK (pip install anthropic)
    - indxr binary (cargo build --release)
"""

import argparse
import json
import math
import os
import sys
import time
from dataclasses import asdict
from pathlib import Path

from .agent import run_agent, BASELINE_SYSTEM_PROMPT, make_indxr_system_prompt
from .tools_baseline import make_baseline_tools, BaselineToolkit
from .tools_indxr import IndxrToolkit
from .scoring import score_answer
from .stats import compute_question_stats, compute_summary
from .output import print_results, write_json


DEFAULT_MODEL = "claude-sonnet-4-6"
DEFAULT_QUESTIONS = "bench_questions/indxr.json"
DEFAULT_OUTPUT = "benchmark_results_v2.json"


def _indicator(s: float) -> str:
    """Score indicator: OK (>=0.8), ~ (>=0.5), X (below)."""
    return "OK" if s >= 0.8 else ("~" if s >= 0.5 else "X")


def main():
    parser = argparse.ArgumentParser(
        description="indxr Accuracy Benchmark v2 — Agent Loop",
    )
    parser.add_argument(
        "--questions", default=DEFAULT_QUESTIONS,
        help="Questions JSON file (default: bench_questions/indxr.json)",
    )
    parser.add_argument("--repo", default=".", help="Path to repository")
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="Output JSON path")
    parser.add_argument("--model", default=DEFAULT_MODEL, help="Claude model")
    parser.add_argument(
        "--runs", type=int, default=3,
        help="Runs per question (default: 3)",
    )
    parser.add_argument(
        "--max-rounds", type=int, default=10,
        help="Max agent tool-call rounds per question (default: 10)",
    )
    parser.add_argument("--filter", default=None, help="Run only this category")
    parser.add_argument("--question", default=None, help="Run only this question ID")
    parser.add_argument(
        "--dry-run", action="store_true",
        help="Show setup without calling API",
    )
    parser.add_argument(
        "--verbose", "-v", action="store_true",
        help="Show agent tool traces and answers",
    )
    args = parser.parse_args()

    repo_path = Path(args.repo).resolve()

    # Load questions
    questions_path = Path(args.questions)
    if not questions_path.is_absolute():
        questions_path = Path(__file__).parent.parent / questions_path

    if not questions_path.exists():
        print(f"ERROR: Questions file not found: {questions_path}")
        sys.exit(1)

    with open(questions_path) as f:
        data = json.load(f)

    questions = data["questions"]

    # Apply filters
    if args.filter:
        questions = [q for q in questions if q["category"] == args.filter]
    if args.question:
        questions = [q for q in questions if q["id"] == args.question]

    if not questions:
        print("No matching questions found.")
        sys.exit(1)

    print(f"Loaded {len(questions)} questions from {questions_path.name}")
    print(f"Repository: {repo_path}")
    print(f"Model: {args.model}")
    print(f"Runs: {args.runs}")
    print(f"Max rounds per agent: {args.max_rounds}")

    # Start indxr MCP server
    print("\nStarting indxr MCP server...")
    indxr = IndxrToolkit(str(repo_path))
    indxr_tools = indxr.get_tools()
    print(f"  indxr: {len(indxr_tools)} tools loaded from MCP server")

    # Fetch codebase tree for context injection (simulates real INDEX.md usage).
    # get_tree is callable even without --all-tools (unlisted but functional).
    tree_context = indxr.execute("get_tree", {})
    indxr_system_prompt = make_indxr_system_prompt(tree_context)
    tree_tokens = len(tree_context) // 4
    print(f"  Tree context: ~{tree_tokens} tokens injected into system prompt")

    # Set up baseline tools
    baseline_tools = make_baseline_tools()
    baseline = BaselineToolkit(str(repo_path))
    print(f"  Baseline: {len(baseline_tools)} tools (grep, read, ls)")

    # Estimate per-round schema overhead (tool definitions are re-sent each round)
    bl_schema_per_round = _estimate_schema_tokens(baseline_tools, BASELINE_SYSTEM_PROMPT)
    ix_schema_per_round = _estimate_schema_tokens(indxr_tools, indxr_system_prompt)
    print(f"  Schema overhead: baseline ~{bl_schema_per_round} tok/round, "
          f"indxr ~{ix_schema_per_round} tok/round")

    if args.dry_run:
        _dry_run(questions, baseline_tools, indxr_tools)
        indxr.close()
        return

    # Verify API key
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("\nERROR: ANTHROPIC_API_KEY not set")
        indxr.close()
        sys.exit(1)

    import anthropic
    client = anthropic.Anthropic()

    n_sessions = len(questions) * 2 * args.runs
    print(
        f"\nRunning {len(questions)} questions x 2 conditions x {args.runs} runs "
        f"= {n_sessions} agent sessions"
    )
    print()

    # -- Main benchmark loop --
    all_runs: list[dict] = []
    question_runs: dict[str, list[dict]] = {}

    for run_idx in range(args.runs):
        if args.runs > 1:
            print(f"{'='*60} Run {run_idx + 1}/{args.runs} {'='*60}")

        for q_idx, q in enumerate(questions):
            qid = q["id"]
            short_q = q["question"][:55]
            print(f"  [{q_idx + 1}/{len(questions)}] {qid}: {short_q}...")

            # --- Baseline agent (grep + read + ls) ---
            bl_result = run_agent(
                client=client,
                model=args.model,
                tools=baseline_tools,
                execute_tool=baseline.execute,
                question=q["question"],
                max_rounds=args.max_rounds,
                system_prompt=BASELINE_SYSTEM_PROMPT,
            )
            bl_score = score_answer(q, bl_result.answer)

            # --- indxr agent (MCP tools + context) ---
            ix_result = run_agent(
                client=client,
                model=args.model,
                tools=indxr_tools,
                execute_tool=indxr.execute,
                question=q["question"],
                max_rounds=args.max_rounds,
                system_prompt=indxr_system_prompt,
            )
            ix_score = score_answer(q, ix_result.answer)

            # Metrics
            bl_tok = bl_result.total_input_tokens
            ix_tok = ix_result.total_input_tokens
            reduction = bl_tok / ix_tok if ix_tok > 0 else 0

            print(
                f"    BL: {_indicator(bl_score.score)} "
                f"({bl_score.score:.2f}, {bl_tok:,}tok, {bl_result.rounds}rnd)  "
                f"IX: {_indicator(ix_score.score)} "
                f"({ix_score.score:.2f}, {ix_tok:,}tok, {ix_result.rounds}rnd)  "
                f"{reduction:.1f}x"
            )

            if args.verbose:
                print(f"    BL tools: {[tc.name for tc in bl_result.tool_calls]}")
                print(f"    IX tools: {[tc.name for tc in ix_result.tool_calls]}")
                print(f"    BL answer: {bl_result.answer[:120]}...")
                print(f"    IX answer: {ix_result.answer[:120]}...")
                if bl_score.penalty > 0:
                    print(f"    BL scoring: {bl_score.details}")
                if ix_score.penalty > 0:
                    print(f"    IX scoring: {ix_score.details}")

            # Schema overhead: fixed cost per round from tool definitions + system prompt
            bl_schema = bl_schema_per_round * bl_result.rounds
            ix_schema = ix_schema_per_round * ix_result.rounds
            bl_content = max(0, bl_tok - bl_schema)
            ix_content = max(0, ix_tok - ix_schema)
            content_reduction = bl_content / ix_content if ix_content > 0 else 0

            run_data = {
                "run": run_idx,
                "id": qid,
                "category": q["category"],
                "question": q["question"],
                "baseline_score": bl_score.score,
                "indxr_score": ix_score.score,
                "baseline_base_score": bl_score.base_score,
                "indxr_base_score": ix_score.base_score,
                "baseline_penalty": bl_score.penalty,
                "indxr_penalty": ix_score.penalty,
                "baseline_input_tokens": bl_tok,
                "indxr_input_tokens": ix_tok,
                "baseline_output_tokens": bl_result.total_output_tokens,
                "indxr_output_tokens": ix_result.total_output_tokens,
                "baseline_schema_tokens": bl_schema,
                "indxr_schema_tokens": ix_schema,
                "baseline_content_tokens": bl_content,
                "indxr_content_tokens": ix_content,
                "baseline_rounds": bl_result.rounds,
                "indxr_rounds": ix_result.rounds,
                "baseline_stop_reason": bl_result.stop_reason,
                "indxr_stop_reason": ix_result.stop_reason,
                "baseline_answer": bl_result.answer,
                "indxr_answer": ix_result.answer,
                "baseline_tool_calls": [asdict(tc) for tc in bl_result.tool_calls],
                "indxr_tool_calls": [asdict(tc) for tc in ix_result.tool_calls],
                "baseline_scoring_details": bl_score.details,
                "indxr_scoring_details": ix_score.details,
                "token_reduction": reduction,
                "content_token_reduction": content_reduction,
            }
            all_runs.append(run_data)
            question_runs.setdefault(qid, []).append(run_data)

    # -- Compute statistics --
    q_stats = [
        compute_question_stats(q["id"], q["category"], question_runs[q["id"]])
        for q in questions
        if q["id"] in question_runs
    ]

    summary = compute_summary(q_stats, args.runs)

    # -- Output --
    indxr.close()

    print_results(q_stats, summary, args.model, args.verbose)

    metadata = {
        "version": 2,
        "model": args.model,
        "date": time.strftime("%Y-%m-%d %H:%M:%S"),
        "repo": str(repo_path),
        "n_questions": len(questions),
        "n_runs": args.runs,
        "max_rounds": args.max_rounds,
        "questions_file": str(questions_path),
    }
    write_json(q_stats, summary, all_runs, metadata, args.output)


def _dry_run(questions, baseline_tools, indxr_tools):
    """Show benchmark setup without calling the API."""
    print(f"\n{'='*60} DRY RUN {'='*60}\n")

    print("Baseline tools:")
    for t in baseline_tools:
        params = list(t["input_schema"].get("properties", {}).keys())
        print(f"  {t['name']}({', '.join(params)})")

    print(f"\nindxr tools ({len(indxr_tools)}):")
    for t in indxr_tools:
        params = list(t["input_schema"].get("properties", {}).keys())
        req = t["input_schema"].get("required", [])
        param_str = ", ".join(
            f"{p}*" if p in req else p for p in params
        )
        print(f"  {t['name']}({param_str})")

    print(f"\nQuestions ({len(questions)}):")
    categories: dict[str, list] = {}
    for q in questions:
        categories.setdefault(q["category"], []).append(q)

    for cat, qs in sorted(categories.items()):
        print(f"\n  {cat} ({len(qs)}):")
        for q in qs:
            anti = len(q.get("scoring_anti_targets", []))
            anti_str = f" [{anti} anti-targets]" if anti else ""
            print(f"    {q['id']}: {q['question'][:60]}...{anti_str}")

    print(
        f"\nTotal: {len(questions)} questions "
        f"across {len(categories)} categories"
    )


def _estimate_schema_tokens(tools: list[dict], system_prompt: str) -> int:
    """Estimate per-round token overhead from tool definitions + system prompt.

    Tool definitions and the system prompt are re-sent by the API on every round.
    This gives a rough estimate (~4 chars/token) to separate schema cost from
    content cost in the benchmark output.
    """
    schema_chars = len(json.dumps(tools)) + len(system_prompt)
    return math.ceil(schema_chars / 4)


if __name__ == "__main__":
    main()
