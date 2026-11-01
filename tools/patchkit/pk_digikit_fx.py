#!/usr/bin/env python3
"""The `fx` kind of the digikit scenarios: effects loaded at run time (docs/design/digikit-dkm.md, DSP section version 4; finding 55).

Run by `tools/patchkit/pk_digikit_scen.py`. The image is the runtime with `core/dynfx` (the effect chain of `core/fx` with free rows); the sound is the run-time
machine `test/sine` on track 1. The three retained Python legs each use their own daemon/card:

  plain  the sine alone on an image without `core/dynfx`: the PCM that the hook and the free rows must not change.
  dry    the sine alone: the reference PCM (equal to the plain one); every effect row is free; CAPACITY FX_SLOTS has the device file's limit and none used.
  full   the sine and one more effect than the device has slots (the same file under other names): the effects fill the slots, the last is refused with
         `E_DSP_ROWS` and leaves nothing behind; the PCM is the dry PCM times 0.5 per effect; the fit rule (`dkneeds.fit`, the DIGIKIT menu's) refuses one more
         effect, naming the effect slots, from the live CAPACITY and from an empty one.

The one-effect gain/metadata/relocation/state/deselect-restart leg is covered by
`tests/regression/runtime/fx-reuse.test.ts`, including lifetime write/flush and card-sector checks.
Its accepted contract proves FX behavior on audible input, not direct selected-track identity.
The retained dry/full paths still perform their DT2 selected-sine checks.
"""

from __future__ import annotations

import struct
from pathlib import Path
from typing import Any

import numpy as np

from tools.device import dkneeds
from tools.patchkit import pk_device
from tools.patchkit import pk_digikit_debug as D
from tools.patchkit import pk_dkrt_machine as M
from tools.patchkit.hook_ab import compare_aligned as scaled

FREE_PC = 0xFFFF_FFFF
ROW_WORDS = 24  # a row of the chain (core/fx module.toml)
# row words: pc, desc, state, enabled, p0 .. p7, then the adapter's was (12), dis (14), nfault (17)
W_PC, W_DESC, W_STATE, W_EN, W_P0, W_WAS, W_DIS, W_NFAULT = 0, 1, 2, 3, 4, 12, 14, 17
SINE = "test/sine"
FX = "test/dyn-fx-gain"
E_DSP_ROWS = -34


def chain_rows(rig: Any, n: int) -> list[tuple[int, ...]]:
    """The first N rows of the effect chain as the DSP holds them (words of ROW_WORDS)."""
    base = M.sym(rig.plan, "core/fx", "fx.rows")
    words = struct.unpack(f"<{n * ROW_WORDS}I", rig.dsp_bytes(base, 4 * ROW_WORDS * n))
    return [tuple(words[i * ROW_WORDS : (i + 1) * ROW_WORDS]) for i in range(n)]


def run_fx(c: Any) -> None:
    from tools.patchkit.pk_digikit_scen import RT

    t = c.table()
    hint = t.get("hint", 7)
    trig = t.get("trig_frame", M.TRIG_FRAME)
    slots = pk_device.limits(c.device).fx_slots
    img = c.image_for(
        RT + ",core/dynfx,core/fxpages", "fx"
    )  # the test effect has a page: core/fxpages
    sine = c.pack(SINE, "m0.dkm")
    fx_files, fx_bytes = [], []
    for k in range(
        slots + 1
    ):  # the same effect under other names: a module is loaded once under its name
        out = c.env.folder / "digikit" / "modules" / f"m{k + 1}.dkm"
        name = FX if k == 0 else f"{FX}-{k + 1}"
        c.env.tools.patchkit_run(
            "pack", FX, "--device", c.device, "--name", name, "-o", out
        )
        fx_files.append(f"modules/m{k + 1}.dkm")
        fx_bytes.append(out.read_bytes())
    sec = M.parse_dsp_section(fx_bytes[0])
    fx = sec["fxs"][0]
    pcm: dict[str, np.ndarray] = {}

    def sound(rig: Any, wav: Path, restarted: bool = False) -> np.ndarray:
        """The sine on track 1 and a note from the DSP block TRIG (a fresh boot); after a restart the frame counter goes on, so the note starts 300 blocks on."""
        rig.pick(rig.dyn_type(SINE, "sine"), hint)
        return rig.note(wav, rig.frames() + 300 if restarted else trig)

    def check_pcm(tag: str, got: np.ndarray, gain: float, what: str) -> None:
        r = scaled(pcm["dry"], got, gain)
        db = 20 * np.log10(gain)
        c.check(
            f"{tag}: the PCM is the dry PCM times {gain} ({what}): {db:.4f} dB, within 2 ulps from the onset on",
            r["rms_db"] is not None
            and r["max_err_ulps"] <= 2.0
            and abs(r["rms_db"] - db) < 0.001
            and abs(r["shift"]) <= 128,
            f"rms {r['rms_db']}, max err {r['max_err_ulps']:.2f} ulps, onset shift {r['shift']}",
        )

    def chain_ok(
        tag: str, rig: Any, loaded: list[tuple[dict[str, Any], dict[str, Any]]]
    ) -> list[tuple[int, ...]]:
        """The chain's rows: the first rows hold the loaded effects (pc, desc, state, enabled, p0 .. p7 as the record says), the rest are free."""
        rows = chain_rows(rig, slots)
        area = M.sym(rig.plan, "core/dsp-link", "_dl_area")
        for k, (rec, e) in enumerate(loaded):
            base = area + rec["dsp_off"]
            state_at = (len(sec["image"]) + sec["bss"] + 7) & ~7
            want = (
                M.L2_SW_BASE + (base + e["process"] - M.L2_BYTE_BASE) // 2,
                base + e["desc"],
                base + state_at,
                1,
                *e["defaults"],
            )
            c.check(
                f"{tag}: row {k} of the chain holds the effect: pc, desc, state, enabled, p0 .. p7",
                rows[k][:12] == want,
                f"{[hex(x) for x in rows[k][:12]]} want {[hex(x) for x in want]}",
            )
        free = rows[len(loaded) :]
        c.check(
            f"{tag}: the other {len(free)} rows are free (pc 0xffffffff)",
            all(r[W_PC] == FREE_PC for r in free),
            f"{[hex(r[W_PC]) for r in free]}",
        )
        return rows

    def plain() -> None:
        """The same boot on an image without `core/dynfx` and so without the hook: the reference of "free rows change nothing"."""
        plain_img = c.image_for(RT, "plain")
        with c.rig(plain_img, c.card([sine]), "plain") as rig:
            info = rig.boot_info()
            c.check(
                "plain: the sine loaded on the image without the effect chain",
                info["results"] == [0] and info["safe"] == 0,
                f"{info['results']}",
            )
            pcm["plain"] = sound(rig, c.work / "plain.wav")
            c.done(rig)

    def dry() -> None:
        with c.rig(img, c.card([sine]), "dry") as rig:
            info = rig.boot_info()
            c.check(
                "dry: the sine loaded, nothing else",
                info["results"] == [0] and info["safe"] == 0,
                f"{info['results']}",
            )
            cap = D.uploader(rig).capacity()
            c.check(
                "dry: CAPACITY FX_SLOTS has the device file's limit and none used",
                cap["FX_SLOTS"] == {"limit": slots, "used": 0, "largest": 0},
                f"{cap['FX_SLOTS']}",
            )
            rows = chain_rows(rig, slots)
            c.check(
                f"dry: all {slots} rows of the chain are free",
                all(r[W_PC] == FREE_PC for r in rows),
                f"{[hex(r[W_PC]) for r in rows]}",
            )
            pcm["dry"] = sound(rig, c.work / "dry.wav")
            c.check(
                "dry: the sine is audible",
                float(abs(pcm["dry"]).max()) > 0.01,
                f"peak {float(abs(pcm['dry']).max()):.3f}",
            )
            c.done(rig)

    def full() -> None:
        sel = [sine, *fx_files]
        with c.rig(img, c.card(sel), "full") as rig:
            info = rig.boot_info()
            want = [0] * (1 + slots) + [E_DSP_ROWS]
            c.check(
                f"full: the sine and {slots} effects loaded, the next effect is refused with E_DSP_ROWS",
                info["results"] == want and info["safe"] == 0,
                f"{info['results']} want {want}",
            )
            up = D.uploader(rig)
            mods = up.modules()
            cap = up.capacity()
            c.check(
                f"full: MODULES lists the sine and {slots} effects; CAPACITY FX_SLOTS is full",
                mods["total"] == 1 + slots
                and cap["FX_SLOTS"] == {"limit": slots, "used": slots, "largest": 0},
                f"{mods['total']} {cap['FX_SLOTS']}",
            )
            recs = mods["modules"][1:]
            chain_ok("full", rig, [(r, fx) for r in recs])
            c.check(
                "full: the refused effect left nothing behind: the DSP ranges are disjoint and the table check never failed",
                ranges_ok(mods["modules"]) and table_ok(rig, up),
                "",
            )
            pcm["full"] = sound(rig, c.work / "full.wav")
            # the fit rule: one more effect does not fit the slots that are taken
            short = dkneeds.fit(cap, [dkneeds.needs_of(fx_bytes[slots])])["each"][0]
            c.check(
                "full: the fit rule refuses one more effect and names the effect slots",
                short is not None
                and (short["resource"], short["need"], short["free"])
                == ("FX_SLOTS", 1, 0)
                and short["describe"] == "effect slots 1 needed, 0 free"
                and short["text"] == "FX 1 > 0 FREE",
                f"{short}",
            )
            set_short = dkneeds.fit(
                cap, [dkneeds.needs_of(b) for b in fx_bytes], empty=True
            )["set"]
            c.check(
                f"full: from an empty runtime the set of {slots + 1} effects stops at effect {slots + 1} for the effect slots",
                set_short is not None
                and (
                    set_short["index"],
                    set_short["resource"],
                    set_short["need"],
                    set_short["free"],
                )
                == (slots, "FX_SLOTS", 1, 0),
                f"{set_short}",
            )
            c.done(rig)

    c.parallel(plain, dry, full)
    # the sounds are compared with the dry one, which a part that ran beside the others had not finished yet
    r0 = scaled(pcm["plain"], pcm["dry"], 1.0)
    c.check(
        "dry: with the chain's hook and its free rows the PCM is the PCM of the image without them, bit for bit from the onset on",
        bool(r0["identical"]) and abs(r0["shift"]) <= 128,
        f"onset shift {r0['shift']}, max err {r0.get('max_abs_err')}",
    )
    check_pcm("full", pcm["full"], 0.5**slots, f"{slots} effects in series, 0.5 each")
    c.common_checks()


def table_ok(rig: Any, up: Any) -> bool:
    """The runtime's table check never failed (`DKRT_TABLE_BAD` is 0, no `TABLE_BAD` event)."""
    bad = rig.w(M.sym(rig.plan, "core/runtime", "DKRT_TABLE_BAD")) >> 24
    return bad == 0 and not D.events(rig, "TABLE_BAD")


def ranges_ok(mods: list[dict[str, Any]]) -> bool:
    """The DSP ranges of the listed modules are pairwise disjoint."""
    rs = sorted(
        (m["dsp_off"], m["dsp_off"] + m["dsp_len"]) for m in mods if m["dsp_len"]
    )
    return all(rs[i][1] <= rs[i + 1][0] for i in range(len(rs) - 1))
