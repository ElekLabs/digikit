#!/usr/bin/env python3
"""Byte-level report of a patched image against its stock image and port, read without patchkit's own decoder.

    uv run python -m tools.patchkit.image_report STOCK.syx PATCHED.syx PLAN.json [--out REPORT.md]

A static check for a dry run (nothing is run): the second reader of what `patchkit apply` wrote. It decodes both .syx
files with `tools/firmware/container.py`, then checks and prints

  * the per-packet and content checksums, and the trailer (a sealed device's last 32 bytes are not checked here);
  * every section: stored and decoded sha256 in both images, and which sections differ;
  * the sections that `[container] readonly` of the device names are byte for byte the stock ones;
  * the MAIN OS: the byte ranges that differ, and that each one is a plan write or the appended segment chain;
  * every patch site: the stock bytes equal the plan's old hash and the hash of each port anchor that covers the site;
  * the regions of the port: placements inside them, and the instruction operands that name them (listed, not failed);
  * the features a module switched off for want of an optional anchor (`disabled: <owner>:<anchor>`, listed).

Exit 1 when a check fails.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
import tomllib
from pathlib import Path

from tools.firmware import container as C
from tools.patchkit import port_resolve as R

ROOT = R.ROOT


def sha(b: bytes) -> str:
    return R.sha(b)


def packet_checks(path: Path) -> tuple[int, list[str]]:
    """(data messages, failures): counters and packet checksums of every data message, and the framing counts."""
    d = path.read_bytes()
    msgs = []
    i = 0
    while i < len(d):
        j = d.index(0xF7, i)
        msgs.append(d[i : j + 1])
        i = j + 1
    k = msgs[0][1:-1][7]
    data = msgs[1:-1]
    bad = []
    count = lambda m: m[12] * 16384 + m[13] * 128 + m[14]  # noqa: E731
    if count(msgs[0]) != len(data) or count(msgs[-1]) != len(data):
        bad.append("framing count differs from the number of data messages")
    for n, m in enumerate(data):
        body = m[1:-1]
        total = k + sum((body[6 + x] ^ ((x + k) & 0xFF)) for x in range(119))
        if body[125] != total & 0x7F:
            bad.append(f"message {n}: packet checksum")
        c = 242 + n
        if (body[6], body[7], body[8]) != ((c >> 14) & 0x7F, (c >> 7) & 0x7F, c & 0x7F):
            bad.append(f"message {n}: counter")
    return len(data), bad[:5]


def content_sum(c: bytes) -> int:
    acc = 0
    for i in range(len(c) // 4):
        acc = (acc + ((i + 1) ^ struct.unpack_from(">I", c, 4 * i)[0])) & 0xFFFFFFFF
    return acc


def stream_of(path: Path) -> tuple[bytes, int, int]:
    """(container, length word, checksum word) of the decoded stream."""
    dec = C.decode_syx(str(path))
    total, csum = struct.unpack_from(">II", dec, 0)
    return dec[8 : 8 + total], total, csum


def stored_sections(path: Path) -> dict[int, tuple[bytes, int]]:
    c, table = C.sections(str(path))
    return {sid: (bytes(c[off : off + ln]), dest) for sid, off, ln, dest in table}


def diff_ranges(a: bytes, b: bytes) -> list[tuple[int, int]]:
    """Ranges [lo, hi) where the common prefix of A and B differs, then the tail of the longer one."""
    n = min(len(a), len(b))
    out: list[tuple[int, int]] = []
    i = 0
    while i < n:
        if a[i] != b[i]:
            j = i
            while j < n and a[j] != b[j]:
                j += 1
            out.append((i, j))
            i = j
        else:
            i += 1
    if len(a) != len(b):
        out.append((n, max(len(a), len(b))))
    return out


def report(stock: Path, patched: Path, plan_path: Path) -> tuple[list[str], bool]:
    out: list[str] = []
    ok = True

    def line(passed: bool, text: str) -> None:
        nonlocal ok
        ok &= passed
        out.append(("ok    " if passed else "FAIL  ") + text)

    plan = json.loads(plan_path.read_text())
    base = plan["base"]
    name = f"{base['device']}-{base['version']}"
    prof = next(
        p for p in R.load_profiles().values() if f"{p['device']}-{p['version']}" == name
    )
    port = tomllib.loads((ROOT / "devices" / "ports" / f"{name}.toml").read_text())
    cont = prof["container"]
    readonly = set(cont.get("readonly", []))
    # read-only sections that a module may store packed (`[container] repack`, roles): the decoded bytes must still be the stock bytes
    repack = {cont[r] for r in cont.get("repack", []) if r in cont}
    out.append(f"image {name}: stock {sha(stock.read_bytes())}")
    out.append(f"       patched {sha(patched.read_bytes())}")

    # features a module switched off because the port lacks an optional anchor (`optional_anchors` of the manifest)
    for d in plan.get("disabled", []):
        out.append(f"disabled: {d}")

    n, bad = packet_checks(patched)
    line(
        not bad,
        f"{n} data messages: counters, packet checksums, framing count {bad or ''}",
    )
    c, total, csum = stream_of(patched)
    line(
        content_sum(c) == csum and total == len(c),
        f"content checksum {csum:#010x} over {total} bytes",
    )
    cs, totals, csums = stream_of(stock)
    line(content_sum(cs) == csums, f"stock content checksum {csums:#010x} reproduces")

    a, b = stored_sections(stock), stored_sections(patched)
    line(sorted(a) == sorted(b), f"section ids {sorted(b)} equal the stock ones")
    dec_a, dec_b = R.sections_of(stock), R.sections_of(patched)
    out.append("section  role         stored sha256 (patched)  decoded bytes  verdict")
    for sid in sorted(b):
        role = next((k for k, v in cont.items() if v == sid), "-")
        same = a[sid] == b[sid] and dec_a[sid] == dec_b[sid]
        packed = sid in repack and not same and dec_a[sid] == dec_b[sid]
        verdict = (
            "identical to stock"
            if same
            else "stored packed, decodes to stock"
            if packed
            else "DIFFERS"
        )
        allowed = sid not in readonly
        line(
            same or packed or allowed,
            f"{sid}        {role:<12} {sha(b[sid][0])[:16]}  {len(dec_b[sid]):>10}  {verdict}"
            + ("" if same or packed or allowed else " (read-only section)"),
        )
    changed = [s for s in b if a[s] != b[s]]
    main = cont["main"]
    # a repacked section differs in storage only: its decoded bytes are compared above
    repacked = [s for s in changed if s in repack and dec_a[s] == dec_b[s]]
    changed = [s for s in changed if s not in repacked]
    line(changed == [main], f"only section {main} (MAIN OS) differs: {changed}")
    line(
        not (readonly & set(changed)),
        f"read-only sections {sorted(readonly)} are byte-identical"
        + (
            f" (sections {sorted(repacked)} stored packed, decoded bytes identical)"
            if repacked
            else ""
        ),
    )

    m0, m1 = dec_a[main], dec_b[main]
    base_addr = b[main][1]
    writes = plan["writes"]
    covered = [
        (w["addr"] - base_addr, w["addr"] - base_addr + w["len"])
        for w in writes
        if w["cpu"] == "cf" and w["kind"] in ("patch", "place")
    ]
    ranges = diff_ranges(m0, m1)
    out.append(
        f"MAIN OS {len(m0)} -> {len(m1)} bytes; {len(ranges)} differing range(s):"
    )
    for lo, hi in ranges:
        inside = any(clo <= lo and hi <= chi for clo, chi in covered)
        tail = lo >= len(m0)
        line(
            inside or tail,
            f"  {base_addr + lo:#010x}..{base_addr + hi:#010x} ({hi - lo} bytes): "
            + (
                "a plan write"
                if inside
                else "appended segment chain"
                if tail
                else "NOT in the plan"
            ),
        )

    for w in writes:
        if w["cpu"] != "cf" or w["kind"] != "patch":
            continue
        o = w["addr"] - base_addr
        was = m0[o : o + w["len"]]
        line(
            sha(was) == w["old_sha256"],
            f"site {w['addr']:#x}+{w['len']} ({w['what']}): stock bytes equal the plan's old hash",
        )
        for an in port["anchor"]:
            if (
                an["cpu"] == "cf"
                and an["kind"] == "code"
                and an["addr"] <= w["addr"]
                and w["addr"] + w["len"] <= an["addr"] + an["len"]
            ):
                ao = an["addr"] - base_addr
                line(
                    sha(m0[ao : ao + an["len"]]) == an["sha256"],
                    f"  anchor {an['name']} {an['addr']:#x}+{an['len']}: stock bytes equal the port hash",
                )

    regions = {r["name"]: (r["start"], r["end"]) for r in port["region"]}
    for w in writes:
        if w["kind"] == "patch" or w["cpu"] != "cf":
            continue
        reg = next(
            (
                n
                for n, (lo, hi) in regions.items()
                if lo <= w["addr"] and w["addr"] + w["len"] <= hi
            ),
            None,
        )
        line(
            reg is not None,
            f"placement {w['addr']:#x}+{w['len']} ({w['what']}) lies in {reg}",
        )
    cave = regions.get("cf.flash_cave")
    if cave:
        o = cave[0] - base_addr
        line(
            not any(m0[o : cave[1] - base_addr]),
            f"cf.flash_cave {cave[0]:#x}..{cave[1]:#x} is zero in the stock MAIN OS",
        )
    facts = R.CfFacts(name, m0)
    r = facts.reset
    line(
        len(m0) + base_addr <= regions["cf.expansion"][0]
        and r["clear_end"] <= regions["cf.expansion"][0],
        f"cf.expansion starts {regions['cf.expansion'][0]:#x}, above the image end {len(m0) + base_addr:#x} and the bss end {r['clear_end']:#x}",
    )
    if cave:
        line(
            cave[1] == r["clear_start"],
            f"the cave ends at the reset clear start {r['clear_start']:#x}",
        )
    # instruction operands that name a region (the scan decodes linearly: a hit may be data read as code, and a constant, such as an
    # EMAC coefficient, may look like an address), in both views of RAM. They are listed to be classified, not failed.
    from tools.coldfire import refscan
    from tools.coldfire import regions as cf_regions

    for rname, (lo, hi) in regions.items():
        found = set()
        for a, b in ((lo, hi), (lo + cf_regions.ALIAS, hi + cf_regions.ALIAS)):
            hits, *_ = refscan.scan(m0, base_addr, base_addr, base_addr + len(m0), a, b)
            found.update((h[0], h[2], h[3], h[-1]) for h in hits)
        out.append(
            f"info  {len(found)} instruction operand(s) of the stock MAIN OS name {rname} {lo:#x}..{hi:#x}"
            + "".join(
                f"\n        {pc:#x}  {op}  {arg}  -> {t:#x}"
                for pc, op, arg, t in sorted(found)
            )
        )
    return out, ok


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("stock", type=Path)
    ap.add_argument("patched", type=Path)
    ap.add_argument("plan", type=Path)
    ap.add_argument("--out", type=Path)
    a = ap.parse_args()
    lines, ok = report(a.stock, a.patched, a.plan)
    text = (
        "\n".join(lines) + "\n" + ("ALL CHECKS PASSED\n" if ok else "CHECKS FAILED\n")
    )
    if a.out:
        a.out.write_text(text)
    sys.stdout.write(text)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
