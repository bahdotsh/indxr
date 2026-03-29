"""
Agent loop for the indxr accuracy benchmark.

Both conditions (baseline grep/read and indxr MCP) use the same loop.
The LLM decides which tools to call — the only difference is the toolbox.
"""

import time
from dataclasses import dataclass, field
from typing import Callable

API_DELAY = 0.3  # seconds between rounds to avoid rate limits


@dataclass
class ToolCall:
    """Record of a single tool invocation."""
    round: int
    name: str
    input: dict
    output: str


@dataclass
class AgentResult:
    """Complete result from an agent run."""
    answer: str
    tool_calls: list[ToolCall] = field(default_factory=list)
    rounds: int = 0
    total_input_tokens: int = 0
    total_output_tokens: int = 0
    stop_reason: str = "complete"  # complete | max_rounds | budget_exceeded | error


BASELINE_SYSTEM_PROMPT = (
    "You are answering a question about a codebase. "
    "Use the provided tools to explore the code and find the answer. "
    "Be thorough but efficient — use the minimum tools needed to answer confidently. "
    "When you have enough information, stop calling tools and give your final answer. "
    "Answer concisely and precisely. Include file paths, function names, "
    "types, and other concrete details. Do not hedge or speculate. "
    "If you cannot determine the answer from the available tools, say so clearly."
)

def make_indxr_system_prompt(tree_context: str) -> str:
    """Build the indxr system prompt with injected codebase structure.

    In real usage, indxr users get INDEX.md loaded as context.
    This simulates that by injecting the directory tree, letting the
    agent answer navigation questions from context alone (0 tool calls).
    """
    return (
        "You are answering a question about a codebase. "
        "The codebase structure is shown below.\n\n"
        "## Codebase Structure\n"
        f"{tree_context}\n\n"
        "## Tools\n"
        "- find(query, mode?) — search for symbols/files. "
        "mode: relevant (default), symbol, callers, signature\n"
        "- summarize(path) — file overview, or symbol explanation (no source). "
        "Pass a glob for batch. scope=public for public API only.\n"
        "- read(path, symbol?) — read source code by symbol name or line range\n\n"
        "Answer from the structure above when possible. "
        "Only call tools when you need details not shown above.\n"
        "Answer concisely and precisely. Include file paths, function names, "
        "types, and concrete details. Do not hedge or speculate."
    )


def run_agent(
    client,
    model: str,
    tools: list[dict],
    execute_tool: Callable[[str, dict], str],
    question: str,
    max_rounds: int = 10,
    max_input_tokens: int = 50_000,
    system_prompt: str = BASELINE_SYSTEM_PROMPT,
) -> AgentResult:
    """
    Run a tool-use agent loop.

    The LLM decides which tools to call and when to stop.
    Returns the final answer along with usage metrics.
    """
    messages = [{"role": "user", "content": question}]
    total_input = 0
    total_output = 0
    all_tool_calls: list[ToolCall] = []
    stop_reason = "complete"
    rounds_used = 0

    for round_idx in range(max_rounds):
        rounds_used = round_idx + 1

        try:
            response = client.messages.create(
                model=model,
                system=system_prompt,
                messages=messages,
                tools=tools,
                max_tokens=2048,
                temperature=0,
            )
        except Exception as e:
            return AgentResult(
                answer=f"[Agent error on round {round_idx}: {e}]",
                tool_calls=all_tool_calls,
                rounds=rounds_used,
                total_input_tokens=total_input,
                total_output_tokens=total_output,
                stop_reason="error",
            )

        total_input += response.usage.input_tokens
        total_output += response.usage.output_tokens

        # Budget guard
        if total_input > max_input_tokens:
            text = _extract_text(response.content)
            if text:
                return AgentResult(
                    answer=text,
                    tool_calls=all_tool_calls,
                    rounds=rounds_used,
                    total_input_tokens=total_input,
                    total_output_tokens=total_output,
                    stop_reason="budget_exceeded",
                )
            stop_reason = "budget_exceeded"
            break

        # If the model didn't request tool use, it's done
        if response.stop_reason != "tool_use":
            return AgentResult(
                answer=_extract_text(response.content),
                tool_calls=all_tool_calls,
                rounds=rounds_used,
                total_input_tokens=total_input,
                total_output_tokens=total_output,
                stop_reason="complete",
            )

        # Model wants to use tools — execute them
        messages.append({"role": "assistant", "content": response.content})

        tool_results = []
        for block in response.content:
            if block.type == "tool_use":
                try:
                    output = execute_tool(block.name, block.input)
                except Exception as e:
                    output = f"Tool error: {e}"

                all_tool_calls.append(ToolCall(
                    round=round_idx,
                    name=block.name,
                    input=block.input,
                    output=output[:3000],
                ))

                tool_results.append({
                    "type": "tool_result",
                    "tool_use_id": block.id,
                    "content": output,
                })

        messages.append({"role": "user", "content": tool_results})
        time.sleep(API_DELAY)
    else:
        stop_reason = "max_rounds"

    # Agent didn't finish naturally — force a final text answer
    messages.append({
        "role": "user",
        "content": (
            "You have used all available tool-call rounds. "
            "Based on the information gathered so far, provide your final answer now."
        ),
    })

    try:
        response = client.messages.create(
            model=model,
            system=system_prompt,
            messages=messages,
            max_tokens=2048,
            temperature=0,
            # No tools — forces text-only response
        )
        total_input += response.usage.input_tokens
        total_output += response.usage.output_tokens
        answer = _extract_text(response.content)
    except Exception:
        answer = "[Failed to get final answer after budget/round limit]"

    return AgentResult(
        answer=answer,
        tool_calls=all_tool_calls,
        rounds=rounds_used,
        total_input_tokens=total_input,
        total_output_tokens=total_output,
        stop_reason=stop_reason,
    )


def _extract_text(content) -> str:
    """Extract text from response content blocks."""
    parts = []
    for block in content:
        if hasattr(block, "text") and block.type == "text":
            parts.append(block.text)
    return "\n".join(parts)
