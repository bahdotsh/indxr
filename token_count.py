#!/usr/bin/env python3
"""
Count tokens using real tokenizers from OpenAI (tiktoken) and Anthropic.

Usage:
    echo "text" | python3 token_count.py                  # both tokenizers
    echo "text" | python3 token_count.py --openai          # OpenAI only
    echo "text" | python3 token_count.py --claude          # Claude only
    python3 token_count.py --file path/to/file             # read from file
    python3 token_count.py --file path/to/file --openai    # OpenAI count only

Output format (tab-separated):
    <openai_tokens>\t<claude_tokens>

If --openai or --claude is specified, outputs a single number.
If a tokenizer is unavailable, outputs "N/A" for that column.

Tokenizers:
    OpenAI:  tiktoken o200k_base (GPT-4o, the current default)
    Claude:  anthropic SDK count_tokens API (requires ANTHROPIC_API_KEY)
"""

import sys
import os
import argparse


def count_openai(text: str) -> int | None:
    """Count tokens using tiktoken (offline, no API key needed)."""
    try:
        import tiktoken
        enc = tiktoken.get_encoding("o200k_base")  # GPT-4o tokenizer
        return len(enc.encode(text))
    except ImportError:
        return None


def count_claude(text: str) -> int | None:
    """Count tokens using Anthropic's API (requires ANTHROPIC_API_KEY)."""
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        return None
    try:
        import anthropic
        client = anthropic.Anthropic(api_key=api_key)
        resp = client.messages.count_tokens(
            model="claude-sonnet-4-20250514",
            messages=[{"role": "user", "content": text}],
        )
        return resp.input_tokens
    except Exception:
        return None


def main():
    parser = argparse.ArgumentParser(description="Count tokens with real tokenizers")
    parser.add_argument("--file", "-f", help="Read input from file instead of stdin")
    parser.add_argument("--openai", action="store_true", help="OpenAI count only")
    parser.add_argument("--claude", action="store_true", help="Claude count only")
    args = parser.parse_args()

    # If neither flag is set, do both
    if not args.openai and not args.claude:
        args.openai = True
        args.claude = True

    # Read input
    if args.file:
        with open(args.file, "r", errors="replace") as f:
            text = f.read()
    else:
        text = sys.stdin.read()

    openai_count = count_openai(text) if args.openai else None
    claude_count = count_claude(text) if args.claude else None

    # Output
    if args.openai and not args.claude:
        print(openai_count if openai_count is not None else "N/A")
    elif args.claude and not args.openai:
        print(claude_count if claude_count is not None else "N/A")
    else:
        o = str(openai_count) if openai_count is not None else "N/A"
        c = str(claude_count) if claude_count is not None else "N/A"
        print(f"{o}\t{c}")


if __name__ == "__main__":
    main()
