"""Read-only SQLite and graph primitives for the firmware query command."""

from __future__ import annotations

import json
import os
import pickle
import re
import sqlite3
from collections import deque
from collections.abc import Iterable, Sequence
from typing import Any

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CF_DIR = os.path.join(ROOT, "out", "cfdb")
SHARC_DIR = os.path.join(ROOT, "out", "sharcdb")
ATLAS_DIR = os.path.join(ROOT, "out", "atlas")
CACHE_DIR = os.path.join(ROOT, "out", "cache", "fw")
REFERENCE = os.path.join(ROOT, "ports", "bindings", "reference.json")


class FwError(Exception):
    """A usage or lookup problem reported as one line, not a traceback."""


def capped(lines: Sequence[str], cap: int) -> str:
    lines = list(lines)
    if cap and len(lines) > cap:
        more = len(lines) - cap
        lines = lines[:cap] + ["... %d more (--max N, 0 = all)" % more]
    return "\n".join(lines)


def clip(text: str, n: int = 100) -> str:
    text = " ".join(text.split())
    return text if len(text) <= n else text[: n - 3] + "..."


def short_image(name: str) -> str:
    return name[: -len("-emac")] if name.endswith("-emac") else name


def parse_addr(text: str) -> int | None:
    t = text.strip().lower()
    if t.startswith("fun_") or t.startswith("lab_") or t.startswith("dat_"):
        t = t[4:]
    try:
        if t.startswith("0x"):
            return int(t, 16)
        if re.fullmatch(r"[0-9a-f]{6,8}", t) and re.search(r"[a-f]", t + "a"):
            return int(t, 16)
    except ValueError:
        return None
    return None


def open_ro(path: str) -> sqlite3.Connection:
    if not os.path.exists(path):
        raise FwError("missing database %s" % os.path.relpath(path, ROOT))
    return sqlite3.connect("file:%s?mode=ro" % path, uri=True)


def db_key(con: sqlite3.Connection, path: str) -> str:
    meta = (
        dict(con.execute("SELECT key, value FROM meta").fetchall())
        if _has(con, "meta")
        else {}
    )
    sha = (meta.get("image_sha256") or "nosha")[:12]
    return "%s-%s-%s-%d" % (
        sha,
        meta.get("db_version", "0"),
        (meta.get("build_time") or "")[-9:-1].replace(":", ""),
        os.path.getsize(path),
    )


def _has(con: sqlite3.Connection, table: str) -> bool:
    return (
        con.execute("SELECT 1 FROM sqlite_master WHERE name=?", (table,)).fetchone()
        is not None
    )


class Graph:
    """Function-level call graph as plain dicts (no networkx import)."""

    def __init__(
        self, succ: dict[int, tuple[int, ...]], pred: dict[int, tuple[int, ...]]
    ):
        self.succ = succ
        self.pred = pred

    @staticmethod
    def load(tag: str, key: str, edges: Iterable[tuple[int, int]]) -> tuple[Graph, str]:
        path = os.path.join(CACHE_DIR, "%s.%s.graph.pkl" % (tag, key))
        try:
            with open(path, "rb") as fh:
                succ, pred = pickle.load(fh)
            return Graph(succ, pred), "cached"
        except (OSError, pickle.UnpicklingError, EOFError, ValueError):
            pass
        s: dict[int, set[int]] = {}
        p: dict[int, set[int]] = {}
        for a, b in edges:
            s.setdefault(a, set()).add(b)
            p.setdefault(b, set()).add(a)
        succ = {k: tuple(sorted(v)) for k, v in s.items()}
        pred = {k: tuple(sorted(v)) for k, v in p.items()}
        try:
            os.makedirs(CACHE_DIR, exist_ok=True)
            for old in os.listdir(CACHE_DIR):
                if old.startswith(tag + ".") and old.endswith(".graph.pkl"):
                    os.remove(os.path.join(CACHE_DIR, old))
            tmp = path + ".%d" % os.getpid()
            with open(tmp, "wb") as fh:
                pickle.dump((succ, pred), fh, protocol=pickle.HIGHEST_PROTOCOL)
            os.replace(tmp, path)
        except OSError:
            pass
        return Graph(succ, pred), "built"

    def shortest(self, a: int, b: int, limit: int = 200000) -> list[int] | None:
        if a == b:
            return [a]
        prev = {a: a}
        q = deque([a])
        seen = 0
        while q:
            n = q.popleft()
            for m in self.succ.get(n, ()):
                if m in prev:
                    continue
                prev[m] = n
                if m == b:
                    out = [b]
                    while out[-1] != a:
                        out.append(prev[out[-1]])
                    return out[::-1]
                q.append(m)
            seen += 1
            if seen > limit:
                break
        return None


def load_reference() -> list[dict[str, Any]]:
    try:
        with open(REFERENCE) as fh:
            return json.load(fh)["entries"]
    except (OSError, ValueError, KeyError):
        return []
