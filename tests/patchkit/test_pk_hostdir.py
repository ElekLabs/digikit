"""The Python reader of the host directory follows runtime/core/abi.toml and agrees with the generated TypeScript ids."""

import re
from pathlib import Path

import pytest

from tools.patchkit import pk_hostdir

ROOT = Path(__file__).resolve().parents[2]


def test_the_ids_are_the_generated_ones():
    ts = (ROOT / "packages/dk-session/src/gen_host_dir.ts").read_text()
    block = ts.split("export const HOST_ID = {")[1].split("} as const")[0]
    generated = {m[0]: int(m[1]) for m in re.findall(r"(\w+): (\d+),", block)}
    assert pk_hostdir.ids() == generated


def _directory(ready: bool, entries: dict[str, int]) -> bytes:
    abi = pk_hostdir._abi()
    const = {c["name"]: c["value"] for c in abi["const"]}
    words = [
        const["HOST_DIR_MAGIC"],
        len(pk_hostdir.ids()),
        const["HOST_DIR_READY_MAGIC"] if ready else 0,
    ]
    words += [entries.get(name, 0) for name in pk_hostdir.ids()]
    return b"".join(w.to_bytes(4, "big") for w in words)


def test_an_entry_is_read_once_the_directory_is_ready_and_not_before():
    entries = {"TICKS": 0x4012_3400}
    assert len(_directory(True, entries)) == pk_hostdir.directory_bytes()
    assert pk_hostdir.package_address("TICKS", _directory(True, entries)) == 0x4012_3400
    with pytest.raises(pk_hostdir.NoPackage, match="not ready"):
        pk_hostdir.package_address("TICKS", _directory(False, entries))
    with pytest.raises(pk_hostdir.NoPackage, match="no address"):
        pk_hostdir.package_address("DSP", _directory(True, entries))
    with pytest.raises(ValueError):
        pk_hostdir.package_address("DSP", bytes(pk_hostdir.directory_bytes()))
