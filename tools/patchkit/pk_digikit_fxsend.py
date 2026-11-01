#!/usr/bin/env python3
"""The `fxsend` kind of the digikit scenarios: a run-time effect on the send route of `core/fx` (Digitakt II; docs/findings/62-fx-send-route.md).

Run by `tools/patchkit/pk_digikit_scen.py` (scenario `fx-send-dt2`). The image is the runtime with `core/dynfx` and `core/fxpages`; the sound is the run-time machine `test/sine` on track 1, the effect
`test/dyn-fx-gain` (p0 = 0.5, a call counter in its state) is loaded from the card at boot into row 0 of the chain. The runtime does not write the route and the amounts yet (a later
step): the scenario writes the row's words 18 to 22 with `dsp.patch` after the boot, as the runtime will over window 2 of `core/dsp-link`. Five boots, each in its own daemon:

  dry    the sine alone: the reference PCM.
  zero   the effect on route 1 (send), all amounts 0: the PCM is the dry PCM bit for bit (the send bus is silent, the return gets nothing); the effect ran once a block (the counter) and the
         master chain did not run it as well (no second call, no 0.5 gain on the master).
  other  an amount of 127 on track 2 (silent): the same, bit for bit.
  full   an amount of 127 on track 1 (the sine): the return carries 0.5 x the track, the master mixes it in: the PCM is the dry PCM times 1.5: the master mixes the FX return at unity, so the send adds 0.5 x the track.
  half   an amount of 64 on track 1: the added part is (64/127)^2 of the full amount's, within the tolerance of the master's own smoothing (the gain law is the stock sends' square)."""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path
from typing import Any

import numpy as np

from tools.patchkit import hook_ab, pk_device
from tools.patchkit import pk_dkrt_machine as M
from tools.patchkit.pk_digikit_fx import (
    FX,
    SINE,
    W_DIS,
    W_NFAULT,
    W_WAS,
    chain_rows,
    scaled,
)

W_ROUTE, W_AMT0 = 18, 19  # row words of the send route (core/fx module.toml)
GAIN = 0.5  # test/dyn-fx-gain
SLOT_BUS, SLOT_BYTES = (
    0x257EE8,
    256,
)  # DN2: the 16 voice slots' buses (anchor dsp.fx.value.send.slot_bus; 32 left then 32 right floats each)


def aligned(ref: np.ndarray, run: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """REF and RUN from the note's onset in each, over the common length (the boots reach the note up to a few blocks apart)."""

    def onset(x: np.ndarray) -> int:
        nz = np.flatnonzero(np.abs(x) > 1e-6)
        return int(nz[0]) if len(nz) else 0

    a, b = onset(ref), onset(run)
    n = min(len(ref) - a, len(run) - b)
    return ref[a : a + n], run[b : b + n]


def added_factor(dry: np.ndarray, out: np.ndarray) -> float:
    """The factor of the part that the send adds: OUT is the dry note times (1 + f), possibly started a few frames later than DRY (the note's start in the coupled run depends on how many
    DSP instructions the blocks before it cost, so a heavier hook starts it later). The least-squares gain of OUT over DRY at the best frame shift (even interleaved offsets, up to 256 frames
    either way; the shift that leaves the smallest residue, which is 0 for a note that starts together) is 1 + f. A median of ratios is noisy where the sound is quiet."""
    pad = 600
    n = min(len(dry), len(out)) - 2 * pad
    d = dry[pad : pad + n].astype(np.float64)
    dd = float(np.dot(d, d))
    best = (float("inf"), 0.0)
    for sh in range(-512, 513, 2):
        o = out[pad + sh : pad + sh + n].astype(np.float64)
        a = float(np.dot(o, d) / dd)
        res = float(np.abs(o - a * d).max())
        best = min(best, (res, a - 1.0))
    return best[1]


def run_fxsend(c: Any) -> None:
    from tools.patchkit.pk_digikit_scen import RT

    t = c.table()
    hint = t.get("hint", 7)
    trig = t.get("trig_frame", M.TRIG_FRAME)
    slots = pk_device.limits(c.device).fx_slots
    img = c.image_for(
        RT + ",core/dynfx,core/fxpages", "fxsend"
    )  # the test effect has a page: core/fxpages
    sine = c.pack(SINE, "m0.dkm")
    fx_file = c.pack(FX, "m1.dkm")
    pcm: dict[str, np.ndarray] = {}
    sounding = [0]
    counts: dict[str, tuple[int, int]] = {}

    def sound(rig: Any, wav: Path) -> np.ndarray:
        rig.pick(rig.dyn_type(SINE, "sine"), hint)
        return rig.note(wav, trig)

    def set_row(rig: Any, words: dict[int, int]) -> None:
        base = M.sym(rig.plan, "core/fx", "fx.rows")
        text = "".join(f"DM {base + 4 * w:x} {v:08x}\n" for w, v in words.items())
        rig.emu.patch(text=text)

    def slots_sounding(rig: Any) -> int:
        """DN2: the voice slots whose bus carries a sound in the block that ran last (the slot buses are read from the DSP memory)."""
        raw = rig.dsp_bytes(SLOT_BUS, 16 * SLOT_BYTES)
        f = np.frombuffer(raw, dtype="<f4").reshape(16, 64)
        return int((np.abs(f).max(axis=1) > 1e-5).sum())

    def chord(rig: Any, wav: Path) -> np.ndarray:
        """DN2: two notes of track 1, the second while the first still sounds (a release tail): two voice slots. TRIG 1 down, up, down; the PCM is captured from the first."""
        emu = rig.emu
        emu.call("run_until", frames=trig)
        emu.step(M.PRESS_OFFSET)
        emu.call("pcm.tap", action="capture", path=str(wav))
        btn = M.H.button(M.TRIG1)
        emu.button(btn, True)
        emu.call("run_until", frames=trig + 60)
        emu.button(btn, False)
        emu.call("run_until", frames=trig + 120)
        emu.button(btn, True)
        emu.call("run_until", frames=trig + 220)
        sounding[0] = slots_sounding(rig)
        emu.call("run_until", frames=trig + M.NOTE_FRAMES)
        emu.call("pcm.tap", action="capture_stop")
        emu.button(btn, False)
        rig.s.run_ms(50)
        rig.s.health(f"chord {rig.tag}")
        return M.hook_ab.read_wav(wav)

    def idle(rig: Any, wav: Path) -> np.ndarray:
        """No note: the PCM of the same span as a note's."""
        emu = rig.emu
        emu.call("run_until", frames=trig)
        emu.call("pcm.tap", action="capture", path=str(wav))
        emu.call("run_until", frames=trig + M.NOTE_FRAMES // 2)
        emu.call("pcm.tap", action="capture_stop")
        return M.hook_ab.read_wav(wav)

    def boot(
        tag: str,
        amounts: int | None,
        play: Any = None,
        words: dict[int, int] | None = None,
    ) -> None:
        """Sine and effect; AMOUNTS None: the effect stays on the master route (not used), else route 1 and the word 19 = AMOUNTS (the others 0, or WORDS). PLAY replaces the single note."""
        card = c.card([sine, fx_file] if amounts is not None else [sine])
        with c.rig(img, card, tag) as rig:
            info = rig.boot_info()
            want = [0, 0] if amounts is not None else [0]
            c.check(
                f"{tag}: the sine{' and the effect' if amounts is not None else ''} loaded",
                info["results"] == want and info["safe"] == 0,
                f"{info['results']}",
            )
            if amounts is not None:
                row = chain_rows(rig, slots)[0]
                c.check(
                    f"{tag}: row 0 of the chain holds the effect, on the master route with no amount (words 18 to 22 are 0)",
                    row[0] != 0xFFFF_FFFF
                    and row[3] == 1
                    and row[W_ROUTE : W_AMT0 + 4] == (0,) * 5,
                    f"{[hex(x) for x in row]}",
                )
                set_row(rig, {W_ROUTE: 1, W_AMT0: amounts, **(words or {})})
                row = chain_rows(rig, slots)[0]
                c.check(
                    f"{tag}: the route word is 1 and the amount word {amounts:#x}",
                    row[W_ROUTE] == 1 and row[W_AMT0] == amounts,
                    f"{row[W_ROUTE]} {row[W_AMT0]:#x}",
                )
            if play is None:
                pcm[tag] = sound(rig, c.work / f"{tag}.wav")
            else:
                rig.pick(rig.dyn_type(SINE, "sine"), hint)
                pcm[tag] = play(rig, c.work / f"{tag}.wav")
            if amounts is not None:
                rows = chain_rows(rig, slots)
                after = rows[0]
                c.check(
                    f"{tag}: the effect ran (was 1), not switched off, no fault",
                    after[W_WAS] == 1 and after[W_DIS] == 0 and after[W_NFAULT] == 0,
                    f"was {after[W_WAS]} dis {after[W_DIS]} faults {after[W_NFAULT]}",
                )
                state_addr = after[2]
                n0 = struct.unpack("<I", rig.dsp_bytes(state_addr, 4))[0]
                rig.emu.call("run_until", frames=rig.frames() + 100)
                n1 = struct.unpack("<I", rig.dsp_bytes(state_addr, 4))[0]
                counts[tag] = (n0, n1)
                c.check(
                    f"{tag}: the effect's call counter goes up by one a block: the send hook runs it and the master chain does not",
                    n0 > 1000 and (90 <= n1 - n0 <= 110 or play is idle),
                    f"{n0} then {n1}",
                )
            c.done(rig)

    dn2 = c.device == "dn2"
    boots = [
        lambda: boot("dry", None),
        lambda: boot("zero", 0),
        lambda: boot("other", 0x7F00),
        lambda: boot("full", 0x7F),
        lambda: boot("half", 0x40),
    ]
    if dn2:  # the Digitone II sums voice slots into the tracks: a chord, and slots that are free
        boots += [
            lambda: boot("chordzero", 0, chord),
            lambda: boot("chordfull", 0x7F, chord),
            lambda: boot(
                "idle", 0x7F7F7F7F, idle, {19 + k: 0x7F7F7F7F for k in range(1, 4)}
            ),
        ]
    c.parallel(*boots)
    dry = pcm["dry"]
    c.check(
        "dry: the sine is audible",
        float(abs(dry).max()) > 0.01,
        f"peak {float(abs(dry).max()):.3f}",
    )
    for tag, what in (
        ("zero", "route 1, all amounts 0"),
        ("other", "route 1, amount 127 on track 2 (silent)"),
    ):
        r = scaled(dry, pcm[tag], 1.0)
        c.check(
            f"{tag}: with {what} the PCM is the dry PCM, bit for bit from the onset on",
            bool(r["identical"]) and abs(r["shift"]) <= 128,
            f"onset shift {r['shift']}, max err {r.get('max_abs_err')}",
        )
    # the reference of the factor is the run with the effect loaded and no amount (`zero`, equal to the dry PCM): a run without the effect module starts the note some 60 samples earlier
    e_full = added_factor(pcm["zero"], pcm["full"])
    e_half = added_factor(pcm["zero"], pcm["half"])
    ratio = e_half / e_full if e_full else float("nan")
    want = (64 / 127) ** 2
    c.check(
        f"full: with an amount of 127 on track 1 the master mixes the effect's return: the PCM is the dry PCM times {1 + e_full:.5f} (the send adds {GAIN} x the track x the return's level {e_full / GAIN:.4f})",
        abs(e_full / GAIN - 1.0) < 1e-3,
        f"added factor {e_full:.5f}",
    )
    c.check(
        f"half: with an amount of 64 the added part is (64/127)^2 = {want:.5f} of the full amount's (the stock sends square their amount): {ratio:.5f}",
        abs(ratio / want - 1.0) < 0.01,
        f"added factor {e_half:.5f} against {e_full:.5f}",
    )
    c.check(
        "full: the PCM differs from the dry PCM (the effect is audible through the return)",
        not bool(scaled(dry, pcm["full"], 1.0)["identical"]),
        "",
    )
    if dn2:
        e_chord = added_factor(pcm["chordzero"], pcm["chordfull"])
        c.check(
            f"chord: the two notes of track 1 sound in {sounding[0]} voice slots at once",
            sounding[0] >= 2,
            f"{sounding[0]} slots with a sound",
        )
        c.check(
            f"chord: with an amount of 127 on track 1 the return carries the sum of the slots: the PCM is the chord without an amount times {1 + e_chord:.5f}",
            abs(e_chord / GAIN - 1.0) < 1e-3,
            f"added factor {e_chord:.5f}",
        )
        c.check(
            "idle: with an amount of 127 on all 16 tracks and no note (every slot free) the PCM is silent",
            len(pcm["idle"]) > 1000 and float(np.abs(pcm["idle"]).max()) == 0.0,
            f"peak {float(np.abs(pcm['idle']).max()):.2e}",
        )
    c.common_checks()


def _read_pcm(path: Path) -> np.ndarray:
    """Read completed capture samples with the existing shared WAV reader."""
    samples = hook_ab.read_wav(path)
    if not len(samples):
        raise ValueError(f"{path}: empty PCM")
    if not np.isfinite(samples).all():
        raise ValueError(f"{path}: PCM contains nonfinite samples")
    return samples


def slot_bus_address(device: str, firmware: str) -> int:
    """Read the current DN2 send-slot bus from its named port anchor."""
    from tools.patchkit.pk_dsplink import plan_anchors

    if (device, firmware) != ("dn2", "1.12"):
        raise ValueError("slot-bus supports only dn2 firmware 1.12")
    name = "dsp.fx.value.send.slot_bus"
    anchors = [a for a in plan_anchors(device, firmware) if a.get("name") == name]
    if len(anchors) != 1:
        raise ValueError(f"expected exactly one {name} anchor, found {len(anchors)}")
    address = anchors[0].get("addr")
    if type(address) is not int or address <= 0:
        raise ValueError(f"{name}: invalid address")
    return address


def main(argv: list[str] | None = None) -> int:
    """Small JSON analysis interface for host-owned FX send scenarios."""
    parser = argparse.ArgumentParser(description=main.__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    factor = commands.add_parser("added-factor")
    factor.add_argument("base", type=Path)
    factor.add_argument("out", type=Path)
    pcm = commands.add_parser("pcm")
    pcm.add_argument("file", type=Path)
    slot = commands.add_parser("slot-bus")
    slot.add_argument("--device", required=True)
    slot.add_argument("--firmware", required=True)
    args = parser.parse_args(argv)
    result: dict[str, int | float | bool]
    try:
        if args.command == "slot-bus":
            result = {"address": slot_bus_address(args.device, args.firmware)}
        elif args.command == "pcm":
            samples = _read_pcm(args.file)
            result = {
                "samples": len(samples),
                "finite": True,
                "peak": float(np.abs(samples).max()),
            }
        else:
            base, out = _read_pcm(args.base), _read_pcm(args.out)
            n = min(len(base), len(out)) - 1200
            if n <= 0:
                raise ValueError("added-factor requires more than 1200 samples per WAV")
            dry = base[600 : 600 + n].astype(np.float64)
            if float(np.dot(dry, dry)) == 0:
                raise ValueError(
                    "added-factor requires a non-silent reference fit window"
                )
            value = added_factor(base, out)
            if not np.isfinite(value):
                raise ValueError("added-factor produced a nonfinite result")
            result = {"added_factor": value}
    except (OSError, ValueError, KeyError, SystemExit) as error:
        print(f"{parser.prog}: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, allow_nan=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
