"""
Statistical analysis for benchmark results.

Supports multiple runs with bootstrap confidence intervals.
"""

import random
from dataclasses import dataclass, field


@dataclass
class QuestionStats:
    """Aggregated stats for a single question across K runs."""
    id: str
    category: str
    baseline_scores: list[float] = field(default_factory=list)
    indxr_scores: list[float] = field(default_factory=list)
    baseline_tokens: list[int] = field(default_factory=list)
    indxr_tokens: list[int] = field(default_factory=list)
    baseline_content_tokens: list[int] = field(default_factory=list)
    indxr_content_tokens: list[int] = field(default_factory=list)
    baseline_rounds: list[int] = field(default_factory=list)
    indxr_rounds: list[int] = field(default_factory=list)

    @property
    def baseline_mean(self) -> float:
        return _mean(self.baseline_scores)

    @property
    def indxr_mean(self) -> float:
        return _mean(self.indxr_scores)

    @property
    def baseline_token_mean(self) -> float:
        return _mean(self.baseline_tokens)

    @property
    def indxr_token_mean(self) -> float:
        return _mean(self.indxr_tokens)

    @property
    def baseline_content_token_mean(self) -> float:
        return _mean(self.baseline_content_tokens)

    @property
    def indxr_content_token_mean(self) -> float:
        return _mean(self.indxr_content_tokens)

    @property
    def token_reduction(self) -> float:
        ix = self.indxr_token_mean
        return self.baseline_token_mean / ix if ix > 0 else 0.0

    @property
    def content_token_reduction(self) -> float:
        ix = self.indxr_content_token_mean
        return self.baseline_content_token_mean / ix if ix > 0 else 0.0


@dataclass
class SummaryStats:
    """Overall benchmark summary."""
    n_questions: int
    n_runs: int
    baseline_accuracy: float
    indxr_accuracy: float
    accuracy_delta: float
    baseline_accuracy_ci: tuple[float, float] | None
    indxr_accuracy_ci: tuple[float, float] | None
    delta_ci: tuple[float, float] | None
    total_baseline_tokens: int
    total_indxr_tokens: int
    total_baseline_content_tokens: int
    total_indxr_content_tokens: int
    avg_token_reduction: float
    avg_content_token_reduction: float
    baseline_rounds_mean: float
    indxr_rounds_mean: float


def compute_question_stats(
    question_id: str, category: str, runs: list[dict]
) -> QuestionStats:
    """Aggregate results for one question across multiple runs."""
    return QuestionStats(
        id=question_id,
        category=category,
        baseline_scores=[r["baseline_score"] for r in runs],
        indxr_scores=[r["indxr_score"] for r in runs],
        baseline_tokens=[r["baseline_input_tokens"] for r in runs],
        indxr_tokens=[r["indxr_input_tokens"] for r in runs],
        baseline_content_tokens=[r.get("baseline_content_tokens", r["baseline_input_tokens"]) for r in runs],
        indxr_content_tokens=[r.get("indxr_content_tokens", r["indxr_input_tokens"]) for r in runs],
        baseline_rounds=[r.get("baseline_rounds", 1) for r in runs],
        indxr_rounds=[r.get("indxr_rounds", 1) for r in runs],
    )


def compute_summary(
    question_stats: list[QuestionStats], n_runs: int
) -> SummaryStats:
    """Compute overall summary from per-question stats."""
    n = len(question_stats)

    bl_means = [q.baseline_mean for q in question_stats]
    ix_means = [q.indxr_mean for q in question_stats]
    deltas = [ix - bl for ix, bl in zip(ix_means, bl_means)]

    bl_acc = _mean(bl_means)
    ix_acc = _mean(ix_means)
    delta = _mean(deltas)

    # Confidence intervals (only meaningful with multiple runs)
    bl_ci = bootstrap_ci(bl_means) if n_runs > 1 else None
    ix_ci = bootstrap_ci(ix_means) if n_runs > 1 else None
    delta_ci = bootstrap_ci(deltas) if n_runs > 1 else None

    total_bl_tok = sum(int(q.baseline_token_mean) for q in question_stats)
    total_ix_tok = sum(int(q.indxr_token_mean) for q in question_stats)
    total_bl_content = sum(int(q.baseline_content_token_mean) for q in question_stats)
    total_ix_content = sum(int(q.indxr_content_token_mean) for q in question_stats)

    bl_rounds = _mean([_mean(q.baseline_rounds) for q in question_stats])
    ix_rounds = _mean([_mean(q.indxr_rounds) for q in question_stats])

    return SummaryStats(
        n_questions=n,
        n_runs=n_runs,
        baseline_accuracy=bl_acc,
        indxr_accuracy=ix_acc,
        accuracy_delta=delta,
        baseline_accuracy_ci=bl_ci,
        indxr_accuracy_ci=ix_ci,
        delta_ci=delta_ci,
        total_baseline_tokens=total_bl_tok,
        total_indxr_tokens=total_ix_tok,
        total_baseline_content_tokens=total_bl_content,
        total_indxr_content_tokens=total_ix_content,
        avg_token_reduction=total_bl_tok / total_ix_tok if total_ix_tok else 0,
        avg_content_token_reduction=total_bl_content / total_ix_content if total_ix_content else 0,
        baseline_rounds_mean=bl_rounds,
        indxr_rounds_mean=ix_rounds,
    )


def bootstrap_ci(
    data: list[float],
    n_bootstrap: int = 10_000,
    ci: float = 0.95,
) -> tuple[float, float]:
    """Bootstrap confidence interval for the mean."""
    if len(data) <= 1:
        m = _mean(data)
        return (m, m)

    rng = random.Random(42)  # reproducible
    n = len(data)
    means = sorted(
        _mean(rng.choices(data, k=n)) for _ in range(n_bootstrap)
    )

    lo_idx = int((1 - ci) / 2 * n_bootstrap)
    hi_idx = int((1 + ci) / 2 * n_bootstrap) - 1
    return (means[lo_idx], means[hi_idx])


def _mean(data) -> float:
    """Safe mean for numeric lists."""
    if not data:
        return 0.0
    return sum(float(x) for x in data) / len(data)
