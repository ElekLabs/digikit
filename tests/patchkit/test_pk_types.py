"""The permanent machine type numbers (modules/core/adapters/machines/types.toml)."""

from __future__ import annotations

import tomllib

from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.patchkit import modroots  # noqa: E402

TYPES = ROOT / "modules" / "core" / "adapters" / "machines" / "types.toml"
STOCK_TYPES = {"dt2": 7, "dn2": 5}  # the first number that is not a stock type


def machine_entries() -> list[tuple[str, str, list[str]]]:
    out = []
    for m in sorted(modroots.manifests().values()):
        d = tomllib.loads((m / "module.toml").read_text())
        rows = [e for e in d.get("entry", []) if e["registry"] == "machines"]
        for e in rows:
            # a module without a device filter is meant for the devices its [targets] name
            on = {k.split("-")[0] for k in d.get("targets", {})}
            out.append((d["module"]["id"], e["id"], e.get("devices") or sorted(on)))
    return out


def test_every_machine_entry_has_a_number_per_device() -> None:
    t = tomllib.loads(TYPES.read_text())
    for mod, eid, devs in machine_entries():
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
    for dev, first in STOCK_TYPES.items():
        for table in ("alloc",):
            nums = list(t[dev].get(table, {}).values())
            assert len(nums) == len(set(nums)), f"{dev} {table}"
            assert all(first <= n <= t[dev]["last"] for n in nums), f"{dev} {table}"
        # the run-time range is core/dynreg's: a built-in machine never takes a number in it
        dyn_first = t[dev].get("dynamic_first")
        for k, v in t[dev]["alloc"].items():
            assert dyn_first is None or v < dyn_first or k.startswith("core/dynreg:"), k
    assert t["dt2"]["last"] < 24  # the DSP selector table was read to entry 23
