#!/usr/bin/env python3
"""Unit tests for patchkit ColdFire hook routines on the native core (a ready DT2/DN2 image, tools/patchkit/pk_cfcall.py).

A hook routine is entered by a `jmp` from the middle of stock code with the registers, the flags and the stack of
that site, and leaves through a tail or an exit of the stock code. This harness runs one such routine in isolation:

    unit = CfUnit(
        "Digitakt_II_OS1.17.syx",
        sources=[Path("image-modules/points/machines/src/cf/dt2/machines.s")],
        stubs=["_mf_t_dispatch", ("_mf_site", 6)],  # undefined symbols the routine jumps to: each ends the call
        symbols={"_mf_first": 7, "_mf_rows": ROWS},  # every other undefined symbol: an absolute value
        routines=["mf_dispatch", ...],
    )
    r = unit.run_case("mf_dispatch", d={0: 5}, stack=[unit.rts, 8], mem={ROWS: rows_bytes})
    r.stub, r.d[0], r.a[0], r.z, r.stack[1], r.reads

The routine sees the registers `d`/`a` (a dict index -> value, default 0), the flags `ccr` (bit 2 = Z) and the stack
`stack` (word 0 on top; `unit.rts` as word 0 makes a routine that ends in `rts` come back). The result names the
stub it left through (``"rts"`` for a return), the registers, the flags and the 16 stack words at that moment. The
module's own bss starts zeroed on every case. Backed by packages/emulator/boot/examples/cf_call.rs (state cached under
out/cache/pk_cfcall/).
"""

from __future__ import annotations

import tempfile
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.patchkit import patchkit  # noqa: E402
from tools.patchkit.pk_cfcall import CfMachine  # noqa: E402

BASE = 0x47E8B000
DATA = 0x47EC0000
NW = 16  # stack words visible to a case

GLUE = """\
    .text
    .macro TRAMP name
    .globl t_\\name
t_\\name:
    move.l  (%sp)+,u_ret
    move.l  %sp,u_sp
    move.l  60(%sp),%d0
    move.w  %d0,%ccr
    movem.l (%sp),%d0-%d7/%a1-%a6
    movea.l 56(%sp),%a0
    lea     64(%sp),%sp
    jmp     \\name
    .endm
    .macro STUB name, id
    .globl \\name
\\name:
    lea     -64(%sp),%sp
    movem.l %d0-%d7/%a0-%a6,(%sp)
    smi     %d0
    seq     %d1
    svs     %d2
    scs     %d3
    move.b  %d0,60(%sp)
    move.b  %d1,61(%sp)
    move.b  %d2,62(%sp)
    move.b  %d3,63(%sp)
    bra.w   u_common_\\id
    .endm
"""

COMMON = """\
u_common_%(id)d:
    moveq   #%(id)d,%%d0
    move.l  %%d0,u_out
    bra.w   u_finish
"""


@dataclass
class CaseResult:
    stub: str
    d: list[int]
    a: list[
        int
    ]  # a0..a6, then a7 (the stack pointer at the stub, after the case's words)
    n: int
    z: int
    v: int
    c: int
    stack: list[int]
    reads: list[bytes]
    insns: int
    clobbered: list[str] = field(default_factory=list)


class CfUnit:
    def __init__(
        self,
        syx: str | Path,
        sources: list[Path],
        stubs: list[str],
        symbols: dict[str, int],
        routines: list[str],
        base: int = BASE,
    ):
        self.machine = CfMachine(syx)
        # a stub is a name, or (name, k): the symbol `name` is the stub's address minus k, so `jmp name + k` lands on it
        self.stub_names = ["rts", *[x if isinstance(x, str) else x[0] for x in stubs]]
        offsets = {x[0]: x[1] for x in stubs if not isinstance(x, str)}
        glue = [GLUE]
        for i, n in enumerate(self.stub_names):
            if n == "rts":
                glue.append(
                    "    .globl u_rts\nu_rts:\n    lea -4(%sp),%sp\n    bra.w u_rts_go\n"
                )
            elif n in offsets:
                glue.append(
                    f"    STUB {n}__stub, {i}\n    .globl {n}\n    .set {n}, {n}__stub - {offsets[n]}\n"
                )
            else:
                glue.append(f"    STUB {n}, {i}\n")
        glue.append(
            "u_rts_go:\n    lea -64(%sp),%sp\n    movem.l %d0-%d7/%a0-%a6,(%sp)\n"
            "    smi %d0\n    seq %d1\n    svs %d2\n    scs %d3\n"
            "    move.b %d0,60(%sp)\n    move.b %d1,61(%sp)\n    move.b %d2,62(%sp)\n    move.b %d3,63(%sp)\n"
            "    bra.w u_common_0\n"
        )
        for i in range(len(self.stub_names)):
            glue.append(COMMON % {"id": i})
        glue.append(
            "u_finish:\n"
            "    lea     u_out,%a0\n"
            "    lea     4(%a0),%a0\n"
            f"    moveq   #{16 + NW},%d0\n"
            "1:  move.l  (%sp)+,(%a0)+\n"
            "    subq.l  #1,%d0\n"
            "    bne.s   1b\n"
            "    movea.l u_sp,%sp\n"
            "    lea     -4(%sp),%sp\n"
            "    move.l  u_ret,(%sp)\n"
            "    rts\n"
        )
        for r in routines:
            glue.append(f"    TRAMP {r}\n")
        glue.append(
            "    .bss\n    .balign 4\nu_ret: .space 4\nu_sp: .space 4\n"
            f"    .globl u_out\nu_out: .space {4 + 4 * (16 + NW)}\n"
        )
        self.tmp = tempfile.TemporaryDirectory(prefix="pk-cfunit-")
        gpath = Path(self.tmp.name) / "glue.s"
        gpath.write_text("".join(glue))
        syms = dict(symbols)
        self.linked = patchkit.cf_link([*[Path(s) for s in sources], gpath], base, syms)
        self.hist = self.linked.histogram
        self.image = self.linked.image + b"\0" * self.linked.bss_size
        self.sym = self.linked.symbols
        self.rts = self.sym["u_rts"]
        self.out = self.sym["u_out"]

    def _call(self, routine, d, a, ccr, stack, reads):
        regs = [d.get(i, 0) for i in range(8)]
        regs += [a.get(i, 0) for i in range(1, 7)]
        regs += [a.get(0, 0), ccr]
        words = list(stack) + [0] * (NW - len(stack))
        assert len(stack) <= NW
        return {
            "pc": self.sym["t_" + routine],
            "args": [x & 0xFFFFFFFF for x in regs + words],
            "reads": [(self.out, 4 * (1 + 16 + NW)), *reads],
        }

    def run_cases(self, mem: dict[int, bytes], cases: list[dict]) -> list[CaseResult]:
        """cases: dicts with routine, d, a, ccr, stack, reads. One process, shared memory image."""
        pokes = {self.linked.base: self.image, **mem}
        calls = [
            self._call(
                c["routine"],
                c.get("d", {}),
                c.get("a", {}),
                c.get("ccr", 0),
                c.get("stack", []),
                c.get("reads", []),
            )
            for c in cases
        ]
        # `run` pokes once: cases share the starting memory, so a case that writes memory affects the next one
        res = self.machine.run(pokes, calls)
        out = []
        for r in res:
            if not r.ok:
                raise RuntimeError(f"case did not return: pc={r.pc:#x} {r.error}")
            blob = r.reads[0]
            w = [int.from_bytes(blob[i : i + 4], "big") for i in range(0, len(blob), 4)]
            sid = w[0]
            regs = w[1 : 1 + 15]
            flags = blob[4 + 60 : 4 + 64]
            out.append(
                CaseResult(
                    stub=self.stub_names[sid],
                    d=regs[:8],
                    a=regs[8:15],
                    n=flags[0] & 1,
                    z=flags[1] & 1,
                    v=flags[2] & 1,
                    c=flags[3] & 1,
                    stack=w[1 + 16 :],
                    reads=r.reads[1:],
                    insns=r.insns,
                    clobbered=r.clobbered,
                )
            )
        return out

    def run_case(self, routine: str, mem=None, **kw) -> CaseResult:
        return self.run_cases(mem or {}, [{"routine": routine, **kw}])[0]
