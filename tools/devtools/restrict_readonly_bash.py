#!/usr/bin/env python3
"""Fail-closed Bash policy for read-only agents.

This script is the PreToolUse hook of the read-only project agents (`.claude/agents/`).

Default profile (reviewer): read-only Git commands (including ``git grep``) and
the read-only Nx graph queries ``pnpm nx:pilot show target|project|projects ...``
(``--check PATH`` answers whether a file is a target input).
``--profile architect``: the same, plus the read-only firmware query CLI
``uv run python -m tools.coldfire.fw SUBCOMMAND ...``.
"""

import json
import re
import shlex
import sys
from typing import NoReturn

ALLOWED_SUBCOMMANDS = {
    "blame",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "grep",
    "log",
    "ls-files",
    "ls-tree",
    "merge-base",
    "rev-parse",
    "show",
    "status",
}

ALLOWED_GLOBAL_FLAGS = {
    "--literal-pathspecs",
    "--no-optional-locks",
    "--no-pager",
}

FW_PREFIX = ["uv", "run", "python", "-m", "tools.coldfire.fw"]
FW_SUBCOMMANDS = {
    "atlas",
    "callees",
    "callers",
    "catalog",
    "class",
    "decomp",
    "disasm",
    "dossier",
    "fn",
    "path",
    "port",
    "search",
    "sql",
    "stores",
    "str",
    "xref",
}
NX_SHOW_PREFIX = ["pnpm", "nx:pilot", "show"]
NX_SHOW_SUBCOMMANDS = {"project", "projects", "target"}
PROFILES = {"reviewer", "architect"}

FORBIDDEN_ARGUMENTS = {
    "--ext-diff",
    "--no-index",
    "--paginate",
    "--textconv",
    "-p",
    # git grep: runs a program on the matches
    "-O",
}


PROFILE = "reviewer"


def deny(reason: str) -> NoReturn:
    print(f"Blocked {PROFILE} Bash command: {reason}", file=sys.stderr)
    raise SystemExit(2)


def parse_profile() -> str:
    args = sys.argv[1:]
    if not args:
        return "reviewer"
    if len(args) == 2 and args[0] == "--profile" and args[1] in PROFILES:
        return args[1]
    deny("hook was configured with an unknown profile")
    return "reviewer"


def check_fw(words: list[str]) -> None:
    rest = words[len(FW_PREFIX) :]
    if not rest or rest[0] not in FW_SUBCOMMANDS:
        deny("fw.py subcommand is not on the architect allowlist")
    print("{}")


def check_nx_show(words: list[str]) -> None:
    rest = words[len(NX_SHOW_PREFIX) :]
    if not rest or rest[0] not in NX_SHOW_SUBCOMMANDS:
        deny("only `pnpm nx:pilot show target|project|projects` is allowed")
    print("{}")


def main() -> None:
    global PROFILE
    PROFILE = parse_profile()
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        deny("hook input was not valid JSON")

    tool_input = payload.get("tool_input")
    command = tool_input.get("command") if isinstance(tool_input, dict) else None
    if not isinstance(command, str) or not command.strip():
        deny("missing command")

    if re.search(r"[\n;&|<>`]", command) or "$(" in command:
        deny("shell operators, substitutions, and redirections are not allowed")

    try:
        words = shlex.split(command, posix=True)
    except ValueError:
        deny("command could not be parsed")

    if PROFILE == "architect" and words[: len(FW_PREFIX)] == FW_PREFIX:
        check_fw(words)
        return

    if words[: len(NX_SHOW_PREFIX)] == NX_SHOW_PREFIX:
        check_nx_show(words)
        return

    if not words or words[0] != "git":
        deny("only read-only Git commands are allowed")

    index = 1
    saw_directory = False
    while index < len(words):
        word = words[index]
        if word in ALLOWED_GLOBAL_FLAGS:
            index += 1
            continue
        if word == "-C" and not saw_directory:
            if index + 1 >= len(words):
                deny("git -C requires a directory")
            saw_directory = True
            index += 2
            continue
        break

    if index >= len(words) or words[index] not in ALLOWED_SUBCOMMANDS:
        deny(f"Git subcommand is not on the {PROFILE} allowlist")

    for word in words[index + 1 :]:
        if word in FORBIDDEN_ARGUMENTS:
            deny(f"{word} is not allowed")
        if word == "-c" or word.startswith("-c="):
            deny("Git configuration overrides are not allowed")
        if word.startswith("--config-env") or word.startswith("--exec-path"):
            deny("Git execution/configuration overrides are not allowed")
        if word.startswith("--open-files-in-pager") or (
            word.startswith("-O") and len(word) > 2
        ):
            deny("commands may not run a program on the matches")
        if word.startswith("--output"):
            deny("commands may not write output files")

    print("{}")


if __name__ == "__main__":
    main()
