"""
Output formatting for benchmark results.

Terminal tables and JSON output with full run details.
"""

import json
import time


def print_results(question_stats, summary, model: str, verbose: bool = False):
    """Print formatted results table to stdout."""
    print()
    print("=" * 120)
    print("indxr Accuracy Benchmark v2 — Agent Loop")
    print("=" * 120)
    print(f"Model:     {model}")
    print(f"Questions: {summary.n_questions}")
    print(f"Runs:      {summary.n_runs}")
    print(f"Date:      {time.strftime('%Y-%m-%d %H:%M:%S')}")
    print()

    # Header
    hdr = (
        f"{'ID':<14} {'Category':<14} "
        f"{'BL Acc':>7} {'IX Acc':>7} "
        f"{'BL Tok':>9} {'IX Tok':>9} "
        f"{'Reduc':>7} "
        f"{'Content':>8} "
        f"{'BL Rnd':>6} {'IX Rnd':>6}"
    )
    print(hdr)
    print("-" * 120)

    for q in question_stats:
        red = f"{q.token_reduction:.1f}x" if q.token_reduction > 0 else "N/A"
        c_red = f"{q.content_token_reduction:.1f}x" if q.content_token_reduction > 0 else "N/A"
        bl_rnd = f"{_mean(q.baseline_rounds):.1f}"
        ix_rnd = f"{_mean(q.indxr_rounds):.1f}"
        print(
            f"{q.id:<14} {q.category:<14} "
            f"{q.baseline_mean:>7.2f} {q.indxr_mean:>7.2f} "
            f"{int(q.baseline_token_mean):>9,} {int(q.indxr_token_mean):>9,} "
            f"{red:>7} "
            f"{c_red:>8} "
            f"{bl_rnd:>6} {ix_rnd:>6}"
        )

    print("-" * 120)
    print()

    # Summary
    print("Summary:")
    _print_stat("Baseline accuracy", summary.baseline_accuracy, summary.baseline_accuracy_ci)
    _print_stat("indxr accuracy   ", summary.indxr_accuracy, summary.indxr_accuracy_ci)

    delta_sign = "+" if summary.accuracy_delta >= 0 else ""
    if summary.delta_ci:
        lo_s = "+" if summary.delta_ci[0] >= 0 else ""
        hi_s = "+" if summary.delta_ci[1] >= 0 else ""
        print(
            f"  Accuracy delta:       {delta_sign}{summary.accuracy_delta:.3f} "
            f"[{lo_s}{summary.delta_ci[0]:.3f}, {hi_s}{summary.delta_ci[1]:.3f}]"
        )
    else:
        print(f"  Accuracy delta:       {delta_sign}{summary.accuracy_delta:.3f}")

    print(f"  Total baseline tokens:   {summary.total_baseline_tokens:,}")
    print(f"  Total indxr tokens:      {summary.total_indxr_tokens:,}")
    print(f"  Avg token reduction:     {summary.avg_token_reduction:.1f}x")
    print(f"  Content tokens (BL/IX):  {summary.total_baseline_content_tokens:,} / {summary.total_indxr_content_tokens:,}")
    print(f"  Content token reduction: {summary.avg_content_token_reduction:.1f}x")
    print(f"  Avg baseline rounds:     {summary.baseline_rounds_mean:.1f}")
    print(f"  Avg indxr rounds:        {summary.indxr_rounds_mean:.1f}")
    print()

    # Per-category breakdown
    categories = sorted(set(q.category for q in question_stats))
    if len(categories) > 1:
        print("Per-category breakdown:")
        print(f"  {'Category':<14} {'Count':>5} {'BL Acc':>7} {'IX Acc':>7} {'Reduction':>10} {'Content':>10}")
        print("  " + "-" * 60)
        for cat in categories:
            cat_qs = [q for q in question_stats if q.category == cat]
            cat_bl = _mean([q.baseline_mean for q in cat_qs])
            cat_ix = _mean([q.indxr_mean for q in cat_qs])
            cat_bl_tok = sum(q.baseline_token_mean for q in cat_qs)
            cat_ix_tok = sum(q.indxr_token_mean for q in cat_qs)
            cat_bl_ct = sum(q.baseline_content_token_mean for q in cat_qs)
            cat_ix_ct = sum(q.indxr_content_token_mean for q in cat_qs)
            cat_red = cat_bl_tok / cat_ix_tok if cat_ix_tok else 0
            cat_ct_red = cat_bl_ct / cat_ix_ct if cat_ix_ct else 0
            print(
                f"  {cat:<14} {len(cat_qs):>5} "
                f"{cat_bl:>7.2f} {cat_ix:>7.2f} "
                f"{cat_red:>9.1f}x "
                f"{cat_ct_red:>9.1f}x"
            )
    print()

    # Verdict
    if summary.indxr_accuracy >= summary.baseline_accuracy:
        qual = "better" if summary.indxr_accuracy > summary.baseline_accuracy else "equal"
        print(
            f"  => indxr achieves {qual} accuracy "
            f"with {summary.avg_token_reduction:.1f}x token reduction "
            f"({summary.avg_content_token_reduction:.1f}x content-only)."
        )
    else:
        print(
            f"  => indxr: {summary.avg_token_reduction:.1f}x token reduction "
            f"({summary.avg_content_token_reduction:.1f}x content-only) "
            f"with {abs(summary.accuracy_delta):.3f} accuracy tradeoff."
        )

    if summary.n_runs == 1:
        print(
            "  NOTE: Single run — no confidence intervals. "
            "Use --runs 3+ for statistical rigor."
        )
    print()


def write_json(question_stats, summary, all_runs, metadata, output_path: str):
    """Write detailed JSON results."""
    output = {
        "metadata": metadata,
        "summary": {
            "n_questions": summary.n_questions,
            "n_runs": summary.n_runs,
            "baseline_accuracy": summary.baseline_accuracy,
            "indxr_accuracy": summary.indxr_accuracy,
            "accuracy_delta": summary.accuracy_delta,
            "baseline_accuracy_ci_95": summary.baseline_accuracy_ci,
            "indxr_accuracy_ci_95": summary.indxr_accuracy_ci,
            "delta_ci_95": summary.delta_ci,
            "total_baseline_tokens": summary.total_baseline_tokens,
            "total_indxr_tokens": summary.total_indxr_tokens,
            "total_baseline_content_tokens": summary.total_baseline_content_tokens,
            "total_indxr_content_tokens": summary.total_indxr_content_tokens,
            "avg_token_reduction": summary.avg_token_reduction,
            "avg_content_token_reduction": summary.avg_content_token_reduction,
            "baseline_rounds_mean": summary.baseline_rounds_mean,
            "indxr_rounds_mean": summary.indxr_rounds_mean,
        },
        "per_question": [
            {
                "id": q.id,
                "category": q.category,
                "baseline_score_mean": q.baseline_mean,
                "indxr_score_mean": q.indxr_mean,
                "baseline_token_mean": int(q.baseline_token_mean),
                "indxr_token_mean": int(q.indxr_token_mean),
                "baseline_content_token_mean": int(q.baseline_content_token_mean),
                "indxr_content_token_mean": int(q.indxr_content_token_mean),
                "token_reduction": round(q.token_reduction, 2),
                "content_token_reduction": round(q.content_token_reduction, 2),
                "baseline_scores": q.baseline_scores,
                "indxr_scores": q.indxr_scores,
            }
            for q in question_stats
        ],
        "runs": all_runs,
    }

    with open(output_path, "w") as f:
        json.dump(output, f, indent=2, default=str)
    print(f"Results written to {output_path}")


def _print_stat(label: str, value: float, ci=None):
    if ci:
        print(f"  {label}  {value:.3f} [{ci[0]:.3f}, {ci[1]:.3f}]")
    else:
        print(f"  {label}  {value:.3f}")


def _mean(data) -> float:
    return sum(float(x) for x in data) / len(data) if data else 0.0
