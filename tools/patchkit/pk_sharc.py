#!/usr/bin/env python3
"""SHARC+ patchkit modules: assemble with Selache, link ourselves, re-decode every word, run on the core.

Selache (public, pinned; tools/patchkit/pk_toolchains.py selache) assembles VISA source to a relocatable ELF whose
parcels are big-endian. This module

  1. assembles a source file (``assemble``) and reads sections, symbols and relocations (tools/patchkit/pk_elf.py),
  2. places the section at a chosen short-word (PC) address and resolves the relocations (``link``),
  3. swaps every 16-bit parcel to the boot-stream order and re-decodes every instruction with our own
     decoder (tools/sharc_dev/sharc_isa.py): Selache's instruction extents must equal ours, no instruction may be
     uncertain, and every relocated field must read back as the intended value (``verify``),
  4. optionally runs the linked code on the concrete SHARC+ core (``run_call``, tools/sharc_dev/sharc_run.py).

Relocations Selache emits (offsets and symbol values are in 16-bit parcels, the PC unit):
  type 11  absolute 24-bit target of a 48-bit jump/call/cjump (low 24 bits of the instruction)
  type 12  absolute 32-bit immediate/address of a 48-bit instruction (low 32 bits)
Both carry the symbol's short-word address; the addend is encoded in the symbol name ("sym + 4").

    uv run python -m tools.patchkit.pk_sharc module.s --base 0xB90000 [--extern name=0xADDR ...] [--json OUT]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent

from tools.patchkit import (  # noqa: E402
    pk_elf,  # noqa: E402
    pk_toolchains,  # noqa: E402
)
from tools.sharc_dev import sharc_selache as sel  # noqa: E402

SECTION = "seg_pmco"
PROCESSOR = "ADSP-21569"
R_ABS24, R_ABS32 = 11, 12
SW_ALIAS_BASE = (
    0x28000000  # loader byte address of short-word 0 (tools/sharc_dev/sharcldr.py)
)


class PkSharcError(RuntimeError):
    pass


@dataclass
class SharcReloc:
    parcel: int  # offset of the instruction, in parcels
    rtype: int
    symbol: str
    addend: int


@dataclass
class SharcObject:
    source_sha256: str
    section: bytes  # Selache parcel order (big-endian per 16 bits)
    symbols: dict[
        str, tuple[int, bool]
    ]  # name -> (parcel offset, global); defined symbols only
    relocs: list[SharcReloc]
    rows: list[tuple[int, str]]  # Selache instruction extents: (parcel offset, raw hex)
    toolchain: dict[str, str] = field(default_factory=dict)
    # Data sections (DM): name -> (bytes in memory order, alignment, is_bss); data symbols: name -> (section, byte offset, global)
    data: dict[str, tuple[bytes, int, bool]] = field(default_factory=dict)
    data_symbols: dict[str, tuple[str, int, bool]] = field(default_factory=dict)

    def to_json(self) -> dict[str, object]:
        return {
            "arch": "sharc+",
            "parcel_order": "selache-be",
            "source_sha256": self.source_sha256,
            "toolchain": self.toolchain,
            "section": self.section.hex(),
            "symbols": {
                k: {"parcel": v[0], "global": v[1]} for k, v in self.symbols.items()
            },
            "relocs": [r.__dict__ for r in self.relocs],
        }


@dataclass
class Linked:
    base_sw: int
    code_selache: bytes
    code_boot: bytes  # little-endian per parcel, the order of the boot stream and of tools/sharc_dev/sharc_isa.py
    symbols: dict[str, int]  # short-word addresses
    checked: int = 0  # instructions re-decoded
    ureg_class: list[str] = field(
        default_factory=list
    )  # stores whose source register the instruction class excludes


def _tool(name: str) -> Path:
    d = pk_toolchains.selache_dir()
    if d is None:
        raise PkSharcError(
            "selache is not built (uv run python -m tools.patchkit.pk_toolchains selache)"
        )
    return d / "target" / "release" / name


def split_symbol(name: str) -> tuple[str, int]:
    m = re.fullmatch(r"\s*([A-Za-z_.$][\w.$]*)\s*(?:([+-])\s*(\w+))?\s*", name)
    if not m:
        raise PkSharcError(f"cannot parse relocation symbol {name!r}")
    add = int(m.group(3), 0) * (-1 if m.group(2) == "-" else 1) if m.group(2) else 0
    return m.group(1), add


def _object(obj: Path, source_sha: str, section: str | None) -> SharcObject:
    elf = pk_elf.parse(obj.read_bytes())
    if section is None:
        section = next(
            (x.name for x in elf.sections if x.name in (SECTION, "seg_swco")), SECTION
        )
    listing = subprocess.run(
        [str(_tool("seldump")), "-ns", section, str(obj)],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    sec = elf.section(section)
    if sec is None:
        raise PkSharcError(
            f"no section {section} (a .SECTION/PM {section}; directive is required)"
        )
    rows = sel.parse_listing(listing)
    if b"".join(r.raw for r in rows) != sec.data:
        raise PkSharcError("Selache listing bytes do not equal the section bytes")
    symbols = {
        s.name: (s.value, s.bind != "local")
        for s in elf.symbols
        if s.name and 0 < s.shndx < 0xFF00 and elf.sections[s.shndx].name == section
    }
    relocs = []
    for r in elf.relocs:
        if r.section != section:
            raise PkSharcError(f"relocation in unexpected section {r.section}")
        if r.symbol.name == "":
            raise PkSharcError("section-relative relocation not supported")
        name, add = split_symbol(r.symbol.name)
        relocs.append(SharcReloc(r.offset, r.rtype, name, add + r.addend))
    tc = {"selache_rev": sel.PINNED_REVISION}
    for n in ("selas", "seldump"):
        tc[n + "_sha256"] = hashlib.sha256(_tool(n).read_bytes()).hexdigest()
    data: dict[str, tuple[bytes, int, bool]] = {}
    for x in elf.sections:
        if (
            x.flags & pk_elf.SHF_ALLOC
            and not x.flags & pk_elf.SHF_EXECINSTR
            and x.name != section
            and x.size
        ):
            data[x.name] = (
                b"" if x.type == pk_elf.SHT_NOBITS else x.data,
                max(x.align, 4),
                x.type == pk_elf.SHT_NOBITS,
            )
    data_symbols = {
        s.name: (elf.sections[s.shndx].name, s.value, s.bind != "local")
        for s in elf.symbols
        if s.name and 0 < s.shndx < 0xFF00 and elf.sections[s.shndx].name in data
    }
    native = _native_rows(
        [(r.parcel_address, r.raw.hex()) for r in rows], relocs, sec.data
    )
    code, native = fix_shift_counts(sec.data, native)
    return SharcObject(
        source_sha,
        code,
        symbols,
        relocs,
        native,
        tc,
        data,
        data_symbols,
    )


# Shift-immediate (Type 6) forms. shiftimm[22:16] is the ShiftImm opcode, PRM Table 17-9 (bits 21-16); these are the defined values.
SHIFT_IMM_FORMS = frozenset({"6a_mem", "6a_nomem", "6b_shiftimm"})
SHIFT_IMM_OPCODES = frozenset(
    {0x00, 0x01, 0x02, 0x08, 0x09, 0x10, 0x11, 0x12, 0x13, 0x14, 0x19, 0x1B, 0x1D, 0x1F}
    | {0x30, 0x31, 0x32, 0x33}
)
# LSHIFT, ASHIFT, OR LSHIFT, OR ASHIFT take a signed 8-bit count in shiftimm[15:8]; dataex[3:0] holds its sign extension (0xF for a
# negative count, else 0) in every such instruction of the stock images (finding 05, "Selache shift-immediate encodings"). Selache
# writes 0, and the silicon raises ILOPI on a negative count without it.
SIGNED_SHIFT_OPCODES = frozenset({0x00, 0x01, 0x08, 0x09})


def ci_problem(fields: dict[str, int], form: str) -> str | None:
    """An indirect jump or call with (CI) set: no patch routine returns from an interrupt, and selas's 48-bit
    `JUMP (M13, I13)` comes out as `JUMP (M13, I8) (CI)` (the 16-bit form is right)."""
    if form.startswith("9") and fields.get("ci"):
        return "(CI) on an indirect transfer (selas's 48-bit JUMP (Md, Ic): assemble it compressed)"
    return None


def shift_imm_problem(fields: dict[str, int], form: str) -> str | None:
    """Why a decoded shift-immediate instruction is wrong for the silicon, or None."""
    if form not in SHIFT_IMM_FORMS:
        return None
    op = fields.get("shiftimm[22:16]")
    if op not in SHIFT_IMM_OPCODES:
        return f"undefined ShiftImm opcode {op} (PRM Table 17-9)"
    if op in SIGNED_SHIFT_OPCODES:
        negative = (fields.get("shiftimm[15:0]", 0) >> 8) & 0x80
        if fields.get("dataex[3:0]") != (0xF if negative else 0):
            return "shift count without its sign extension (dataex)"
    return None


def fix_shift_counts(
    code: bytes, rows: list[tuple[int, str]]
) -> tuple[bytes, list[tuple[int, str]]]:
    """Set dataex of each signed shift count to its sign extension (the one Selache shift encoding that is repaired; the others are
    refused, ENCODE_HAZARDS). CODE and ROWS are in Selache parcel order; dataex is bits 6-3 of byte 2 of the 48-bit word."""
    out = bytearray(code)
    fixed = list(rows)
    items = sel.compare_rows(
        [sel.ExternalInstruction(p, bytes.fromhex(h), "") for p, h in rows]
    ).instructions
    for k, item in enumerate(items):
        fields = dict(item.native_fields)
        form = item.native_form_id or ""
        if form not in SHIFT_IMM_FORMS:
            continue
        if fields.get("shiftimm[22:16]") not in SIGNED_SHIFT_OPCODES:
            continue
        negative = (fields.get("shiftimm[15:0]", 0) >> 8) & 0x80
        at = item.external.parcel_address * 2 + 2
        out[at] = (out[at] & ~0x78) | (0x78 if negative else 0)
        p, h = rows[k]
        fixed[k] = (p, out[p * 2 : p * 2 + len(h) // 2].hex())
    return bytes(out), fixed


def _native_rows(
    selache_rows: list[tuple[int, str]], relocs: list[SharcReloc], section: bytes
) -> list[tuple[int, str]]:
    """The instruction rows of the section by our decoder's extents, cross-checked against Selache's listing.

    seldump lists the unrelocated object and splits some 48-bit forms early (Type 3a `DM(I,M) = Ureg` is
    listed as 4 bytes plus a separate `0000` row), so Selache's rows cannot be the extents. Ours are; the
    check against Selache is that every row it lists either starts where ours does with the same length, or
    is a shorter listing of ours whose missing parcels are zero, and that each relocation Selache emitted
    sits on the start of one of our instructions."""
    # Width ties: a 16-bit short compute (Type 2c) and a 32-bit Type 2b share their leading bits, so a free-running decode
    # of the object reads `R0 = R0 + R1;` (0xc001) plus the next word as one 2b. The emulator breaks the tie with a lookahead
    # over the successors (packages/emulator/sharc-rt/src/decode.rs, `decode_width_resolved`); here the assembler, which
    # chose the form, decides: its shorter extent is taken when that extent itself decodes to a form (finding 45).
    selache_len = {parcel: len(raw) // 2 for parcel, raw in selache_rows}
    isa = sel.load_instruction_set()
    boot = sel.swap_parcels(section)
    rows = []
    offset = 0
    while offset < len(boot):
        result = isa.decode_bytes(boot[offset : offset + 6])
        extent = (
            min(2, len(boot) - offset)
            if result.instruction is None
            else result.instruction.extent_bytes
        )
        short = selache_len.get(offset // 2)
        if short and short < extent and short in (2, 4):
            narrow = isa.decode_bytes(boot[offset : offset + short], 8 * short)
            if (
                narrow.instruction is not None
                and narrow.instruction.extent_bytes == short
            ):
                extent = short
        rows.append(
            (offset // 2, sel.swap_parcels(boot[offset : offset + extent]).hex())
        )
        offset += extent
    native = rows
    nat = dict(native)
    for parcel, raw in selache_rows:
        if parcel in nat:
            mine = nat[parcel]
            if mine != raw and not (
                mine.startswith(raw) and set(mine[len(raw) :]) <= {"0"}
            ):
                raise PkSharcError(
                    f"parcel {parcel:#x}: Selache lists {raw}, our decoder {mine}"
                )
    for r in relocs:
        if r.parcel not in nat:
            raise PkSharcError(
                f"relocation at parcel {r.parcel:#x} is not on one of our instruction starts"
            )
    return native


# Selache encode bugs found against the firmware image and PRM Table 17-9 (docs/findings/05-sharc-isa-and-decoding.md, "Selache
# shift-immediate encodings"): each form assembles to another instruction or to an undefined opcode, and the bytes still decode to a
# valid-looking form, so the source form is refused. A shift count in a register (`R0 = ASHIFT R1 BY R2;`) assembles correctly.
# LSHIFT by a negative count is repaired instead (fix_shift_counts).
_IMM = r"\bby\s+[-+]?\s*\d"
ENCODE_HAZARDS = [
    (
        re.compile(r"\bashift\b[^;]*" + _IMM, re.I),
        "ASHIFT ... BY <count> gets an undefined ShiftImm opcode (100000, PRM: 000001)",
    ),
    (
        re.compile(r"\bor\s+lshift\b[^;]*" + _IMM, re.I),
        "Rn = Rn OR LSHIFT ... BY <count> assembles as a plain LSHIFT",
    ),
    (
        re.compile(r"\bfdep\b[^;]*" + _IMM, re.I),
        "FDEP ... BY <pos:len> gets the (SE) or the plain FDEP opcode",
    ),
    (
        re.compile(r"\bfext\b[^;]*" + _IMM + r"[^;]*\(\s*se\s*\)", re.I),
        "FEXT ... BY <pos:len> (SE) assembles to an unrelated word",
    ),
]


# Selache drops the addend of a symbol in a register's immediate load (`R1 = _x + 0xec;` relocates as `_x`; `DM(_x + 0xec)` keeps
# it). A name that the source #defines is a constant, which it evaluates correctly.
_SYM_ADDEND = re.compile(
    r"\b[RIMBL]\d{1,2}\s*=\s*([A-Za-z_.$][\w.$]*)\s*[-+]\s*[\w.$]+\s*;", re.I
)
_REGISTER = re.compile(r"[RFIMBLS]\d{1,2}", re.I)
_DEFINED = re.compile(r"^[ \t]*#define[ \t]+([A-Za-z_]\w*)", re.M)


def _hazards(source: str) -> None:
    code = re.sub(r"/\*.*?\*/|//[^\n]*", "", source, flags=re.S)
    for pat, why in ENCODE_HAZARDS:
        m = pat.search(code)
        if m:
            raise PkSharcError(f"known Selache encode bug ({why}): {m.group(0)!r}")
    defined = set(_DEFINED.findall(code))
    for m in _SYM_ADDEND.finditer(code):
        if m.group(1) not in defined and not _REGISTER.fullmatch(m.group(1)):
            raise PkSharcError(
                f"known Selache encode bug (the addend of a symbol in an immediate load is dropped): {m.group(0)!r}"
            )


# Selache ignores the (SW)/(SWSE) suffix of a direct load (it emits the normal-word form), so
# `Rn = DM(symbol) (SW|SWSE);` is assembled as the normal-word load and rewritten here to the short-word Type 14d
# word (opcode 0x1a, bit 38 short word, bit 36 sign extension; the form tools/sharc_dev/sharc_asm.py `load_sw_abs` emits).
# The re-decode in `verify` then sees the short-word form and its relocated address.
_SW_LOAD = re.compile(
    r"^(\s*)R(\d{1,2})\s*=\s*DM\(\s*([A-Za-z_.$][\w.$]*)\s*\)\s*\((SWSE|SW)\)\s*;",
    re.I | re.M,
)


def _sw_prepare(source: str) -> tuple[str, list[tuple[str, bool]]]:
    marks: list[tuple[str, bool]] = []

    def sub(m: re.Match) -> str:
        label = f"_pk_sw{len(marks)}"
        marks.append((label, m.group(4).upper() == "SWSE"))
        return f"{m.group(1)}.GLOBAL {label};\n{m.group(1)}{label}:\n{m.group(1)}R{m.group(2)} = DM({m.group(3)});"

    return _SW_LOAD.sub(sub, source), marks


def _sw_apply(obj: SharcObject, marks: list[tuple[str, bool]]) -> None:
    if not marks:
        return
    code = bytearray(obj.section)
    at_reloc = {r.parcel: r for r in obj.relocs}
    for label, signed in marks:
        parcel = obj.symbols.pop(label)[0]
        at = parcel * 2
        if parcel not in at_reloc or at_reloc[parcel].rtype != R_ABS32:
            raise PkSharcError(f"{label}: expected a 32-bit relocated load")
        if code[at] != 0x10 or code[at + 1] & 0xF0:
            raise PkSharcError(
                f"{label}: not a normal-word direct load ({code[at : at + 2].hex()})"
            )
        code[at] = 0x1A
        code[at + 1] |= 0x40 | (0x10 if signed else 0)
    obj.section = bytes(code)


# Selache drops `.VAR name[N];` and cannot emit a multi-value `.VAR`, so a zero-filled array is declared with
# `.PKDATA name, bytes;` (loaded zeros, part of the boot-stream block) or `.PKBSS name, bytes;` (reserved memory the
# stream does not load: the program must write it before it reads it) instead: the line is removed before
# assembly, `name` becomes an extern of the source and the object gets a data or bss section of that size with a
# global data symbol `name` (the linker allocates it). `.PKHEX name, HEX;` is the same with loaded contents: HEX is the
# section's bytes as they sit in memory (little-endian words), for tables a generator writes.
_PKHEX = re.compile(
    r"^[ \t]*\.PKHEX[ \t]+([A-Za-z_.$][\w.$]*)[ \t]*,[ \t]*([0-9a-fA-F]+)[ \t]*;[^\n]*$",
    re.M,
)
_PKBSS = re.compile(
    r"^[ \t]*\.PK(BSS|DATA)[ \t]+([A-Za-z_.$][\w.$]*)[ \t]*,[ \t]*(0x[0-9a-fA-F]+|\d+|[A-Za-z_]\w*)[ \t]*;[^\n]*$",
    re.M,
)
_DEFINE = re.compile(
    r"^[ \t]*#define[ \t]+([A-Za-z_]\w*)[ \t]+(0x[0-9a-fA-F]+|\d+)[ \t]*(?://[^\n]*)?$",
    re.M,
)


def _bss_prepare(source: str) -> tuple[str, list[tuple[str, int, bool, bytes | None]]]:
    found: list[tuple[str, int, bool, bytes | None]] = []

    def hexsub(m: re.Match) -> str:
        if len(m.group(2)) % 2:
            raise PkSharcError(f".PKHEX {m.group(1)}: an odd number of hex digits")
        blob = bytes.fromhex(m.group(2))
        found.append((m.group(1), len(blob), False, blob))
        return f".EXTERN {m.group(1)};"

    source = _PKHEX.sub(hexsub, source)
    # the size may be a macro defined as a plain number in the same source (the generated device includes of `patchkit gen`)
    defines = {m.group(1): int(m.group(2), 0) for m in _DEFINE.finditer(source)}

    def sub(m: re.Match) -> str:
        size = m.group(3)
        if size not in defines and not size[0].isdigit():
            raise PkSharcError(
                f".PK{m.group(1)} {m.group(2)}: size {size} is not a number or a #define of one"
            )
        found.append(
            (
                m.group(2),
                defines[size] if size in defines else int(size, 0),
                m.group(1) == "BSS",
                None,
            )
        )
        return f".EXTERN {m.group(2)};"

    return _PKBSS.sub(sub, source), found


def assemble(source: str, section: str | None = None) -> SharcObject:
    _hazards(source)
    original = source
    source, bss_decls = _bss_prepare(source)
    source, sw_marks = _sw_prepare(source)
    with tempfile.TemporaryDirectory(prefix="pk-sharc-") as d:
        src, obj = Path(d) / "m.s", Path(d) / "m.doj"
        src.write_text(source)
        r = subprocess.run(
            [str(_tool("selas")), "-proc", PROCESSOR, "-o", str(obj), str(src)],
            capture_output=True,
            text=True,
        )
        if r.returncode:
            raise PkSharcError(f"selas failed:\n{r.stdout}{r.stderr}")
        result = _object(obj, hashlib.sha256(original.encode()).hexdigest(), section)
        _sw_apply(result, sw_marks)
        for name, size, is_bss, blob in bss_decls:
            if size <= 0 or size % 4:
                raise PkSharcError(
                    f".PKBSS/.PKDATA/.PKHEX {name}: size {size} is not a positive multiple of 4"
                )
            sec = f"pkbss_{name}" if is_bss else f"pkdata_{name}"
            result.data[sec] = (blob if blob is not None else bytes(size), 4, is_bss)
            result.data_symbols[name] = (sec, 0, True)
        return result


def compile_c(source: str, section: str | None = None) -> SharcObject:
    """Selache's C compiler (selcc); evaluated in docs/findings/20-patch-toolchains.md."""
    with tempfile.TemporaryDirectory(prefix="pk-sharc-") as d:
        src, obj, asm = Path(d) / "m.c", Path(d) / "m.doj", Path(d) / "m.s"
        src.write_text(source)
        # the compiler's assembly passes the same source check as hand-written assembly
        s = subprocess.run(
            [str(_tool("selcc")), "-proc", PROCESSOR, "-S", "-o", str(asm), str(src)],
            capture_output=True,
            text=True,
        )
        if s.returncode:
            raise PkSharcError(f"selcc failed:\n{s.stdout}{s.stderr}")
        _hazards(asm.read_text())
        r = subprocess.run(
            [str(_tool("selcc")), "-proc", PROCESSOR, "-c", "-o", str(obj), str(src)],
            capture_output=True,
            text=True,
        )
        if r.returncode:
            raise PkSharcError(f"selcc failed:\n{r.stdout}{r.stderr}")
        return _object(obj, hashlib.sha256(source.encode()).hexdigest(), section)


def self_check(code: bytearray, at: int, size: int, r: SharcReloc) -> None:
    """Selache leaves the field zero or holding the addend; anything else means we misread the relocation."""
    mask = (1 << (8 * size)) - 1
    have = int.from_bytes(code[at : at + size], "big")
    if have not in (0, r.addend & mask):
        raise PkSharcError(
            f"parcel {r.parcel:#x}: field {have:#x} is neither 0 nor the addend {r.addend} before relocation"
        )


def link(
    obj: SharcObject, base_sw: int, externs: dict[str, int] | None = None
) -> Linked:
    externs = externs or {}
    code = bytearray(obj.section)
    syms = {n: base_sw + off for n, (off, _g) in obj.symbols.items()}
    extents = dict(obj.rows)
    for r in obj.relocs:
        if r.symbol in syms:
            target = syms[r.symbol]
        elif r.symbol in externs:
            target = externs[r.symbol]
        else:
            raise PkSharcError(f"unresolved symbol {r.symbol}")
        target += r.addend
        row = bytes.fromhex(extents.get(r.parcel, ""))
        if len(row) != 6:
            raise PkSharcError(
                f"relocation at parcel {r.parcel:#x}: not a 48-bit instruction"
            )
        at = r.parcel * 2
        if r.rtype == R_ABS24:
            if not 0 <= target < 1 << 24:
                raise PkSharcError(
                    f"{r.symbol}: {target:#x} does not fit a 24-bit target"
                )
            self_check(code, at + 3, 3, r)
            code[at + 3 : at + 6] = target.to_bytes(3, "big")
        elif r.rtype == R_ABS32:
            self_check(code, at + 2, 4, r)
            code[at + 2 : at + 6] = (target & 0xFFFFFFFF).to_bytes(4, "big")
        else:
            raise PkSharcError(f"relocation type {r.rtype} not supported")
    out = Linked(base_sw, bytes(code), sel.swap_parcels(bytes(code)), syms)
    verify(obj, out, externs)
    return out


def _field(fields: dict[str, int], stem: str) -> int | None:
    """Join the pieces ``stem[hi:lo]`` of a decoded form into one number."""
    parts = []
    for k, v in fields.items():
        m = re.fullmatch(re.escape(stem) + r"\[(\d+):(\d+)\]", k)
        if m:
            parts.append((int(m.group(2)), v))
    if not parts:
        return fields.get(stem)
    return sum(v << lo for lo, v in parts)


# The forms that address memory through an index register (Ia/Ic, with an immediate or register modifier): the ISA encodes the source of
# their stores as UREGXDAG1 / UREGXDAG2. Type 14a (`DM(<addr32>) = Ureg`) has no index register and takes the full UREG class.
INDEXED_MEMORY_FORMS = frozenset({"3a", "3b", "3d", "15a", "15b"})


def ureg_class_problems(fields: dict[str, int], where: str, form: str) -> list[str]:
    """A store through DM with an index register cannot take a DAG1 register (I0-I7, M0-M7, L0-L7, B0-B7) and a store through PM cannot
    take a DAG2 register: the ISA encodes the source of such a store as UREGXDAG1 / UREGXDAG2 (SHARC+ PRM 15-7, 15-10, 13-13: "the Ureg
    may not be from the same DAG as Ia/Mb or Ic/Md"). `form` is the instruction form id (Types 3a, 3b, 3d, 15a, 15b are checked; Type 14a,
    the direct-address store, is exempt, as is every other form). Loads take every register. The stock DSP image has not one store of the
    excluded kind (finding 49 section 10). The assembler and the emulator accept them; the silicon does not run them as written."""
    if form not in INDEXED_MEMORY_FORMS:
        return []
    d = _field(fields, "d")
    u = _field(fields, "ureg")
    if d != 1 or u is None or not 0x10 <= u <= 0x4F:
        return []
    g = _field(fields, "g")
    if g is None:
        return []  # a form with no DM/PM select (an instruction word with no memory-side choice)
    dag = 1 if (u & 0xF) < 8 else 2
    if (g == 0 and dag == 1) or (g == 1 and dag == 2):
        name = "IMLB"[(u >> 4) - 1] + str(u & 0xF)
        return [f"{where}: {'PM' if g else 'DM'} store of {name} (DAG{dag})"]
    return []


def verify(obj: SharcObject, linked: Linked, externs: dict[str, int]) -> None:
    """Every instruction decodes natively with the extent Selache chose; relocated fields read back."""
    rows = []
    for parcel, raw_hex in obj.rows:
        at = parcel * 2
        raw = linked.code_selache[at : at + len(raw_hex) // 2]
        rows.append(sel.ExternalInstruction(parcel, raw, ""))
    cmp = sel.compare_rows(rows)
    problems = []
    by_parcel = {}
    isa = sel.load_instruction_set()
    for item in cmp.instructions:
        by_parcel[item.external.parcel_address] = item
        linked.ureg_class += ureg_class_problems(
            dict(item.native_fields),
            f"parcel {item.external.parcel_address:#x}",
            item.native_form_id or "",
        )
        shift = shift_imm_problem(
            dict(item.native_fields), item.native_form_id or ""
        ) or ci_problem(dict(item.native_fields), item.native_form_id or "")
        if shift:
            problems.append(
                f"parcel {item.external.parcel_address:#x} {item.external.raw.hex()}: {shift}"
            )
        if item.native_form_id is None or not item.extent_agrees:
            # a width tie (Type 2c against Type 2b): the assembler's extent decides when a form of that width matches
            n = item.external.extent_bytes
            if n in (2, 4):
                tie = isa.decode_bytes(item.boot_bytes, 8 * n)
                if tie.instruction is not None and tie.instruction.extent_bytes == n:
                    continue
            problems.append(
                f"parcel {item.external.parcel_address:#x} {item.external.raw.hex()}: "
                f"native {item.native_form_id or item.native_candidates} extent_agrees={item.extent_agrees}"
            )
    # The emulator breaks the 2b / 2c width tie with a lookahead over the successors; the silicon has no such choice. An
    # instruction the emulator would read with another extent than the assembler's runs differently there (finding 45).
    from tools.sharc_dev import sharc_disasm

    for parcel, raw_hex in obj.rows:
        mine = len(raw_hex) // 2
        seen = sharc_disasm.decode_confident(linked.code_boot, parcel * 2)
        if seen.length_bytes is not None and seen.length_bytes != mine:
            problems.append(
                f"parcel {parcel:#x} {raw_hex}: the emulator's width resolution reads {seen.type_name} of "
                f"{seen.length_bytes} bytes, the assembler wrote {mine} (a 16-bit `Rn = Rn op Rx`: the emulator would run "
                f"another instruction than the silicon; use distinct registers, e.g. `R0 = R1 + R2`; finding 45)"
            )
    for r in obj.relocs:
        item = by_parcel[r.parcel]
        fields = dict(item.native_fields)
        want = (linked.symbols.get(r.symbol, externs.get(r.symbol)) or 0) + r.addend
        got = None
        for stem in ("addr", "data", "reladdr"):
            got = _field(fields, stem)
            if got is not None:
                break
        mask = 0xFFFFFF if r.rtype == R_ABS24 else 0xFFFFFFFF
        if got != want & mask:
            problems.append(
                f"parcel {r.parcel:#x} ({item.native_form_id}): relocated field reads {got} want {want & mask:#x}"
            )
    if problems:
        raise PkSharcError(
            "Selache output disagrees with our decoder:\n  " + "\n  ".join(problems)
        )
    linked.checked = len(cmp.instructions)


def build(
    source: str, base_sw: int, externs: dict[str, int] | None = None
) -> tuple[SharcObject, Linked]:
    obj = assemble(source)
    return obj, link(obj, base_sw, externs)


# ---------------------------------------------------------------- running on the concrete core


_DECODE_CACHES: dict[bytes, dict] = {}


def run_call(
    linked: Linked,
    entry: str,
    regs: dict[str, int] | None = None,
    dm_words: dict[int, int] | None = None,
    reads: dict[str, tuple[int, int]] | None = None,
    max_steps: int = 100000,
    return_sw: int = 0xB9F000,
    approx_recips: bool = False,
    others: tuple[Linked, ...] = (),
    read_regs: tuple[str, ...] = (),
    extra_breaks: tuple[int, ...] = (),
) -> dict[str, object]:
    """Run ``entry`` (a symbol) to its RTS on the concrete core and return registers and memory reads.

    ``dm_words`` maps normal-word DM addresses (L2: 0x08000000 up) to values to poke before the run;
    ``reads`` maps a result name to (normal-word address, word count). Floats are passed as raw bits.
    """
    from tools.sharc_dev import sharc_run
    from tools.sharc_dev import sharc_trace as st
    from tools.sharc_dev.sharcldr import LoadedMemory

    start = linked.symbols[entry]
    blocks, data = [], b""
    for i, obj in enumerate((linked, *others)):
        blocks.append(
            {
                "target_address": SW_ALIAS_BASE + 2 * obj.base_sw,
                "byte_count": len(obj.code_boot),
                "fill": False,
                "payload_offset": len(data),
                "payload_len": len(obj.code_boot),
                "index": i,
            }
        )
        data += obj.code_boot
    mem = LoadedMemory(data, blocks)
    # The decode of a pc depends only on the loaded code bytes and their placement, so calls that load the same code
    # (a rig calling a routine block after block) share one decode cache instead of decoding every instruction again.
    cache_key = hashlib.sha256(
        repr([(b["target_address"], b["byte_count"]) for b in blocks]).encode() + data
    ).digest()
    runner = sharc_run.Runner(
        mem,
        start,
        regs=regs or {},
        breakpoints=[return_sw, *extra_breaks],
        approx_recips=approx_recips,
    )
    runner._cache = _DECODE_CACHES.setdefault(cache_key, {})
    for addr, word in (dm_words or {}).items():
        if not st._dm_write(
            runner.state, addr, 4, st.Const(word & 0xFFFFFFFF), normal_word=True
        ):
            raise PkSharcError(f"cannot poke DM word {addr:#x}")
    runner.state.call_stack.append(return_sw)
    res = runner.run(max_steps)
    out: dict[str, object] = {
        "halt": res.halt.to_json(),
        "instructions": res.instructions,
        "final_pc_sw": res.final_pc_sw,
    }
    for name, (addr, count) in (reads or {}).items():
        vals = []
        for k in range(count):
            v = st._dm_read(runner.state, addr + k, 4, normal_word=True)
            vals.append(None if v is None else v.value & 0xFFFFFFFF)
        out[name] = vals
    out["regs"] = {
        n: getattr(runner.state.uregs.get(st.UREG_CODES[n]), "value", None)
        for n in (
            "R0",
            "R1",
            "R2",
            "R4",
            "R5",
            "R6",
            "R7",
            "R8",
            "R9",
            "R10",
            "R11",
            "R12",
            "R13",
            "I0",
            *read_regs,
        )
    }
    return out


class CallSession:
    """A concrete core that keeps its memory between calls (``run_call`` builds a new core for every call).

    ``call`` starts a routine at a symbol the way ``run_call`` does: the registers given, a return address that stops the
    run, and all registers it does not name as the last call left them. DM written by a call stays (a delay line, a
    routine's state); ``poke`` and ``peek`` read and write DM words in normal-word addresses. A rig that calls a routine
    block after block uses this instead of copying its state in and out of every call."""

    def __init__(
        self,
        linked: list[Linked],
        return_sw: int,
        breaks: tuple[int, ...] = (),
        approx_recips: bool = False,
    ) -> None:
        from tools.sharc_dev import sharc_run
        from tools.sharc_dev.sharcldr import LoadedMemory

        self._st = sharc_run.st
        self._run = sharc_run
        self.return_sw = return_sw
        blocks, data = [], b""
        for i, obj in enumerate(linked):
            blocks.append(
                {
                    "target_address": SW_ALIAS_BASE + 2 * obj.base_sw,
                    "byte_count": len(obj.code_boot),
                    "fill": False,
                    "payload_offset": len(data),
                    "payload_len": len(obj.code_boot),
                    "index": i,
                }
            )
            data += obj.code_boot
        mem = LoadedMemory(data, blocks)
        cache_key = hashlib.sha256(
            repr([(b["target_address"], b["byte_count"]) for b in blocks]).encode()
            + data
        ).digest()
        self.runner = sharc_run.Runner(
            mem,
            linked[0].base_sw,
            regs={},
            breakpoints=[return_sw, *breaks],
            approx_recips=approx_recips,
        )
        self.runner._cache = _DECODE_CACHES.setdefault(cache_key, {})
        self.symbols: dict[str, int] = {}
        for obj in linked:
            self.symbols.update(obj.symbols)

    def record_stores(self) -> None:
        """Log every DM store of the following calls (address, width, PC) in ``stores`` (canonical DM byte addresses). Pokes and
        peeks between calls are not logged. Uses the driver's write watchpoints (tools/sharc_dev/sharc_run.py), so a store
        has one semantics path."""
        wp = self._run.Watchpoint(0, 1 << 40, on_read=False, stop=False, label="store")
        self.runner.attach_watchpoints([wp])
        self.runner.set_watch_active(False)
        self._recording = True

    stores: tuple = ()
    _recording = False

    def poke(self, addr: int, word: int) -> None:
        if not self._st._dm_write(
            self.runner.state,
            addr,
            4,
            self._st.Const(word & 0xFFFFFFFF),
            normal_word=True,
        ):
            raise PkSharcError(f"cannot poke DM word {addr:#x}")

    def peek(self, addr: int) -> int | None:
        v = self._st._dm_read(self.runner.state, addr, 4, normal_word=True)
        return None if v is None else v.value & 0xFFFFFFFF

    def call(
        self,
        entry: str,
        regs: dict[str, int],
        max_steps: int = 100000,
        read_regs: tuple[str, ...] = (),
    ) -> dict[str, object]:
        import dataclasses

        st, runner = self._st, self.runner
        old = runner.state
        new_uregs = dict(old.uregs)
        for key, value in regs.items():
            new_uregs[st._seed_code(key)] = st._seed_value(value)
        state = dataclasses.replace(
            old,
            pc_sw=self.symbols[entry],
            uregs=new_uregs,
            call_stack=[self.return_sw],
            pc_stack=(
                [0x01000000 | (self.return_sw & 0xFFFFFF)] if old.stack_model else []
            ),
            pc_stack_pending=-1,
            pc_stack_requested=-1,
            loops=[],
            loop_depth=0,
            loop_slots=list(old.loop_slots),
            status_stack=[],
            mmrs=dict(old.mmrs),
            special=dict(old.special),
            bank_alt=dict(old.bank_alt),
            pending=None,
            stopped=None,
        )  # the overlay (DM written so far) is shared, not copied
        if state.stack_model:
            st._sync_pc_stack(state)
            st._sync_status_stack(state)
        runner.state = state
        runner.instructions = 0
        if self._recording:
            runner.clear_watch_log()
            runner.set_watch_active(True)
        try:
            res = runner.run(max_steps)
        finally:
            if self._recording:
                runner.set_watch_active(False)
                self.stores = runner.watch_log
        names = (
            "R0",
            "R1",
            "R2",
            "R4",
            "R5",
            "R6",
            "R7",
            "R8",
            "R9",
            "R10",
            "R11",
            "R12",
            "R13",
            "I0",
            *read_regs,
        )
        return {
            "halt": res.halt.to_json(),
            "instructions": res.instructions,
            "final_pc_sw": res.final_pc_sw,
            "regs": {
                n: getattr(state.uregs.get(st.UREG_CODES[n]), "value", None)
                for n in names
            },
        }


def main() -> None:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("source")
    ap.add_argument(
        "--base",
        type=lambda x: int(x, 0),
        required=True,
        help="short-word (PC) address of the section",
    )
    ap.add_argument("--extern", action="append", default=[], metavar="NAME=ADDR")
    ap.add_argument("--json", help="write the object prototype here")
    a = ap.parse_args()
    externs = {k: int(v, 0) for k, v in (e.split("=") for e in a.extern)}
    obj, linked = build(Path(a.source).read_text(), a.base, externs)
    print(
        f"linked {len(linked.code_boot)} bytes at sw {a.base:#x}; {linked.checked} instructions re-decoded OK"
    )
    for n, v in sorted(linked.symbols.items(), key=lambda kv: kv[1]):
        print(f"  {v:#08x} {n}")
    if a.json:
        Path(a.json).write_text(json.dumps(obj.to_json(), indent=1))


if __name__ == "__main__":
    main()
