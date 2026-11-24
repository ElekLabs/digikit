"""The host directory of the runtime (`DKRT_HOST_DIR`) for the Python rigs: where a rig finds a static of the system package.

The system package is a card file placed at a different address each boot, so its statics (`DKRT_DSP`, `DKRT_PROVIDERS`, `DKRT_TICKS`, `DKRT_TABLE_BAD`) have no symbol in
the image or in a patch plan. The resident core keeps one table of their addresses; this module is the one Python reader of it. The ids and the layout come from
`runtime/core/abi.toml` (`[[host_dir]]`, the `HOST_DIR_*` consts), never from a copy here; the TypeScript and Rust readers use the same file through `patchkit gen`.

Layout (big-endian words, the core's `HostDir`): magic, count of ids the core knows, ready, then one address per id. The core marks the directory ready last, once the
package has attached: a directory that is not ready, or an entry that is empty, means no package is loaded.
"""

from __future__ import annotations

import tomllib
from functools import cache
from pathlib import Path

ABI = Path(__file__).resolve().parents[2] / "runtime" / "core" / "abi.toml"
HEADER = 12  # magic, count, ready


class NoPackage(Exception):
    """The directory is not ready or the entry is empty: no package is loaded."""


@cache
def _abi() -> dict:
    return tomllib.loads(ABI.read_text())


def ids() -> dict[str, int]:
    """The id of each name (`DSP`, `PROVIDERS`, ...): the order of `[[host_dir]]` in abi.toml. A removed id (tombstone) has no name here: it resolves to nothing."""
    return {
        h["name"]: i
        for i, h in enumerate(_abi()["host_dir"])
        if h["owner"] != "removed"
    }


def owners() -> dict[str, str]:
    """`core` or `package` per name: who fills the entry."""
    return {
        h["name"]: h["owner"] for h in _abi()["host_dir"] if h["owner"] != "removed"
    }


def _const(name: str) -> int:
    return next(c["value"] for c in _abi()["const"] if c["name"] == name)


def directory_bytes() -> int:
    """The bytes of the directory a rig reads."""
    return HEADER + 4 * len(_abi()["host_dir"])


def entry_address(name: str, directory_at: int) -> int:
    """The address of the entry of static `name` in the directory at `directory_at` (the plan symbol `DKRT_HOST_DIR`): a script op reads the word stored there with `ptr`."""
    return directory_at + HEADER + 4 * ids()[name]


def package_address(name: str, directory: bytes) -> int:
    """The address of package static `name` from the bytes of the directory (`directory_bytes` long); raises `NoPackage` when none is loaded."""
    if len(directory) < HEADER or int.from_bytes(directory[0:4], "big") != _const(
        "HOST_DIR_MAGIC"
    ):
        raise ValueError("DKRT_HOST_DIR: not a host directory")
    if int.from_bytes(directory[8:12], "big") != _const("HOST_DIR_READY_MAGIC"):
        raise NoPackage(
            f"no package loaded: the host directory is not ready (DKRT_{name})"
        )
    at = HEADER + 4 * ids()[name]
    address = int.from_bytes(directory[at : at + 4], "big")
    if address == 0:
        raise NoPackage(
            f"no package loaded: the host directory has no address for DKRT_{name}"
        )
    return address


def read_package_address(word, directory_at: int, name: str) -> int:
    """The address of package static `name`, reading the directory at `directory_at` with `word(address) -> int` (a rig's big-endian word reader); raises `NoPackage` when none is loaded."""
    raw = b"".join(
        word(directory_at + 4 * i).to_bytes(4, "big")
        for i in range(directory_bytes() // 4)
    )
    return package_address(name, raw)
