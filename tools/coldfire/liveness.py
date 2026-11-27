#!/usr/bin/env python3
"""Register liveness of stock ColdFire code after a patch site, from the image bytes.

    uv run python -m tools.coldfire.liveness PORT SITE [--from ADDR ...] [--depth N] [--trace]
    uv run python -m tools.coldfire.liveness PORT --addr 0x40139804 [--len 6]
    uv run python -m tools.coldfire.liveness --decode HEX [--addr 0x40139804]
    uv run python -m tools.coldfire.liveness PORT SITE --match HEX
    uv run python -m tools.coldfire.liveness PORT --stable FROM TO REG
    uv run python -m tools.coldfire.liveness PORT --preds FUNC ADDR
    uv run python -m tools.coldfire.liveness PORT --writes ADDR [--writes ADDR ...]

PORT is a port file (ports/dt2-1.17.toml) or a tag (dt2-1.17); SITE is a resolved
site name of that port (cf.lfo.site.tick_call) or --addr. The image is
out/sections/<tag>/section_3_MAIN_OS.bin at 0x40000400, checked against `main_sha256` of the
port. Instructions are decoded by pypcode with the repo's ColdFireEMAC SLEIGH language
(tools/coldfire/ghidra); the register effects are read from its p-code, so no encoding table
is kept here.

From each start address the tool follows the stock code along fall-through and branch
targets, up to --depth instructions per path (default 300). For each register (D0-D7,
A0-A6, the accumulators, MACSR and the CCR flags) it reports

  dead   every path writes the whole register before any read, call, return or
         indirect jump (the writing instruction is shown)
  live   some path reads it first (the reading instruction is shown), or reaches a
         call, a return, an indirect jump or the depth bound with the register untouched:
         a callee or the caller may read it, so the tool counts it live
         (the reason says which)

The `--abi gcc` model (a call clobbers d0, d1, a0, a1 and the flags, a return reads d0 and the registers a function keeps) is a guess about compiled code, not a fact of the image. It is
printed for orientation only; no decision (a stub scratch register, a register a routine skips saving) may rest on it. `--stable` and `--writes` use no calling convention: they read
what each callee writes (`jsr (An)` after one `lea (ABS).l,An` is resolved; `--skip ADDR` leaves a named callee out and the output lists it).

A7 is never reported (the stack pointer is always live). A write of less than 32 bits does
not kill a register. Default starts for a site: site+len (fall-through) and the branch target
of the site instruction when it is a branch after the displaced bytes, found by decoding from
the site (`--from` overrides). The trace lists the code from the first start along fall-through
(and unconditional branches) up to the first call, with the byte count: the stock code that
makes the call arguments, which a replacement routine has to reproduce.

--match HEX looks for the bytes HEX in the stock code between the start and the first call and
prints where and how they decode: it proves that a hand-encoded `.word` sequence in a routine is
the stock instruction it claims to be.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import tomllib
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LANG_DIR = (
    ROOT / "tools" / "coldfire" / "ghidra" / "ColdfireEMAC" / "data" / "languages"
)
BASE = 0x40000400
MAX_DEPTH = 300

DREG = [f"D{i}" for i in range(8)]
AREG = [f"A{i}" for i in range(8)]


@dataclass
class Insn:
    addr: int
    length: int
    text: str
    reads: list[str]  # registers read before they are written inside the instruction
    kills: set[str]  # registers written in full
    part: set[str]  # registers written in part (not a kill)
    kind: str  # 'seq' | 'branch' | 'cbranch' | 'call' | 'return' | 'indirect' | 'bad'
    targets: list[int] = field(
        default_factory=list
    )  # branch target (kind branch/cbranch)


class Image:
    def __init__(self, data: bytes, base: int = BASE):
        from pypcode import ArchLanguage, Context

        ldef = ET.parse(LANG_DIR / "coldfire_emac.ldefs").getroot().find("language")
        assert ldef is not None
        self.ctx = Context(ArchLanguage(str(LANG_DIR), ldef))
        self.data = data
        self.base = base
        self._cache: dict[int, Insn] = {}
        self._regnames: dict[tuple[int, int], str] = {}

    def reg_of(self, v) -> tuple[str, bool] | None:
        """(register name, whole register?) for a register-space varnode, else None."""
        if v.space.name != "register":
            return None
        key = (v.offset, v.size)
        if v.offset < 64:  # D0-D7 at 0..31, A0-A7 at 32..63, 4 bytes each, big endian
            return (DREG + AREG)[v.offset // 4], v.size == 4 and v.offset % 4 == 0
        name = self._regnames.get(key)
        if name is None:
            name = (
                self.ctx.getRegisterName(v.space, v.offset, v.size) or f"reg{v.offset}"
            )
            self._regnames[key] = name
        return name, True

    def insn(self, addr: int) -> Insn:
        hit = self._cache.get(addr)
        if hit is not None:
            return hit
        off = addr - self.base
        if off < 0 or off + 2 > len(self.data):
            ins = Insn(addr, 2, "<outside image>", [], set(), set(), "bad")
            self._cache[addr] = ins
            return ins
        try:
            tr = self.ctx.translate(self.data[off : off + 12], addr, max_instructions=1)
            dis = self.ctx.disassemble(
                self.data[off : off + 12], addr, max_instructions=1
            )
        except Exception:
            tr = None
        if tr is None or not tr.ops or not dis.instructions:
            ins = Insn(addr, 2, "<no decode>", [], set(), set(), "bad")
            self._cache[addr] = ins
            return ins
        d = dis.instructions[0]
        text = f"{d.mnem} {d.body}".strip()
        length = d.length
        reads: list[str] = []
        kills: set[str] = set()
        part: set[str] = set()
        kind = "seq"
        targets: list[int] = []
        seen_write: set[str] = set()
        cond = False  # an internal branch (const space) was seen: later writes may be skipped
        for op in tr.ops:
            n = op.opcode.name
            if n == "IMARK":
                continue
            for v in op.inputs:
                r = self.reg_of(v)
                if r and r[0] not in seen_write and r[0] not in reads:
                    reads.append(r[0])
            if op.output is not None:
                r = self.reg_of(op.output)
                if r:
                    if r[1] and not cond:
                        kills.add(r[0])
                        seen_write.add(r[0])
                    else:
                        part.add(
                            r[0]
                        )  # a partial write keeps the other bits: it kills nothing
            if n in ("BRANCH", "CBRANCH") and op.inputs[0].space.name == "ram":
                kind = "branch" if n == "BRANCH" else "cbranch"
                targets.append(op.inputs[0].offset)
            elif n in ("BRANCH", "CBRANCH"):
                cond = True  # internal (const space) branch inside one instruction
            elif n in ("CALL", "CALLIND"):
                kind = "call"
            elif n == "BRANCHIND":
                kind = "indirect"
            elif n == "RETURN":
                kind = "return"
        ins = Insn(addr, length, text, reads, kills, part, kind, targets)
        self._cache[addr] = ins
        return ins


def load_port(arg: str) -> tuple[Path, dict]:
    p = Path(arg)
    if p.suffix != ".toml":
        p = ROOT / "ports" / f"{arg}.toml"
    with open(p, "rb") as f:
        return p, tomllib.load(f)


def load_image(port: dict, tag: str) -> Image:
    path = ROOT / "out" / "sections" / tag / "section_3_MAIN_OS.bin"
    data = path.read_bytes()
    want = port["port"]["main_sha256"]
    got = hashlib.sha256(data).hexdigest()
    if got != want:
        sys.exit(f"{path}: sha256 {got} is not the port's main_sha256 {want}")
    return Image(data)


def tracked() -> list[str]:
    return (
        DREG
        + AREG[:7]
        + [
            "ACC0",
            "ACC1",
            "ACC2",
            "ACC3",
            "MACSR",
            "MASK",
            "NF",
            "ZF",
            "VF",
            "CF",
            "XF",
        ]
    )


GCC_CLOBBERED = {"D0", "D1", "A0", "A1", "NF", "ZF", "VF", "CF", "XF"}
GCC_KEPT = {"D2", "D3", "D4", "D5", "D6", "D7", "A2", "A3", "A4", "A5", "A6"}


def liveness(
    img: Image, start: int, reg: str, depth: int = MAX_DEPTH, abi: str = "strict"
):
    """-> ('dead', [kill addrs]) or ('live', reason string).

    abi "strict": a call, a return or an indirect jump may read any register, so a register
    that reaches one untouched is live. abi "gcc": the call convention of the compiled stock
    code (GCC for m68k: arguments on the stack, a call clobbers d0, d1, a0, a1 and the flags and
    keeps the other registers, a return reads d0 and the registers a function keeps); the
    accumulators and MACSR stay strict.
    """
    kills: list[int] = []
    visited: set[int] = set()
    stack = [(start, 0)]
    while stack:
        a, d = stack.pop()
        if a in visited:
            continue
        visited.add(a)
        if d > depth:
            return "live", f"depth bound {depth} at {a:#x}"
        ins = decode_cached(img, a)
        if ins.kind == "bad":
            return "live", f"{ins.text} at {a:#x}"
        if reg in ins.reads:
            return "live", f"read at {a:#x}: {ins.text}"
        if reg in ins.kills:
            kills.append(a)
            continue  # this path is done: written before read
        if ins.kind == "call":
            if abi == "gcc" and reg in GCC_CLOBBERED:
                kills.append(
                    a
                )  # the callee overwrites it: the caller cannot rely on it
                continue
            if abi == "gcc" and reg in GCC_KEPT:
                stack.append(
                    (a + ins.length, d + 1)
                )  # kept by the callee: look past the call
                continue
            return "live", f"call at {a:#x}: {ins.text}"
        if ins.kind == "return":
            if abi == "gcc" and reg in GCC_CLOBBERED - {"D0"}:
                kills.append(a)
                continue
            return "live", f"return at {a:#x}"
        if ins.kind == "indirect":
            return "live", f"indirect jump at {a:#x}: {ins.text}"
        nxt = a + ins.length
        if ins.kind == "branch":
            stack.append((ins.targets[0], d + 1))
        elif ins.kind == "cbranch":
            stack.append((ins.targets[0], d + 1))
            stack.append((nxt, d + 1))
        else:
            stack.append((nxt, d + 1))
    return "dead", sorted(set(kills))


def call_target(img: Image, a: int) -> int | None:
    """The target of a direct call (bsr, jsr abs) at `a`, else None."""
    ins = img.insn(a)
    if ins.kind != "call":
        return None
    off = a - img.base
    for op in img.ctx.translate(img.data[off : off + 12], a, max_instructions=1).ops:
        if op.opcode.name == "CALL" and op.inputs[0].space.name == "ram":
            return op.inputs[0].offset
    return None


LEA_ABS = re.compile(r"^lea \((0x[0-9a-fA-F]+)\)\.l,(A[0-7])$")
JSR_IND = re.compile(r"^jsr \((A[0-7])\)$")


def writes_of(
    img: Image, entry: int, limit: int = 4000, skip: frozenset[int] = frozenset()
) -> tuple[set[str], list[str]]:
    """Registers a function (with its direct callees) writes, and the unresolved things in it
    (indirect calls or jumps, undecodable bytes, the instruction bound).

    `jsr (An)` is resolved when the explored code writes An only with one `lea (ABS).l,An` (the
    stock code loads a helper's address that way). A call to an address in `skip` is not followed
    and not counted: the caller states why (a fault reporter on an error branch)."""
    written: set[str] = set()
    unknown: list[str] = []
    seen: set[int] = set()
    todo = [entry]
    leas: dict[str, set[int]] = {}
    other: set[str] = set()
    pending: dict[int, str] = {}
    while True:
        while todo:
            a = todo.pop()
            if a in seen:
                continue
            seen.add(a)
            if len(seen) > limit:
                unknown.append(f"instruction bound {limit}")
                return written - {"A7"}, unknown
            ins = img.insn(a)
            if ins.kind == "bad":
                unknown.append(f"{ins.text} at {a:#x}")
                continue
            written |= ins.kills | ins.part
            m = LEA_ABS.match(ins.text)
            for r in ins.kills | ins.part:
                if m and r == m.group(2):
                    leas.setdefault(r, set()).add(int(m.group(1), 16))
                else:
                    other.add(r)
            if ins.kind == "return":
                continue
            if ins.kind == "indirect":
                unknown.append(f"indirect jump at {a:#x}: {ins.text}")
                continue
            if ins.kind == "call":
                t = call_target(img, a)
                if t is None:
                    j = JSR_IND.match(ins.text)
                    if j:
                        pending[a] = j.group(1)
                    else:
                        unknown.append(f"indirect call at {a:#x}: {ins.text}")
                elif t not in skip:
                    todo.append(t)
                todo.append(a + ins.length)
                continue
            if ins.kind == "branch":
                todo.append(ins.targets[0])
            elif ins.kind == "cbranch":
                todo.append(ins.targets[0])
                todo.append(a + ins.length)
            else:
                todo.append(a + ins.length)
        for a, r in list(pending.items()):
            if len(leas.get(r, ())) == 1 and r not in other:
                del pending[a]
                (t,) = leas[r]
                if t not in skip:
                    todo.append(t)
        if not todo:
            break
    for a, r in pending.items():
        unknown.append(f"indirect call at {a:#x}: jsr ({r})")
    written.discard("A7")
    return written, unknown


def writers_between(
    img: Image,
    start: int,
    stop: int,
    reg: str,
    limit: int = 4000,
    skip: frozenset[int] = frozenset(),
) -> tuple[list[int], bool, list[tuple[int, int | None, bool]]]:
    """The addresses that write `reg` on the paths from `start` to `stop` (a path ends where it first reaches
    `stop` and does not pass `start` again), whether `stop` was reached at all, and the calls on those paths:
    (call address, direct target or None, callee writes `reg` or cannot be resolved). Strict model: a call
    writes `reg` only if the callee (`writes_of`, direct callees included) does, an indirect call counts as a
    writer. No calling convention is assumed."""
    succ: dict[int, list[int]] = {}
    writes: set[int] = set()
    calls: dict[int, tuple[int | None, bool]] = {}
    todo = [start]
    while todo and len(succ) < limit:
        a = todo.pop()
        if a in succ or a == stop:
            continue
        ins = img.insn(a)
        nxt: list[int] = []
        if ins.kind != "bad":
            if reg in ins.kills or reg in ins.part:
                writes.add(a)
            if ins.kind == "call":
                t = call_target(img, a)
                if t is None:
                    calls[a] = (None, True)
                else:
                    w, unk = writes_of(img, t, skip=skip)
                    calls[a] = (t, reg in w or bool(unk))
                if calls[a][1]:
                    writes.add(a)
            if ins.kind == "branch":
                nxt = [ins.targets[0]]
            elif ins.kind == "cbranch":
                nxt = [ins.targets[0], a + ins.length]
            elif ins.kind in ("return", "indirect"):
                nxt = []
            else:
                nxt = [a + ins.length]
        nxt = [
            x for x in nxt if x != start
        ]  # the next pass over `start` reads the field again
        succ[a] = nxt
        todo += nxt
    # the nodes from which `stop` can be reached
    good = {n for n, v in succ.items() if stop in v}
    changed = True
    while changed:
        changed = False
        for n, v in succ.items():
            if n not in good and any(x in good for x in v):
                good.add(n)
                changed = True
    on_path = [(a, t, w) for a, (t, w) in sorted(calls.items()) if a in good]
    return sorted(writes & good), start in good or start == stop, on_path


def preds_of(img: Image, entry: int, target: int, limit: int = 4000) -> list[int]:
    """The instructions of the function at `entry` (calls are not entered) that fall through or
    branch to `target`: every way into it."""
    seen: set[int] = set()
    out: list[int] = []
    todo = [entry]
    while todo and len(seen) < limit:
        a = todo.pop()
        if a in seen:
            continue
        seen.add(a)
        ins = img.insn(a)
        if ins.kind == "bad":
            continue
        if ins.kind == "branch":
            nxt = [ins.targets[0]]
        elif ins.kind == "cbranch":
            nxt = [ins.targets[0], a + ins.length]
        elif ins.kind in ("return", "indirect"):
            nxt = []
        else:
            nxt = [a + ins.length]
        if target in nxt:
            out.append(a)
        todo += nxt
    return sorted(out)


def decode_cached(img: Image, a: int) -> Insn:
    return img.insn(a)


def trace_to_call(img: Image, start: int, limit: int = 120) -> list[Insn]:
    out: list[Insn] = []
    a = start
    seen: set[int] = set()
    while len(out) < limit and a not in seen:
        seen.add(a)
        ins = img.insn(a)
        out.append(ins)
        if ins.kind in ("call", "return", "indirect", "bad"):
            break
        if ins.kind == "branch":
            a = ins.targets[0]
        else:
            a += ins.length
    return out


def anchor_addr(port: dict, name: str) -> int:
    for a in port.get("anchor", []):
        if a["name"] == name:
            return a["addr"]
    sys.exit(f"no anchor {name}")


def starts_for(img: Image, site: int, length: int) -> list[int]:
    """Fall-through after the site and, when the displaced bytes end in a branch, its target."""
    out = [site + length]
    a = site
    while a < site + length:
        ins = img.insn(a)
        if ins.kind in ("branch", "cbranch"):
            out.append(ins.targets[0])
        a += ins.length
    return out


def report(
    img: Image, start: int, depth: int, abi: str = "strict"
) -> dict[str, tuple[str, object]]:
    return {r: liveness(img, start, r, depth, abi) for r in tracked()}


def fmt_table(res: dict[str, tuple[str, object]]) -> str:
    lines = []
    dead = [r for r, (s, _) in res.items() if s == "dead"]
    live = [r for r, (s, _) in res.items() if s == "live"]
    lines.append("  dead: " + (" ".join(dead) or "-"))
    lines.append("  live: " + (" ".join(live) or "-"))
    for r, (s, w) in res.items():
        if r.startswith(("D", "A")) and len(r) == 2:
            if s == "dead":
                kills = w if isinstance(w, list) else []
                lines.append(
                    f"    {r} dead, written at " + ", ".join(f"{k:#x}" for k in kills)
                )
            else:
                lines.append(f"    {r} live: {w}")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("port", nargs="?", help="port file or tag, e.g. dt2-1.17")
    ap.add_argument(
        "site", nargs="?", help="site name of the port, e.g. cf.lfo.site.tick_call"
    )
    ap.add_argument("--addr", type=lambda s: int(s, 0))
    ap.add_argument("--len", dest="length", type=int, default=6)
    ap.add_argument("--from", dest="froms", type=lambda s: int(s, 0), action="append")
    ap.add_argument(
        "--from-site",
        dest="from_sites",
        action="append",
        help="start at a site of the port (repeatable)",
    )
    ap.add_argument("--abi", choices=["strict", "gcc", "both"], default="both")
    ap.add_argument("--depth", type=int, default=MAX_DEPTH)
    ap.add_argument(
        "--trace",
        action="store_true",
        help="list the code from the first start to the first call",
    )
    ap.add_argument("--match", help="hex bytes to find in that code")
    ap.add_argument("--decode", help="hex bytes to decode (needs no port)")
    ap.add_argument(
        "--stable",
        nargs=3,
        metavar=("FROM", "TO", "REG"),
        help="the writers of REG on the paths from address FROM to address TO, and the calls on them with what their callees write",
    )
    ap.add_argument(
        "--preds",
        nargs=2,
        type=lambda s: int(s, 0),
        metavar=("FUNC", "ADDR"),
        help="the instructions of the function at FUNC that fall through or branch to ADDR",
    )
    ap.add_argument(
        "--skip",
        action="append",
        type=lambda s: int(s, 0),
        metavar="ADDR",
        help="with --stable and --writes: do not follow calls to ADDR (say why in the finding; for example a fault reporter on an error branch)",
    )
    ap.add_argument(
        "--writes",
        action="append",
        type=lambda s: int(s, 0),
        help="registers the function at ADDR (and its direct callees) writes (repeatable)",
    )
    args = ap.parse_args(argv)

    if args.stable:
        if not args.port:
            ap.error("PORT is required")
        _, port = load_port(args.port)
        img = load_image(port, f"{port['port']['device']}-{port['port']['version']}")
        lo, hi, reg = (
            int(args.stable[0], 0),
            int(args.stable[1], 0),
            args.stable[2].upper(),
        )
        wr, reached, calls = writers_between(
            img, lo, hi, reg, skip=frozenset(args.skip or [])
        )
        for a, t, bad in calls:
            print(
                f"call at {a:#x} ({img.insn(a).text}): "
                + (f"callee {t:#x}" if t is not None else "indirect, unresolved")
                + (f", may write {reg}" if bad else f", does not write {reg}")
            )
        if not calls:
            print("no call on the paths")
        print(
            f"{reg} from {lo:#x} to {hi:#x}: "
            + ("reached; " if reached else "NOT REACHED; ")
            + (
                "no writer"
                if not wr
                else "written at "
                + ", ".join(f"{x:#x} ({img.insn(x).text})" for x in wr)
            )
        )
        return 0 if reached and not wr else 1

    if args.preds:
        if not args.port:
            ap.error("PORT is required")
        _, port = load_port(args.port)
        img = load_image(port, f"{port['port']['device']}-{port['port']['version']}")
        entry, target = args.preds
        ps = preds_of(img, entry, target)
        print(f"ways into {target:#x} inside the function at {entry:#x}:")
        for a in ps:
            print(f"  {a:#x}: {img.insn(a).text}")
        return 0

    if args.writes:
        if not args.port:
            ap.error("PORT is required")
        _, port = load_port(args.port)
        img = load_image(port, f"{port['port']['device']}-{port['port']['version']}")
        union: set[str] = set()
        for f in args.writes:
            w, unk = writes_of(img, f, skip=frozenset(args.skip or []))
            union |= w
            print(
                f"{f:#x}: writes {' '.join(sorted(w)) or '-'}"
                + (f"  UNRESOLVED: {'; '.join(unk)}" if unk else "")
            )
        print(f"union: {' '.join(sorted(union))}")
        return 0

    if args.decode is not None:
        data = bytes.fromhex(args.decode.replace(" ", ""))
        img = Image(data + bytes(12), base=args.addr or 0)
        a = img.base
        while a < img.base + len(data):
            ins = img.insn(a)
            print(
                f"{a:#010x}  {img.data[a - img.base : a - img.base + ins.length].hex():<14} {ins.text}"
            )
            a += ins.length
        return 0

    if not args.port:
        ap.error("PORT is required")
    path, port = load_port(args.port)
    tag = f"{port['port']['device']}-{port['port']['version']}"
    img = load_image(port, tag)
    site, length = args.addr, args.length
    if args.site:
        for s in port.get("anchor", []):
            if s["name"] == args.site:
                site, length = s["addr"], s["len"]
                break
        else:
            sys.exit(f"{path}: no site {args.site}")
    if site is None:
        ap.error("SITE or --addr is required")
    print(f"{tag} {args.site or ''} site {site:#x} len {length}")
    a = site
    while a < site + length:
        ins = img.insn(a)
        print(f"  {a:#x}: {ins.text}")
        a += ins.length
    starts = list(args.froms or [])
    for n in args.from_sites or []:
        starts.append(anchor_addr(port, n))
    if not starts:
        starts = starts_for(img, site, length)
    for st in starts:
        for abi in ("strict", "gcc") if args.abi == "both" else (args.abi,):
            print(f"from {st:#x} ({abi})")
            print(fmt_table(report(img, st, args.depth, abi)))
    code = trace_to_call(img, starts[0])
    nbytes = sum(i.length for i in code)
    if args.trace:
        print(
            f"code from {starts[0]:#x} to the first call: {len(code)} instructions, {nbytes} bytes"
        )
        for i in code:
            raw = img.data[i.addr - img.base : i.addr - img.base + i.length].hex()
            print(f"  {i.addr:#x}  {raw:<14} {i.text}")
    if args.match:
        want = bytes.fromhex(args.match.replace(" ", ""))
        blob = b"".join(
            img.data[i.addr - img.base : i.addr - img.base + i.length] for i in code
        )
        at = blob.find(want)
        if at < 0:
            print(f"match: {args.match} NOT FOUND in the {nbytes} bytes after the site")
            return 1
        # find the instruction boundary
        pos = 0
        for i in code:
            if pos == at:
                print(f"match: {args.match} at {i.addr:#x} ({i.text})")
                break
            pos += i.length
        else:
            print(
                f"match: {args.match} found at byte {at} but not on an instruction boundary"
            )
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
