"""The permanent machine type numbers (modules/core/machines/types.toml) and `pk_new.allocate`."""

from __future__ import annotations

import tomllib
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.patchkit import modroots, pk_new  # noqa: E402

TYPES = ROOT / "modules" / "core" / "machines" / "types.toml"


def machine_entries() -> list[tuple[str, str, list[str]]]:
    out = []
    for m in sorted(modroots.manifests().values()):
        d = tomllib.loads((m / "module.toml").read_text())
        # hand-written rows and the typed [[machine]] sections
        rows = [e for e in d.get("entry", []) if e["registry"] == "machines"]
        for e in [*rows, *d.get("machine", [])]:
            # a module without a device filter is meant for the devices its [targets] name
            on = {k.split("-")[0] for k in d.get("targets", {})}
            out.append((d["module"]["id"], e["id"], e.get("devices") or sorted(on)))
    return out


def test_every_machine_entry_has_a_number_per_device() -> None:
    t = tomllib.loads(TYPES.read_text())
    for mod, eid, devs in machine_entries():
        if (
            mod == "core/dynreg"
        ):  # its rows reserved the run-time range, which this fork retires
            continue
        dynamic = tomllib.loads((modroots.module_dir(mod) / "module.toml").read_text())[
            "module"
        ].get("dynamic", False)
        for dev in devs:
            # a run-time module has no number: the runtime assigns it at load (D7), so it is in no table
            if dynamic:
                assert f"{mod}:{eid}" not in t[dev]["alloc"], f"{mod}:{eid} on {dev}"
                assert "dynamic" not in t[dev], dev
            else:
                assert f"{mod}:{eid}" in t[dev]["alloc"], f"{mod}:{eid} on {dev}"


def test_numbers_are_unique_in_range_and_never_stock() -> None:
    t = tomllib.loads(TYPES.read_text())
    for dev, first in pk_new.STOCK_TYPES.items():
        for table in ("alloc",):
            nums = list(t[dev].get(table, {}).values())
            assert len(nums) == len(set(nums)), f"{dev} {table}"
            assert all(first <= n <= t[dev]["last"] for n in nums), f"{dev} {table}"
        # the run-time range is core/dynreg's: a built-in machine never takes a number in it
        dyn_first = t[dev].get("dynamic_first")
        for k, v in t[dev]["alloc"].items():
            assert dyn_first is None or v < dyn_first or k.startswith("core/dynreg:"), k
    assert t["dt2"]["last"] < 24  # the DSP selector table was read to entry 23


# A table with free numbers below the run-time range (the real one can be full there).
ROOMY = """\
[dt2]
last = 23
dynamic_first = 16

[dt2.alloc]
"test/a:a" = 7
"test/b:b" = 9
"core/dynreg:dyn16" = 16

[dn2]
last = 23
dynamic_first = 16

[dn2.alloc]
"test/c:c" = 5
"core/dynreg:dyn16" = 16
"""


def test_allocate_takes_the_next_number_and_keeps_existing_ones(tmp_path: Path) -> None:
    p = tmp_path / "types.toml"
    p.write_text(ROOMY)
    before = tomllib.loads(p.read_text())
    got = pk_new.allocate("machines/foo:foo", ["dt2", "dn2"], p)
    after = tomllib.loads(p.read_text())
    for dev in ("dt2", "dn2"):
        first_dyn = before[dev].get("dynamic_first", 1 << 30)
        top = max(v for v in before[dev]["alloc"].values() if v < first_dyn)
        assert got[dev] == top + 1
        assert after[dev]["alloc"]["machines/foo:foo"] == top + 1
        for k, v in before[dev]["alloc"].items():
            assert after[dev]["alloc"][k] == v
    assert pk_new.allocate("machines/foo:foo", ["dt2"], p) == {"dt2": got["dt2"]}


def test_allocate_stops_below_the_run_time_range(tmp_path: Path) -> None:
    p = tmp_path / "types.toml"
    p.write_text(ROOMY.replace('"test/b:b" = 9', '"test/b:b" = 15'))
    with pytest.raises(SystemExit, match=r"\[dt2\] is full"):
        pk_new.allocate("machines/foo:foo", ["dt2"], p)
