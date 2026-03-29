"""
indxr MCP tools for the accuracy benchmark.

Wraps the indxr MCP server subprocess, dynamically fetching tool
definitions so the benchmark stays in sync as tools are added.
"""

import json
import os
import subprocess
import sys
from pathlib import Path


class IndxrToolkit:
    """MCP client that wraps `indxr serve` for the benchmark."""

    def __init__(self, repo_path: str):
        indxr_bin = os.environ.get("INDXR_BIN") or _find_indxr()
        self.proc = subprocess.Popen(
            [indxr_bin, "serve", repo_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        self._msg_id = 0
        self._initialize()
        self._tool_defs = self._fetch_tools()

    # -- MCP JSON-RPC plumbing --

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

    def _initialize(self):
        self._send({
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": "initialize",
            "params": {},
        })
        self._read_response()
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _fetch_tools(self) -> list[dict]:
        """Fetch tool defs from MCP server and convert to Anthropic format."""
        self._send({
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": "tools/list",
        })
        resp = self._read_response()
        mcp_tools = resp.get("result", {}).get("tools", [])

        anthropic_tools = []
        for tool in mcp_tools:
            anthropic_tools.append({
                "name": tool["name"],
                "description": tool.get("description", ""),
                "input_schema": tool.get(
                    "inputSchema",
                    {"type": "object", "properties": {}},
                ),
            })
        return anthropic_tools

    # -- Public interface --

    def get_tools(self) -> list[dict]:
        """Return tool definitions in Anthropic format."""
        return self._tool_defs

    def execute(self, name: str, args: dict) -> str:
        """Execute an MCP tool and return the text result."""
        msg_id = self._next_id()
        self._send({
            "jsonrpc": "2.0",
            "id": msg_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": args},
        })
        resp = self._read_response()

        # Check for JSON-RPC error
        if "error" in resp and resp["error"]:
            return f"MCP error: {resp['error'].get('message', 'unknown')}"

        content = resp.get("result", {}).get("content", [])
        texts = [c.get("text", "") for c in content if c.get("type") == "text"]
        return "\n".join(texts) if texts else "(empty response)"

    def close(self):
        """Shut down the MCP server."""
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()


def _find_indxr() -> str:
    """Locate the indxr binary."""
    script_dir = Path(__file__).parent.parent
    for candidate in [
        script_dir / "target" / "release" / "indxr",
        script_dir / "target" / "debug" / "indxr",
    ]:
        if candidate.exists():
            return str(candidate)
    result = subprocess.run(["which", "indxr"], capture_output=True, text=True)
    if result.returncode == 0:
        return result.stdout.strip()
    print("ERROR: indxr binary not found. Build with: cargo build --release")
    sys.exit(1)
