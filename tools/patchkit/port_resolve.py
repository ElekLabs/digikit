#!/usr/bin/env python3
"""Resolve the anchors and regions of one firmware into its port lockfile.

    uv run python -m tools.patchkit.port_resolve resolve FIRMWARE.syx [--out devices/ports/NAME.toml]
    uv run python -m tools.patchkit.port_resolve resolve-all [--root DIR]      # the four supported firmwares
    uv run python -m tools.patchkit.port_resolve rule-sharc IMAGE PC [--len N] # print an anchor_rule for a SHARC pc

A port (devices/ports/<short>-<version>.toml) records, for one stock `.syx` (keyed by its sha256), the
named anchors the modules and frameworks need (address, length, sha256 of the bytes there; never the bytes),
the allocatable regions and the container limits. patchkit only reads it and verifies the hashes.

Anchors are declared by the modules that need them, as `[[anchor_rule]]` entries in module.toml:

    [[anchor_rule]]
    name = "dsp.master.q31_call"     # the anchor's name
    devices = ["dt2"]                # rules are per device; the same name may have one rule per device
    kind = "sharc_func"              # see below
    ...

Rule kinds (SHARC rules use the image's sharcdb through tools/sharc_dev/sharc.py):

  sharc_func     reloc_hash = <hash of the containing function>, offset = <parcels from its entry>, len = <bytes>.
                 The function is found by its relocation-insensitive hash (it must be unique in the image);
                 the anchor is the `len` bytes at entry + offset. SHARC `pm` is the entry + offset pc.
  sharc_literal  as sharc_func, but the instruction at entry + offset loads a constant (a literal in the
                 sharcdb): the anchor's value is that constant (a data address) plus `add` (default 0, for a
                 field the code reaches at a fixed distance from the literal); `src` is the instruction.

  sharc_table    as sharc_literal, but the anchor is the table the literal points to: a data anchor of `len` bytes at the
                 software alias 0x28000000 + the literal (`src` = the instruction).

  sharc_mem_addr as sharc_literal, but the instruction at entry + offset is a direct DM access (Type 14d): the
                 anchor's value is its absolute address (a frame word or other fixed data), `src` the instruction.

  cf_words       words = [u32 or "*", ...]: a table row found by a masked signature (big-endian u32 words at 4-byte
                 alignment, exactly one match in the MAIN OS image); a data anchor over the row.

  cf_func        hash = <cfdb h_strict of the containing function>, ordinal = <index among the functions with that
                 hash, in address order; default 0>, offset = <bytes from its entry>, len = <bytes>: a code
                 anchor. The hash masks address operands, so one rule serves relinked builds; a hash with fewer
                 functions than the ordinal needs is an error. Needs the image's cfdb (out/cfdb/<image>-emac).

  cf_params_t1   the parameter descriptor table (60-byte rows), found by the 36-byte signature of its reader (finding 02,
                 exactly one match): a data anchor over all rows (the row count is the length / 60).
  cf_params_sites  the sites of the code that depend on that table, for core/params: the 2-aligned words that point
                 into it and the clamp bounds (`cmpi.l #N,Dn` / `cmpa.l #N,An` with N the row count or one less,
                 followed by scs/scc/bhi/bls), found by the scan of scan_param_sites below. An anchor of
                 kind "sites": addr = the table, len = number of sites, sha256 = their digest.

  cf_ptr         as cf_func (hash, ordinal, offset), but the 4 bytes at entry + offset are an absolute address literal
                 (a `lea table,An`, a `pea`, an `addi.l #table,Dn`): the anchor is the table the literal points to
                 (kind data, `src` = the literal, `len` bytes of the table hashed).

  cf_reset_init_calls   the two reset-path calls (copy helper, clear helper) at the start of the reset code, found by
                 tools/coldfire/regions.py `reset_facts` (a masked signature of the clear helper plus the call sites):
                 an 8-byte code anchor.
  cf_reset_helper       which = "copy" | "clear": a value anchor, the address of that helper, read from the same
                 8 bytes (the displacement of its jsr).

Regions: dsp.l2 (the stream's loaded end to the end of L2, minus excludes), cf.flash_cave (the zero cave that ends at
the reset clear's start) and cf.expansion (the gap above the DMA arena), from tools/coldfire/regions.py `analyse`.

Every anchor is stamped with the rule that produced it and a mark (D = measured or read once). A rule that
matches nothing or more than one place is an error, so a firmware that moved code fails here, not at patch time.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import tomllib
from pathlib import Path

from tools.firmware import config

ROOT = Path(__file__).resolve().parents[2]

RESOLVED_BY = "tools/patchkit/port_resolve.py 1"
LIMITS = {
    "container_max": 0x300000,
    "dsp_stored_max": 0x100000,
    "dsp_depacked_max": 0xFFFFF,
    "main_staged_max": 0x3FFC00,
}
# (device short, version) -> sharcdb image name; devices/*.toml carry the same in [firmware.dsp]
FIRMWARE = {
    "dt2-1.16": "Digitakt_II_OS1.16.syx",
    "dt2-1.17": "Digitakt_II_OS1.17.syx",
    "dn2-1.11": "Digitone_II_OS1.11.syx",
    "dn2-1.12": "Digitone_II_OS1.12.syx",
}
L2_LO, L2_HI = 0x2000_0000, 0x2010_0000
# Free L2 that two DT2 functions load as literals: kept out until a path that uses it is found (finding 06).
DSP_L2_EXCLUDES = {"dt2": [[0x200FA000, 0x200FE000]]}


class ResolveError(RuntimeError):
    pass


def sha(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def load_profiles() -> dict[str, dict]:
    """sha256 -> {device, version, dsp image}"""
    out = {}
    for f in (ROOT / "devices").glob("*.toml"):
        d = tomllib.loads(f.read_text())
        if "device" not in d or "firmware" not in d:
            continue
        for fw in d["firmware"]:
            out[fw["sha256"]] = {
                "device": d["device"]["short"],
                "version": fw["version"],
                "dsp": (fw.get("dsp") or {}).get("image"),
            }
    return out


def module_rules(root: Path) -> list[dict]:
    rules = []
    for m in sorted((root / "modules").rglob("module.toml")):
        d = tomllib.loads(m.read_text())
        for r in d.get("anchor_rule", []):
            rules.append({**r, "_module": d["module"]["id"]})
    return rules


def sections_of(syx: Path) -> dict[int, bytes]:
    from tools.firmware.container import sections
    from tools.firmware.elz import depack_section
    from tools.firmware.extract import classify

    c, table = sections(str(syx))
    out = {}
    for sid, off, clen, _dest in table:
        kind, payload = classify(bytes(c[off : off + clen]))
        out[sid] = depack_section(payload) if kind == "packed" else bytes(payload)
    return out


class Dsp:
    """The DSP boot stream plus its sharcdb."""

    def __init__(self, blob: bytes, image: str):
        from tools.patchkit import dsp_patch
        from tools.sharc_dev import sharc

        self.stream = dsp_patch.Stream(blob)
        self.db = sharc.load(image)
        self.image = image
        self._dsp_patch = dsp_patch

    def byte_addr(self, pc: int) -> int:
        return self._dsp_patch.pc_address(pc, self.stream.present)

    def func_by_hash(self, reloc_hash: str) -> int:
        rows = self.db.sql(
            "SELECT entry_sw FROM func_hash WHERE image=? AND reloc_hash=?",
            self.image,
            reloc_hash,
        )
        if len(rows) != 1:
            raise ResolveError(
                f"reloc_hash {reloc_hash[:12]}.. matches {len(rows)} functions in {self.image}"
            )
        return int(rows[0][0])


class CfFacts:
    """Reset-path facts of one MAIN OS image (tools/coldfire/regions.py)."""

    def __init__(self, name: str, main: bytes):
        from tools.coldfire import regions

        self.name = name
        self.analysis = regions.analyse(name)
        self.reset = self.analysis["reset"]
        self.main = main
        self.base = regions.MAIN_BASE

    def bytes_at(self, addr: int, n: int) -> bytes:
        return self.main[addr - self.base : addr - self.base + n]

    def funcs_by_hash(self, h: str) -> list[int]:
        """Entries of the functions whose cfdb h_strict is H, in address order."""
        from tools.coldfire import cf

        image = f"{self.name}-emac"
        db = cf.load(image).db
        return [
            r[0]
            for r in db.execute(
                "SELECT entry FROM func_hash WHERE image=? AND h_strict=? ORDER BY entry",
                (image, h),
            )
        ]

    def cave(self) -> dict:
        return next(
            c
            for c in self.analysis["flash_caves"]
            if c["keep"] and c["ends_at_clear_start"]
        )


def resolve_cf_words(cf: CfFacts, rule: dict) -> dict:
    """A table row found by a masked signature: `words` are big-endian u32 values (or "*" for any), matched
    at 4-byte alignment over the whole MAIN OS image; exactly one match is required."""
    import struct

    words = rule["words"]
    n = len(words)
    want = [None if w == "*" else int(w) for w in words]
    first = next(i for i, w in enumerate(want) if w is not None)
    anchor_bytes = struct.pack(">I", want[first])
    hits = []
    i = cf.main.find(anchor_bytes)
    while i >= 0:
        row = i - 4 * first
        if row >= 0 and row % 4 == 0 and row + 4 * n <= len(cf.main):
            got = struct.unpack(f">{n}I", cf.main[row : row + 4 * n])
            if all(w is None or w == g for w, g in zip(want, got, strict=True)):
                hits.append(row)
        i = cf.main.find(anchor_bytes, i + 1)
    if len(hits) != 1:
        raise ResolveError(f"{rule['name']}: {len(hits)} rows match the signature")
    row = hits[0]
    return {
        "kind": "data",
        "addr": cf.base + row,
        "len": 4 * n,
        "sha256": sha(cf.main[row : row + 4 * n]),
    }


PARAM_ROW = 60
# `move.l 4(sp),D0; cmpi.l #count,D0; scs D1b; lea table,A0; mvs.b D1b,D1; and.l D1,D0; move.l D0,D1; lsl.l #2,D1;
# lsl.l #6,D0; sub.l D1,D0; move.l (4,A0,D0*1),D0 ...`: the reader of the mirror index (finding 02)
_PARAM_READER = re.compile(
    rb"\x20\x2f\x00\x04\x0c\x80\x00\x00(..)\x55\xc1\x41\xf9(....)\x73\x01\xc0\x81\x22\x00\xe5\x89"
    rb"\xed\x88\x90\x81\x20\x30\x08\x04\x4e\x75",
    re.S,
)


def param_table(cf: CfFacts, rule: dict) -> tuple[int, int]:
    """(table address, row count) from the reader's signature."""
    hits = list(_PARAM_READER.finditer(cf.main))
    if len(hits) != 1:
        raise ResolveError(f"{rule['name']}: {len(hits)} parameter table readers match")
    m = hits[0]
    return int.from_bytes(m.group(2), "big"), int.from_bytes(m.group(1), "big")


def scan_param_sites(
    main: bytes, main_base: int, t1: int, count: int
) -> tuple[list[int], list[tuple[int, int]]]:
    """The same scan as `params::scan` in packages/patchkit: references into the table and clamp bounds."""
    import numpy as np

    end = min(t1 - main_base, len(main))
    hi = t1 + count * PARAM_ROW
    refs: list[int] = []
    bounds: list[tuple[int, int]] = []
    for start in (0, 2):  # 2-aligned words: two 4-aligned passes
        n = (end - start) // 4
        w = np.frombuffer(main, dtype=">u4", count=n, offset=start)
        for i in np.nonzero((w >= t1) & (w < hi))[0]:
            refs.append(main_base + start + 4 * int(i))
    refs.sort()
    ops = np.frombuffer(main, dtype=">u2", count=end // 2)
    for i in np.nonzero(((ops & 0xFFF8) == 0x0C80) | ((ops & 0xF1FF) == 0xB1FC))[0]:
        o = 2 * int(i)
        if o + 7 > end:
            continue
        imm = int.from_bytes(main[o + 2 : o + 6], "big")
        if imm in (count, count - 1) and main[o + 6] in (0x54, 0x55, 0x62, 0x63):
            bounds.append((main_base + o, imm))
    return refs, bounds


def params_sites_digest(refs: list[int], bounds: list[tuple[int, int]]) -> str:
    b = b"".join(r.to_bytes(4, "big") for r in refs) + b"\xff" * 4
    b += b"".join(a.to_bytes(4, "big") + i.to_bytes(4, "big") for a, i in bounds)
    return sha(b)


# Multi-site rules: one rule, many anchors `<name>.0 .. <name>.N-1` (patchkit reads them from the lockfile and a
# `write` op with `anchors = "<name>.*"` visits each; no scan runs at patch time).
MULTI_KINDS = ("cf_params_refs", "cf_params_bounds")


def resolve_cf_multi(cf: CfFacts, rule: dict, known: dict[str, dict]) -> list[dict]:
    """The sites of `params::scan`, one anchor each, in address order.

    `cf_params_refs`: every 2-aligned big-endian word that points into the first parameter table (4-byte data
    anchors; the operand of the instruction or the table word itself). `cf_params_bounds`: every compare of the
    row count that clamps an index (7-byte code anchors: opcode, immediate, next opcode byte; the immediate is
    `image.u32(this + 2)`). `exclude_anchors = [...]` drops sites inside those (already resolved) anchors, for
    code the module replaces whole.
    """
    t1, count = param_table(cf, rule)
    refs, bounds = scan_param_sites(cf.main, cf.base, t1, count)
    skip = []
    for n in rule.get("exclude_anchors", []):
        if n not in known:
            raise ResolveError(
                f"{rule['name']}: exclude_anchors names {n}, which no earlier rule resolves"
            )
        skip.append((known[n]["addr"], known[n]["addr"] + known[n]["len"]))

    def kept(addr: int) -> bool:
        return not any(lo <= addr < hi for lo, hi in skip)

    if rule["kind"] == "cf_params_refs":
        sites = [(r, 4) for r in refs if kept(r)]
        kind = "data"
    else:
        sites = [(a, 7) for a, _ in bounds if kept(a)]
        kind = "code"
    return [
        {"kind": kind, "addr": a, "len": n, "sha256": sha(cf.bytes_at(a, n))}
        for a, n in sorted(sites)
    ]


def cf_func_addr(cf: CfFacts, rule: dict) -> int:
    """Address `offset` bytes into the function whose cfdb h_strict hash is `hash` (`ordinal` among equals)."""
    hits = cf.funcs_by_hash(rule["hash"])
    ordinal = int(rule.get("ordinal", 0))
    if ordinal >= len(hits):
        raise ResolveError(
            f"{rule['name']}: {len(hits)} function(s) have hash {rule['hash'][:12]}.. (ordinal {ordinal})"
        )
    if "ordinal" not in rule and len(hits) != 1:
        raise ResolveError(
            f"{rule['name']}: {len(hits)} functions share hash {rule['hash'][:12]}.. and the rule gives no ordinal"
        )
    return hits[ordinal] + int(rule["offset"])


def resolve_cf_anchor(cf: CfFacts, rule: dict) -> dict:
    kind = rule["kind"]
    if kind == "cf_params_t1":
        t1, count = param_table(cf, rule)
        n = count * PARAM_ROW
        return {"kind": "data", "addr": t1, "len": n, "sha256": sha(cf.bytes_at(t1, n))}
    if kind == "cf_params_sites":
        t1, count = param_table(cf, rule)
        refs, bounds = scan_param_sites(cf.main, cf.base, t1, count)
        return {
            "kind": "sites",
            "addr": t1,
            "len": len(refs) + len(bounds),
            "sha256": params_sites_digest(refs, bounds),
        }
    if kind == "cf_words":
        return resolve_cf_words(cf, rule)
    if kind == "cf_ptr":
        # a table the code reaches through an absolute 32-bit literal: the anchor is the table (`len` bytes of it are
        # hashed), `src` the literal at entry + offset of the function `hash`
        src = cf_func_addr(cf, rule)
        target = int.from_bytes(cf.bytes_at(src, 4), "big")
        n = int(rule["len"])
        if not cf.base <= target < cf.base + len(cf.main) - n:
            raise ResolveError(
                f"{rule['name']}: the literal {target:#x} is not an address in the MAIN OS image"
            )
        return {
            "kind": "data",
            "addr": target,
            "len": n,
            "src": src,
            "sha256": sha(cf.bytes_at(target, n)),
        }
    if kind == "cf_func":
        addr = cf_func_addr(cf, rule)
        n = int(rule["len"])
        return {
            "kind": "code",
            "addr": addr,
            "len": n,
            "sha256": sha(cf.bytes_at(addr, n)),
        }
    hook = cf.reset["calls"]["copy"]
    if cf.reset["calls"]["clear"] != hook + 4:
        raise ResolveError("the reset calls are not adjacent")
    code = cf.bytes_at(hook, 8)
    if code[:2] != b"\x4e\xba" or code[4:6] != b"\x4e\xba":
        raise ResolveError("the reset calls are not two jsr d16(pc)")
    if kind == "cf_reset_init_calls":
        return {"kind": "code", "addr": hook, "len": 8, "sha256": sha(code)}
    if kind == "cf_reset_helper":
        which = rule["which"]
        return {
            "kind": "value",
            "addr": cf.reset[f"{which}_helper"],
            "src": hook,
            "len": 8,
            "sha256": sha(code),
        }
    raise ResolveError(f"{rule['name']}: unknown rule kind {kind!r}")


def resolve_anchor(dsp: Dsp, rule: dict) -> dict:
    kind = rule["kind"]
    if kind in ("sharc_func", "sharc_literal"):
        entry = dsp.func_by_hash(rule["reloc_hash"])
        pc = entry + int(rule.get("offset", 0))
        addr = dsp.byte_addr(pc)
        if kind == "sharc_func":
            n = int(rule["len"])
            data = dsp.stream.read(addr, n)
            return {
                "kind": "code",
                "addr": addr,
                "pm": pc,
                "len": n,
                "sha256": sha(data),
            }
        rows = dsp.db.sql(
            "SELECT value FROM literals WHERE image=? AND sw=?", dsp.image, pc
        )
        if len(rows) != 1:
            raise ResolveError(
                f"{rule['name']}: {len(rows)} literals at pc {pc:#x} in {dsp.image}"
            )
        width = dsp.db.sql(
            "SELECT width FROM insn WHERE image=? AND sw=?", dsp.image, pc
        )[0][0]
        data = dsp.stream.read(addr, int(width))
        return {
            "kind": "value",
            "addr": int(rows[0][0]) + int(rule.get("add", 0)),
            "src": addr,
            "len": int(width),
            "sha256": sha(data),
        }
    if kind == "sharc_table":
        # a table the code reaches through a literal: the instruction at entry + offset loads its address; the anchor is
        # the table in the DSP stream (`len` bytes, at the software alias 0x28000000 + the literal)
        entry = dsp.func_by_hash(rule["reloc_hash"])
        pc = entry + int(rule.get("offset", 0))
        rows = dsp.db.sql(
            "SELECT value FROM literals WHERE image=? AND sw=?", dsp.image, pc
        )
        if len(rows) != 1:
            raise ResolveError(
                f"{rule['name']}: {len(rows)} literals at pc {pc:#x} in {dsp.image}"
            )
        addr = 0x2800_0000 + int(rows[0][0])
        n = int(rule["len"])
        return {
            "kind": "data",
            "addr": addr,
            "len": n,
            "src": dsp.byte_addr(pc),
            "sha256": sha(dsp.stream.read(addr, n)),
        }
    if kind == "sharc_mem_addr":
        entry = dsp.func_by_hash(rule["reloc_hash"])
        pc = entry + int(rule.get("offset", 0))
        rows = dsp.db.sql(
            "SELECT abs_address FROM mem_access WHERE image=? AND sw=? AND abs_address IS NOT NULL",
            dsp.image,
            pc,
        )
        if len(rows) != 1:
            raise ResolveError(
                f"{rule['name']}: {len(rows)} direct DM accesses at pc {pc:#x} in {dsp.image}"
            )
        width = dsp.db.sql(
            "SELECT width FROM insn WHERE image=? AND sw=?", dsp.image, pc
        )[0][0]
        addr = dsp.byte_addr(pc)
        data = dsp.stream.read(addr, int(width))
        return {
            "kind": "value",
            "addr": int(rows[0][0]),
            "src": addr,
            "len": int(width),
            "sha256": sha(data),
        }
    raise ResolveError(f"{rule['name']}: unknown rule kind {kind!r}")


def dsp_l2_region(stream, device: str) -> dict:
    end = max(
        (b["target_address"] + b["byte_count"])
        for b in stream.mem.blocks
        if L2_LO <= b["target_address"] < L2_HI
    )
    start = (end + 3) // 4 * 4
    return {
        "name": "dsp.l2",
        "cpu": "sharc",
        "start": start,
        "end": L2_HI,
        "excludes": DSP_L2_EXCLUDES.get(device, []),
        "mark": "D",
    }


def toml_int(v: int) -> str:
    return f"0x{v:x}" if v >= 10 else str(v)


def render(port: dict, regions: list[dict], anchors: list[dict]) -> str:
    o = [
        "# Generated by tools/patchkit/port_resolve.py: do not edit by hand. Addresses and sha256 of bytes only.",
        "",
        "[port]",
    ]
    for k in (
        "device",
        "version",
        "syx_sha256",
        "main_sha256",
        "dsp_sha256",
        "resolved_by",
    ):
        o.append(f'{k} = "{port[k]}"')
    o += ["", "[limits]"]
    for k, v in LIMITS.items():
        o.append(f"{k} = {toml_int(v)}")
    for r in regions:
        o += ["", "[[region]]", f'name = "{r["name"]}"', f'cpu = "{r["cpu"]}"']
        o += [f"start = {toml_int(r['start'])}", f"end = {toml_int(r['end'])}"]
        ex = ", ".join(f"[{toml_int(a)}, {toml_int(b)}]" for a, b in r["excludes"])
        o.append(f"excludes = [{ex}]")
        o.append(f'mark = "{r["mark"]}"')
    for a in sorted(anchors, key=lambda x: x["name"]):
        o += ["", "[[anchor]]", f'name = "{a["name"]}"', f'cpu = "{a["cpu"]}"']
        o.append(f'kind = "{a["kind"]}"')
        o.append(f"addr = {toml_int(a['addr'])}")
        if "pm" in a:
            o.append(f"pm = {toml_int(a['pm'])}")
        if "src" in a:
            o.append(f"src = {toml_int(a['src'])}")
        o.append(f"len = {a['len']}")
        o.append(f'sha256 = "{a["sha256"]}"')
        o.append(f'mark = "{a["mark"]}"')
        o.append(f'rule = "{a["rule"]}"')
    return "\n".join(o) + "\n"


def resolve(syx: Path, root: Path = ROOT) -> tuple[str, str]:
    raw = syx.read_bytes()
    digest = sha(raw)
    prof = load_profiles().get(digest)
    if prof is None:
        raise ResolveError(f"{syx}: not a firmware listed in devices/*.toml ({digest})")
    secs = sections_of(syx)
    device, version = prof["device"], prof["version"]
    dsp = Dsp(secs[7], prof["dsp"])
    cf = CfFacts(f"{device}-{version}", secs[3])
    anchors = []
    pending = []
    for rule in module_rules(root):
        if rule.get("devices") and device not in rule["devices"]:
            continue
        if rule.get("kind") in MULTI_KINDS:
            pending.append(rule)  # after the single anchors they may exclude
            continue
        a = (
            resolve_cf_anchor(cf, rule)
            if rule.get("cpu") == "cf"
            else resolve_anchor(dsp, rule)
        )
        anchors.append(
            {
                "name": rule["name"],
                "cpu": rule.get("cpu", "sharc"),
                "mark": rule.get("mark", "D"),
                "rule": f"{rule['_module']}:{rule['kind']}",
                **a,
            }
        )
    known = {a["name"]: a for a in anchors}
    for rule in pending:
        for i, a in enumerate(resolve_cf_multi(cf, rule, known)):
            anchors.append(
                {
                    "name": f"{rule['name']}.{i}",
                    "cpu": "cf",
                    "mark": rule.get("mark", "D"),
                    "rule": f"{rule['_module']}:{rule['kind']}",
                    **a,
                }
            )
    # one anchor name may be declared by several modules; they must agree
    seen: dict[str, dict] = {}
    unique = []
    for a in anchors:
        prev = seen.get(a["name"])
        if prev is None:
            seen[a["name"]] = a
            unique.append(a)
        elif {k: v for k, v in prev.items() if k != "rule"} != {
            k: v for k, v in a.items() if k != "rule"
        }:
            raise ResolveError(
                f"anchor {a['name']} resolves differently for two modules"
            )
    # Built-in anchor: the whole MAIN OS image, for modules that write at fixed offsets of one pinned firmware.
    main_dest = 0x40000400
    anchors.append(
        {
            "name": "cf.main",
            "cpu": "cf",
            "kind": "code",
            "addr": main_dest,
            "len": len(secs[3]),
            "sha256": sha(secs[3]),
            "mark": "V",
            "rule": "builtin",
        }
    )
    unique.append(anchors[-1])
    cave = cf.cave()
    exp = cf.analysis["expansion"]
    regions = [
        {
            "name": "cf.flash_cave",
            "cpu": "cf",
            "start": cave["start"],
            "end": cave["start"] + cave["size"],
            "excludes": [],
            "mark": "D",
        },
        {
            "name": "cf.expansion",
            "cpu": "cf",
            "start": exp["start"],
            "end": exp["end"],
            "excludes": [],
            "mark": "D",
        },
        dsp_l2_region(dsp.stream, device),
    ]
    port = {
        "device": device,
        "version": version,
        "syx_sha256": digest,
        "main_sha256": sha(secs[3]),
        "dsp_sha256": sha(secs[7]),
        "resolved_by": RESOLVED_BY,
    }
    return f"{device}-{version}", render(port, regions, unique)


def cmd_rule_sharc(a: argparse.Namespace) -> int:
    from tools.patchkit import dsp_patch
    from tools.sharc_dev import sharc

    img = sharc.load(a.image)
    fn = img.func(a.pc)
    if fn is None:
        raise SystemExit(f"no function contains pc {a.pc:#x} in {a.image}")
    entry = int(fn["entry_sw"], 16)
    end = int(fn["end_sw"], 16)
    h = img.sql(
        "SELECT reloc_hash FROM func_hash WHERE image=? AND entry_sw=?", a.image, entry
    )[0][0]
    n = img.sql(
        "SELECT count(*) FROM func_hash WHERE image=? AND reloc_hash=?", a.image, h
    )[0][0]
    print(
        f"# {a.image}: function {entry:#x}..{end:#x}; {n} function(s) share this hash"
    )
    print(f'reloc_hash = "{h}"')
    print(f"offset = {a.pc - entry:#x}")
    if a.len:
        print(f"len = {a.len}")
    del dsp_patch
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("resolve")
    r.add_argument("syx", type=Path)
    r.add_argument("--out", type=Path)
    r.add_argument("--root", type=Path, default=ROOT)
    ra = sub.add_parser("resolve-all")
    ra.add_argument("--root", type=Path, default=ROOT)
    rs = sub.add_parser("rule-sharc")
    rs.add_argument("image")
    rs.add_argument("pc", type=lambda s: int(s, 0))
    rs.add_argument("--len", type=int)
    argv = sys.argv[1:]
    # `mise run port:resolve [-- X.syx]`: a firmware resolves that one, nothing resolves all four
    if not argv:
        argv = ["resolve-all"]
    elif argv[0] not in ("resolve", "resolve-all", "rule-sharc") and not argv[
        0
    ].startswith("-"):
        argv = ["resolve", *argv]
    a = ap.parse_args(argv)
    try:
        if a.cmd == "rule-sharc":
            return cmd_rule_sharc(a)
        files = (
            [
                Path(f) if Path(f).exists() else config.stock(f)
                for f in FIRMWARE.values()
            ]
            if a.cmd == "resolve-all"
            else [a.syx]
        )
        for f in files:
            name, text = resolve(f, a.root)
            out = (
                a.out
                if getattr(a, "out", None)
                else a.root / "devices" / "ports" / f"{name}.toml"
            )
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(text)
            print(f"{out}: {text.count('[[anchor]]')} anchors")
    except ResolveError as e:
        print(f"port_resolve: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
