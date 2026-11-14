"""tools/devtools/agent_guard.py: the commands the Bash hook blocks and the ones it lets through."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]

from tools import python_env  # noqa: E402
from tools.devtools import agent_guard  # noqa: E402

BLOCKED = [
    "sleep 120",
    "sleep 58; sleep 58; tail -3 /tmp/x",
    "uv run pytest tests/patchkit",
    "mise run test:full",
    "tail -f out/runs/x.log",
]

ALLOWED = [
    "sleep 60",
    "uv run pytest tests/patchkit/test_patchkit.py",
    "mise run test",
    'git commit -m "the hook blocks --tier full, test:full and sleep 300"',
    "echo 'tail -f x'",
    'git commit -m "a \\"quoted\\" sleep 120"',
]


@pytest.mark.parametrize("cmd", BLOCKED)
def test_blocked(cmd: str) -> None:
    assert agent_guard.check(cmd, ROOT) is not None


@pytest.mark.parametrize("cmd", ALLOWED)
def test_allowed(cmd: str) -> None:
    assert agent_guard.check(cmd, ROOT) is None


def run_hook(stdin: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-m", "tools.devtools.agent_guard"],
        input=stdin,
        capture_output=True,
        text=True,
        cwd=ROOT,
        env=python_env(),
        check=False,
    )


def event(command: str, tool: str = "Bash") -> str:
    return json.dumps(
        {"tool_name": tool, "tool_input": {"command": command}, "cwd": str(ROOT)}
    )


def test_main_blocks_with_exit_2() -> None:
    r = run_hook(event("sleep 600"))
    assert r.returncode == 2
    assert "agent_guard" in r.stderr


def test_main_passes() -> None:
    assert run_hook(event("DIGI_GUARD=off sleep 600")).returncode == 0
    assert run_hook(event("sleep 600", tool="Read")).returncode == 0
    assert run_hook("not json").returncode == 0
