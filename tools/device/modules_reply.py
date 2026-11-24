"""The MODULES reply (runtime 0.11.5): the fixed part of a record ends at `dsp_len`, then `u8 n_sections` and per section `u32 service name hash, u8 n, n words`.

The runtime reads no meaning into the words; this module does, for the two services a host knows: `machines` (five words) and `fx` (one word per effect row). The fixture
`tests/fixtures/protocol/modules-reply.json` is written by the runtime's encoder test and read by every decoder (here and `packages/dk-session/src/modules-reply.ts`).
"""

import struct
from typing import Any

#: The INFO `caps` bit of a runtime whose records end with sections (`sdk/sdk.toml` `[caps]` `MODULE_SECTIONS`).
CAP_MODULE_SECTIONS = 21

#: What a host says when the device's MODULES records end at `dsp_len` (a runtime before 0.11.5 does not set the capability).
OLD_MODULES = "Update the firmware: this runtime lists modules in an older layout"


class OldModulesLayout(ValueError):
    """The reply is of the older layout: the device does not report the `MODULE_SECTIONS` capability."""


#: The bytes of a record before its sections (`u8 index` to `u32 dsp_len`).
MODULE_FIXED = 61


def service_hash(name: str) -> int:
    """FNV-1a 32 of a service name: the hash a section is named by (`dkneeds::keys::name_hash`)."""
    h = 0x811C9DC5
    for c in name.encode():
        h = ((h ^ c) * 0x01000193) & 0xFFFFFFFF
    return h


def _key(name: str) -> str:
    return f"{service_hash(name):08x}"


def _machines(words: list[int]) -> dict[str, Any]:
    """The `machines` section: voice rows, the guard's faults, `nt` with the guard flag in bit 8, then types 0 to 3 and 4 to 7 packed from the low byte."""
    rows, faults, nt, lo, hi = (words + [0] * 5)[:5]
    types = [
        ((lo if i < 4 else hi) >> (8 * (i % 4))) & 0xFF
        for i in range(min(nt & 0xFF, 8))
    ]
    return {
        "rows": rows,
        "types": types,
        "faults": faults,
        "guard_off": bool(nt & 0x100),
    }


def _effects(words: list[int]) -> list[dict[str, int]]:
    """The `fx` section: a word per effect row: row (bits 0 to 7), then four bits each of the route it runs on, the saved route, the allowed routes and the default."""
    return [
        {
            "row": w & 0xFF,
            "running": (w >> 8) & 0xF,
            "saved": (w >> 12) & 0xF,
            "allowed": (w >> 16) & 0xF,
            "dflt": (w >> 20) & 0xF,
        }
        for w in words
    ]


def decode_page(b: bytes, caps: int | None) -> dict[str, Any]:
    """One page of the MODULES reply (the body after the status): `{arena_free, arena_largest, dsp_free, dsp_largest, total, modules: [...]}`. `caps` is the INFO capability word: without the `MODULE_SECTIONS` bit the records are of the older layout and `OldModulesLayout` is raised.

    A module is `{index, name, version, needs (the runtime version it needs, packed major << 16 | minor << 8 | patch), uiInstance (only with a Workbench token), cf_base, cf_size,
    dsp_off, dsp_len, sections ({service hash in 8 hex digits: words}), rows (voice-row mask), types (machine types), faults (the guard's count for them), guard_off (the guard
    switched one off), effects ([{row, running, saved, allowed, dflt}])}`."""
    if caps is None or not caps >> CAP_MODULE_SECTIONS & 1:
        raise OldModulesLayout(OLD_MODULES)
    af, al, df, dl = struct.unpack_from(">IIII", b, 0)
    total, n = b[16], b[17]
    modules: list[dict[str, Any]] = []
    at = 18
    for _ in range(n):
        name = b[at + 1 : at + 33].split(b"\0")[0].decode()
        version, needs, token, cf_base, cf_size, dsp_off, dsp_len = struct.unpack_from(
            ">IIIIIII", b, at + 33
        )
        p = at + MODULE_FIXED
        sections: dict[str, list[int]] = {}
        ns = b[p]
        p += 1
        for _ in range(ns):
            (h,) = struct.unpack_from(">I", b, p)
            count = b[p + 4]
            sections[f"{h:08x}"] = list(struct.unpack_from(f">{count}I", b, p + 5))
            p += 5 + 4 * count
        module: dict[str, Any] = {
            "index": b[at],
            "name": name,
            "version": version,
            "needs": needs,
        }
        if token:
            module["uiInstance"] = token
        module.update(
            cf_base=cf_base,
            cf_size=cf_size,
            dsp_off=dsp_off,
            dsp_len=dsp_len,
            sections=sections,
            **_machines(sections.get(_key("machines"), [])),
            effects=_effects(sections.get(_key("fx"), [])),
        )
        modules.append(module)
        at = p
    return {
        "arena_free": af,
        "arena_largest": al,
        "dsp_free": df,
        "dsp_largest": dl,
        "total": total,
        "modules": modules,
    }
