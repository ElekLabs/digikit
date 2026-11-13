#!/usr/bin/env python3
"""addrnames: names for code addresses in dev tools (docs/ADDRMAP.md). A thin client of `patchkit addr`; there is no Python reader of the map.

    from tools.devtools.addrnames import AddrNames, read_load_table
    names = AddrNames.for_image(Path("out/patch/x.syx"), modules=[Path("out/patch/demo-dt2.dkm")], loads=read_load_table(peek32, state))
    names.text(0x401be1be)                       # "FUN_401be1be", "dkrt::card::gate+0x14" or "0x401be1be"
    names.text(ret_addr, ret=True)               # a return address: looks at the address minus 2

    uv run python -m tools.devtools.addrnames --image out/patch/x.syx [--module X.dkm] [--loads L.json] [--ret] ADDR...
    uv run python -m tools.devtools.addrnames --stock dt2-1.17 sharc:0xb80105      # firmware names only

An image has `X.addrmap.json` (the ship map `patchkit patch` writes, layers our and anchor) beside it and `X.plan.json` (symbols). A module
has `M.addrmap.json` beside its .dkm. The firmware names are the dev map out/addrmap/<device>-<version>.dev.json
(`tools.coldfire.addrmap_export`, made on first use). Without a database the lookup still names the code of the image and its modules.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from collections.abc import Callable, Iterable, Sequence
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
DEV_DIR = ROOT / "out" / "addrmap"

# modules/core/runtime/src/rs/src/lib.rs: `State` is ten words, then MAX_MODULES `Module` records of 13 words
# (used, name[32], version, base, size, fini); the symbol is DKRT_STATE of the runtime's plan symbols.
STATE_HEADER_BYTES = 40
MODULE_RECORD_BYTES = 52


class AddrNamesError(RuntimeError):
    pass


def dev_map(image: str) -> Path:
    """The dev map of `dt2-1.17`, `dn2-1.12`, ... (the firmware names), exported on first use. Raises when the databases are missing."""
    path = DEV_DIR / f"{image}.dev.json"
    if not path.is_file():
        from tools.coldfire import addrmap_export, fw

        try:
            doc = addrmap_export.export(image)
        except fw.FwError as e:
            raise AddrNamesError(str(e)) from e
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(doc, separators=(",", ":")) + "\n")
    return path


def plan_symbol(plan: dict[str, Any], name: str) -> int:
    """The address of a runtime symbol in a patch plan: the exact name after the module tag (`core/runtime@0.6.0:DKRT_STATE`)."""
    found = {v for k, v in plan["symbols"].items() if k.endswith(":" + name)}
    if len(found) != 1:
        raise AddrNamesError(f"symbol {name}: {len(found)} addresses in the plan")
    return found.pop()


def read_load_table(
    peek32: Callable[[int], int | None], state: int, max_modules: int = 8
) -> dict[str, Any]:
    """The load table of a running emulator: `{"modules": [{"id", "base", "size"}]}` from the runtime's module table at `state` (DKRT_STATE).
    `peek32` reads one guest longword (`digi_client` `peek32`)."""
    out = []
    first = state + STATE_HEADER_BYTES
    for i in range(max_modules):
        at = first + i * MODULE_RECORD_BYTES

        def word(k: int, at: int = at) -> int:
            return peek32(at + 4 * k) or 0

        if word(0) == 0:
            continue
        raw = b"".join(word(1 + k).to_bytes(4, "big") for k in range(8))
        name = raw.split(b"\0")[0].decode("ascii", "replace")
        out.append({"id": name, "base": word(10), "size": word(11)})
    return {"modules": out}


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class AddrNames:
    def __init__(self, maps: Sequence[Path], loads: dict[str, Any] | None = None):
        self.maps = [Path(m) for m in maps]
        self.loads = loads or {"modules": []}
        self._cache: dict[tuple[str, int, bool], dict[str, Any]] = {}
        self.unloaded: list[str] = []

    @classmethod
    def for_image(
        cls,
        syx: Path,
        modules: Iterable[Path] = (),
        loads: dict[str, Any] | None = None,
        dev: bool = True,
    ) -> AddrNames:
        """The names of a patched image, the modules loaded on it (`.dkm` files with their `.addrmap.json`) and, with `dev`, the firmware."""
        syx = Path(syx)
        ship = syx.with_suffix(".addrmap.json")
        if not ship.is_file():
            raise AddrNamesError(
                f"{ship}: no address map beside the image (`patchkit patch` writes it)"
            )
        key = json.loads(ship.read_text())["key"]
        if key.get("output_sha256") and key["output_sha256"] != _sha256(syx):
            raise AddrNamesError(
                f"{ship} is stale: it belongs to another build of {syx.name}"
            )
        maps = [ship]
        for m in modules:
            side = Path(m).with_suffix(".addrmap.json")
            if not side.is_file():
                raise AddrNamesError(f"{side}: no address map beside the module")
            maps.append(side)
        if dev:
            try:
                maps.append(dev_map(f"{key['device']}-{key['firmware']}"))
            except AddrNamesError as e:
                print(f"addrnames: no firmware names ({e})", file=sys.stderr)
        return cls(maps, loads)

    @classmethod
    def for_stock(cls, image: str) -> AddrNames:
        """Firmware names only, for an unpatched image (`dt2-1.17`)."""
        return cls([dev_map(image)])

    def _run(self, queries: list[tuple[str, int]], ret: bool) -> list[dict[str, Any]]:
        from tools.patchkit import patchkit

        argv = [str(patchkit.binary()), "addr"]
        for m in self.maps:
            argv += ["--map", str(m)]
        for mod in self.loads["modules"]:
            argv += ["--load", f"{mod['id']}={mod['base']:x}"]
        if ret:
            argv.append("--ret")
        argv.append("--stdin")
        stdin = "".join(f"{cpu}:{addr:#x}\n" for cpu, addr in queries)
        r = subprocess.run(argv, input=stdin, capture_output=True, text=True, cwd=ROOT)
        if r.returncode:
            raise AddrNamesError(
                r.stderr.strip() or f"patchkit addr failed ({r.returncode})"
            )
        doc = json.loads(r.stdout)
        self.unloaded = doc["unloaded"]
        return list(doc["results"])

    def resolve(
        self, addrs: Iterable[int], cpu: str = "cf", ret: bool = False
    ) -> list[dict[str, Any]]:
        """One result per address: `{name, text, layer, sym_addr, offset, size, module}` (`name` None when no layer holds it)."""
        addrs = list(addrs)
        todo = [a for a in dict.fromkeys(addrs) if (cpu, a, ret) not in self._cache]
        if todo:
            for a, res in zip(
                todo, self._run([(cpu, a) for a in todo], ret), strict=True
            ):
                self._cache[(cpu, a, ret)] = res
        return [self._cache[(cpu, a, ret)] for a in addrs]

    def name(self, addr: int, cpu: str = "cf", ret: bool = False) -> str | None:
        """`name` or `name+0xoff`, or None."""
        return self.resolve([addr], cpu, ret)[0]["text"]

    def text(self, addr: int, cpu: str = "cf", ret: bool = False) -> str:
        """The name, or the raw address when no layer holds it."""
        return self.name(addr, cpu, ret) or f"{addr:#x}"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    where = ap.add_mutually_exclusive_group(required=True)
    where.add_argument(
        "--image", type=Path, help="a patched .syx with its .addrmap.json"
    )
    where.add_argument("--stock", help="an image name (dt2-1.17): firmware names only")
    ap.add_argument(
        "--module",
        type=Path,
        action="append",
        default=[],
        help="a .dkm with its .addrmap.json (repeatable)",
    )
    ap.add_argument(
        "--loads", type=Path, help="a load table JSON {modules:[{id, base}]}"
    )
    ap.add_argument("--cpu", default="cf", choices=["cf", "sharc"])
    ap.add_argument(
        "--ret", action="store_true", help="the addresses are return addresses"
    )
    ap.add_argument("addr", nargs="+", type=lambda s: int(s, 0))
    args = ap.parse_args(argv)
    try:
        loads = json.loads(args.loads.read_text()) if args.loads else None
        names = (
            AddrNames.for_stock(args.stock)
            if args.stock
            else AddrNames.for_image(args.image, args.module, loads)
        )
        for a in args.addr:
            print(f"{a:#x}  {names.text(a, args.cpu, args.ret)}")
    except AddrNamesError as e:
        print(f"addrnames: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
