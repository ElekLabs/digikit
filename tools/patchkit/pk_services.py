"""Card services for a development card: which services a module needs, and the bound `.dks` file of each.

A service with hooks is a file of the card (`services/<name>.dks`), bound to the stock firmware of the device (`patchkit bind`). A module that needs a service has a
hard NEEDS item for its name hash in the header table of its packed `.dkm`. The service of a name is the module whose `module.toml` has `role = "service"` and
`service = "<name>"`. The Nx targets `<id>:pack` and `<id>:bind` and tests/regression/runtime/service-files.ts do the same for the device tests; the sidecar check
here is the same as `staleBinding` there.
"""

from __future__ import annotations

import hashlib
import json
import struct
import tomllib
from collections.abc import Iterable
from pathlib import Path

from tools.patchkit import modroots, patchkit

STOCK_VERSION = {"dt2": "1.17", "dn2": "1.12"}
TAG_NEEDS = 1  # packages/dkneeds/src/keys.rs
TABLE_MAGIC = 0x444B5442


def name_hash(name: str) -> int:
    """FNV-1a 32 of a service or export name (`nameHash` in tests/regression/runtime/dkm-parse.ts)."""
    h = 0x811C9DC5
    for b in name.encode():
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def needed_hashes(dkm: bytes) -> list[int]:
    """The service name hashes of the NEEDS entry of a packed `.dkm`; none for a file without a header table (`neededServiceHashes`)."""
    if len(dkm) < 0x98:
        return []
    (table,) = struct.unpack_from(">I", dkm, 0x90)
    if table + 8 > len(dkm) or struct.unpack_from(">I", dkm, table)[0] != TABLE_MAGIC:
        return []
    (entries,) = struct.unpack_from(">H", dkm, table + 6)
    at = table + 8
    for _ in range(entries):
        tag, count, size = struct.unpack_from(">HHI", dkm, at)
        if tag == TAG_NEEDS:
            return [
                struct.unpack_from(">I", dkm, at + 8 + 12 * k)[0] for k in range(count)
            ]
        at += 8 + size
    return []


def service_modules() -> dict[str, str]:
    """Service name -> module id, from the manifests (`role = "service"`, `service = NAME`)."""
    found = {}
    for mid, folder in modroots.manifests().items():
        manifest = tomllib.loads((folder / "module.toml").read_text())["module"]
        if manifest.get("role") == "service" and manifest.get("service"):
            found[manifest["service"]] = mid
    return found


def service_name(mid: str) -> str | None:
    """The service name of a module id, or None when the module is not a service."""
    return next((n for n, m in service_modules().items() if m == mid), None)


def needed_services(files: Iterable[Path]) -> list[str]:
    """The names of the services that the packed `.dkm` files need, sorted."""
    needed = {h for f in files for h in needed_hashes(Path(f).read_bytes())}
    return sorted(n for n in service_modules() if name_hash(n) in needed)


def store_path(name: str) -> str:
    """The path of a service file on the card, relative to the digikit folder."""
    return f"services/{name}.dks"


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stale_binding(out: Path, packed: Path, firmware: Path) -> str | None:
    """Why the bound file OUT does not match the packed `.dks` and the firmware image (its `.src.json` sidecar), or None when it does."""
    sidecar = out.with_suffix(".src.json")
    if not sidecar.is_file():
        return "has no .src.json sidecar"
    src = json.loads(sidecar.read_text())
    if src.get("unbound_sha256") != _sha256(packed):
        return f"was bound from another {packed.name}"
    if firmware.is_file() and src.get("firmware_sha256") != _sha256(firmware):
        return "was bound to another firmware image"
    return None


def bound_service(device: str, mid: str, work: Path, trace: bool = False) -> Path:
    """Pack the service MID for DEVICE, then bind it to the stock firmware (`patchkit pack`, `patchkit bind`, as the Nx target `<id>:bind` runs them).

    The bound file is kept in WORK between runs; a file whose sidecar does not match the fresh pack or the firmware is bound again.
    """
    from tools.patchkit import pk_harness

    syx, _ready = pk_harness.firmware_inputs(
        pk_harness.Cell(device, STOCK_VERSION[device])
    )
    leaf = mid.rpartition("/")[2]
    work.mkdir(parents=True, exist_ok=True)
    packed, out = work / f"{leaf}.unbound.dks", work / f"{leaf}.dks"
    pack = ["pack", mid, "--device", device, "-o", packed]
    bind = ["bind", packed, "--firmware", syx, "-o", out]
    for args in (pack + (["--trace"] if trace else []), bind):
        if args is bind and out.is_file() and not stale_binding(out, packed, Path(syx)):
            break
        result = patchkit.run([str(a) for a in args], capture=True)
        if result.returncode:
            raise RuntimeError(
                f"patchkit {args[0]} {mid} for {device} failed:\n{(result.stderr or result.stdout).strip()}"
            )
    return out
