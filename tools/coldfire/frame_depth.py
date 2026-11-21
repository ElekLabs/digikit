#!/usr/bin/env python3
"""Static worst-case stack depth of the functions in a patchkit object (`build/*.dko.json`).

    uv run python -m tools.coldfire.frame_depth runtime/core/build/runtime.dko.json dkrt_load dkrt_unload ...

Each `.text.*` section is one function. A linear scan of its code follows the stack pointer through `lea -N(a7),a7`,
`link`, `pea`, `move.l x,-(a7)`, `movem.l ..,-(a7)`, `subq/addq/subi/addi/suba/adda #N,a7` and the matching releases; at
every `jsr`/`bsr` the pushed bytes plus the return address are added to the callee's depth (the callee found through the
object's relocations). Not followed: calls through a register or memory (reported as `indirect`), and the firmware
functions the object imports (reported by name with the depth at the call, so the caller can add what the firmware's own
function needs, measured with `DIGI_STACK_WATCH`, docs/INSTRUMENTATION.md). Branches are not followed: the scan is straight
through the function, which over-counts a frame that is released and pushed again on another path and cannot under-count
a `lea`-built frame (gcc allocates the whole frame in the prologue). Recursion is cut (and reported).
"""

from __future__ import annotations

import argparse
import json
import re
import sys

from tools.coldfire.disasm import disasm


def _num(t: str) -> int:
    t = t.strip().lstrip("#")
    neg = t.startswith("-")
    t = t.lstrip("-").lstrip("$")
    v = int(t, 16) if re.fullmatch(r"[0-9a-f]+", t) else int(t, 0)
    return -v if neg else v


def _regs_count(regs: str) -> int:
    n = 0
    for part in regs.split("/"):
        if "-" in part:
            a, b = part.split("-")
            n += int(b[1:]) - int(a[1:]) + 1
        else:
            n += 1
    return n


class Fn:
    def __init__(self, name: str) -> None:
        self.name = name
        self.ext = 0  # largest extent (bytes below the entry stack pointer, return address excluded)
        self.calls: list[
            tuple[int, str]
        ] = []  # (extent at the call, callee symbol or "indirect")


def scan(sec: dict, relocs: list[dict]) -> Fn:
    f = Fn(sec["name"])
    code = bytes.fromhex(sec["hex"])
    at = {r["offset"]: r["symbol"] for r in relocs if r["section"] == sec["name"]}
    sp = 0  # bytes pushed (positive = deeper)
    regsym: dict[
        str, str
    ] = {}  # address registers loaded with a relocated symbol (`lea sym,a4`), for `jsr (a4)`
    for a, _, mn, ops in disasm(code, 0, 0, len(code)):
        m = mn.split(".")[0]
        o = ops.replace(" ", "")
        if m == "lea" and (g := re.fullmatch(r"(-?\$?[0-9a-f]+)\(a7\),a7", o)):
            sp -= _num(g.group(1))
        elif m in ("subq", "subi", "suba") and (
            g := re.fullmatch(r"#(\$?[0-9a-f]+),a7", o)
        ):
            sp += _num(g.group(1))
        elif m in ("addq", "addi", "adda") and (
            g := re.fullmatch(r"#(\$?[0-9a-f]+),a7", o)
        ):
            sp -= _num(g.group(1))
        elif m == "link" and (g := re.fullmatch(r"a\d,#(-?\$?[0-9a-f]+)", o)):
            sp += 4 - _num(g.group(1))
        elif m == "unlk":
            sp = max(0, sp - 4)
        elif m == "pea" or (m == "move" and o.endswith(",-(a7)")):
            sp += 4
        elif m == "movem" and o.endswith(",-(a7)"):
            sp += 4 * _regs_count(o.split(",")[0])
        elif m == "movem" and o.startswith("(a7)+,"):
            sp -= 4 * _regs_count(o.split(",")[1])
        elif m == "move" and o.startswith("(a7)+,"):
            sp -= 4
        f.ext = max(f.ext, sp)
        if (
            (g := re.fullmatch(r".*,(a\d)", o))
            and (a + 2) in at
            and m in ("lea", "movea", "move")
        ):
            regsym[g.group(1)] = at[a + 2]
        if m in ("jsr", "bsr"):
            reg = re.fullmatch(r"\((a\d)\)", o)
            f.calls.append(
                (
                    sp,
                    at.get(a + 2)
                    or (regsym.get(reg.group(1)) if reg else None)
                    or "indirect",
                )
            )
        if m == "rts":
            sp = 0
    return f


def depths(path: str) -> tuple[dict[str, Fn], dict[str, str]]:
    d = json.load(open(path))  # noqa: SIM115
    fns = {
        ".sec." + s["name"]: scan(s, d["relocs"])
        for s in d["sections"]
        if s["kind"] == "text"
    }
    # the exports are named symbols for a section
    export = {
        s["name"]: ".sec." + s["section"]
        for s in d["symbols"]
        if s["section"] in {x["name"] for x in d["sections"]}
    }
    # a relocation names an exported function by its symbol, a private one by its section
    for sy in d["symbols"]:
        if sy["section"] and ".sec." + sy["section"] in fns:
            fns.setdefault(sy["name"], fns[".sec." + sy["section"]])
    return fns, export


def worst(fns: dict[str, Fn], key: str, stack: tuple = ()) -> tuple[int, list[str]]:
    """(bytes, chain) the function may use below its caller's stack pointer after the return address, with the longest chain."""
    f = fns[key]
    best, chain = f.ext, [f.name]
    unresolved: list[str] = []
    for ext, callee in f.calls:
        if callee in fns and callee not in stack + (key,):
            sub, sc = worst(fns, callee, stack + (key,))
            if ext + 4 + sub > best:
                best, chain = ext + 4 + sub, [f.name, *sc]
        else:
            unresolved.append(f"{callee}@{ext}")
            if ext + 4 > best:
                best, chain = ext + 4, [f.name, f"-> {callee}"]
    return best, chain


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("object")
    ap.add_argument(
        "entry", nargs="*", help="exported function names (default: all exports)"
    )
    a = ap.parse_args(argv)
    fns, export = depths(a.object)
    for name in a.entry or sorted(k for k, v in export.items() if v in fns):
        key = export.get(name) or ".sec.." + "text." + name
        if key not in fns:
            print(f"{name}: no such function")
            continue
        n, chain = worst(fns, key)
        print(
            f"{name}: {n} bytes   {' > '.join(c.replace('.text.', '')[-28:] for c in chain)}"
        )
    ext = sorted({c for f in fns.values() for _, c in f.calls if c not in fns})
    print("not followed (firmware imports and indirect calls):", ", ".join(ext))
    return 0


if __name__ == "__main__":
    sys.exit(main())
