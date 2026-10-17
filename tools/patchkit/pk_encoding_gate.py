"""Encoding gate: refuse added DSP instructions whose encoding shape is not proven.

Patched DSP code assembled by a third-party assembler once stalled the owner's unit: one instruction
encoded differently from the vendor toolchain's output, and the emulator, which reads the encoding
the assembler's way, could not catch it. The gate checks every instruction in the DSP code a patch
adds (and in the hook sites it rewrites) against two sets of encoding shapes: the shapes that occur
in real stock 1.17 DSP code, and the shapes listed in encoding_proven.toml (executed in an image that
passed on the unit).

    uv run python -m tools.patchkit.pk_encoding_gate OUT.syx PLAN.json      # exit 1 on violations

A shape is the decoded form name plus the instruction bits that are not register numbers, immediates,
addresses, condition codes or index/modify selectors. Compute fields keep their operation bits
(compute[22:12]; 2c keeps [11:8]); 6b_shiftimm keeps the shift op (bits 22:16), the sign of the 8-bit
amount (bit 15) and data_hi (bits 30:27).

Code versus data: a plan lists, for every object placed in L2 ("place" writes), the byte ranges that
are code ("code"); the rest of the object (tables, buffers) is data and is not decoded. Hook sites
(`hooks[]` with cpu "sharc") are code too. Registries and sections are data.
"""

from __future__ import annotations

import json
import sqlite3
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

from tools.sharc_dev import sharc_disasm
from tools.sharc_dev.sharc_visa_tables import get_type
from tools.sharc_dev.sharcldr import LoadedMemory

ROOT = Path(__file__).resolve().parents[2]
PROVEN = Path(__file__).with_name("encoding_proven.toml")
L2_BYTE = 0x20000000
L2_SW = 0xB80000
L1_BYTE = 0x28000000

# Field names whose bits are register numbers, immediates, addresses, conditions or index/modify selectors.
MASKED = {
    "addr",
    "reladdr",
    "data",
    "imm",
    "shiftimm",
    "dataex",
    "cond",
    "term",
    "dreg",
    "ureg",
    "dstureg",
    "srcureghigh",
    "srcureglow",
    "sreg",
    "cdreg",
    "breg",
    "dmdreg",
    "pmdreg",
    "dmi",
    "dmm",
    "pmi",
    "pmm",
    "i",
    "m",
    "idis",
    "is",
    "operand",
    "by",
    "toby",
}


@dataclass(frozen=True)
class Violation:
    pc: int  # short-word address
    raw: str  # the instruction in listing order (most significant parcel first)
    shape: str


def _span(hi: int, lo: int) -> int:
    return ((1 << (hi - lo + 1)) - 1) << lo


_keep_cache: dict[str, int] = {}


def _keep_mask(type_name: str) -> int:
    """The bits of a form that stay in its shape."""
    cached = _keep_cache.get(type_name)
    if cached is not None:
        return cached
    entry = get_type(type_name)
    if entry is None:
        raise KeyError(type_name)
    keep = (1 << entry["bits"]) - 1
    for label, (hi, lo) in entry["fields"].items():
        base = label.split("[")[0]
        if type_name == "6b_shiftimm":
            # shift op = shiftimm[22:16]; the amount's sign is bit 15; data_hi = dataex.
            if base == "cond":
                keep &= ~_span(hi, lo)
            elif label == "shiftimm[15:0]":
                keep &= ~_span(14, 0)
            continue
        if base == "compute":
            op_lo = 8 if hi == 11 else 12  # 2c: op nibble [11:8]; else compute[22:12]
            if op_lo > lo:
                keep &= ~_span(min(hi, op_lo - 1), lo)
        elif base in MASKED:
            keep &= ~_span(hi, lo)
    _keep_cache[type_name] = keep
    return keep


def shape_of(type_name: str, raw: int) -> str:
    return f"{type_name}:{raw & _keep_mask(type_name):x}"


def shape(ins: sharc_disasm.Instruction) -> str:
    assert ins.type_name is not None and ins.raw is not None
    return shape_of(ins.type_name, ins.raw)


def raw_hex(ins: sharc_disasm.Instruction) -> str:
    assert ins.raw is not None and ins.length_bytes is not None
    return f"{ins.raw:0{2 * ins.length_bytes}x}"


def stock_shapes(db: Path) -> set[str]:
    """Shapes of the stock instructions that belong to real code.

    The sweep database lists a decode at every short-word offset. Rows with aligned = 1 are the ones
    reached by the instruction chain from known code, and confidence = 'confident' drops forms the
    decode table marks unconfirmed. (function_sw IS NOT NULL would drop 269 aligned, confident rows
    not attributed to a function yet.)
    """
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        rows = con.execute(
            "SELECT DISTINCT form, raw FROM insn WHERE aligned = 1 AND confidence = 'confident'"
        ).fetchall()
    finally:
        con.close()
    out: set[str] = set()
    for form, raw in rows:
        value = int(raw if isinstance(raw, str) else raw.hex(), 16)
        out.add(shape_of(form, value))
    return out


def proven_shapes(path: Path = PROVEN) -> set[str]:
    doc = tomllib.loads(path.read_text())
    out = set()
    for e in doc.get("proven", []):
        for key in ("shape", "evidence", "note"):
            if not e.get(key):
                raise SystemExit(f"{path}: a [[proven]] entry lacks {key!r}")
        out.add(e["shape"])
    return out


def check_code(
    mem: sharc_disasm.ShortWordReader,
    ranges: list[tuple[int, int]],
    allowed: set[str],
) -> list[Violation]:
    """Decode each [start_sw, end_sw) range and report instructions whose shape is not allowed."""
    out: list[Violation] = []
    for start, end in ranges:
        pc = start
        while pc < end:
            ins = sharc_disasm.decode_confident_loaded(mem, pc)
            if ins.kind == "unknown" or ins.length_bytes is None:
                head = mem.read_sw(pc, 2)
                raw = f"{int.from_bytes(head, 'little'):04x}" if head else ""
                out.append(Violation(pc, raw, "unknown"))
                pc += 1
                continue
            s = shape(ins)
            if s not in allowed:
                out.append(Violation(pc, raw_hex(ins), s))
            pc += ins.length_bytes // 2
    return out


def decode_listing(listing_hex: str) -> sharc_disasm.Instruction:
    """Decode one instruction written in listing order (most significant parcel first)."""

    class _Words:
        data = b"".join(
            bytes.fromhex(listing_hex[i : i + 4])[::-1]
            for i in range(0, len(listing_hex), 4)
        )

        def read_sw(self, pc_sw: int, size: int = 6) -> bytes | None:
            lo = 2 * pc_sw
            return self.data[lo : lo + size] if lo + size <= len(self.data) else None

    return sharc_disasm.decode_confident_loaded(_Words(), 0)


def byte_to_sw(addr: int) -> int:
    if addr >= L1_BYTE:
        return (addr - L1_BYTE) // 2
    return L2_SW + (addr - L2_BYTE) // 2


def code_ranges(plan: dict) -> list[tuple[int, int]]:
    """Short-word ranges of the DSP code a patch adds: object code ranges and hook sites."""
    out: list[tuple[int, int]] = []
    for w in plan["writes"]:
        if w["cpu"] == "sharc" and w["kind"] == "place":
            for lo, hi in w.get("code") or []:
                out.append((byte_to_sw(lo), byte_to_sw(hi)))
    for h in plan.get("hooks", []):
        if h["cpu"] == "sharc":
            lo = byte_to_sw(h["addr"])
            out.append((lo, lo + h["site_len"] // 2))
    return out


def default_db(plan: dict) -> Path:
    base = plan["base"]
    return ROOT / "out" / "sharcdb" / f"{base['device']}-{base['version']}.sqlite"


def check_image(syx: Path, plan: Path, db: Path | None = None) -> list[Violation]:
    """Violations in the DSP code of a patched image (empty when the patch adds no DSP code)."""
    from tools.patchkit import dsp_patch

    doc = json.loads(plan.read_text())
    ranges = code_ranges(doc)
    if not ranges:
        return []
    db = db or default_db(doc)
    if not db.exists():
        raise SystemExit(
            f"encoding gate needs the stock instruction database {db}; build it with "
            "`uv run python -m tools.sharc_dev.sharcdb build out/sections/<image>/section_7_BLOB.bin`"
        )
    mem = LoadedMemory.from_stream(dsp_patch.section_blob(syx, 7))
    return check_code(mem, ranges, stock_shapes(db) | proven_shapes())


def format_violation(v: Violation) -> str:
    return f"  pc {v.pc:#08x}  {v.raw}  {v.shape}"


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__)
        return 2
    found = check_image(Path(argv[0]), Path(argv[1]))
    for v in found:
        print(format_violation(v))
    print(f"{len(found)} violation(s)")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
