"""Static per-function SHARC cost estimate, computed from the sharcdb on the fly.

What it estimates: the number of instructions the emulator executes for one
call of a function, the unit of `devices/cycle-factors.json` and `patchkit
budget`. There is no stall model (no memory wait states, no pipeline effects).

It is an upper bound over all paths: every instruction of the function counts
once per iteration of the DO loops that contain it, and conditional branches
are not pruned (both sides of an if are counted). Callees are added times the
multiplier of their call site, down to `max_depth`; recursion is cut and
noted. Indirect calls and jumps have no known target and are only counted in
`unknown_calls`.

Trip counts are literal when the DO instruction carries an immediate and
symbolic (the register name, e.g. R2) when it loads the count from a register.
Costs are polynomials in these names; `Poly.evaluate` substitutes values.

Run it through the firmware query tool (needs out/sharcdb/<image>.sqlite):

    fw.py cost ADDR -i dt2-1.17 [--vars R2=32,R12=16] [--no-calls]
"""

from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass, field

from tools.sharc_dev import sharcinv
from tools.sharc_dev.sharcfn import sign_extend
from tools.sharc_dev.sharcfn.decode import ureg_name

_Key = tuple[str, ...]


class Poly:
    """Immutable linear combination of monomials: sorted symbol tuple -> int."""

    __slots__ = ("_t",)

    def __init__(self, terms: dict[_Key, int] | None = None):
        self._t: tuple[tuple[_Key, int], ...] = tuple(
            sorted(
                ((tuple(sorted(k)), c) for k, c in (terms or {}).items() if c),
                key=lambda kc: (len(kc[0]), kc[0]),
            )
        )

    @staticmethod
    def const(n: int) -> Poly:
        return Poly({(): n})

    @staticmethod
    def sym(name: str) -> Poly:
        return Poly({(name,): 1})

    @property
    def terms(self) -> dict[_Key, int]:
        return dict(self._t)

    @property
    def is_const(self) -> bool:
        return all(k == () for k, _ in self._t)

    @property
    def value(self) -> int:
        return self.terms.get((), 0)

    def __add__(self, other: Poly) -> Poly:
        out = self.terms
        for k, c in other._t:
            out[k] = out.get(k, 0) + c
        return Poly(out)

    def __mul__(self, other: Poly) -> Poly:
        out: dict[_Key, int] = {}
        for k1, c1 in self._t:
            for k2, c2 in other._t:
                k = tuple(sorted(k1 + k2))
                out[k] = out.get(k, 0) + c1 * c2
        return Poly(out)

    def scale(self, n: int) -> Poly:
        return Poly({k: c * n for k, c in self._t})

    def evaluate(self, vars: dict[str, int]) -> Poly:
        out: dict[_Key, int] = {}
        for k, c in self._t:
            rest = []
            for s in k:
                if s in vars:
                    c *= vars[s]
                else:
                    rest.append(s)
            key = tuple(rest)
            out[key] = out.get(key, 0) + c
        return Poly(out)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Poly) and self._t == other._t

    def __hash__(self) -> int:
        return hash(self._t)

    def __str__(self) -> str:
        if not self._t:
            return "0"
        parts = []
        for k, c in self._t:
            if not k:
                parts.append(str(c))
            else:
                mono = "*".join(k)
                parts.append(mono if c == 1 else "%d*%s" % (c, mono))
        return " + ".join(parts)

    __repr__ = __str__


@dataclass(frozen=True)
class Loop:
    do_sw: int
    start_sw: int
    end_sw: int  # inclusive: the last body instruction
    trip: int | str  # literal count, or the register name
    depth: int  # 1 for an outermost loop
    body_insns: int


@dataclass(frozen=True)
class FnCost:
    entry_sw: int
    n_insns: int
    loops: list[Loop]
    self_cost: Poly
    dm_loads: Poly
    dm_stores: Poly
    calls: list[tuple[int, Poly, bool]]  # (callee_sw, multiplier, is_tail_jump)
    unknown_calls: int
    total: Poly
    notes: list[str] = field(default_factory=list)


def _trip_poly(trip: int | str) -> Poly:
    return Poly.const(trip) if isinstance(trip, int) else Poly.sym(trip)


@dataclass
class _Own:
    """Everything of one function that does not need its callees."""

    n_insns: int
    loops: list[Loop]
    self_cost: Poly
    dm_loads: Poly
    dm_stores: Poly
    calls: list[tuple[int, Poly, bool]]
    unknown_calls: int
    notes: list[str]


def _own(db: sqlite3.Connection, image: str, entry: int) -> _Own:
    rows = db.execute(
        "SELECT sw, form, fields, width FROM insn "
        "WHERE image=? AND function_sw=? AND aligned=1 ORDER BY sw",
        (image, entry),
    ).fetchall()
    notes: list[str] = []
    if not rows:
        notes.append("no instructions for function 0x%x" % entry)
    raw_loops: list[tuple[int, int, int, int | str]] = []
    for sw, form, fields, width in rows:
        if form not in ("12a_imm", "12a_ureg"):
            continue
        f = sharcinv.merge_fields(json.loads(fields))
        reladdr = f.get("reladdr")
        if reladdr is None:
            continue
        # width is in bytes; sw counts 16-bit words (same as cfg.py).
        start = sw + (width or 6) // 2
        # Inclusive: end is the last body instruction, as cfg.py's end_sw.
        end = sw + sign_extend(reladdr, 23)
        trip: int | str
        trip = f.get("data", 0) if form == "12a_imm" else ureg_name(f.get("ureg", 0))
        raw_loops.append((sw, start, end, trip))

    def mult_at(sw: int) -> Poly:
        m = Poly.const(1)
        for _do, start, end, trip in raw_loops:
            if start <= sw <= end:
                m = m * _trip_poly(trip)
        return m

    mults = {sw: mult_at(sw) for sw, *_ in rows}
    loops = []
    for do_sw, start, end, trip in raw_loops:
        depth = sum(1 for _d, s, e, _t in raw_loops if s <= start <= e)
        body = sum(1 for sw, *_ in rows if start <= sw <= end)
        loops.append(Loop(do_sw, start, end, trip, depth, body))

    self_cost = Poly()
    for m in mults.values():
        self_cost = self_cost + m
    loads, stores = Poly(), Poly()
    for sw, direction in db.execute(
        "SELECT m.sw, m.direction FROM mem_access m JOIN insn i "
        "ON i.image=m.image AND i.sw=m.sw "
        "WHERE m.image=? AND i.function_sw=? AND i.aligned=1 AND m.space='DM'",
        (image, entry),
    ):
        m = mults.get(sw, Poly.const(1))
        if direction == "store":
            stores = stores + m
        else:
            loads = loads + m

    calls: list[tuple[int, Poly, bool]] = []
    unknown = 0
    tails: dict[int, Poly] = {}
    for from_sw, to_sw, kind, to_fn in db.execute(
        "SELECT from_sw, to_sw, kind, to_function FROM edges "
        "WHERE image=? AND from_function=? AND kind IN ('call','jump','indirect')",
        (image, entry),
    ):
        mult = mults.get(from_sw, Poly.const(1))
        if kind == "indirect":
            unknown += 1
        elif kind == "call":
            target = to_fn if to_fn is not None else to_sw
            if target is None:
                unknown += 1
            else:
                calls.append((target, mult, False))
        elif to_fn is not None and to_fn != entry:
            # Tail jumps are alternatives: one entry per target function, with
            # the largest multiplier (compared with every symbol set to 1).
            old = tails.get(to_fn)
            if old is None or sum(mult.terms.values()) > sum(old.terms.values()):
                tails[to_fn] = mult
    calls.extend((t, m, True) for t, m in sorted(tails.items()))
    return _Own(len(rows), loops, self_cost, loads, stores, calls, unknown, notes)


def fn_cost(
    db: sqlite3.Connection,
    image: str,
    entry_sw: int,
    *,
    follow_calls: bool = True,
    max_depth: int = 8,
) -> FnCost:
    owns: dict[int, _Own] = {}

    def own(e: int) -> _Own:
        if e not in owns:
            owns[e] = _own(db, image, e)
        return owns[e]

    notes: list[str] = []
    memo: dict[tuple[int, int], Poly] = {}

    def total(e: int, path: tuple[int, ...], depth: int) -> Poly:
        o = own(e)
        t = o.self_cost
        if not follow_calls:
            return t
        for callee, mult, _tail in o.calls:
            if callee in path or callee == e:
                note = "recursion cut at 0x%x -> 0x%x" % (e, callee)
                if note not in notes:
                    notes.append(note)
                continue
            if depth <= 0:
                note = "depth cut at 0x%x -> 0x%x" % (e, callee)
                if note not in notes:
                    notes.append(note)
                continue
            key = (callee, depth - 1)
            if key not in memo:
                memo[key] = total(callee, path + (callee,), depth - 1)
            if not own(callee).n_insns:
                note = "missing function 0x%x" % callee
                if note not in notes:
                    notes.append(note)
            t = t + mult * memo[key]
        return t

    top = own(entry_sw)
    tot = total(entry_sw, (entry_sw,), max_depth)
    return FnCost(
        entry_sw,
        top.n_insns,
        top.loops,
        top.self_cost,
        top.dm_loads,
        top.dm_stores,
        top.calls,
        top.unknown_calls,
        tot,
        top.notes + notes,
    )
