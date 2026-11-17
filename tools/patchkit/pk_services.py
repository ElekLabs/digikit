"""Card services for a development card, by `patchkit services` (packages/patchkit/src/services.rs): the one rule for which services modules need and for the bound file of each.

A service with hooks is a file of the card (`services/<name>.dks`), bound to the stock firmware of the device. The command reads the NEEDS of the packed `.dkm` files,
maps them to the service modules of the manifests, and packs and binds each into a work folder (a file whose `.src.json` sidecar matches is kept).
"""

from __future__ import annotations

import json
from collections.abc import Iterable
from pathlib import Path
from typing import Any

from tools.patchkit import patchkit

STOCK_VERSION = {"dt2": "1.17", "dn2": "1.12"}


def _run(args: list[Any]) -> list[dict[str, Any]]:
    result = patchkit.run([str(a) for a in args], capture=True)
    if result.returncode:
        raise RuntimeError(
            f"patchkit {args[0]} failed:\n{(result.stderr or result.stdout).strip()}"
        )
    return [json.loads(line) for line in result.stdout.splitlines() if line.strip()]


def is_service(device: str, mid: str) -> bool:
    """Whether the module id MID is a service module (`role = "service"`)."""
    return not patchkit.run(
        ["services", "--device", device, "--module", mid], capture=True
    ).returncode


def bound_services(
    device: str, work: Path, files: Iterable[Path] = (), modules: Iterable[str] = ()
) -> list[dict[str, Any]]:
    """The services that the packed `.dkm` FILES need and the service modules named in MODULES, each packed and bound to the stock firmware of DEVICE in WORK.

    One dict each: `service`, `module_id`, `store_path` (relative to the digikit folder of a card) and `bound_path`.
    """
    from tools.patchkit import pk_harness

    syx, _ready = pk_harness.firmware_inputs(
        pk_harness.Cell(device, STOCK_VERSION[device])
    )
    return _run(
        ["services", "--device", device, "--firmware", syx, "--bind-dir", work]
        + [a for m in modules for a in ("--module", m)]
        + [str(f) for f in files]
    )
