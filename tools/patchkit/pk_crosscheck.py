"""Operand cross-check of a Selache object: Selache's own listing against our decoder, instruction by instruction.

pk_sharc's `verify` checks that Selache and our decoder agree on each instruction's width and on the relocated
fields. That does not catch an encoder that puts an operand in the wrong bits while its own disassembler reads the
same wrong bits back: Selache's 48-bit `JUMP (M13, I12)` under `.NOCOMPRESS` lists as `jump (m13,i12)` and is, by the
PRM layout our decoder follows, `JUMP (M13, I8) (DB)` (docs/findings/20-patch-toolchains.md). Here every instruction
of the object (by our decoder's extents) is listed by seldump and decoded by us, and the two must name the same
operands:

  registers  the set of data, index, modify and universal registers (F and R name the same register file),
  immediates every number Selache prints, compared with our field values modulo the field width (so a 16-bit
             sign-extended literal printed as -0x4 equals 0xfffc), relocated fields included (unrelocated: both read
             the field as the assembler left it),
  condition  the IF condition (none = always),
  flags      the delayed-branch flag (DB), clear-interrupt (CI) and loop-abort (LA) of jumps and calls.

An assembly source (pk_sharc.assemble) must map one statement per instruction (a count mismatch is an error), and
each statement must also name the same data, index and modify registers (R/F, I, M) as our decoder reads in its
word; a C unit (pk_sharc.compile_c) gets the listing check only. Selache writes
`PM(I12, M14) = R2` as `DM(I4, M6) = R2`, and its listing agrees with that word, so only the source shows it. Two
listing quirks are accepted when the source and our decoder agree (`listing_quirk`): seldump prints a Type 3a
transfer's M0-M3 as M4-M7, and lists a 16-bit Type 3c with M0-M3 as a 32-bit word.

Most forms are compared as sets (registers, immediates) plus condition and flags; the forms in `STRICT_FORMS`
(Types 16a/16b, `DM|PM(Ia, Mb) = data`, which Selache's C emits for every call) are compared operand by operand in
their roles (memory, index, modify, immediate). A disagreement, or a Selache line we cannot read, or a form we do
not cross-check yet, fails the build with the source statement it came from.

seldump lists a section linearly and loses step after a width it misjudges (it lists a 48-bit Type 3a as 32 bits
plus a stray parcel), and it replaces numbers equal to a symbol's address by the symbol's name. So the object is
listed from a copy whose symbol table is hidden and whose section holds each of our instructions followed by three
NOPs, which brings seldump back in step before the next instruction.

Relocation addends (`check_addends`, called by pk_sharc): Selache drops the constant of `symbol + constant` in some
forms. Every reference to a symbol in an assembly statement must match the relocation of its instruction; for a C
unit, the references in selcc's own assembly output (`-S`) must each have a relocation with the same addend.
"""

from __future__ import annotations

import re
import struct
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

from tools.sharc_dev import sharc_selache as sel
from tools.sharc_dev import sharcinv
from tools.sharc_dev.sharc_trace import UREG_NAMES
from tools.sharc_dev.sharcfn.decode import render_compute, render_shortcompute

NOP = bytes.fromhex("0001")  # Type 21c NOP in Selache's (big-endian) parcel order
PAD = NOP * 3
SHT_SYMTAB, SHT_NOTE = 2, 7

UREG_CODE = {name.lower(): code for code, name in enumerate(UREG_NAMES)}
UREG_CODE.update({f"f{n}": n for n in range(16)})
I0, M0 = UREG_CODE["i0"], UREG_CODE["m0"]
USTAT1, LCNTR = UREG_CODE["ustat1"], UREG_CODE["lcntr"]

# Selache's condition spellings -> the 5-bit COND code (PGR Table 10-4 order).
COND_CODE = {
    "eq": 0x00,
    "lt": 0x01,
    "le": 0x02,
    "ac": 0x03,
    "av": 0x04,
    "mv": 0x05,
    "ms": 0x06,
    "sv": 0x07,
    "sz": 0x08,
    "flag0_in": 0x09,
    "flag1_in": 0x0A,
    "flag2_in": 0x0B,
    "flag3_in": 0x0C,
    "tf": 0x0D,
    "bm": 0x0E,
    "not lce": 0x0F,
    "lce": 0x0F,
    "ne": 0x10,
    "ge": 0x11,
    "gt": 0x12,
    "not ac": 0x13,
    "not av": 0x14,
    "not mv": 0x15,
    "not ms": 0x16,
    "not sv": 0x17,
    "not sz": 0x18,
    "not flag0_in": 0x19,
    "not flag1_in": 0x1A,
    "not flag2_in": 0x1B,
    "not flag3_in": 0x1C,
    "not tf": 0x1D,
    "nbm": 0x1E,
}
ALWAYS = 0x1F
FLAG_WORDS = {"db", "ci", "la"}
# Selache words that are neither registers nor numbers (operations, keywords); anything else is unreadable.
KEYWORDS = {
    "dm",
    "pm",
    "nop",
    "jump",
    "call",
    "cjump",
    "rts",
    "rti",
    "rframe",
    "do",
    "until",
    "pc",
    "by",
    "modify",
    "bitrev",
    "lw",
    "nw",
    "sw",
    "bw",
    "swse",
    "bwse",
    "se",
    "ex",
    "lr",
    "comp",
    "compu",
    "pass",
    "and",
    "or",
    "xor",
    "not",
    "abs",
    "min",
    "max",
    "clip",
    "float",
    "fix",
    "trunc",
    "rnd",
    "lshift",
    "ashift",
    "rot",
    "fext",
    "fdep",
    "bset",
    "bclr",
    "btgl",
    "btst",
    "scalb",
    "logb",
    "mant",
    "recips",
    "rsqrts",
    "copysign",
    "leftz",
    "lefto",
    "exp",
    "exp2",
    "fpack",
    "funpack",
    "sat",
    "ssf",
    "ssi",
    "uuf",
    "uui",
    "sui",
    "usi",
    "suf",
    "usf",
    "sfr",
    "ufr",
    "sir",
    "uir",
    "mrf",
    "mrb",
    "ci",
    "db",
    "la",
    "inc",
    "dec",
    "neg",
    "avg",
    "cdec",
    "cinc",
    "zs",
    "if",
    "else",
    "else_",
    "idle",
    "emuidle",
    "sync",
    "push",
    "pop",
    "loop",
    "sts",
    "bit",
    "set",
    "clr",
    "tgl",
    "tst",
}
COMPARE_OPS = {"comp", "compu", "fcomp", "fcompu"}


class CrossCheckError(RuntimeError):
    pass


@dataclass(frozen=True)
class Operands:
    regs: frozenset[int]  # ureg codes; F and R are both 0..15
    imms: tuple[int, ...]  # Selache: the numbers as printed (signed)
    cond: int
    flags: frozenset[str]


@dataclass(frozen=True)
class NativeOperands:
    regs: frozenset[int]
    imms: tuple[tuple[int, int], ...]  # (value, field width in bits)
    cond: int
    flags: frozenset[str]


@dataclass(frozen=True)
class Mismatch:
    parcel: int
    raw: str  # our instruction, Selache parcel order
    selache: str
    native: str
    why: str
    statement: str = ""
    line: int | None = None

    def format(self) -> str:
        where = f"line {self.line}: " if self.line is not None else ""
        src = f"{where}`{self.statement}` " if self.statement else ""
        return (
            f"{src}at parcel {self.parcel:#x} ({self.raw}): {self.why}; "
            f"Selache lists `{self.selache}`, our decoder reads {self.native}"
        )


# ---------------------------------------------------------------- Selache side


_TOKEN = re.compile(r"0x[0-9a-f]+|\d+|[a-z_][a-z0-9_]*|\S")


def selache_operands(text: str) -> Operands:
    """Registers, numbers, condition and branch flags of one seldump line; CrossCheckError when unreadable."""
    t = text.strip().lower()
    if t.startswith("???") or "compute(" in t or "ureg(" in t:
        raise CrossCheckError(f"Selache cannot list it: {text!r}")
    cond = ALWAYS
    m = re.match(r"if\s+(not\s+\w+|\w+)\s+", t)
    if m:
        if m.group(1) not in COND_CODE:
            raise CrossCheckError(f"unknown Selache condition {m.group(1)!r}")
        cond = COND_CODE[m.group(1)]
        t = t[m.end() :]
    t = re.sub(r"until\s+(not\s+)?lce", "", t)
    t = re.sub(r"\(\s*pc\s*,", "(", t)  # PC-relative operand: the offset is the number
    flags = set()
    for f in re.findall(r"\(\s*(\w+)\s*\)", t):
        if f in FLAG_WORDS:
            flags.add(f)
    regs: set[int] = set()
    imms: list[int] = []
    toks = _TOKEN.findall(t)
    for k, tok in enumerate(toks):
        if tok[0].isdigit():
            v = int(tok, 0)
            prev = toks[k - 1] if k else None
            before = toks[k - 2] if k > 1 else None
            if prev == "-" and before in (None, "=", "(", ",", "by", ":"):
                v = -v
            imms.append(v)
        elif tok[0].isalpha() or tok[0] == "_":
            if tok in UREG_CODE:
                regs.add(UREG_CODE[tok])
            elif tok not in KEYWORDS:
                raise CrossCheckError(f"unknown word {tok!r} in Selache line {text!r}")
    return Operands(frozenset(regs), tuple(imms), cond, frozenset(flags))


# ---------------------------------------------------------------- our side


def _sx(v: int, bits: int) -> int:
    return v - (1 << bits) if v >> (bits - 1) & 1 else v


def _compute_regs(text: str | None) -> tuple[set[int], list[tuple[int, int]]]:
    regs: set[int] = set()
    imms: list[tuple[int, int]] = []
    if not text:
        return regs, imms
    for part in text.split(";"):
        m = re.match(r"\s*([RF]\d+)\s*=\s*(\w+)\(", part)
        if m and m.group(2) in COMPARE_OPS:
            part = part[m.end() - len(m.group(2)) - 1 :]
        for name in re.findall(r"\b([RF]\d+)\b", part):
            regs.add(int(name[1:]))
        if re.search(r"\b(inc|dec)\(", part):
            imms.append((1, 32))
        if "?" in part:
            raise CrossCheckError(f"our compute decode is incomplete: {part!r}")
    return regs, imms


def _dag(i: int, g: int) -> int:
    return I0 + i + 8 * g


def _mdag(m: int, g: int) -> int:
    return M0 + m + 8 * g


def native_operands(form: str, raw_fields: dict[str, int]) -> NativeOperands:
    """The operands of one decoded instruction, in the terms selache_operands uses."""
    f = sharcinv.merge_fields(dict(raw_fields))
    regs: set[int] = set()
    imms: list[tuple[int, int]] = []
    flags: set[str] = set()
    cond = f.get("cond", ALWAYS)

    compute = f.get("compute")
    if compute and form != "2c":
        r, i = _compute_regs(render_compute(compute & 0x7FFFFF)[0])
        regs |= r
        imms += i

    def jflags() -> None:
        if f.get("j"):
            flags.add("db")
        if f.get("ci"):
            flags.add("ci")
        if f.get("a"):
            flags.add("la")

    if form == "2c":
        r, i = _compute_regs(render_shortcompute(f["compute"]))
        regs |= r
        imms += i
    elif form in ("2a", "2a_short", "21a", "21c", "25c_rframe", "22c", "20a"):
        pass
    elif form in ("17a", "17b"):
        regs.add(f["ureg"])
        imms.append((f["data"], 32 if form == "17a" else 16))
    elif form == "14a":
        regs.add(f["ureg"])
        imms.append((f["addr"], 32))
    elif form == "15a":
        regs |= {f["ureg"], _dag(f["i"], f["g"])}
        imms.append((f["addr"], 32))
    elif form == "15b":
        regs |= {f["ureg"], _dag(f["i"], f["g"])}
        imms.append((f["data"], 7))
    elif form in ("3a", "3b", "3d"):
        regs |= {f["ureg"], _dag(f["i"], f["g"]), _mdag(f["m"], f["g"])}
    elif form == "3c":
        regs |= {f["dreg"], _dag(f["dmi"], 0), _mdag(f["dmm"], 0)}
    elif form in ("4a", "4b", "4d"):
        regs |= {f["dreg"], _dag(f["i"], f["g"])}
        imms.append((f["data"], 6))
    elif form in ("1a", "1b"):
        regs |= {
            f["dmdreg"],
            f["pmdreg"],
            _dag(f["dmi"], 0),
            _mdag(f["dmm"], 0),
            _dag(f["pmi"], 1),
            _mdag(f["pmm"], 1),
        }
    elif form in ("5a_move", "5b_move"):
        src = f["srcureghigh"] << 2 | f.get("srcureglow", 0)
        regs |= {f["dstureg"], src}
    elif form == "6b_shiftimm":
        field = f["shiftimm"]
        op, data8 = field >> 16 & 0x3F, field >> 8 & 0xFF
        regs |= {field >> 4 & 0xF, field & 0xF}
        if op in (
            0x10,
            0x11,
            0x12,
            0x13,
            0x19,
            0x1B,
        ):  # fext / fdep: bit position and length
            imms += [(data8 & 0x3F, 6), ((f.get("dataex", 0) << 2) | (data8 >> 6), 6)]
        else:  # shift amount (8-bit signed) or bit number
            imms.append((data8, 8))
    elif form in ("19a", "19a_scaled"):
        # idis = 0: the result goes back to the source register (every use so far); else a second I register
        g = f.get("g", 0)
        regs.add(_dag(f["is"], g))
        if f["idis"]:
            regs.add(_dag(f["idis"], g))
        imms.append((f["data"], 32))
    elif form in ("7a", "7b"):
        g = f.get("g", 0)
        regs |= {_dag(f["is"], g), _mdag(f["m"], g)}
        if f.get("idis"):
            regs.add(_dag(f["idis"], g))
    elif form in ("8a_abs", "25a_direct"):
        jflags()
        if form == "25a_direct":
            flags.add("db")
        imms.append((f["addr"], 24))
    elif form in ("8a_rel", "25a_pcrel"):
        jflags()
        if form == "25a_pcrel":
            flags.add("db")
        imms.append((f["reladdr"], 24))
    elif form in ("9a_abs", "9b_abs"):
        jflags()
        regs |= {_dag(f["pmi"], 1), _mdag(f["pmm"], 1)}
    elif form == "9a_rel":
        jflags()
        imms.append((f["reladdr"], 6))
    elif form in ("11a", "11c"):
        jflags()
    elif form == "12a_imm":
        regs.add(LCNTR)
        imms += [(f["data"], 16), (f["reladdr"], 23)]
    elif form == "12a_ureg":
        regs |= {LCNTR, f["ureg"]}
        imms.append((f["reladdr"], 23))
    elif form == "18a":
        regs.add(USTAT1 + f["sreg"])
        imms.append((f["data"], 32))
    elif form in STRICT_FORMS:
        g = f.get("g", 0)
        regs |= {_dag(f["i"], g), _mdag(f["m"], g)}
        imms.append((f["data"], 32 if form == "16a" else 16))
    else:
        raise CrossCheckError(
            f"form {form} is not cross-checked yet (tools/patchkit/pk_crosscheck.py)"
        )
    return NativeOperands(frozenset(regs), tuple(imms), cond, frozenset(flags))


# Forms compared operand by operand in their roles (`strict_check`), not as register and immediate sets: the listing
# is parsed by the form's own pattern and every field is checked where it belongs. Each was added with a stock
# example (tests/patchkit/test_pk_crosscheck.py).
_STORE_IMM = re.compile(
    r"(dm|pm)\s*\(\s*i(\d+)\s*,\s*m(\d+)\s*\)\s*=\s*(-?)\s*(0x[0-9a-f]+|\d+)"
)


def _strict_store_imm(form: str, f: dict[str, int], text: str) -> str | None:
    """Type 16a (48 bits, 32-bit data) / 16b (32 bits, 16-bit data): `DM|PM(Ia, Mb) = data`, post-modify. g picks
    the memory (0 = DM, DAG1 I0-I7/M0-M7; 1 = PM, DAG2 I8-I15/M8-M15); 16a's sl/by width bits must be clear (a
    short-word or byte store is not cross-checked)."""
    if f.get("sl") or f.get("by"):
        raise CrossCheckError(
            f"form {form} with width bits (sl/by) is not cross-checked yet"
        )
    m = _STORE_IMM.fullmatch(text.strip().lower())
    if not m:
        return "Selache's line is not `DM|PM(Ia, Mb) = data`"
    space, i, mm, neg, num = m.groups()
    g = f["g"]
    if space != ("pm" if g else "dm"):
        return f"memory {space} vs {'pm' if g else 'dm'}"
    if int(i) != f["i"] + 8 * g:
        return f"index register I{i} vs I{f['i'] + 8 * g}"
    if int(mm) != f["m"] + 8 * g:
        return f"modify register M{mm} vs M{f['m'] + 8 * g}"
    bits = 32 if form == "16a" else 16
    value = int(num, 0) * (-1 if neg else 1)
    if value & ((1 << bits) - 1) != f["data"] & ((1 << bits) - 1):
        return f"immediate {value:#x} vs {f['data']:#x}"
    return None


STRICT_FORMS = {"16a": _strict_store_imm, "16b": _strict_store_imm}


def strict_check(form: str, raw_fields: dict[str, int], text: str) -> str | None:
    """None when Selache's line names the same operands, in the same roles, as our decode of a STRICT_FORMS form."""
    try:
        return STRICT_FORMS[form](form, sharcinv.merge_fields(dict(raw_fields)), text)
    except CrossCheckError as e:
        return str(e)


def _imms_match(sel_imms: tuple[int, ...], nat: tuple[tuple[int, int], ...]) -> bool:
    if len(sel_imms) != len(nat):
        return False
    left = list(nat)
    for v in sel_imms:
        for k, (n, bits) in enumerate(left):
            if v & ((1 << bits) - 1) == n & ((1 << bits) - 1):
                del left[k]
                break
        else:
            return False
    return True


def compare(s: Operands, n: NativeOperands) -> str | None:
    """None when they agree, else what differs."""
    if s.cond != n.cond:
        return f"condition {s.cond:#x} vs {n.cond:#x}"
    if s.flags != n.flags:
        return f"flags {sorted(s.flags)} vs {sorted(n.flags)}"
    if s.regs != n.regs:
        return f"registers {sorted(UREG_NAMES[r] for r in s.regs)} vs {sorted(UREG_NAMES[r] for r in n.regs)}"
    if not _imms_match(s.imms, n.imms):
        return f"immediates {list(s.imms)} vs {[v for v, _ in n.imms]}"
    return None


DATA_AND_DAG = (
    frozenset(range(16)) | frozenset(range(I0, I0 + 16)) | frozenset(range(M0, M0 + 16))
)


def source_registers(statement: str) -> frozenset[int]:
    """The data (R/F), index (I) and modify (M) registers a source statement names."""
    out = set()
    for cls, num in re.findall(
        r"(?<![\w.$])([RFIMrfim])(\d{1,2})(?![\w.$])", statement
    ):
        n = int(num)
        if n < 16:
            out.add({"r": n, "f": n, "i": I0 + n, "m": M0 + n}[cls.lower()])
    return frozenset(out)


def compare_source(statement: str, n: NativeOperands) -> str | None:
    """None when the statement names the same R/F/I/M registers our decoder reads."""
    src, ours = source_registers(statement), n.regs & DATA_AND_DAG
    if src != ours:
        named = sorted(UREG_NAMES[r] for r in src)
        held = sorted(UREG_NAMES[r] for r in ours)
        return f"the source names registers {named} but the word holds {held}"
    return None


def listing_quirk(
    text: str,
    listed_bytes: int,
    n: NativeOperands,
    form: str,
    fields: dict[str, int],
    width: int,
) -> bool:
    """Two places where seldump's listing disagrees with the word Selache's own encoder wrote for the source
    (docs/findings/20-patch-toolchains.md), accepted only after the source check passed:
      - a Type 3a transfer with M0-M3 is listed with M4-M7 (the only difference);
      - a 16-bit Type 3c transfer with M1-M3 (or M0) is listed as a 32-bit word (the stock code has such 16-bit
        words in its instruction chains)."""
    if form == "3c":
        return fields.get("dmm[2:0]", 8) < 4 and listed_bytes != width
    if form != "3a":
        return False
    try:
        s = selache_operands(text)
    except CrossCheckError:
        return False
    if (s.cond, s.flags) != (n.cond, n.flags) or not _imms_match(s.imms, n.imms):
        return False
    diff = s.regs ^ n.regs
    return any(diff == {M0 + k, M0 + k + 4} and M0 + k in n.regs for k in range(4))


def describe(n: NativeOperands) -> str:
    regs = ",".join(sorted(UREG_NAMES[r] for r in n.regs))
    imms = ",".join(f"{v:#x}/{b}" for v, b in n.imms)
    cond = "" if n.cond == ALWAYS else f" cond={n.cond:#x}"
    return f"[{regs}] imm[{imms}]{cond} {' '.join(sorted(n.flags))}".rstrip()


# ---------------------------------------------------------------- listing each instruction


def _section_headers(data: bytes) -> tuple[str, int, int, int]:
    if data[:4] != b"\x7fELF" or data[4] != 1:
        raise CrossCheckError("not an ELF32 object")
    e = "<" if data[5] == 1 else ">"
    shoff = struct.unpack_from(e + "I", data, 0x20)[0]
    entsize, count, _ = struct.unpack_from(e + "HHH", data, 0x2E)
    return e, shoff, entsize, count


def _section_index(data: bytes, name: str) -> int:
    e, shoff, entsize, count = _section_headers(data)
    strndx = struct.unpack_from(e + "H", data, 0x32)[0]
    str_off = struct.unpack_from(e + "I", data, shoff + strndx * entsize + 0x10)[0]
    for k in range(count):
        n_off = struct.unpack_from(e + "I", data, shoff + k * entsize)[0]
        start = str_off + n_off
        if data[start : data.index(b"\0", start)].decode() == name:
            return k
    raise CrossCheckError(f"no section {name}")


def listing_copy(obj: bytes, section: str, payload: bytes) -> bytes:
    """The object with `section` holding `payload` (appended at the end) and its symbol tables hidden."""
    out = bytearray(obj)
    e, shoff, entsize, count = _section_headers(obj)
    names = _section_index(obj, section)
    for k in range(count):
        h = shoff + k * entsize
        sh_type = struct.unpack_from(e + "I", obj, h + 4)[0]
        if sh_type == SHT_SYMTAB:
            struct.pack_into(e + "I", out, h + 4, SHT_NOTE)
        if k == names:
            struct.pack_into(e + "II", out, h + 0x10, len(obj), len(payload))
    return bytes(out) + payload


def selache_texts(
    obj: bytes, section: str, instructions: list[tuple[int, bytes]]
) -> tuple[dict[int, str], dict[int, int]]:
    """seldump's text and listed width in bytes for each (parcel, raw bytes in Selache order), one instruction at a
    time."""
    payload = bytearray()
    at: dict[int, int] = {}
    for parcel, raw in instructions:
        at[len(payload) // 2] = parcel
        payload += raw + PAD
    with tempfile.TemporaryDirectory(prefix="pk-xcheck-") as d:
        path = Path(d) / "x.doj"
        path.write_bytes(listing_copy(obj, section, bytes(payload)))
        from tools.patchkit import pk_sharc

        text = subprocess.run(
            [str(pk_sharc._tool("seldump")), "-ns", section, str(path)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    out: dict[int, str] = {}
    widths: dict[int, int] = {}
    for row in sel.parse_listing(text):
        if row.parcel_address in at:
            out[at[row.parcel_address]] = row.text
            widths[at[row.parcel_address]] = len(row.raw)
    return out, widths


# ---------------------------------------------------------------- source statements


def statements(preprocessed: str) -> list[tuple[int, str]]:
    """(line, text) of every instruction statement in order: `;`-terminated, labels and directives dropped.

    `preprocessed` is Selache's preprocessor output, which keeps the source's line numbers and drops comments."""
    # `#line N` makes the next line source line N.
    source_line: list[int] = []
    n = 1
    for text in preprocessed.split("\n"):
        marker = re.match(r"\s*#\s*line\s+(\d+)", text)
        source_line.append(n)
        n = int(marker.group(1)) if marker else n + 1
    out: list[tuple[int, str]] = []
    pos = 0
    for m in re.finditer(";", preprocessed):
        chunk = preprocessed[pos : m.start()]
        base = preprocessed.count("\n", 0, pos)
        pos = m.end()
        lines = [ln for ln in chunk.split("\n")]
        # drop preprocessor line markers
        kept = ["" if ln.lstrip().startswith("#") else ln for ln in lines]
        body = "\n".join(kept)
        while True:
            lab = re.match(r"\s*[A-Za-z_.$][\w.$]*\s*:(?!=)", body)
            if not lab:
                break
            body = re.sub(r"[^\n]", " ", body[: lab.end()]) + body[lab.end() :]
        stripped = body.strip()
        if not stripped or stripped.startswith("."):
            continue
        first = len(body) - len(body.lstrip())
        out.append(
            (source_line[base + body.count("\n", 0, first)], " ".join(stripped.split()))
        )
    return out


def statement_count_message(stmts: list[tuple[int, str]], cmp: sel.Comparison) -> str:
    n = len(cmp.instructions)
    where = ""
    if stmts:
        where = f" (statements from line {stmts[0][0]} `{stmts[0][1]}` to line {stmts[-1][0]} `{stmts[-1][1]}`)"
    return (
        f"{len(stmts)} source statements but {n} instructions{where}: the source-statement check needs exactly one "
        "instruction per `;`-terminated statement (a statement that assembles to no instruction or to several, "
        "e.g. a data definition in the code section, cannot be checked; move it or split it)"
    )


# ---------------------------------------------------------------- symbol addends
#
# Selache drops the constant of `symbol + constant` in some forms (e.g. `R0 = _x + 8;` is relocated as `_x`, the
# field left 0; docs/findings/20-patch-toolchains.md) and keeps it in others (`JUMP _f + 2`, `DM(I7, M7) = _l - 1`).
# `check_addends` compares every symbol reference of the source with the relocation Selache emitted for it.

_NUM = re.compile(r"\s*(0x[0-9a-fA-F]+|\d+)")
_END = re.compile(r"\s*(?:[),;]|$)")


def symbol_refs(statement: str, names: set[str]) -> list[tuple[str, int | None]]:
    """(symbol, addend) for every reference to one of `names` in a statement: 0 for a bare name, c for `name + c`,
    -c for `name - c`; None for any other expression around the name (which cannot be verified)."""
    out: list[tuple[str, int | None]] = []
    for m in re.finditer(r"[A-Za-z_.$][\w.$]*", statement):
        name = m.group(0)
        if name not in names:
            continue
        before = statement[: m.start()].rstrip()
        if before.endswith(("+", "-", "*", "/", "<<", ">>", "&", "|")):
            out.append((name, None))
            continue
        rest = statement[m.end() :]
        op = rest.lstrip()[:1]
        if op in ("+", "-"):
            num = _NUM.match(rest.lstrip()[1:])
            if num and _END.match(rest.lstrip()[1:][num.end() :]):
                value = int(num.group(1), 0)
                out.append((name, value if op == "+" else -value))
            else:
                out.append((name, None))
        elif _END.match(rest) or op == "(" or rest[:1].isspace() or not rest:
            out.append((name, 0))
        else:
            out.append((name, None))
    return out


def check_addends(
    stmts: list[tuple[int, str]],
    parcels: list[int],
    relocs: list[tuple[int, str, int]],
    names: set[str],
) -> list[str]:
    """Errors for an assembly source mapped one statement per instruction (`parcels[k]` is statement k's instruction):
    a reference `name + c` must be relocated with addend c; an addend on an instruction without a relocation, or an
    expression we cannot read, is refused. `relocs` are (parcel, symbol, addend)."""
    at = {p: (s, a) for p, s, a in relocs}
    out: list[str] = []
    for (line, stmt), parcel in zip(stmts, parcels, strict=True):
        rel = at.get(parcel)
        for sym, add in symbol_refs(stmt, names):
            if add is None:
                out.append(
                    f"line {line}: `{stmt}`: the expression around {sym} cannot be verified (Selache drops "
                    "`symbol + constant` addends in some forms): compute the offset in a register"
                )
            elif rel is not None and rel[0] == sym:
                if rel[1] != add:
                    out.append(
                        f"line {line}: `{stmt}`: Selache relocates {sym}{rel[1]:+d}, the source says {sym}{add:+d} "
                        "(it drops the constant of `symbol + constant` here): compute the offset in a register"
                    )
            elif add:
                out.append(
                    f"line {line}: `{stmt}`: {sym}{add:+d} on an instruction without a relocation of {sym}: the "
                    "addend cannot be verified; compute the offset in a register"
                )
    return out


def check_addends_unmapped(
    stmts: list[tuple[int, str]], relocs: list[tuple[int, str, int]], names: set[str]
) -> list[str]:
    """The same for a compiled unit whose assembly (selcc -S) does not map onto the instructions one to one: every
    `name + c` reference with c != 0 needs its own relocation of `name` with addend c."""
    want: dict[tuple[str, int], list[tuple[int, str]]] = {}
    out: list[str] = []
    for line, stmt in stmts:
        for sym, add in symbol_refs(stmt, names):
            if add is None:
                out.append(
                    f"line {line}: `{stmt}`: the expression around {sym} cannot be verified"
                )
            elif add:
                want.setdefault((sym, add), []).append((line, stmt))
    have: dict[tuple[str, int], int] = {}
    for _p, s, a in relocs:
        have[(s, a)] = have.get((s, a), 0) + 1
    for (sym, add), uses in want.items():
        if have.get((sym, add), 0) < len(uses):
            line, stmt = uses[0]
            out.append(
                f"line {line}: `{stmt}`: {len(uses)} reference(s) {sym}{add:+d} but {have.get((sym, add), 0)} "
                f"relocation(s) with that addend (Selache drops `symbol + constant` addends in some forms)"
            )
    return out


def check(
    obj: bytes,
    section: str,
    section_bytes: bytes,
    source_statements: list[tuple[int, str]] | None = None,
) -> list[Mismatch]:
    """Mismatches over the whole section (our extents): between seldump and our decoder, and, when the source
    statements map one to one onto the instructions, between the registers each statement names and our decoder."""
    cmp = sel.compare_external_bytes(section_bytes)
    if source_statements is not None and len(source_statements) != len(
        cmp.instructions
    ):
        # An assembly source must map statement by statement onto the instructions: otherwise the source check
        # (Selache's silent PM -> DM encoding) and the listing quirks it vouches for would be skipped unseen.
        raise CrossCheckError(statement_count_message(source_statements, cmp))
    rows = [(i.external.parcel_address, i.external.raw) for i in cmp.instructions]
    texts, listed = selache_texts(obj, section, rows)
    stmts = source_statements or []
    use_stmts = source_statements is not None
    out: list[Mismatch] = []
    for k, item in enumerate(cmp.instructions):
        parcel = item.external.parcel_address
        raw = item.external.raw.hex()
        text = texts.get(parcel, "")
        line, stmt = stmts[k] if use_stmts else (None, "")
        native = "?"
        try:
            if item.native_form_id is None:
                raise CrossCheckError("our decoder cannot decode it")
            n = native_operands(item.native_form_id, dict(item.native_fields))
            native = f"{item.native_form_id} {describe(n)}"
            why = compare_source(stmt, n) if use_stmts else None
            if not why:
                if not text:
                    raise CrossCheckError("seldump lists no instruction here")
                try:
                    if item.native_form_id in STRICT_FORMS:
                        why = strict_check(
                            item.native_form_id, dict(item.native_fields), text
                        )
                    else:
                        why = compare(selache_operands(text), n)
                except CrossCheckError as e:
                    why = str(e)
                if (
                    why
                    and use_stmts
                    and listing_quirk(
                        text,
                        listed.get(parcel, 0),
                        n,
                        item.native_form_id,
                        dict(item.native_fields),
                        len(item.external.raw),
                    )
                ):
                    why = None  # the source and our decoder agree; seldump's listing is off
        except CrossCheckError as e:
            why = str(e)
        if why:
            out.append(Mismatch(parcel, raw, text, native, why, stmt, line))
    return out
