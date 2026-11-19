"""Card services for a development card, by `patchkit services` (packages/patchkit/src/services.rs): the one rule for which services modules need and for the packed file of each.

A service is a file of the card (`services/<name>.dks`) as packed, with the port file of the device's firmware (`ports/<device>-<version>.dkport`). The command reads the NEEDS
of the packed `.dkm` files, maps them to the service modules of the manifests, and packs each into a work folder.
"""

from __future__ import annotations

import json
import shutil
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


def card_services(
    device: str, work: Path, files: Iterable[Path] = (), modules: Iterable[str] = ()
) -> list[dict[str, Any]]:
    """The card files that the packed `.dkm` FILES and the service modules named in MODULES need on DEVICE: each service packed in WORK, and the port file of the stock firmware.

    One dict each: `service`, `module_id`, `store_path` (relative to the digikit folder of a card) and `file`. The last is the port file
    (`ports/<device>-<version>.dkport`, built by `patchkit port build` from committed data), when any service is needed.
    """
    out = _run(
        ["services", "--device", device, "--pack-dir", work]
        + [a for m in modules for a in ("--module", m)]
        + [str(f) for f in files]
    )
    if out:
        version = STOCK_VERSION[device]
        ports = work / "ports"
        shutil.rmtree(ports, ignore_errors=True)
        ports.mkdir(parents=True)
        result = patchkit.run(
            [
                "port",
                "build",
                "--device",
                device,
                "--version",
                version,
                "--out",
                str(ports),
            ],
            capture=True,
        )
        name = f"{device}-{version}.dkport"
        if result.returncode or not (ports / name).is_file():
            raise RuntimeError(
                f"patchkit port build failed:\n{(result.stderr or result.stdout).strip()}"
            )
        out.append(
            {
                "service": "port",
                "module_id": None,
                "store_path": f"ports/{name}",
                "file": str(ports / name),
            }
        )
    return out
