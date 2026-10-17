"""pk_build: a DSP object may list C sources next to its assembly sources.

Each `.c` is compiled by Selache's C compiler (pk_sharc.compile_c, with the source's directory on the include path),
the `.s` files are assembled as one translation unit (pk_sharc.assemble), and the pieces are merged into one object
(pk_sharc.merge): one code section, the data sections joined by name, every symbol and relocation moved to its place.
Skips when Selache is not built (uv run python -m tools.patchkit.pk_toolchains selache)."""

from __future__ import annotations

import json

import pytest

from tools.patchkit import pk_build, pk_sharc, pk_toolchains

pytestmark = pytest.mark.skipif(
    pk_toolchains.selache_dir() is None,
    reason="selache not built (tools/patchkit/pk_toolchains.py selache)",
)

MODULE = """[module]
id = "test/c-fixture"
version = "0.1.0"
name = "C fixture"
category = "test"
status = "experimental"
summary = "fixture"
ports = ["dt2-1.17"]

[[object]]
path = "build/fixture-dsp.dko.json"
sources = ["src/dsp/caller.s", "src/dsp/twice.c"]
place = "dsp.l2"
devices = ["dt2"]
"""

TWICE_H = "int twice(int x);\nextern const int fx_table[2];\n"
TWICE_C = '#include "twice.h"\nconst int fx_table[2] = {3, 5};\nint twice(int x) { return x + x + fx_table[1]; }\n'
CALLER_S = """.SECTION/PM seg_pmco;
.GLOBAL _caller;
.EXTERN twice.;
_caller:
    JUMP twice.;
"""


def _fixture(tmp_path):
    d = tmp_path / "c-fixture"
    (d / "src" / "dsp").mkdir(parents=True)
    (d / "module.toml").write_text(MODULE)
    (d / "src" / "dsp" / "twice.h").write_text(TWICE_H)
    (d / "src" / "dsp" / "twice.c").write_text(TWICE_C)
    (d / "src" / "dsp" / "caller.s").write_text(CALLER_S)
    return d


def test_dsp_object_with_c_and_s_sources_builds(tmp_path):
    d = _fixture(tmp_path)
    msgs = pk_build.build_module(d, check=False)
    assert msgs and "re-decoded OK" in msgs[0]
    obj = json.loads((d / "build" / "fixture-dsp.dko.json").read_text())
    assert obj["arch"] == "sharc"
    syms = {s["name"]: s for s in obj["symbols"]}
    # both symbols exported from the one code section, the C one after the assembly
    assert syms["_caller"]["section"] == pk_sharc.SECTION and syms["_caller"]["global"]
    assert syms["twice."]["section"] == pk_sharc.SECTION and syms["twice."]["global"]
    assert syms["twice."]["value"] > syms["_caller"]["value"]
    # the C object's initialised table is a data section with its symbol
    table = syms["fx_table."]
    data = {s["name"]: s for s in obj["sections"]}[table["section"]]
    assert data["kind"] == "data"
    words = bytes.fromhex(data["hex"])[table["value"] : table["value"] + 8]
    assert words == (3).to_bytes(4, "little") + (5).to_bytes(4, "little")
    # the jump in the assembly resolves to the compiled function (relocation by name, inside the object)
    (jump,) = [r for r in obj["relocs"] if r["symbol"] == "twice."]
    assert jump["offset"] == syms["_caller"]["value"] and jump["type"] == "sharc.abs24"
    # the C code's reference to its table is relocated against the data symbol
    assert any(r["symbol"] == "fx_table." for r in obj["relocs"])
    # every source is in the hash, and a rebuild is current
    assert pk_build.build_module(d, check=True) == [
        f"{d / 'build' / 'fixture-dsp.dko.json'}: current"
    ]
    (d / "src" / "dsp" / "twice.c").write_text(TWICE_C.replace("x + x", "x + x + 1"))
    with pytest.raises(pk_build.BuildError, match="stale"):
        pk_build.build_module(d, check=True)


def test_linked_c_code_runs_on_the_core(tmp_path):
    """The merged object links and the compiled function computes on the concrete core (frame as the C ABI
    expects: [I6] = the caller's frame, [I6 - 1] = return address - 1; arguments in R4; the result in R0)."""
    d = _fixture(tmp_path)
    paths = [d / "src" / "dsp" / n for n in ("caller.s", "twice.c")]
    obj = pk_build.dsp_object(paths)
    data_at = 0x20090000
    externs = {
        name: data_at + off for name, (_sec, off, _g) in obj.data_symbols.items()
    }
    base = 0xB90000
    linked = pk_sharc.link(obj, base, externs)
    blob = obj.data[next(iter(obj.data))][0]
    frame, ret = 0x20093000, 0xB9E000
    mem = {frame // 4: 0, frame // 4 - 1: ret - 1}
    mem.update(
        {
            (data_at + 4 * k) // 4: int.from_bytes(blob[4 * k : 4 * k + 4], "little")
            for k in range(len(blob) // 4)
        }
    )
    r = pk_sharc.run_call(
        linked,
        "_caller",
        regs={
            "R4": 20,
            "I6": frame,
            "I7": frame - 8,
            "M5": 0,
            "M6": 1,
            "M7": -1,
            "M13": 0,
            "M14": 1,
            "M15": -1,
        },
        dm_words=mem,
        return_sw=ret,
    )
    halt: dict = r["halt"]  # type: ignore[assignment]
    regs: dict = r["regs"]  # type: ignore[assignment]
    assert halt["reason"] == "breakpoint", halt
    assert regs["R0"] == 45
