"""
Baseline tools for the accuracy benchmark.

Gives the agent grep, read, and list_directory — the universal
toolset every code agent has without indxr.
"""

import os
import subprocess
from pathlib import Path


def make_baseline_tools() -> list[dict]:
    """Return tool definitions in Anthropic format."""
    return [
        {
            "name": "grep_codebase",
            "description": (
                "Search for a regex pattern across files in the codebase. "
                "Returns matching lines with file paths and line numbers. "
                "Use this to find where functions, types, or patterns are defined or used."
            ),
            "input_schema": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for",
                    },
                    "path": {
                        "type": "string",
                        "description": (
                            "Subdirectory to scope the search (relative to repo root). "
                            "Omit to search everywhere."
                        ),
                    },
                    "glob": {
                        "type": "string",
                        "description": "File glob pattern to filter (e.g. '*.rs', '*.py')",
                    },
                },
                "required": ["pattern"],
            },
        },
        {
            "name": "read_file",
            "description": (
                "Read the contents of a source file. Returns lines with line numbers. "
                "Use start_line and end_line to read a specific section. "
                "Capped at 300 lines per call to keep context manageable."
            ),
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the repo root",
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "First line to read (1-based). Omit to start from line 1.",
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Last line to read (1-based, inclusive). Omit to read to end.",
                    },
                },
                "required": ["path"],
            },
        },
        {
            "name": "list_directory",
            "description": (
                "List files and subdirectories at a path. "
                "Use this to understand the project layout before reading files."
            ),
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": (
                            "Directory path relative to repo root. "
                            "Omit or use '.' for the project root."
                        ),
                    },
                },
                "required": [],
            },
        },
    ]


class BaselineToolkit:
    """Executes baseline tools (grep, read, ls) against a repo directory."""

    def __init__(self, repo_path: str):
        self.repo = Path(repo_path).resolve()
        self._rg = self._find_rg()

    def _find_rg(self) -> str | None:
        """Find ripgrep binary."""
        try:
            subprocess.run(
                ["rg", "--version"], capture_output=True, check=True
            )
            return "rg"
        except (FileNotFoundError, subprocess.CalledProcessError):
            return None

    def execute(self, name: str, args: dict) -> str:
        """Execute a baseline tool by name."""
        if name == "grep_codebase":
            return self._grep(args)
        elif name == "read_file":
            return self._read(args)
        elif name == "list_directory":
            return self._list_dir(args)
        else:
            return f"Unknown tool: {name}"

    def _grep(self, args: dict) -> str:
        pattern = args.get("pattern", "")
        if not pattern:
            return "Error: pattern is required"

        search_path = str(self.repo)
        if args.get("path"):
            search_path = str(self.repo / args["path"])

        if self._rg:
            cmd = [
                "rg", "-n", "--no-heading", "--max-count", "50",
                "-e", pattern,
            ]
            if args.get("glob"):
                cmd.extend(["-g", args["glob"]])
            cmd.append(search_path)
        else:
            cmd = ["grep", "-rn", f"--max-count=50", pattern, search_path]

        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=10,
                cwd=str(self.repo),
            )
            output = result.stdout.strip()
            if not output:
                return "No matches found."
            # Make paths relative to repo root
            output = output.replace(str(self.repo) + "/", "")
            lines = output.split("\n")
            if len(lines) > 50:
                return (
                    "\n".join(lines[:50])
                    + f"\n... ({len(lines) - 50} more matches truncated)"
                )
            return output
        except subprocess.TimeoutExpired:
            return "Search timed out (10s limit)."
        except Exception as e:
            return f"Grep error: {e}"

    def _read(self, args: dict) -> str:
        rel_path = args.get("path", "")
        if not rel_path:
            return "Error: path is required"

        full_path = self.repo / rel_path
        if not full_path.exists():
            return f"Error: file not found: {rel_path}"
        if not full_path.is_file():
            return f"Error: not a file: {rel_path}"

        # Security: prevent path traversal
        try:
            full_path.resolve().relative_to(self.repo)
        except ValueError:
            return f"Error: path outside repo: {rel_path}"

        try:
            all_lines = full_path.read_text(errors="replace").split("\n")
        except Exception as e:
            return f"Read error: {e}"

        start = max(1, args.get("start_line", 1))
        end = args.get("end_line", len(all_lines))

        # Cap at 300 lines per call
        max_lines = 300
        if end - start + 1 > max_lines:
            end = start + max_lines - 1

        selected = all_lines[start - 1 : end]
        numbered = [f"{start + i:4d} | {line}" for i, line in enumerate(selected)]

        header = f"File: {rel_path} (lines {start}-{end} of {len(all_lines)})"
        return header + "\n" + "\n".join(numbered)

    def _list_dir(self, args: dict) -> str:
        rel_path = args.get("path", ".")
        full_path = self.repo / rel_path

        if not full_path.exists():
            return f"Error: path not found: {rel_path}"
        if not full_path.is_dir():
            return f"Error: not a directory: {rel_path}"

        entries = []
        try:
            for item in sorted(full_path.iterdir()):
                name = item.name
                if name.startswith("."):
                    continue
                if name in {"target", "node_modules", "__pycache__", ".git"}:
                    continue
                prefix = "[dir] " if item.is_dir() else "[file]"
                entries.append(f"  {prefix} {name}")
        except Exception as e:
            return f"Error listing directory: {e}"

        if not entries:
            return f"Directory '{rel_path}' is empty (or all entries hidden)."

        return f"Contents of {rel_path}/:\n" + "\n".join(entries)
