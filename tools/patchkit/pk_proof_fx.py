#!/usr/bin/env python3
"""patchkit slice proof: core/fx is invisible with an empty chain and test/fx-gain halves the master in the DAC and the USB ring.

    uv run python -m tools.patchkit.pk_proof_fx [dt2|dn2] [--run-dir DIR] [--out DIR] [--only empty,gain] [--quick] [--jobs N]

DT2 1.17 or DN2 1.12, the stock firmware and ready state of tools/patchkit/pk_harness.py. Each part starts a coupled daemon on the stock firmware,
plays the same note (DT2 a one-sample card and TRIG 1; DN2 the scheduled note) in a session and, after the same number of DSP frames, reads the master
block, the DAC ring (A) and the USB ring (C) with `dsp.peek` (tools/patchkit/fx_rings.py has the formats and the addresses). The PCM tap records the
DAC side of the whole run.

  empty   stock, then the image with `core/fx` alone (no effect in the chain):
            PCM bit-identical to stock; master, ring A, ring C and the flag bit-identical at the snapshot frame.
  gain    the image with `core/fx` and `test/fx-gain` (0.5):
            master = 0.5 x stock (exact); ring A = the DAC conversion of that master; ring C master pair = the USB conversion of it; ring C
            channels 2 to 15 bit-identical to stock (the effect is in USB MAIN and does not touch the track outputs); PCM -6.0206 dB within 2 ulps.
          the same image with the gain word set to 1.0 with `dsp.patch`: master, rings equal to stock again.
          the same image with the chain row's `enabled` word cleared in mid-run: master, rings equal to stock after the fade-out block.
  send    the send route of core/fx (Digitakt II finding 62, Digitone II finding 67: the send bus is the sum of the voice slots of the track): the image with `core/fx` and `test/fx-gain`, the row's route word set to 1 and send amounts set with
          `dsp.patch` before the note (no amount: PCM, master, return and rings equal stock, the effect ran, the master chain did not run it; an amount on the playing
          track: the send bus is the track times the squared amount, the FX return is the stock return plus 0.5 x the bus, bit for bit; half the amount: the bus is
          (64/127)^2 of the full one; an amount on a silent track: all equal stock; MIX 0.5: half the return).

The note's frame count is the same in every run (the steps are instruction counts), so the snapshots of two runs are the same moment.
Requires DIGI_EMUD (the digi-emud binary of `mise run emu:serve -- --build-only`) or builds it.
"""

from __future__ import annotations

import argparse
import contextlib
import json
import pathlib
import struct
import sys
import time
from collections.abc import Iterator

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools.control.digi_client import Emu  # noqa: E402
from tools.emulator import card_check  # noqa: E402
from tools.firmware import device as device_files  # noqa: E402
from tools.patchkit import fx_rings as R  # noqa: E402
from tools.patchkit import hook_ab  # noqa: E402
from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402
from tools.patchkit.patched_audio_check import DN2_NOTE, TRIG1  # noqa: E402

FW = {"dt2": "1.17", "dn2": "1.12"}
RETURN_BUS = (
    0x254778  # Digitakt II: the FX return bus (anchor dsp.fx.value.send.return_bus)
)
TRACK_BUS = 0x252DF8  # the 16 track buses, 0x100 bytes apart (anchor dsp.fx.value.send.track_bus)
RETURN_BUS_DN2 = (
    0x25B868  # Digitone II: the FX return bus (anchor dsp.fx.value.send.return_bus)
)
SLOT_BUS = 0x257EE8  # Digitone II: the 16 voice slots' buses, 0x100 bytes apart (anchor dsp.fx.value.send.slot_bus)
SLOT_MAP = 0x257898  # Digitone II: the slot -> track map, 16 halfwords (anchor dsp.fx.value.send.slot_map)
W_MIX, W_WAS, W_DIS, W_NFAULT, W_ROUTE, W_AMT0 = (
    13,
    12,
    14,
    17,
    18,
    19,
)  # words of a chain row (core/fx module.toml)
EXTRA = 150  # frames after the note has started, to the snapshot
CORE = "core/fx@0.1.0"
GAIN = "test/fx-gain@0.1.0"


def f32_hex(x: float) -> str:
    return f"{struct.unpack('<I', struct.pack('<f', x))[0]:08x}"


class Image:
    """A patched image and the addresses of its plan (symbols of the fixture)."""

    def __init__(self, device: str, mods: list[str]):
        self.cell = H.Cell(device, FW[device], mods)
        self.stock, self.ready = H.firmware_inputs(self.cell)
        if mods:
            self.syx = pathlib.Path(H.patched_image(self.cell, self.stock)).resolve()
            plan = json.loads(self.syx.with_suffix(".plan.json").read_text())
            self.sym = plan["symbols"]
        else:
            self.syx, self.sym = self.stock, {}


def play(
    emu: Emu,
    device: str,
    wav: pathlib.Path,
    samples_card: bool,
    patches: list[str],
    mid_patch: list[str] | None,
    frame_hint: int | None,
    extras: dict[str, tuple[int, int]] | None = None,
    audio_s: float = 1.0,
):
    """Restart, apply the DSP patches, play the note with the PCM tap on, stop EXTRA frames after the note started
    (MID_PATCH is applied half way) and return (pcm, snapshot)."""
    emu.restart(auto_no=True)
    for p in patches:
        emu.patch(text=p)
    if device == "dn2":
        emu.call("pcm.tap", action="capture", path=str(wav))
        emu.schedule(DN2_NOTE)
        emu.run_until(audio_s=audio_s, timeout_wall_s=900)
    else:
        emu.step(20_000_000)
        emu.call("pcm.tap", action="capture", path=str(wav))
        emu.call("input.button", code=TRIG1, down=True)
        emu.step(40_000_000)
        emu.call("input.button", code=TRIG1, down=False)
    fr = emu.call("status")["dsp_frames"]
    if mid_patch:
        emu.call("run_until", frames=fr + EXTRA // 2)
        for p in mid_patch:
            emu.patch(text=p)
    emu.call("run_until", frames=fr + EXTRA)
    frame = emu.call("status")["dsp_frames"]
    snap = R.snap(emu, device, frame)
    extra = {
        k: bytes.fromhex(emu.dsp_peek(addr, n)["hex"].replace("??", "00"))
        for k, (addr, n) in (extras or {}).items()
    }
    emu.call("pcm.tap", action="capture_stop")
    if extras is not None:
        return hook_ab.read_wav(wav), snap, extra
    return hook_ab.read_wav(wav), snap


def same(a: R.Snap, b: R.Snap) -> dict[str, bool]:
    return {
        "frame": a.frame == b.frame,
        "flag": a.flag == b.flag,
        "master": a.master == b.master,
        "ring_a": a.a_words() == b.a_words(),
        "ring_c": a.c_words() == b.c_words(),
    }


class FxProof(K.Proof):
    tool = "fx"
    args: argparse.Namespace

    @contextlib.contextmanager
    def _session(self, image: Image, name: str) -> Iterator[Emu]:
        a = self.args
        with (
            H.run_dir_scope(f"fx-{a.device}-{name}", a.run_dir) as run_dir,
            Emu.spawn_coupled(
                image.stock,
                image.ready,
                timeout=300,
                run_dir=run_dir,
                binary=H.daemon_binary(a.device, image.stock),
            ) as emu,
        ):
            yield emu

    def _card(self, emu: Emu):
        if self.args.device == "dt2":
            samples = self.out / "samples"
            card_check.make_samples(samples)
            emu.call("card.load", dir=str(samples))

    def _run_image(
        self,
        name: str,
        mods: list[str],
        runs: dict[str, tuple[list[str], list[str] | None]],
        extras: dict[str, tuple[int, int]] | None = None,
        audio_s: float = 1.0,
    ):
        """Start a daemon, load the image (stock when MODS is empty) and play each run of RUNS (name -> (patches, mid patch))."""
        a = self.args
        image = Image(a.device, mods)
        out = {}
        with self._session(image, name) as emu:
            self._card(emu)
            if mods:
                emu.call("firmware.load", syx=str(image.syx), auto_no=True)
            for rn, (patches, mid) in runs.items():
                out[rn] = play(
                    emu,
                    a.device,
                    self.out / f"{name}-{rn}.wav",
                    a.device == "dt2",
                    patches,
                    mid,
                    None,
                    extras,
                    audio_s,
                )
        return image, out

    def part_empty(self) -> None:
        t0 = time.monotonic()
        _, st = self._run_image("stock", [], {"run": ([], None)})
        _, em = self._run_image("empty", ["core/fx"], {"run": ([], None)})
        self.timings["run_s"] = time.monotonic() - t0
        (pcm0, s0), (pcm1, s1) = st["run"], em["run"]
        r = hook_ab.compare(pcm0, pcm1, 1.0)
        self.check(
            "PCM of the image with an empty chain is bit-identical to stock",
            bool(r["identical"] and r["active_samples"] > 1000),
            f"{r['active_samples']} active samples",
        )
        for what, ok in same(s0, s1).items():
            self.check(f"{what} at frame {s0.frame} is bit-identical to stock", ok)
        self.check(
            "the snapshot holds a sound (the comparison means something)",
            max(abs(x) for x in s0.master_floats()) * 2**31 > 1e5,
            f"peak {max(abs(x) for x in s0.master_floats()):.2e}",
        )

    def part_gain(self) -> None:
        a = self.args
        t0 = time.monotonic()
        _, st = self._run_image("stock", [], {"run": ([], None)})
        image = Image(a.device, ["core/fx", "test/fx-gain"])
        gain_addr = image.sym[f"{GAIN}:_fg_gain"]
        enabled_addr = image.sym[f"{CORE}:fx.rows"] + 4 * 3  # row 0, word 3
        _, runs = self._run_image(
            "gain",
            ["core/fx", "test/fx-gain"],
            {
                "half": ([], None),
                "unity": ([f"DM {gain_addr:x} {f32_hex(1.0)}"], None),
                "off": ([], [f"DM {enabled_addr:x} 0"]),
            },
        )
        self.timings["run_s"] = time.monotonic() - t0
        pcm0, s0 = st["run"]
        pcm_h, sh = runs["half"]
        pcm_u, su = runs["unity"]
        pcm_o, so = runs["off"]
        m0, mh = s0.master_floats(), sh.master_floats()
        self.check(
            "same moment in every run",
            s0.frame == sh.frame == su.frame == so.frame,
            f"frame {s0.frame}",
        )
        self.check(
            "master = 0.5 x stock, exactly",
            all(h == R.f32(m * 0.5) for h, m in zip(mh, m0, strict=True)),
        )
        ea = R.ring_a_error(sh)
        ec = R.ring_c_pair_error(sh)
        self.check(
            "ring A (DAC) is the conversion of the halved master",
            ea <= 1,
            f"largest error {ea:.2f} LSB",
        )
        self.check(
            "ring C master pair (USB MAIN) is the conversion of the halved master",
            ec <= 1,
            f"largest error {ec:.2f} LSB",
        )
        a0, ah = s0.a_words(), sh.a_words()
        c0, ch = s0.c_words(), sh.c_words()
        self.check(
            "ring A is half of stock (the DAC carries the effect)",
            max(
                min(abs(2 * h - x), abs(2 * h + x)) for h, x in zip(ah, a0, strict=True)
            )
            <= 2,
        )
        pair = [16 * k + j for k in range(32) for j in (0, 1)]
        self.check(
            "ring C master pair is half of stock (the USB carries the effect)",
            max(abs(2 * ch[i] - c0[i]) for i in pair) <= 2,
        )
        self.check(
            "ring C channels 2 to 15 are bit-identical to stock (the tracks are untouched)",
            R.c_others(sh) == R.c_others(s0),
        )
        r = hook_ab.compare(pcm0, pcm_h, 0.5)
        self.check(
            "PCM is 0.5 x stock: -6.0206 dB within 2 ulps",
            bool(
                r["length_equal"]
                and r["max_err_ulps"] <= 2.0
                and abs(r["rms_db"] + 6.0206) < 0.001
            ),
            f"rms {r['rms_db']:.4f} dB, max err {r['max_err_ulps']} ulps",
        )
        for what, ok in same(s0, su).items():
            self.check(f"gain word 1.0: {what} equals stock", ok)
        for what, ok in same(s0, so).items():
            self.check(f"effect switched off in mid-run: {what} equals stock", ok)
        out = {
            "device": a.device,
            "frame": s0.frame,
            "ring_a_error": ea,
            "ring_c_pair_error": ec,
            "gain_word": hex(gain_addr),
            "enabled_word": hex(enabled_addr),
        }
        (self.out / "report.json").write_text(json.dumps(out, indent=1))

    def part_send(self) -> None:
        """The send route: `test/fx-gain` (0.5) in the chain with route 1 and send amounts written with `dsp.patch` before the note."""
        a = self.args
        dn2 = a.device == "dn2"
        # the Digitone II's scheduled note sounds from 0.46 s and has decayed to 0.002 at 1.1 s: the snapshot is taken while it sounds
        snap_s = 0.5 if dn2 else 1.0
        t0 = time.monotonic()
        extras = {
            "ret": (RETURN_BUS_DN2 if dn2 else RETURN_BUS, 256),
            "trk": (SLOT_BUS, 16 * 256) if dn2 else (TRACK_BUS, 512),
            "row": (0, 0),
        }
        if dn2:
            extras["map"] = (SLOT_MAP, 32)
        _, st = self._run_image(
            "stock",
            [],
            {"run": ([], None)},
            {k: v for k, v in extras.items() if k != "row"},
            snap_s,
        )
        image = Image(a.device, ["core/fx", "test/fx-gain"])
        rows = image.sym[f"{CORE}:fx.rows"]
        bus_at = image.sym[f"{CORE}:_fxs_bus"] + 4
        ex = {
            "ret": extras["ret"],
            "trk": extras["trk"],
            "bus": (bus_at, 256),
            "row": (rows, 96),
        }
        if dn2:
            ex["map"] = extras["map"]

        def w(word: int, value: int) -> str:
            return f"DM {rows + 4 * word:x} {value:08x}"

        route = w(W_ROUTE, 1)
        runs = {
            "master": ([], None),  # route 0: the master chain, as before
            "send0": ([route], None),  # route 1, no amount
            "on1": (
                [route, w(W_AMT0, 0x7F)],
                None,
            ),  # amount 127 on track 1 (the TRIG 1 sample)
            "half": ([route, w(W_AMT0, 0x40)], None),
            "on2": ([route, w(W_AMT0, 0x7F00)], None),  # amount 127 on track 2 (silent)
            "mixhalf": ([route, w(W_AMT0, 0x7F), w(W_MIX, 0x3F000000)], None),
        }
        _, out = self._run_image("send", ["core/fx", "test/fx-gain"], runs, ex, snap_s)
        self.timings["run_s"] = time.monotonic() - t0
        pcm0, s0, x0 = st["run"]
        floats = lambda b: struct.unpack(f"<{len(b) // 4}f", b)  # noqa: E731
        ret0 = floats(x0["ret"])
        m0 = s0.master_floats()
        pm, sm, xm = out["master"]
        self.check(
            "route 0 (master chain): master = 0.5 x stock, exactly, as before",
            all(
                h == R.f32(m * 0.5) for h, m in zip(sm.master_floats(), m0, strict=True)
            ),
        )
        self.check(
            "route 0: the send hook adds nothing to the FX return (equal to stock)",
            xm["ret"] == x0["ret"],
        )
        p1, s1, x1 = out["send0"]
        row1 = struct.unpack("<24I", x1["row"])
        r = hook_ab.compare(pcm0, p1, 1.0)
        self.check(
            "route 1, all amounts 0: PCM is bit-identical to stock",
            bool(r["identical"] and r["active_samples"] > 1000),
            f"{r['active_samples']} active samples",
        )
        for what, ok in same(s0, s1).items():
            self.check(f"route 1, all amounts 0: {what} equals stock", ok)
        self.check(
            "route 1, all amounts 0: the FX return equals stock, the send bus is silent",
            x1["ret"] == x0["ret"] and all(v == 0.0 for v in floats(x1["bus"])),
        )
        self.check(
            "route 1: the effect ran (was 1, no fault) and the master chain skipped it (master is stock, not 0.5 x stock)",
            row1[W_WAS] == 1
            and row1[W_NFAULT] == 0
            and row1[W_DIS] == 0
            and s1.master == s0.master,
            f"was {row1[W_WAS]} faults {row1[W_NFAULT]}",
        )
        # an amount on the playing track
        p2, s2, x2 = out["on1"]
        bus = floats(x2["bus"])
        ret = floats(x2["ret"])
        if dn2:
            # the track's sound: the slot buses of the stock run whose map entry names track 1 (the slots are summed in order, as the send mix does)
            smap = struct.unpack("<16h", x0["map"])
            sl = floats(x0["trk"])
            trk0 = [0.0] * 64
            for v in range(16):
                if smap[v] == 0:
                    trk0 = [
                        R.f32(p + q)
                        for p, q in zip(trk0, sl[64 * v : 64 * v + 64], strict=True)
                    ]
        else:
            # the track bus after the block (the post-master stage scales it in place)
            trk0 = list(floats(x0["trk"])[:64])
        peak = max(abs(v) for v in bus)
        ratios = [
            b / t
            for b, t in zip(bus, trk0, strict=True)
            if abs(t) > 0.01 * max(abs(v) for v in trk0)
        ]
        k = sum(ratios) / len(ratios)
        spread = max(abs(q / k - 1.0) for q in ratios)
        self.check(
            "route 1, amount 127 on track 1: the send bus is a constant multiple of the track bus (the multiple is the post-master scale, 1 / "
            f"{1 / k:.5f}; relative spread {spread:.1e}), on a sound",
            peak > 1e-3 and spread < 1e-5,
            f"peak {peak:.4f}, multiple {k:.6f}",
        )
        want = tuple(
            R.f32(r0 + R.f32(R.f32(0.5 * b) * 1.0))
            for r0, b in zip(ret0, bus, strict=True)
        )
        self.check(
            "route 1, amount 127 on track 1: the FX return = stock return + 0.5 x the send bus, bit for bit",
            ret == want,
            f"{sum(x != y for x, y in zip(ret, want, strict=True))} of 64 words differ",
        )
        r2 = hook_ab.compare(pcm0, p2, 1.0)
        self.check(
            "route 1, amount 127 on track 1: the master mixes the return (PCM differs from stock, same length)",
            bool(not r2["identical"] and r2["length_equal"]),
            f"max err {r2['max_err_ulps']:.0f} ulps",
        )
        # half the amount: (64 / 127) squared
        _, s3, x3 = out["half"]
        bus3 = floats(x3["bus"])
        q = [b3 / b for b3, b in zip(bus3, bus, strict=True) if abs(b) > 0.01 * peak]
        want_q = (64 / 127) ** 2
        self.check(
            f"route 1, amount 64: the send bus is (64/127)^2 = {want_q:.6f} of the amount-127 bus",
            max(abs(v / want_q - 1.0) for v in q) < 1e-5,
            f"ratios {min(q):.6f} to {max(q):.6f}",
        )
        # an amount on a silent track
        p4, s4, x4 = out["on2"]
        r4 = hook_ab.compare(pcm0, p4, 1.0)
        self.check(
            "route 1, amount 127 on track 2 (silent): the send bus is silent and PCM, master and FX return equal stock",
            bool(r4["identical"])
            and s4.master == s0.master
            and x4["ret"] == x0["ret"]
            and all(v == 0.0 for v in floats(x4["bus"])),
        )
        # MIX is the return level
        _, s5, x5 = out["mixhalf"]
        bus5, ret5 = floats(x5["bus"]), floats(x5["ret"])
        want5 = tuple(
            R.f32(r0 + R.f32(R.f32(0.5 * b) * 0.5))
            for r0, b in zip(ret0, bus5, strict=True)
        )
        self.check(
            "route 1, MIX 0.5: the FX return = stock return + 0.5 x 0.5 x the send bus, bit for bit",
            ret5 == want5 and max(abs(v) for v in bus5) > 1e-3,
        )
        (self.out / "send-report.json").write_text(
            json.dumps(
                {"multiple": k, "spread": spread, "peak": peak, "rows": list(row1)},
                indent=1,
            )
        )


PARTS = [
    K.Part("empty", "stock vs empty chain", FxProof.part_empty, True, 90),
    K.Part(
        "gain",
        "0.5 effect: master, DAC ring, USB ring; unity; off",
        FxProof.part_gain,
        True,
        240,
    ),
    K.Part(
        "send",
        "send route: amounts, return bus, MIX; stock equality",
        FxProof.part_send,
        True,
        240,
    ),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("device", nargs="?", default="dt2", choices=device_files.shorts())
    ap.add_argument(
        "--run-dir",
        type=pathlib.Path,
        help="daemon run dir (default: a temp dir, removed after a clean run)",
    )
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or ROOT / "out" / "runs" / "fx-proof" / a.device
    FxProof.args = a
    FxProof.device = a.device
    FxProof.fw = FW[a.device]
    return K.run_tool(__file__, FxProof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
