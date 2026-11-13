#!/usr/bin/env python3
"""RAM and flash-cave regions of a MAIN OS image: the facts, the candidates, the canary.

Replaces the lost unicorn-reference-final:tools/cavefind.py. Per image (out/sections/IMAGE/section_3_MAIN_OS.bin):

    uv run python -m tools.coldfire.regions report [IMAGE ...] [--json]
    uv run python -m tools.coldfire.regions confirm IMAGE [--region flash_cave|expansion|START:LEN]
                                         [--scenario boot,nav,play] [--extra N] [--dsp READY_DIR]

`report` reads the reset path by signature and prints what the firmware itself
says about its RAM:

  * the stack pointer set at the reset entry (top of RAM),
  * the two .data copy loops (image tail -> SRAM at 0x80000000) and the bss
    clear loop (start/end operands), and the two calls that run them,
  * the cache setup (CACR, ACR0),
  * the C++ heap, a buddy allocator whose descriptor sits in .data,
  * the non-cached DMA arena above the bss end, found from absolute
    references into the +0x08000000 alias of RAM (ACR0 covers only
    0x40000000-0x47ffffff, so the alias is an uncached view of the same DDR),
  * flash caves: zero runs below the clear start that nothing points into (a
    zero run in the image can be live bss or an allocator table, so a run
    that a pointer literal reaches is rejected),
  * the expansion candidate: the top-of-RAM gap between the DMA arena (plus a
    margin) and the stack reserve. Nothing in the linker map owns it.

`confirm` runs the ColdFire emulator (packages/emulator/boot example regions_probe, built
on first use) on the image's .syx with the region filled by an
address-dependent pattern right after the reset clear returns, in both the
cached view and the +0x08000000 alias (the emulator keeps them as separate
memory). Scenarios press panel buttons after ready. It reports words that no
longer hold the pattern, and compares the final state digest against a
baseline run that filled zeros: equal digests mean the firmware never read the
region either. A pass is evidence for the scenarios run, not a proof; the
static report and the canary together are the claim, as in docs/PATCHING.md.

`--dsp READY_DIR` runs the probe coupled: the DSP of that ready directory
(`tools/emulator/ready.py`: digi-audio-dn2-image.bin and cold-idle.dsp) is attached to
the ColdFire's DSPI2 link from reset, so the sequencer, the DSP frames and the
sample loader run as on a device. The probe is then built with the `sharc`
feature against the generated core of that image (a local, firmware-derived
build; SHARC_GEN_DIR overrides it).

    uv run python -m tools.coldfire.regions loader-test IMAGE [--kib 256] [--pack store|aplib]

`loader-test` proves the loader hook end to end, without building patchkit: it
appends a test segment (random bytes, with a length and destination header) to
MAIN OS, writes a 40-byte loader into the flash cave, replaces the two reset
calls (copy helper, clear helper) with one call to the loader, rebuilds the
.syx, cold-boots it in the emulator and compares the expansion region with the
segment at ready. The loader runs before the bss clear, as it must: the
appended bytes sit past the image end, inside the cleared range.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import struct
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools import python_env  # noqa: E402
from tools.firmware import config, device  # noqa: E402

MAIN_BASE = 0x40000400
RESET_ENTRY = 0x400004E8
RAM_BASE = 0x40000000
ALIAS = 0x08000000  # uncached view of the same DDR: addr + ALIAS
PAGE = 0x1000
DMA_MARGIN = 0x10000  # beyond the highest absolute reference into the DMA arena
STACK_RESERVE = 0x20000  # below the reset stack top; the canary saw one 4 KiB page used

# image name -> (sections dir name, firmware file, base image whose profile boots it): the firmwares of devices/*.toml that
# name a MAIN OS (`device.boot_images`)
IMAGES = {
    name: (name, b.firmware.filename, b.base)
    for name, b in device.boot_images(config.DEVICES_DIR).items()
}
# the cold audio-ready boot snapshot of each device's base image
SNAPSHOTS = {
    name: "out/snapshots/%s-audio-ready-cold-2026-10-07/digi-audio-m5.snap"
    % b.device.short
    for name, b in device.boot_images(config.DEVICES_DIR).items()
    if b.base is None
}

# The reset helpers, with their address operands masked ("." = any byte).
CLEAR_SIG = bytes.fromhex(
    "4feffff048d700f0207c40312000223c47e284709288e881428442854286428748d000f0"
    "41e80010538166f44cd700f04fef00104e75"
)
CLEAR_MASK_OPERANDS = ((10, 14), (16, 20))
COPY_FIRST = re.compile(
    rb"\x2f\x0a\x45\xf9(.{4})\x43\xf9\x80\x00\x00\x00.*?\xb5\xfc(.{4})", re.S
)
COPY_SECOND = re.compile(
    rb"\x45\xf9(.{4})\x43\xf9\x80\x00\x80\x00.*?\xb5\xfc(.{4})", re.S
)


def _sections_dir(image: str) -> pathlib.Path:
    return ROOT / "out" / "sections" / IMAGES[image][0]


def load_main(image: str) -> bytes:
    return (_sections_dir(image) / "section_3_MAIN_OS.bin").read_bytes()


def _u32(buf: bytes, addr: int) -> int:
    return struct.unpack_from(">I", buf, addr - MAIN_BASE)[0]


def _find_masked(buf: bytes, sig: bytes, masks) -> list[int]:
    pat = b""
    pos = 0
    for lo, hi in masks:
        pat += re.escape(sig[pos:lo]) + b".{%d}" % (hi - lo)
        pos = hi
    pat += re.escape(sig[pos:])
    return [m.start() + MAIN_BASE for m in re.finditer(pat, buf, re.S)]


def reset_facts(main: bytes) -> dict:
    """What the reset path of this MAIN OS image does, read from its bytes."""
    clears = _find_masked(main, CLEAR_SIG, CLEAR_MASK_OPERANDS)
    if len(clears) != 1:
        raise ValueError("reset clear loop: %d matches" % len(clears))
    clear = clears[0]
    clear_start = _u32(main, clear + 10)
    clear_end = _u32(main, clear + 16)
    # Copy helper: the function that precedes the clear helper.
    window = main[clear - MAIN_BASE - 0x60 : clear - MAIN_BASE]
    m1 = COPY_FIRST.search(window)
    m2 = COPY_SECOND.search(window)
    if not (m1 and m2):
        raise ValueError("reset copy helper not found")
    copy = clear - 0x60 + m1.start()
    copies = [tuple(struct.unpack(">I", g)[0] for g in m.groups()) for m in (m1, m2)]
    entry_off = RESET_ENTRY - MAIN_BASE
    if main[entry_off + 10 : entry_off + 12] != b"\x2e\x7c":
        raise ValueError("reset entry does not set the stack pointer")
    sp0 = struct.unpack_from(">I", main, entry_off + 12)[0]
    # The two calls (jsr d16(pc) = 4eba disp16) that run copy and clear.
    calls = {}
    for name, target in (("copy", copy), ("clear", clear)):
        for off in range(entry_off, entry_off + 0x100, 2):
            if main[off : off + 2] == b"\x4e\xba":
                disp = struct.unpack_from(">h", main, off + 2)[0]
                if MAIN_BASE + off + 2 + disp == target:
                    calls[name] = MAIN_BASE + off
    # CACR and ACR0: move.l #imm,d0; movec d0,cacr (4e7b 0002) / itt0 (4e7b 0004).
    cache = {}
    for reg, code in (("cacr", b"\x4e\x7b\x00\x02"), ("acr0", b"\x4e\x7b\x00\x04")):
        i = main.find(code, entry_off, entry_off + 0x200)
        if i >= 0 and main[i - 6 : i - 4] == b"\x20\x3c":
            cache[reg] = struct.unpack_from(">I", main, i - 4)[0]
    return {
        "entry": RESET_ENTRY,
        "sp0": sp0,
        "image_end": MAIN_BASE + len(main),
        "copy_helper": copy,
        "copies": [{"src": s, "end": e} for s, e in copies],
        "clear_helper": clear,
        "clear_start": clear_start,
        "clear_end": clear_end,
        "calls": calls,
        "cache": cache,
    }


def heap_descriptor(main: bytes) -> dict | None:
    """The buddy allocator descriptor: [arena][bitmap][arena + 32 MiB][21][2^21].

    The descriptor lives in .data (initialised in the image); operator new and
    delete are the two functions with the most callers.
    """
    for off in range(0, len(main) - 20, 4):
        a, b, c, order, blocks = struct.unpack_from(">5I", main, off)
        if (
            order == 0x15
            and blocks == 0x200000
            and c == a + 0x2000000
            and 0x40000000 < a < 0x48000000
            and 0x40000000 < b < 0x48000000
        ):
            return {
                "descriptor": MAIN_BASE + off - 4 * 1,
                "arena": a,
                "arena_end": c,
                "bitmap": b,
                "bitmap_bytes": blocks // 8,
            }
    return None


def alias_refs(main: bytes, lo: int, hi: int) -> list[int]:
    """Absolute addresses in [lo, hi) (either view) that code references."""
    from tools.coldfire import refscan

    out: set[int] = set()
    for a, b in ((lo, hi), (lo + ALIAS, hi + ALIAS)):
        hits, *_ = refscan.scan(main, MAIN_BASE, MAIN_BASE, MAIN_BASE + len(main), a, b)
        out.update(t - ALIAS if t >= RAM_BASE + ALIAS else t for *_, t in hits)
    return sorted(out)


def _zero_runs(main: bytes, end_addr: int, min_len: int):
    limit = end_addr - MAIN_BASE
    for fill in (0x00, 0xFF):
        for m in re.finditer(bytes([fill]) + b"{%d,}" % min_len, main[:limit]):
            yield MAIN_BASE + m.start(), m.end() - m.start(), fill


def _pointer_words(main: bytes) -> list[int]:
    out = []
    for off in range(0, len(main) - 3, 2):
        v = struct.unpack_from(">I", main, off)[0]
        if MAIN_BASE - 0x1000 <= v < MAIN_BASE + len(main) + 0x1000:
            out.append(v)
    return sorted(out)


def flash_caves(
    main: bytes, facts: dict, snapshot: str | None = None, min_len: int = 256
) -> list[dict]:
    """Runs of 0x00/0xFF below the clear start that survive the reset path.

    A run is rejected when any 32-bit literal in the image points into it, or
    to a table that ends just before it (within 0x100 bytes below), and, when a
    ready-state snapshot is given, when the snapshot's bytes differ from the
    image's. Only the 0x00 run that ends exactly at the clear start is a
    candidate that cannot be a live buffer by position (the linker puts bss
    and tables before it); the others are listed with the reason they were
    kept or rejected.
    """
    ptrs = _pointer_words(main)
    import bisect

    snap = None
    if snapshot and pathlib.Path(snapshot).exists():
        snap = _snapshot_pages(snapshot)
    out = []
    for start, length, fill in _zero_runs(main, facts["clear_start"], min_len):
        end = start + length
        i = bisect.bisect_left(ptrs, start - 0x100)
        pointed = i < len(ptrs) and ptrs[i] < end
        differs = False
        if snap is not None:
            differs = any(
                _snap_read(snap, a, 4) != main[a - MAIN_BASE : a - MAIN_BASE + 4]
                for a in range(start, end, 4)
            )
        out.append(
            {
                "start": start,
                "size": length,
                "fill": fill,
                "ends_at_clear_start": end == facts["clear_start"],
                "pointed_into": pointed,
                "changed_in_snapshot": differs if snap is not None else None,
                "keep": not pointed and not differs,
            }
        )
    # Equal runs at a constant stride are the padding of a record table.
    by_size: dict[int, list[dict]] = {}
    for c in out:
        by_size.setdefault(c["size"], []).append(c)
    for group in by_size.values():
        group.sort(key=lambda c: c["start"])
        if (
            len(group) >= 3
            and len(
                {
                    b["start"] - a["start"]
                    for a, b in zip(group, group[1:], strict=False)
                }
            )
            == 1
        ):
            for c in group:
                c["periodic"] = True
                c["keep"] = False
    out.sort(key=lambda c: (-int(c["keep"]), -c["size"]))
    return out


def _snapshot_pages(path: str) -> dict[int, bytes]:
    from tools.emulator import snapread

    with open(path, "rb") as fh:
        d = fh.read()
    i = d.find(b"BORD")
    (n,) = struct.unpack_from("<Q", d, i + 4)
    p = i + 12
    chunks: dict[int, bytes] = {}
    for _ in range(n):
        (idx,) = struct.unpack_from("<I", d, p)
        p += 4
        for b in range(256):
            f = d[p]
            p += 1
            if f:
                chunks[(idx << 20) + b * 4096] = d[p : p + 4096]
                p += 4096
    del snapread
    return chunks


def _snap_read(chunks: dict[int, bytes], addr: int, n: int) -> bytes:
    c = chunks.get(addr & ~0xFFF)
    if c is None:
        return bytes(n)
    return c[addr & 0xFFF : (addr & 0xFFF) + n]


def _first_cluster(refs: list[int], gap: int = 0x10000) -> list[int]:
    """The leading run of references whose neighbours are at most `gap` apart.

    The rest are immediates that only look like addresses (the softfloat
    constant 0x47efffff, odd bytes of tables): they are reported, not trusted.
    """
    out: list[int] = []
    for r in refs:
        if out and r - out[-1] > gap:
            break
        out.append(r)
    return out


def analyse(image: str) -> dict:
    main = load_main(image)
    facts = reset_facts(main)
    heap = heap_descriptor(main)
    bss_end = facts["clear_end"]
    top_lo = (bss_end + PAGE - 1) & ~(PAGE - 1)
    refs = alias_refs(main, top_lo, facts["sp0"])
    cluster = _first_cluster(refs)
    arena_hi = cluster[-1] if cluster else bss_end
    cand_lo = (arena_hi + DMA_MARGIN + PAGE - 1) & ~(PAGE - 1)
    cand_hi = (facts["sp0"] - STACK_RESERVE) & ~(PAGE - 1)
    snap = SNAPSHOTS.get(image)
    caves = flash_caves(main, facts, str(ROOT / snap) if snap else None)
    return {
        "image": image,
        "main_bytes": len(main),
        "reset": facts,
        "heap": heap,
        "dma_arena": {
            "refs": len(cluster),
            "first": cluster[0] if cluster else None,
            "last": arena_hi,
            "outliers": [x for x in refs if x not in cluster],
        },
        "expansion": {
            "start": cand_lo,
            "end": cand_hi,
            "size": cand_hi - cand_lo,
            "alias_start": cand_lo + ALIAS,
            "note": "above the bss end, the DMA arena and its margin; below the stack reserve",
        },
        "flash_caves": caves,
    }


def _hex(v):
    return "0x%08x" % v if isinstance(v, int) else v


def print_report(r: dict) -> None:
    f = r["reset"]
    print(
        "== %s (MAIN OS %d bytes, image end %s)"
        % (r["image"], r["main_bytes"], _hex(f["image_end"]))
    )
    print("  reset stack top      %s" % _hex(f["sp0"]))
    for c in f["copies"]:
        print(
            "  .data copy           %s..%s -> SRAM" % (_hex(c["src"]), _hex(c["end"]))
        )
    print(
        "  bss clear            %s..%s (%.2f MiB), helper %s, called at %s; copy called at %s"
        % (
            _hex(f["clear_start"]),
            _hex(f["clear_end"]),
            (f["clear_end"] - f["clear_start"]) / 1048576,
            _hex(f["clear_helper"]),
            _hex(f["calls"].get("clear", 0)),
            _hex(f["calls"].get("copy", 0)),
        )
    )
    print(
        "  cache                "
        + ", ".join("%s=%s" % (k, _hex(v)) for k, v in f["cache"].items())
    )
    h = r["heap"]
    if h:
        print(
            "  C++ heap (buddy)     arena %s..%s (32 MiB), bitmap %s (+%d bytes), descriptor %s"
            % (
                _hex(h["arena"]),
                _hex(h["arena_end"]),
                _hex(h["bitmap"]),
                h["bitmap_bytes"],
                _hex(h["descriptor"]),
            )
        )
    d = r["dma_arena"]
    print(
        "  DMA arena (alias)    %d refs, %s..%s (+%d outlier immediates)"
        % (
            d["refs"],
            _hex(d["first"]) if d["first"] else "-",
            _hex(d["last"]),
            len(d["outliers"]),
        )
    )
    e = r["expansion"]
    print(
        "  expansion candidate  %s..%s = %d bytes (%.2f MiB); alias %s"
        % (
            _hex(e["start"]),
            _hex(e["end"]),
            e["size"],
            e["size"] / 1048576,
            _hex(e["alias_start"]),
        )
    )
    for c in r["flash_caves"][:6]:
        print(
            "  cave %s %6d B fill %02x  %s%s%s"
            % (
                _hex(c["start"]),
                c["size"],
                c["fill"],
                "KEEP" if c["keep"] else "reject",
                " (ends at clear start)" if c["ends_at_clear_start"] else "",
                " pointed-into"
                if c["pointed_into"]
                else (" periodic" if c.get("periodic") else ""),
            )
        )


# --- confirm ---------------------------------------------------------------


def _tap(events, t, code, hold=2_000_000):
    events.append((t, "event", "%d:%d:down" % (t, code)))
    events.append((t + hold, "event", "%d:%d:up" % (t + hold, code)))


def scenario_args(name: str) -> list[str]:
    """Probe arguments for a panel scenario (instruction counts after ready).

    nav: the NO prelude, then every page, mode and arrow button, encoder turns
    and pushes. play: PLAY, every trig, RECORD, a second PLAY/STOP cycle.
    Codes follow devices/*.toml (pages 1-6, modes 7-9, 10 and 12 confirm,
    arrows 11/13/14/15, transport 19/20/21, trigs 25-40, encoder pushes 41-48).
    """
    ev: list[tuple[int, str, str]] = []
    t = 1_000_000
    if name == "boot":
        return ["--extra", "300000000"]
    _tap(ev, t, 12)
    t += 30_000_000
    if name == "nav":
        for code in list(range(1, 10)) + [
            11,
            13,
            14,
            15,
            10,
            12,
            16,
            17,
            18,
            22,
            23,
            24,
        ]:
            _tap(ev, t, code)
            t += 25_000_000
        for enc in range(1, 10):
            ev.append((t, "turn", "%d:%d:7" % (t, enc)))
            ev.append((t + 5_000_000, "turn", "%d:%d:-7" % (t + 5_000_000, enc)))
            t += 12_000_000
        for code in range(41, 49):
            _tap(ev, t, code)
            t += 20_000_000
    elif name == "play":
        _tap(ev, t, 20)
        t += 60_000_000
        for code in range(25, 41):
            _tap(ev, t, code, hold=40_000_000)
            t += 50_000_000
        _tap(ev, t, 19)
        t += 40_000_000
        for code in (25, 29, 33, 37):
            _tap(ev, t, code, hold=10_000_000)
            t += 30_000_000
        _tap(ev, t, 19)
        t += 80_000_000
        _tap(ev, t, 21)
        t += 40_000_000
        _tap(ev, t, 20)
        t += 200_000_000
        _tap(ev, t, 21)
        t += 20_000_000
    else:
        from tools.coldfire import panel_script

        if name in panel_script.SCRIPTS:
            return panel_script.SCRIPTS[name]().args()
        raise ValueError("unknown scenario %r" % name)
    args = ["--extra", str(t + 50_000_000)]
    for _, kind, spec in ev:
        args += ["--turn" if kind == "turn" else "--event", spec]
    return args


def probe_binary(dsp_dir: pathlib.Path | None = None) -> pathlib.Path:
    from tools.emulator import digi_env, native_target

    env = {k: v for k, v in os.environ.items() if k != "SHARC_GEN_DIR"}
    features: list[str] = []
    if dsp_dir is None:
        target = pathlib.Path(native_target.target_dir(None))
    else:
        gen = digi_env.generated_dir(image=dsp_dir / "digi-audio-dn2-image.bin")
        digi_env.check_generated(gen)
        env["SHARC_GEN_DIR"] = str(gen)
        env = digi_env.rust_env(env)
        target = pathlib.Path(native_target.target_dir(gen))
        features = ["--features", "sharc"]
    binary = target / "release" / "examples" / "regions_probe"
    subprocess.run(
        [
            sys.executable,
            "-m",
            "tools.devtools.benchlock",
            "run",
            "--shared",
            "--",
            "cargo",
            "build",
            "--release",
            "--locked",
            "--target-dir",
            str(target),
            "-p",
            "elektron-native-boot",
            "--example",
            "regions_probe",
            *features,
        ],
        cwd=ROOT,
        env=python_env(env),
        check=True,
    )
    return binary


def run_probe(
    binary,
    image: str,
    fills: list[tuple[int, int]],
    scenario: str,
    pattern: str,
    extra: list[str],
    dsp_dir: pathlib.Path | None = None,
):
    _, syx_name, base = IMAGES[image]
    syx = config.stock(syx_name)
    cmd = [str(binary), str(syx)]
    if base:
        cmd += [
            "--base-sha",
            hashlib.sha256((config.stock(IMAGES[base][1])).read_bytes()).hexdigest(),
        ]
    for start, length in fills:
        cmd += ["--fill", "0x%x:0x%x" % (start, length)]
    if dsp_dir is not None:
        cmd += [
            "--dsp", str(dsp_dir / "digi-audio-dn2-image.bin"),
            "--dsp-idle", str(dsp_dir / "cold-idle.dsp"),
        ]  # fmt: skip
    cmd += ["--pattern", pattern, "--zero-digest"] + scenario_args(scenario) + extra
    lines = subprocess.run(
        cmd, cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.splitlines()
    reports = [json.loads(x) for x in lines if x.startswith("{")]
    return reports


def confirm(
    image: str,
    region: str,
    scenarios: list[str],
    extra: list[str],
    dsp_dir: pathlib.Path | None = None,
) -> int:
    a = analyse(image)
    if region == "expansion":
        start, size = a["expansion"]["start"], a["expansion"]["size"]
    elif region == "flash_cave":
        keep = [c for c in a["flash_caves"] if c["keep"] and c["ends_at_clear_start"]]
        if not keep:
            print("no flash cave for %s" % image)
            return 1
        start, size = keep[0]["start"], keep[0]["size"]
    else:
        s, _, n = region.partition(":")
        start, size = int(s, 0), int(n, 0)
    fills = [(start, size)]
    if start >= RAM_BASE + 0x00300000 and start + size > a["reset"]["clear_end"]:
        fills.append(
            (start + ALIAS, size)
        )  # the uncached view is separate memory in the emulator
    binary = probe_binary(dsp_dir)
    failed = 0
    for name in scenarios:
        canary = run_probe(binary, image, fills, name, "canary", extra, dsp_dir)
        zero = run_probe(binary, image, fills, name, "zero", extra, dsp_dir)
        final = next(r for r in canary if r.get("report") == "final")
        bad = sum(r["bad_words"] for r in final["regions"])
        digest_c = next(r["digest"] for r in canary if "digest" in r)
        digest_z = next(r["digest"] for r in zero if "digest" in r)
        errors = [r for r in canary if "error" in r and r["error"]]
        dsp = next((r["dsp"] for r in canary if "dsp" in r), None)
        dsp_ok = dsp_dir is None or (
            dsp is not None and dsp["halted"] is None and dsp["frames"] > 0
        )
        ok = bad == 0 and digest_c == digest_z and not errors and dsp_ok
        failed += not ok
        print(
            "%-8s %s %s: bad_words=%d digest_equal=%s icount=%s%s %s"
            % (
                name,
                image,
                region,
                bad,
                digest_c == digest_z,
                final["icount"],
                ""
                if dsp is None
                else " dsp_frames=%d halted=%s" % (dsp["frames"], dsp["halted"]),
                "PASS" if ok else "FAIL",
            )
        )
        if bad:
            for r in final["regions"]:
                if r["bad_words"]:
                    print(
                        "    dirty %s: %s"
                        % (r["start"], ", ".join(r["dirty_pages"][:8]))
                    )
    return 1 if failed else 0


def loader_code(cave: int, seg: int, copy_helper: int, clear_helper: int) -> bytes:
    """The reset-path loader (ColdFire). `seg` holds [u32 bytes][u32 dest][payload].

    Copies the payload to dest, then runs the two helpers the call site used to
    call, so the reset path goes on exactly as before. d0, d1, a0 and a1 are
    free there: the reset code reloads a0/d1 right after the clear and holds
    nothing in the others.
    """
    del cave
    code = (
        b"\x41\xf9"
        + struct.pack(">I", seg)  # lea seg.l,a0
        + b"\x20\x18"  # move.l (a0)+,d0   byte count
        + b"\x22\x58"  # movea.l (a0)+,a1  destination
        + b"\xe4\x88"  # lsr.l #2,d0
        + b"\x67\x06"  # beq.b done
        + b"\x22\xd8"  # loop: move.l (a0)+,(a1)+
        + b"\x53\x80"  # subq.l #1,d0
        + b"\x66\xfa"  # bne.b loop
        + b"\x4e\xb9"
        + struct.pack(">I", copy_helper)  # done: jsr copy_helper.l
        + b"\x4e\xb9"
        + struct.pack(">I", clear_helper)  # jsr clear_helper.l
        + b"\x4e\x75"  # rts
    )
    return code


def loader_test(image: str, kib: int, pack: str) -> int:
    import random

    from tools.firmware import build as dbuild

    a = analyse(image)
    f = a["reset"]
    main = bytearray(load_main(image))
    cave = next(c for c in a["flash_caves"] if c["keep"] and c["ends_at_clear_start"])
    seg = f["image_end"]
    dest = a["expansion"]["start"]
    rng = random.Random(0x5EED)
    payload = rng.randbytes(kib * 1024)
    code = loader_code(cave["start"], seg, f["copy_helper"], f["clear_helper"])
    off = cave["start"] - MAIN_BASE
    assert len(code) <= cave["size"] and not any(main[off : off + len(code)])
    hook = f["calls"]["copy"]
    assert f["calls"]["clear"] == hook + 4
    at = hook - MAIN_BASE
    assert (
        main[at : at + 8].startswith(b"\x4e\xba")
        and main[at + 4 : at + 6] == b"\x4e\xba"
    )
    main[at : at + 8] = b"\x4e\xb9" + struct.pack(">I", cave["start"]) + b"\x4e\x71"
    main[off : off + len(code)] = code
    assert seg == MAIN_BASE + len(main) and seg % 4 == 0
    main += struct.pack(">II", len(payload), dest) + payload
    work = ROOT / "out" / "runs" / "regions-loader"
    work.mkdir(parents=True, exist_ok=True)
    syx = work / (image + ".syx")
    syx.write_bytes(
        dbuild.rebuild(str(config.stock(IMAGES[image][1])), {3: bytes(main)}, pack=pack)
    )
    expect = work / (image + ".segment")
    expect.write_bytes(payload)
    base = IMAGES[image][2] or image
    base_sha = hashlib.sha256((config.stock(IMAGES[base][1])).read_bytes()).hexdigest()
    cmd = [str(probe_binary()), str(syx), "--base-sha", base_sha, "--extra", "20000000"]
    cmd += ["--expect", "0x%x:%s" % (dest, expect)]
    out = subprocess.run(
        cmd, cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout
    lines = [json.loads(x) for x in out.splitlines() if x.startswith("{")]
    res = next(r for r in lines if "expect" in r)
    ready = next((r["ready_at"] for r in lines if "ready_at" in r), None)
    ok = res["mismatch_words"] == 0 and ready is not None
    print(
        "%s: loader %d B at %s (hook %s), segment %d KiB at image end %s -> %s: "
        "mismatch_words=%d ready_at=%s %s"
        % (
            image,
            len(code),
            _hex(cave["start"]),
            _hex(hook),
            kib,
            _hex(seg),
            _hex(dest),
            res["mismatch_words"],
            ready,
            "PASS" if ok else "FAIL",
        )
    )
    return 0 if ok else 1


def main(argv=None) -> int:
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("report")
    r.add_argument("images", nargs="*", default=list(IMAGES))
    r.add_argument("--json", action="store_true")
    c = sub.add_parser("confirm")
    c.add_argument("image", choices=list(IMAGES))
    c.add_argument(
        "--region", default="expansion", help="expansion, flash_cave or START:LEN"
    )
    c.add_argument("--scenario", default="boot,nav,play")
    c.add_argument(
        "--dsp",
        type=pathlib.Path,
        help="ready directory whose DSP runs coupled with the probe",
    )
    c.add_argument(
        "--probe-args",
        nargs=argparse.REMAINDER,
        default=[],
        help="extra probe arguments (e.g. --card IMG)",
    )
    t = sub.add_parser("loader-test")
    t.add_argument("image", choices=list(IMAGES))
    t.add_argument("--kib", type=int, default=256)
    t.add_argument("--pack", choices=["store", "aplib"], default="store")
    args = p.parse_args(argv)
    if args.cmd == "loader-test":
        return loader_test(args.image, args.kib, args.pack)
    if args.cmd == "report":
        reports = [analyse(i) for i in args.images]
        if args.json:
            print(json.dumps(reports, indent=1))
        else:
            for rep in reports:
                print_report(rep)
        return 0
    return confirm(
        args.image, args.region, args.scenario.split(","), args.probe_args, args.dsp
    )


if __name__ == "__main__":
    sys.exit(main())
