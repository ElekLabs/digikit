"""Static per-function SHARC cost estimate, computed from the sharcdb on the fly.

What it estimates: the number of instructions the emulator executes for one
call of a function, the unit of `devices/cycle-factors.json` and `patchkit
budget`. There is no stall model (no memory wait states, no pipeline effects).

It is an upper bound over all paths: every instruction of the function counts
once per iteration of the DO loops that contain it, and conditional branches
are not pruned (both sides of an if are counted). Callees are added times the
multiplier of their call site, down to `max_depth`; a depth cut and a recursion
cut are noted (a result that hit a recursion cut is never cached, so totals do
not depend on query order). Indirect calls and jumps have no known target and
are only counted in `unknown_calls`.

Jumps to another function (conditional or not) are treated as tail calls. The
targets of one function are alternatives, but `Poly` has no max, so the total
sums them: "tail targets summed (upper bound)". With `values` that make every
alternative constant, `FnCost.total_max` takes the largest alternative instead.

Trip counts are literal when the DO instruction carries a non-zero immediate and
symbolic when it loads the count from a register. A symbol names the register
and the function it is read in: `R2@1cd286` (register@function entry, hex
without 0x), because a callee's R2 is not the caller's R2. A missing immediate
or a count of 0 (the PRM: LCNTR=0 runs 2^32 times) gives the unknown trip
`trip?@fn`; DO UNTIL loops (form 13a) are costed once and noted.
Costs are polynomials in these symbols; `Poly.evaluate` substitutes values.

Run it through the firmware query tool (needs out/sharcdb/<image>.sqlite):

    fw.py cost ADDR -i <image> [--vars R2=32,R2@1cd286=16] [--vars-all R12=32]
                                [--no-calls]

`--vars NAME=V` sets the entry function's register, `NAME@FN=V` that of one
function, `--vars-all NAME=V` the register in every function.
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

    def symbols(self) -> set[str]:
        return {s for k, _ in self._t for s in k}

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
    total: Poly  # tail targets summed (upper bound)
    notes: list[str] = field(default_factory=list)
    # Total of each entry of `calls` as the main total used it (None: cut or
    # not followed), so that self + sum(mult * callee) equals `total`.
    call_totals: list[Poly | None] = field(default_factory=list)
    # Tail alternatives replaced by their maximum, `values` substituted; None
    # when no `values` were given or an alternative stays symbolic.
    total_max: Poly | None = None


def _sym(reg: str, entry: int) -> str:
    return "%s@%x" % (reg, entry)


def split_sym(sym: str) -> tuple[str, str]:
    """`R2@1cd286` -> ('R2', '1cd286')."""
    name, _, fn = sym.rpartition("@")
    return name, fn


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
        if form == "13a":
            notes.append(
                "unhandled DO UNTIL (form 13a) at 0x%x in 0x%x: costed once"
                % (sw, entry)
            )
            continue
        if form not in ("12a_imm", "12a_ureg"):
            continue
        f = sharcinv.merge_fields(json.loads(fields))
        reladdr = f.get("reladdr")
        if reladdr is None:
            notes.append("DO at 0x%x without reladdr: ignored" % sw)
            continue
        # width is in bytes; sw counts 16-bit words (same as cfg.py).
        start = sw + (width or 6) // 2
        # Inclusive: end is the last body instruction, as cfg.py's end_sw.
        end = sw + sign_extend(reladdr, 23)
        trip: int | str
        if form == "12a_imm":
            data = f.get("data")
            if data is None:
                notes.append("DO at 0x%x has no count data: trip unknown" % sw)
                trip = _sym("trip?", entry)
            elif data == 0:
                notes.append(
                    "DO with count 0 at 0x%x: runs 2^32 times (PRM); trip unknown" % sw
                )
                trip = _sym("trip?", entry)
            else:
                trip = data
        else:
            trip = _sym(ureg_name(f.get("ureg", 0)), entry)
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
    miss_mem: list[int] = []
    for sw, direction in db.execute(
        "SELECT m.sw, m.direction FROM mem_access m JOIN insn i "
        "ON i.image=m.image AND i.sw=m.sw "
        "WHERE m.image=? AND i.function_sw=? AND i.aligned=1 AND m.space='DM'",
        (image, entry),
    ):
        mm = mults.get(sw)
        if mm is None:
            miss_mem.append(sw)
            mm = Poly.const(1)
        if direction == "store":
            stores = stores + mm
        else:
            loads = loads + mm

    if miss_mem:
        notes.append(
            "%d mem_access rows of 0x%x not in its instruction rows, costed x1: %s"
            % (len(miss_mem), entry, " ".join("0x%x" % a for a in miss_mem[:3]))
        )
    miss_edge: list[int] = []
    cond_tail = False
    calls: list[tuple[int, Poly, bool]] = []
    unknown = 0
    tails: dict[int, Poly] = {}
    for from_sw, to_sw, kind, to_fn, cond in db.execute(
        "SELECT from_sw, to_sw, kind, to_function, cond FROM edges "
        "WHERE image=? AND from_function=? AND kind IN ('call','jump','indirect')",
        (image, entry),
    ):
        found = mults.get(from_sw)
        if found is None:
            miss_edge.append(from_sw)
        mult = found if found is not None else Poly.const(1)
        if kind == "indirect":
            unknown += 1
        elif kind == "call":
            target = to_fn if to_fn is not None else to_sw
            if target is None:
                unknown += 1
            else:
                calls.append((target, mult, False))
        elif to_fn is not None and to_fn != entry:
            # Tail jumps (conditional ones too) are alternatives: one entry per
            # target function, with the largest multiplier (compared with every
            # symbol set to 1).
            cond_tail = cond_tail or bool(cond)
            old = tails.get(to_fn)
            if old is None or sum(mult.terms.values()) > sum(old.terms.values()):
                tails[to_fn] = mult
    calls.extend((t, m, True) for t, m in sorted(tails.items()))
    if miss_edge:
        notes.append(
            "%d call/jump edges of 0x%x not in its instruction rows, costed x1: %s"
            % (len(miss_edge), entry, " ".join("0x%x" % a for a in miss_edge[:3]))
        )
    if cond_tail:
        notes.append(
            "0x%x: conditional jumps to another function are treated as tail calls"
            % entry
        )
    return _Own(len(rows), loops, self_cost, loads, stores, calls, unknown, notes)


def fn_cost(
    db: sqlite3.Connection,
    image: str,
    entry_sw: int,
    *,
    follow_calls: bool = True,
    max_depth: int = 8,
    values: dict[str, int] | None = None,
) -> FnCost:
    """Cost of one call of the function at `entry_sw`.

    `values` maps qualified symbols (`R2@1cd286`) to numbers. They do not change
    `total`; they only fill `total_max` (tail alternatives by their maximum).
    """
    owns: dict[int, _Own] = {}

    def own(e: int) -> _Own:
        if e not in owns:
            owns[e] = _own(db, image, e)
        return owns[e]

    notes: list[str] = []

    def note(text: str) -> None:
        if text not in notes:
            notes.append(text)

    def run(tmax: dict[str, int] | None) -> tuple[Poly, list[Poly | None]]:
        # Only results free of a recursion cut are cached: a cut depends on the
        # path, so such a result would make totals depend on query order.
        memo: dict[tuple[int, int], Poly] = {}
        top_calls: list[Poly | None] = []

        def total(
            e: int, path: tuple[int, ...], depth: int, record: list[Poly | None] | None
        ) -> tuple[Poly, bool]:
            o = own(e)
            t = o.self_cost
            if tmax is not None:
                t = t.evaluate(tmax)
            if not follow_calls:
                return t, False
            cut = False
            tails: list[Poly] = []
            for callee, mult, tail in o.calls:
                sub: Poly | None = None
                if callee in path:
                    note("recursion cut at 0x%x -> 0x%x" % (e, callee))
                    cut = True
                elif depth <= 0:
                    note("depth cut at 0x%x -> 0x%x" % (e, callee))
                else:
                    key = (callee, depth - 1)
                    if key in memo:
                        sub = memo[key]
                    else:
                        got, c = total(callee, path + (callee,), depth - 1, None)
                        sub = got
                        if c:
                            cut = True
                        else:
                            memo[key] = sub
                    if not own(callee).n_insns:
                        note("missing function 0x%x" % callee)
                if record is not None:
                    record.append(sub)
                if sub is None:
                    continue
                term = mult * sub
                if tmax is not None:
                    term = term.evaluate(tmax)
                if tail:
                    tails.append(term)
                else:
                    t = t + term
            if len(tails) > 1:
                if tmax is None:
                    note("tail targets of 0x%x summed (upper bound)" % e)
                if tmax is not None and all(x.is_const for x in tails):
                    t = t + max(tails, key=lambda x: x.value)
                else:
                    for x in tails:
                        t = t + x
            else:
                for x in tails:
                    t = t + x
            return t, cut

        tot, _cut = total(entry_sw, (entry_sw,), max_depth, top_calls)
        return tot, top_calls

    tot, call_totals = run(None)
    total_max = None
    if values and any(sum(1 for c in o.calls if c[2]) > 1 for o in owns.values()):
        total_max, _ = run(values)
    top = own(entry_sw)
    all_notes = list(top.notes)
    for e, o in owns.items():
        if e != entry_sw:
            all_notes.extend(n for n in o.notes if not n.startswith("no instructions"))
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
        all_notes + notes,
        call_totals,
        total_max,
    )
