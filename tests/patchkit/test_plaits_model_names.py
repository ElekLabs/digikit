"""PLAITS MODEL value names: the ColdFire formatter of machines/plaits and core/params' `formatter` field.

- `plaits_model_fmt(int raw, char *buf)` runs on the emulated ColdFire core (tools/patchkit/pk_cfcall.py) with the
  stock formatters' convention (research-model-names.md, "Calling convention"): the raw 8.8 word at 4(sp), the buffer
  at 8(sp), d0/d1/a0/a1 scratch, d2-d7/a2-a6 preserved, `rts` with the caller popping the arguments.
- The patched DT2 1.17 images: every appended parameter row is the row the expressions gave before the field
  existed (+0x34 = the template's formatter), except PLAITS' MODEL row, whose +0x34 is plaits_model_fmt.

Skips without the m68k toolchain, the stock image or the patchkit build."""

from __future__ import annotations

import json
import re
import struct
import subprocess
import tomllib
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

from tools.firmware import config
from tools.patchkit import pk_cfcall, pk_link, pk_toolchains
from tools.patchkit import port_resolve as pr
from tools.patchkit.pk_expr import evaluate as eval_expr

ROOT: Path = REPO_ROOT
SYX = config.stock("Digitakt_II_OS1.17.syx")
SRC = ROOT / "modules" / "machines" / "plaits" / "src" / "cf" / "model_fmt.s"
PARAMS = ROOT / "modules" / "core" / "params" / "module.toml"
BASE = 0x47E8B000
BUFS = 0x47EC0000
MAIN_BASE = 0x40000400

NAMES = [
    "VA",
    "ASYM-TRI",
    "2FM",
    "FORMANT",
    "ADDITIVE",
    "WAVETABLE",
    "CHORD",
    "SPEECH",
    "SWARM",
    "NOISE",
    "DUST",
    "STRING",
    "RINGS",
    "KICK",
    "SNARE",
    "HIHAT",
    "VCF-VA",
    "PHASE-DIST",
    "DX7 1",
    "DX7 2",
    "DX7 3",
    "TERRAIN",
    "STRING MACH",
    "SQUARE",
]

needs_cf = pytest.mark.skipif(
    pk_toolchains.m68k_bin("gcc") is None or not SYX.exists(),
    reason="m68k toolchain or firmware/Digitakt_II_OS1.17.syx missing",
)


def expected_text(raw: int) -> str:
    i = (raw & 0xFFFFFFFF) >> 8 if raw >= 0 else -1
    return NAMES[i] if 0 <= i < len(NAMES) else "ERR"


@needs_cf
def test_the_formatter_writes_the_name_with_the_stock_calling_convention() -> None:
    _obj, linked, _hist = pk_link.build([SRC], BASE, {})
    assert linked.bss_size == 0
    fmt = linked.symbols["plaits_model_fmt"]
    raws = [0, 0xFF, 0x100, 0x180, *[i << 8 for i in range(2, 24)], 0x17FF]
    raws += [0x1800, 0x1700 + 0x100, 0x7F00, -1, -0x100, -0x80000000, 0x7FFFFFFF]
    # each case its own 32-byte buffer, pre-filled with 0xAA: the text ends in one NUL and nothing past it is written
    bufs = [BUFS + 32 * k for k in range(len(raws))]
    pokes = {BASE: linked.image, BUFS: b"\xaa" * (32 * len(raws))}
    calls: list[dict[str, object]] = [
        {"pc": fmt, "args": [raw, buf], "reads": [(buf, 32)]}
        for raw, buf in zip(raws, bufs, strict=True)
    ]
    res = pk_cfcall.CfMachine(SYX).run(pokes, calls)
    for raw, r in zip(raws, res, strict=True):
        assert r.ok, (raw, r.error)
        assert not r.clobbered, (raw, r.clobbered)  # d2-d7/a2-a6 as on entry
        want = expected_text(raw).encode()
        got = r.reads[0]
        assert got[: len(want) + 1] == want + b"\0", (raw, got)
        assert got[len(want) + 1 :] == b"\xaa" * (31 - len(want)), (raw, got)
    # index 0, 1, 23, 24 and negative, as the brief lists them
    assert [expected_text(x) for x in (0, 0x100, 0x1700, 0x1800, -1)] == [
        "VA",
        "ASYM-TRI",
        "SQUARE",
        "ERR",
        "ERR",
    ]


def test_names_fit_the_header() -> None:
    # `Model=` + the value in the header: about 15 characters of room (research-model-names.md section 2)
    assert len(NAMES) == 24 and len(set(NAMES)) == 24
    assert max(len(n) for n in NAMES) <= 11
    assert all("%" not in n for n in NAMES)


def test_the_source_has_the_exact_names_in_order() -> None:
    text = SRC.read_text()
    strings = dict(re.findall(r'^(pm\w+):\s*\.asciz\s+"([^"]*)"', text, re.M))
    assert [strings[f"pm{i}"] for i in range(24)] == NAMES
    assert strings["pm_err"] == "ERR" and len(strings) == 25


def test_plaits_names_its_formatter_and_keeps_the_template() -> None:
    m = tomllib.loads(
        (ROOT / "modules" / "machines" / "plaits" / "module.toml").read_text()
    )
    rows = {e["id"]: e for e in m["entry"] if e["registry"] == "params"}
    assert rows["plaits_model"]["formatter"] == "plaits_model_fmt"
    assert rows["plaits_model"]["template"] == 0xFE
    assert all("formatter" not in e for k, e in rows.items() if k != "plaits_model")
    obj = next(o for o in m["object"] if "src/cf/model_fmt.s" in o["sources"])
    assert obj["place"] == "cf.expansion" and obj["devices"] == ["dt2"]
    assert not [o for o in m.get("op", []) if o.get("op") == "hook"]


def test_both_registries_have_the_formatter_field_last_with_default_0() -> None:
    m = tomllib.loads(PARAMS.read_text())
    for reg in m["registry"]:
        f = reg["field"][-1]
        assert (f["name"], f["kind"], f["default"]) == ("formatter", "u32", 0)


# ---- the patched images ----------------------------------------------------------------------------------------------


def patchkit(*args: object) -> None:
    from tools.emulator import native_target

    target = native_target.target_dir(None).parent / "nogen"
    subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "patchkit",
            "--release",
            "--offline",
            "--target-dir",
            str(target),
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    r = subprocess.run(
        [str(target / "release" / "patchkit"), "--root", str(ROOT), *map(str, args)],
        capture_output=True,
        text=True,
    )
    assert r.returncode == 0, r.stderr + r.stdout


def expansion_bytes(main: bytes, stock_len: int, addr: int, n: int) -> bytes:
    """`n` bytes at `addr` from the expansion segment chain appended to the patched MAIN OS (verify.rs
    read_segments: len, dest, bss, payload ...; len 0xffffffff ends it)."""
    p = stock_len
    while True:
        (length,) = struct.unpack_from(">I", main, p)
        if length == 0xFFFFFFFF:
            raise AssertionError(f"{addr:#x} is in no segment")
        dest, _bss = struct.unpack_from(">II", main, p + 4)
        if dest <= addr and addr + n <= dest + length:
            return main[p + 12 + addr - dest : p + 12 + addr - dest + n]
        p += 12 + length


SETS = {
    "pluck": ["core/dsp-voice", "core/machines", "core/params", "machines/pluck"],
    "plaits": [
        "core/machines",
        "core/params",
        "core/dsp-inline",
        "machines/plaits",
        "machines/plaits-48k",
    ],
}


@pytest.mark.skipif(not SYX.exists(), reason="firmware/Digitakt_II_OS1.17.syx missing")
@pytest.mark.parametrize("name", sorted(SETS))
def test_appended_rows_keep_the_template_formatter_unless_named(
    name: str, tmp_path: Path
) -> None:
    out, plan_path = tmp_path / f"{name}.syx", tmp_path / "plan.json"
    patchkit(
        "apply",
        "--firmware",
        SYX,
        "--modules",
        ",".join(SETS[name]),
        "-o",
        out,
        "--plan",
        plan_path,
    )
    plan = json.loads(plan_path.read_text())
    stock_main = pr.sections_of(SYX)[3]
    main = pr.sections_of(out)[3]
    reg = next(r for r in plan["registries"] if r["name"] == "params")
    sec = next(s for s in plan["sections"] if s["name"] == "pf_table")
    t1, copied = sec["from_addr"], sec["copied"]
    table = expansion_bytes(main, len(stock_main), sec["addr"], sec["bytes"])
    assert table[:copied] == stock_main[t1 - MAIN_BASE : t1 - MAIN_BASE + copied]
    regbytes = expansion_bytes(main, len(stock_main), reg["addr"], reg["bytes"])
    off = {f: o for f, _k, o in reg["fields"]}
    syms = {k.split(":", 1)[1]: v for k, v in plan["symbols"].items()}

    def stock_word(code: int, at: int) -> int:
        return struct.unpack_from(">I", stock_main, t1 - MAIN_BASE + 60 * code + at)[0]

    seen_fmt = []
    for k, e in enumerate(reg["entries"]):
        row = table[copied + 60 * k : copied + 60 * (k + 1)]
        words = struct.unpack(">15I", row)
        entry = {
            f: struct.unpack_from(">I", regbytes, reg["row_size"] * k + o)[0]
            for f, o in off.items()
        }
        # the row the manifest gave before the formatter field: word 13 is the template's formatter
        before = [
            stock_word(entry["template"], 0x34)
            if wi == 13
            else eval_expr(
                w,
                {"cf.params.t1": t1},
                entry,
                lambda a, n: stock_main[a - MAIN_BASE : a - MAIN_BASE + n],
            )
            & 0xFFFFFFFF
            for wi, w in enumerate(
                next(
                    s
                    for s in tomllib.loads(PARAMS.read_text())["section"]
                    if s["name"] == "pf_table" and s["devices"] == ["dt2"]
                )["append"]["row"]
            )
        ]
        if e["id"] == "plaits_model":
            assert entry["formatter"] == syms["plaits_model_fmt"]
            assert words[13] == syms["plaits_model_fmt"]
            assert list(words[:13]) + list(words[14:]) == before[:13] + before[14:]
            seen_fmt.append(e["id"])
        else:
            assert entry["formatter"] == 0
            assert list(words) == before, e["id"]
    assert seen_fmt == (["plaits_model"] if name == "plaits" else [])
