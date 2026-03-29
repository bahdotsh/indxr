"""
Scoring logic for the accuracy benchmark.

Deterministic scoring with anti-hallucination penalties.
No LLM-as-judge — fully reproducible.
"""

import re
from dataclasses import dataclass


@dataclass
class ScoreResult:
    """Detailed scoring result."""
    score: float       # Final score after penalties (0.0–1.0)
    base_score: float  # Score before penalties
    penalty: float     # Anti-hallucination penalty applied
    method: str        # Scoring method used
    details: str       # Human-readable breakdown


def score_answer(question: dict, answer: str) -> ScoreResult:
    """
    Score an answer against ground truth.

    Methods:
        substring  — any target found → 1.0
        all_of     — fraction of targets found (all required)
        set_match  — fraction of targets found (partial credit)
        number_range — any extracted number falls in [lo, hi]

    Anti-targets (optional): strings that indicate hallucination.
    Each anti-target found applies a penalty of 0.25 * (found/total).
    """
    method = question["scoring"]
    targets = question["scoring_targets"]
    anti_targets = question.get("scoring_anti_targets", [])
    answer_lower = answer.lower()

    # -- Base score --
    if method == "substring":
        found = [t for t in targets if t.lower() in answer_lower]
        base = 1.0 if found else 0.0
        details = f"Found: {found}" if found else "No targets found"

    elif method == "all_of":
        found = [t for t in targets if t.lower() in answer_lower]
        missing = [t for t in targets if t.lower() not in answer_lower]
        base = len(found) / len(targets) if targets else 0.0
        details = f"Found {len(found)}/{len(targets)}"
        if missing:
            details += f", missing: {missing}"

    elif method == "set_match":
        found = [t for t in targets if t.lower() in answer_lower]
        missing = [t for t in targets if t.lower() not in answer_lower]
        base = len(found) / len(targets) if targets else 0.0
        details = f"Found {len(found)}/{len(targets)}"
        if missing:
            details += f", missing: {missing}"

    elif method == "number_range":
        nums = [int(n) for n in re.findall(r"\b(\d+)\b", answer)]
        lo, hi = int(targets[0]), int(targets[1])
        base = 1.0 if any(lo <= n <= hi for n in nums) else 0.0
        details = f"Numbers found: {nums}, range [{lo}, {hi}]"

    else:
        base = 0.0
        details = f"Unknown scoring method: {method}"

    # -- Anti-hallucination penalty --
    penalty = 0.0
    if anti_targets:
        anti_found = [t for t in anti_targets if t.lower() in answer_lower]
        if anti_found:
            penalty = 0.25 * (len(anti_found) / len(anti_targets))
            details += f" | PENALTY for anti-targets: {anti_found} (-{penalty:.2f})"

    final = max(0.0, base - penalty)

    return ScoreResult(
        score=final,
        base_score=base,
        penalty=penalty,
        method=method,
        details=details,
    )
