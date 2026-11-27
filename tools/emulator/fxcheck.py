#!/usr/bin/env python3
"""Bit-reduction and delay features of DN2 FX scenario runs, as a compact table.

    uv run python -m tools.emulator.fxcheck RUN_DIR [RUN_DIR ...] [--ref fx-dry-tap] [--no-plots]

A RUN_DIR holds report.json and the WAVs (audio-<scenario>.wav, or audio.wav
for a single scenario) of a ``coupled.py scenario --wav`` run. The dry
reference is the scenario named by --ref (default fx-dry-tap) in any of the
given run directories. Parameter values come from the report's ``peek``
(s16 big-endian, UI value = high byte). Writes fxcheck.json and, unless
--no-plots, plots-<scenario>/ (audiocheck plots) into each run directory.

Track FX sweeps (tests/regression/scenarios/dn2-trackfx.ts, ``tf-*``): the reference is
tf-dry-hold or tf-dry-tap by hold time, every run decodes the whole peeked
Sound and Fx arrays by parameter name (descriptor table of --image, default the
DN2 1.12 MAIN OS) and lists the ones that differ from the reference, and the
per-control measures are printed in a second table. ``--ref`` overrides the
reference.

Send FX sweeps (tests/regression/scenarios/dn2-sendfx.ts, ``sf-*``): the dry reference is
sf-dry-tap or sf-dry-hold, the read-back diff is against sf-<fx>-ref (send up,
page at defaults), the wet signal is the scenario minus the dry reference, and
the per-effect measures (delay echoes, reverb decay and onset, chorus
modulation) are printed as one table per effect.

Per scenario: basic checks and L-R; on the 0.1-0.3 s after the note onset
(the same window for every scenario) the distinct sample values, the
fundamental level, THD and the noise floor relative to it, and the RMS of the
difference to the dry reference; for delay runs the echo-only residual
(scenario minus dry): echo onsets, spacing and level ratios.
"""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import struct
import sys
from typing import Any

import numpy as np

from tools.coldfire import paramtable
from tools.emulator import audiocheck as ac
from tools.firmware import device as device_files

F0 = 261.6  # Trig 1 audition pitch, Hz
WIN = (0.1, 0.3)  # analysis window after the note onset, s


def ui(hexval: str | None) -> int | None:
    """UI value (high byte) of a peeked big-endian s16."""
    if not hexval or len(hexval) < 4:
        return None
    return int(hexval[:2], 16)


def onset(x: ac.F, rate: int) -> float:
    o = ac.onsets(x, rate)
    return o[0] if o else 0.0


def window(x: ac.F, rate: int, t0: float) -> ac.F:
    return x[int((t0 + WIN[0]) * rate) : int((t0 + WIN[1]) * rate)]


def tone_fit(w: ac.F, rate: int) -> dict[str, Any]:
    """Harmonics 1..10 of f0 with slowly varying amplitude (audiocheck
    tone_harmonics). Returns f0, the fundamental level (dBFS, peak sine), THD
    (dB re fundamental) and the residual (dB re fundamental): what is not a
    slowly modulated harmonic, i.e. noise, quantisation, aliasing."""
    h = ac.tone_harmonics(w, rate, F0)
    power: list[float] = h["power"]  # type: ignore[assignment]
    resid_ms: float = h["resid_ms"]  # type: ignore[assignment]
    p1 = power[0]
    db = lambda v: 10 * math.log10(max(v, 1e-30))  # noqa: E731
    return {
        "f0_hz": float(h["f0_hz"]),  # type: ignore[arg-type]
        "fund_db": db(2 * p1),
        "thd_db": db(sum(power[1:])) - db(p1),
        "resid_db": db(resid_ms) - db(p1),
        "resid_rms": math.sqrt(resid_ms),
        "harm_db": [round(db(v) - db(p1), 1) for v in power[1:9]],
    }


def align_idx(
    x: ac.F, ref: ac.F, rate: int, t0: float, r0: float
) -> tuple[int, int, int]:
    """Start indices in x and ref (20 ms before their onsets, shifted by the lag
    within +-3 ms that best matches the first 300 ms) and the common length."""
    pre = int(0.02 * rate)
    sa, sb = max(0, int(t0 * rate) - pre), max(0, int(r0 * rate) - pre)
    a, b = ac._mono(x)[sa:], ac._mono(ref)[sb:]
    n = int(0.3 * rate)
    lags = range(-int(0.003 * rate), int(0.003 * rate) + 1)
    lag = max(
        lags,
        key=lambda s: float(np.dot(a[pre + s : pre + s + n], b[pre : pre + n])),
    )
    sa, sb = sa + max(lag, 0), sb + max(-lag, 0)
    return sa, sb, min(len(x) - sa, len(ref) - sb)


def align(x: ac.F, ref: ac.F, rate: int, t0: float, r0: float) -> tuple[ac.F, ac.F]:
    """Mono copies of x and ref that start 20 ms before their onsets, shifted by
    the lag (+-3 ms) that best matches the first 300 ms, cut to equal length."""
    sa, sb, k = align_idx(x, ref, rate, t0, r0)
    return ac._mono(x)[sa : sa + k], ac._mono(ref)[sb : sb + k]


def align2(x: ac.F, ref: ac.F, rate: int, t0: float, r0: float) -> tuple[ac.F, ac.F]:
    """As align, keeping the channels."""
    sa, sb, k = align_idx(x, ref, rate, t0, r0)
    return x[sa : sa + k], ref[sb : sb + k]


def bit_features(
    x: ac.F, rate: int, t0: float, ref: ac.F | None, r0: float
) -> dict[str, object]:
    out: dict[str, object] = {}
    if ref is not None:
        a, b = align(x, ref, rate, t0, r0)
        s = slice(int((0.02 + WIN[0]) * rate), int((0.02 + WIN[1]) * rate))
        den = float(np.sqrt(np.mean(b[s] ** 2)))
        num = float(np.sqrt(np.mean((a[s] - b[s]) ** 2)))
        out["diff_db"] = 20 * math.log10(max(num / den, 1e-12)) if den else None
        out["diff_rms"] = num
        # Uniform quantiser error rms = step / sqrt(12); full scale is +-1.
        out["implied_bits"] = math.log2(2 / (num * math.sqrt(12))) if num else None
    w = window(x, rate, t0)
    out.update(ac.levels(w))
    out.update(tone_fit(w, rate))
    return out


def delay_features(x: ac.F, ref: ac.F, rate: int, t0: float, r0: float) -> dict:
    """Echo-only residual = x - ref after alignment. Onsets of its 5 ms envelope
    are echo starts (seconds after the note onset); the level of an echo is the
    peak envelope up to the next one."""
    a, b = align(x, ref, rate, t0, r0)
    res = (a - b)[:, None]
    t, e = ac.envelope(res, rate, 5.0)
    ons = ac.onsets(res, rate, 5.0, 6.0, 45.0, 0.1)
    bounds = [*ons[1:], float(t[-1]) + 1]
    levels_db = [
        float(e[(t >= p) & (t < q)].max()) for p, q in zip(ons, bounds, strict=False)
    ]
    return {
        "residual_peak_db": float(e.max()),
        "echo_onsets_s": [round(o - 0.02, 4) for o in ons],
        "spacing_s": [round(q - p, 4) for p, q in zip(ons, ons[1:], strict=False)],
        "echo_level_db": [round(v, 2) for v in levels_db],
        "echo_ratio_db": [
            round(q - p, 2) for p, q in zip(levels_db, levels_db[1:], strict=False)
        ],
    }


def diff_vs_dry(
    x: ac.F, ref: ac.F, rate: int, t0: float, r0: float, hold: float
) -> dict[str, object]:
    """Generic difference to the dry reference, split at note-off (hold seconds
    after the onset): peak 5 ms RMS level (dBFS) of x - ref while the note is
    held and after it, the dry level in the same spans, and the number of
    onsets in the tail difference. Below -90 dB counts as no difference."""
    a, b = align(x, ref, rate, t0, r0)
    pre = int(0.02 * rate)
    cut = pre + int(hold * rate)
    d = (a - b)[:, None]
    out: dict[str, object] = {"hold_s": hold}
    for key, sl in (("note", slice(pre, cut)), ("tail", slice(cut, None))):
        if len(d[sl]) < int(0.01 * rate):
            continue
        _, e = ac.envelope(d[sl], rate, 5.0)
        _, er = ac.envelope(b[sl][:, None], rate, 5.0)
        out[f"{key}_diff_db"] = float(e.max())
        out[f"{key}_dry_db"] = float(er.max())
    tail = d[cut:]
    out["tail_onsets"] = (
        len(ac.onsets(tail, rate, 5.0, 6.0, 45.0, 0.1)) if len(tail) > 100 else 0
    )
    tdb = out.get("tail_diff_db")
    out["tail_echo"] = bool(isinstance(tdb, float) and tdb > -90)
    return out


# --- scenario files, run loading ---------------------------------------------

SCENARIO_FILES: tuple[str, ...] = (
    "dn2-fx.json",
    "../regression/scenarios/dn2-trackfx.ts",
    "../regression/scenarios/dn2-sendfx.ts",
)
ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
DEFAULT_IMAGE = ROOT / "out" / "sections" / "dn2-1.12" / "section_3_MAIN_OS.bin"


def scenario_events() -> dict[str, list[dict]]:
    """Events of every scenario in the DN2 FX scenario files, by name."""
    out: dict[str, list[dict]] = {}
    for fn in SCENARIO_FILES:
        f = ROOT / "tests" / "scenarios" / fn
        if f.exists():
            from tools.emulator.coupled import scenario_document

            for s in scenario_document(f)["scenarios"]:
                out[s["name"]] = s["events"]
    return out


def hold_seconds(events: list[dict]) -> float | None:
    """Duration of the Trig 1 (button 25) press in a scenario's events."""
    down = None
    for e in events:
        if e.get("button") == 25:
            if e.get("down"):
                down = e["at_instr"]
            elif down is not None:
                return (e["at_instr"] - down) / 132e6
    return None


def load_run(d: pathlib.Path) -> dict[str, tuple[dict, pathlib.Path]]:
    report = json.loads((d / "report.json").read_text())
    out = {}
    for sc in report["scenarios"]:
        wav = d / f"audio-{sc['name']}.wav"
        out[sc["name"]] = (sc, wav if wav.exists() else d / "audio.wav")
    return out


def reference_for(name: str, hold: float | None) -> str:
    """Dry reference scenario of ``name`` (the Trig 1 hold decides tap or hold)."""
    held = (hold or 0) > 1
    if name.startswith("sf-"):
        return "sf-dry-hold" if held else "sf-dry-tap"
    if name.startswith("tf-"):
        if "-flt" in name:
            return "tf-dry-flt"
        return "tf-dry-hold" if held else "tf-dry-tap"
    return "fx-dry-tap"


def companions(name: str) -> list[str]:
    """Scenarios ``check`` also needs to analyse ``name``: the send page's
    reference (read-back diff) and the routing-0 partner of a '-rt1' run."""
    fx, param, _ = sf_name(name)
    out = []
    if fx in ("del", "rev", "chr") and param != "ref":
        out.append(f"sf-{fx}-ref")
    if name.startswith("tf-") and name.endswith("-rt1"):
        out.append(name.removesuffix("-rt1"))
    return out


# --- read-back by parameter name ----------------------------------------------

# Owners whose values live in the kit's Fx array; the owners of the Sound array
# that are the same for every machine (the others share indices by machine).
FX_OWNERS = {"LFO1", "LFO2", "LFO3", "Chorus", "Delay", "Reverb", "Ext-in", "Master"}
SOUND_OWNERS = {"Filter", "Amp", "FX", "Portamento"}
SND_PEEK = "snd=*0x800052a0+0x48:202"
FX_PEEK = "fx=*0x800052a4+0:202"
BR_IDX, DSEND_IDX = 95, 86  # Sound array indices of Bit Reduction and Delay Send


def configure(device: str) -> None:
    """Switch the module to a device: DN2 (the default) or DT2 1.17, whose track-1
    Sound array is at *0x8000470c+0x14 and Fx array at *0x80004708 (finding 02),
    with the 1 kHz card sample instead of the 261.6 Hz audition sine."""
    global F0, SCENARIO_FILES, DEFAULT_IMAGE, SND_PEEK, FX_PEEK, BR_IDX, DSEND_IDX
    if device == "dt2":
        F0 = 1000.0
        SCENARIO_FILES = (
            "../regression/scenarios/dt2-trackfx.ts",
            "../regression/scenarios/dt2-sendfx.ts",
        )
        DEFAULT_IMAGE = ROOT / "out" / "sections" / "dt2-1.17" / "section_3_MAIN_OS.bin"
        SND_PEEK = "snd=*0x8000470c+0x14:142"
        FX_PEEK = "fx=*0x80004708+0:142"
        BR_IDX, DSEND_IDX = 64, 55
    elif device == "dn2":
        F0 = 261.6
        SCENARIO_FILES = (
            "dn2-fx.json",
            "../regression/scenarios/dn2-trackfx.ts",
            "../regression/scenarios/dn2-sendfx.ts",
        )
        DEFAULT_IMAGE = ROOT / "out" / "sections" / "dn2-1.12" / "section_3_MAIN_OS.bin"
        SND_PEEK = "snd=*0x800052a0+0x48:202"
        FX_PEEK = "fx=*0x800052a4+0:202"
        BR_IDX, DSEND_IDX = 95, 86
    else:
        raise ValueError(device)


def name_maps(image: str | pathlib.Path) -> tuple[dict[int, str], dict[int, str]]:
    """(Sound array, Fx array) mirror index -> 'Owner Long Name' from the
    descriptor table of a MAIN OS image (finding 02). Machine-specific Sound
    indices are left unnamed: several machines share them."""
    snd: dict[int, str] = {}
    fx: dict[int, str] = {}
    for e in paramtable.descriptors(image):
        if e.mirror_index < 0 or not e.long_name:
            continue
        label = f"{e.owner_name} {e.long_name}"
        if e.owner_name in FX_OWNERS:
            fx.setdefault(e.mirror_index, label)
        elif e.owner_name in SOUND_OWNERS:
            snd.setdefault(e.mirror_index, label)
    return snd, fx


def decode_arrays(
    pk: dict[str, str], names: tuple[dict[int, str], dict[int, str]]
) -> dict[str, int]:
    """Peeked Sound and Fx arrays as {'snd[95] FX Bit Reduction': s16, ...}."""
    out: dict[str, int] = {}
    for key, nm in (("snd", names[0]), ("fx", names[1])):
        hx = pk.get(key)
        if not hx:
            continue
        vals = struct.unpack(">%dh" % (len(hx) // 4), bytes.fromhex(hx))
        for i, v in enumerate(vals):
            out[f"{key}[{i}] {nm.get(i, '?')}"] = v
    return out


def readback_diff(vals: dict[str, int], ref: dict[str, int]) -> dict[str, str]:
    """Parameters whose stored value differs from the reference's, as
    {'snd[95] FX Bit Reduction': '0x0000 -> 0x2c00 (UI 44)'}."""
    return {
        k: f"{ref.get(k, 0) & 0xFFFF:#06x} -> {v & 0xFFFF:#06x} (UI {(v & 0xFFFF) >> 8})"
        for k, v in vals.items()
        if ref.get(k) != v
    }


def snd_ui(vals: dict[str, int], idx: int) -> int | None:
    for k, v in vals.items():
        if k.startswith(f"snd[{idx}] "):
            return (v & 0xFFFF) >> 8
    return None


# --- per-control measures (tests/regression/scenarios/dn2-trackfx.ts) ------------------------

HOLD_FAMILIES = ("br", "over", "srr")
SEND_FAMILIES = ("del", "rev", "chr")


def tf_name(name: str) -> tuple[str | None, int | None, bool]:
    """('br', 44, False) for 'tf-br-44'; routing runs end in '-rt1'."""
    p = name.split("-")
    if p[0] != "tf" or len(p) < 3 or not p[2].isdigit():
        return None, None, False
    return p[1], int(p[2]), name.endswith("-rt1")


def db10(v: float) -> float:
    return 10 * math.log10(max(v, 1e-30))


def hold_measures(
    fam: str, x: ac.F, rate: int, t0: float, refw: dict[str, Any]
) -> dict[str, Any]:
    """Held-note controls: harmonics (OVER), alias images (SRR), level change."""
    w = window(x, rate, t0)
    h = ac.tone_harmonics(w, rate, F0)
    power: list[float] = h["power"]  # type: ignore[assignment]
    p1 = power[0]
    harm = [round(db10(v) - db10(p1), 1) for v in power[1:9]]  # 2nd..9th
    out: dict[str, Any] = {
        "fund_change_db": db10(p1) - db10(refw["p1"]),
        "harm_db": harm,
        "h3_db": harm[1],
        "h5_db": harm[3],
        "odd_minus_even_db": float(np.mean(harm[1::2]) - np.mean(harm[0::2])),
    }
    fr, sp = ac.spectrum(w, rate)
    lin = 10 ** (sp / 10)
    edges = (0, 1000, 3000, 6000, 12000, 18000, 24001)
    out["band_db"] = {
        f"{lo // 1000}-{hi // 1000}k": round(
            db10(float(lin[(fr >= lo) & (fr < hi)].sum())), 1
        )
        for lo, hi in zip(edges, edges[1:], strict=False)
    }
    if fam == "srr":
        res: ac.F = h["residual"]
        out["fs_candidates"] = [
            (round(f), round(sc, 1))
            for f, sc in ac.image_search(res[:, None], rate, F0)
        ]
        fr, sp = ac.spectrum(res[:, None], rate)
        out["resid_peaks"] = [
            (round(f, 1), round(v, 1)) for f, v in ac.peaks(sp, fr, 6)
        ]
    return out


def echo_levels(e: ac.F, hop_s: float, t_echo: float, n: int = 8) -> list[float]:
    """Mean-square level (dB) of the envelope e (dB, hop_s per value) 0.05-0.15 s
    after each multiple k * t_echo, k = 1..n (inside the k-th echo's plateau)."""
    out = []
    for k in range(1, n + 1):
        a, b = int((k * t_echo + 0.07) / hop_s), int((k * t_echo + 0.17) / hop_s)
        if b >= len(e):
            break
        out.append(round(db10(float(np.mean(10 ** (e[a:b] / 10)))), 2))
    return out


def send_measures(
    fam: str, x: ac.F, ref: ac.F, rate: int, t0: float, r0: float, hold: float
) -> dict[str, Any]:
    """Wet part of a send run = x - ref (same events, so aligned to the lag)."""
    a, b = align2(x, ref, rate, t0, r0)
    res = a - b
    t, e = ac.envelope(res, rate, 5.0)
    hop = 0.005
    pk = float(e.max())
    pre = 0.02
    out: dict[str, Any] = {
        "wet_peak_db": pk,
        "wet_rms_db": db10(float(np.mean(res**2))),
        "dry_rms_db": db10(float(np.mean(b**2))),
        "first_wet_s": None,
    }
    up = np.flatnonzero(e > pk - 20)
    if len(up):
        out["first_wet_s"] = round(float(t[up[0]]) - pre, 3)
    if fam == "del":
        sp, corr = ac.autocorr_spacing(e, hop, 0.1, 1.5, rises=True)
        out["spacing_s"] = None if math.isnan(sp) else round(sp, 4)
        out["spacing_corr"] = round(corr, 2)
        t_echo = out["first_wet_s"] or sp
        if t_echo and not math.isnan(t_echo):
            lv = echo_levels(e, hop, t_echo + pre)
            out["echo_level_db"] = lv
            out["echo_ratio_db"] = [
                round(q - p, 2) for p, q in zip(lv, lv[1:], strict=False)
            ]
    elif fam == "rev":
        k = int(np.argmax(e))
        tail = res[k * int(hop * rate) :]
        rt = ac.decay_time(tail, rate)
        out["rt60_s"] = None if math.isnan(rt) else round(rt, 2)
        off = pre + hold
        out["level_after_off_db"] = {
            f"{dt:g}s": round(float(e[int((off + dt) / hop)]), 1)
            for dt in (0.5, 1.0, 2.0, 3.0)
            if int((off + dt) / hop) < len(e)
        }
        out["tail_end_db"] = round(float(e[-20:].mean()), 1)
    elif fam == "chr":
        mx, rel = ac.lr_diff(res)
        out["wet_lr_rel_db"] = None if not math.isfinite(rel) else round(rel, 1)
        out["dry_lr_identical"] = bool(np.array_equal(b[:, 0], b[:, 1]))
        keep = np.flatnonzero(e > pk - 30)
        seg = res[int(keep[0] * hop * rate) : int((keep[-1] + 1) * hop * rate)]
        out["audible_s"] = round(len(seg) / rate, 2)
        out.update(
            {f"mod_{k}": v for k, v in ac.modulation_rate(seg, rate, F0).items()}
        )
    out["_res"] = res
    return out


def tf_measures(
    name: str,
    x: ac.F,
    rate: int,
    t0: float,
    ref: ac.F,
    r0: float,
    hold: float,
    refw: dict[str, Any],
) -> dict[str, Any]:
    fam, ui_, rt = tf_name(name)
    out: dict[str, Any] = {"family": fam, "setting": ui_, "routing_1": rt}
    same = x.shape == ref.shape
    out["identical_to_ref"] = bool(same and np.array_equal(x, ref))
    out["max_abs_diff_to_ref"] = float(np.abs(x - ref).max()) if same else None
    if fam in HOLD_FAMILIES:
        out.update(hold_measures(fam, x, rate, t0, refw))
    elif fam in SEND_FAMILIES:
        out.update(send_measures(fam, x, ref, rate, t0, r0, hold))
    return out


def routing_compare(
    x: ac.F, other: ac.F, rate: int, t0: float, hold: float
) -> dict[str, Any]:
    """x against the same run with the routing at 0: the difference's RMS (dB re
    the other run's RMS in the same span) in the attack (first 50 ms), the
    steady note, the release and the end of the run, and where it peaks."""
    n = min(len(x), len(other))
    d = (x[:n] - other[:n]).mean(axis=1)
    o = other[:n].mean(axis=1)
    spans = {
        "attack": (t0, t0 + 0.05),
        "steady": (t0 + 0.1, t0 + hold),
        "release": (t0 + hold, t0 + hold + 0.3),
        "end": (t0 + hold + 0.3, n / rate),
    }
    out: dict[str, Any] = {
        "max_abs_diff": float(np.abs(d).max()),
        "shape_equal": x.shape == other.shape,
        "full_max_abs_diff": float(np.abs(x - other).max())
        if x.shape == other.shape
        else None,
    }
    for k, (a, b) in spans.items():
        i, j = int(a * rate), int(b * rate)
        if j - i < 50:
            continue
        num, den = float(np.mean(d[i:j] ** 2)), float(np.mean(o[i:j] ** 2))
        out[f"{k}_diff_db"] = round(db10(num) - db10(den), 1) if den > 0 else None
    out["peak_diff_s"] = round(float(np.argmax(np.abs(d))) / rate - t0, 3)
    return out


def plot_tf(x: ac.F, rate: int, t0: float, d: pathlib.Path, m: dict[str, Any]) -> None:
    """Spectrum of the note window (images marked for SRR) and, for sends, the
    wet envelope (x - dry) per channel."""
    marks: list[float] = []
    if m.get("fs_candidates"):
        marks = ac.alias_products(F0, float(m["fs_candidates"][0][0]))
    ac.plot_spectrum(
        window(x, rate, t0), rate, d / "spectrum.png", marks, f"spectrum {d.name}"
    )
    res = m.get("_res")
    if res is not None:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt

        fig, ax = plt.subplots(figsize=(10, 3))
        for c, lab in enumerate("LR"):
            tt, ee = ac.envelope(res[:, c : c + 1], rate, 5.0)
            ax.plot(tt - 0.02, ee, lw=0.8, label=lab)
        ax.set(xlabel="s after onset", ylabel="dB FS", title="wet envelope (x - dry)")
        ax.legend()
        ax.grid(alpha=0.3)
        fig.savefig(d / "wet_envelope.png", dpi=100, bbox_inches="tight")
        plt.close(fig)


# --- send FX pages (tests/regression/scenarios/dn2-sendfx.ts) --------------------------------

PRE = 0.02  # alignment pre-roll before the note onset, s


def sf_name(name: str) -> tuple[str | None, str | None, int | None]:
    """('del', 'fb', -127) for 'sf-del-fb-d127'; (fx, 'ref', None) for the
    reference, (None, None, None) for anything else."""
    p = name.split("-")
    if p[0] != "sf" or len(p) < 3:
        return None, None, None
    if len(p) == 4 and p[3][:1] in "ud" and p[3][1:].isdigit():
        return p[1], p[2], int(p[3][1:]) * (1 if p[3][0] == "u" else -1)
    return p[1], p[2], None


def sf_measures(
    fx: str, x: ac.F, dry: ac.F, rate: int, t0: float, r0: float, hold: float
) -> dict[str, Any]:
    """Wet signal = x - dry (aligned): level, onset and the effect's measures."""
    a, b = align2(x, dry, rate, t0, r0)
    res = a - b
    hop = 0.005
    t, e = ac.envelope(res, rate, hop * 1000)
    pk = float(e.max())
    off = PRE + hold
    out: dict[str, Any] = {
        "wet_peak_db": round(pk, 1),
        "wet_rms_db": round(db10(float(np.mean(res**2))), 1),
        "wet_onset30_s": round(ac.first_above(t, e, 30) - PRE, 3),
        "tail_end_db": round(float(e[-20:].mean()), 1),
        "_res": res,
    }
    out["wet_l_db"] = round(db10(float(np.mean(res[:, 0] ** 2))), 1)
    out["wet_r_db"] = round(db10(float(np.mean(res[:, -1] ** 2))), 1)
    mx, rel = ac.lr_diff(res)
    out["wet_lr_rel_db"] = round(rel, 1) if math.isfinite(rel) else None
    if res.shape[1] > 1 and float(res.std()) > 0:
        out["wet_corr"] = round(float(np.corrcoef(res[:, 0], res[:, 1])[0, 1]), 3)
    if fx == "del":
        t_echo = out["wet_onset30_s"]
        sp, corr = ac.autocorr_spacing(e, hop, 0.1, 2.5, rises=True)
        out["spacing_s"] = None if math.isnan(sp) else round(sp, 4)
        out["spacing_corr"] = round(corr, 2)
        n = 6
        starts = [PRE + k * t_echo + 0.1 for k in range(1, n + 1)]
        ln = max(0.05, min(0.15, t_echo - 0.1))
        seg = ac.segment_stats(res, rate, starts, ln)
        lv = [r["rms_db"] for r in seg]
        keep = [i for i, v in enumerate(lv) if math.isfinite(v) and v > pk - 55]
        out["echo_rms_db"] = [round(v, 1) for v in lv]
        out["echo_lr_db"] = [
            None if math.isnan(r["lr_db"]) else round(r["lr_db"], 1) for r in seg
        ]
        out["echo_centroid_hz"] = [
            round(r["centroid_hz"]) if keep and i in keep else None
            for i, r in enumerate(seg)
        ]
        steps = [lv[i + 1] - lv[i] for i in keep[:-1] if i + 1 in keep]
        out["db_per_repeat"] = round(float(np.mean(steps)), 2) if steps else None
        out["n_echoes"] = len(keep)
        if t_echo > 0.36:
            g0, g1 = PRE + t_echo + 0.32, PRE + 2 * t_echo - 0.02
            if g1 - g0 > 0.03:
                out["between_echo_db"] = round(
                    float(np.mean(e[int(g0 / hop) : int(g1 / hop)])), 1
                )
    elif fx == "rev":
        k = int(np.argmax(e))
        rt = ac.decay_time(res[k * int(hop * rate) :], rate)
        out["rt60_s"] = None if math.isnan(rt) else round(rt, 2)
        below = np.flatnonzero((t > off + 0.2) & (e < pk - 45))
        t1 = float(t[below[0]]) if len(below) else float(t[-1])
        rs = ac.rt60_slope(t, e, off + 0.2, t1)
        out["rt60_slope_s"] = None if math.isnan(rs) else round(rs, 2)
        out["onset35_s"] = round(ac.first_above(t, e, 35) - PRE, 3)
        out["onset25_s"] = round(ac.first_above(t, e, 25) - PRE, 3)
        out["level_after_off_db"] = {
            f"{dt:g}s": round(float(e[int((off + dt) / hop)]), 1)
            for dt in (0.3, 1.0, 2.0, 3.0)
            if int((off + dt) / hop) < len(e)
        }
        st = ac.segment_stats(res, rate, [off + 0.3], 1.0)[0]
        out["tail_centroid_hz"] = (
            None if math.isnan(st["centroid_hz"]) else round(st["centroid_hz"])
        )
        out["tail_bands_db"] = {
            k_: round(v, 1) for k_, v in st.items() if k_.startswith("band_")
        }
    elif fx == "mst":
        w, w0 = window(x, rate, t0), window(dry, rate, r0)
        h = ac.tone_harmonics(w, rate, F0)
        h0 = ac.tone_harmonics(w0, rate, F0)
        p: list[float] = h["power"]  # type: ignore[assignment]
        p0: list[float] = h0["power"]  # type: ignore[assignment]
        out["fund_change_db"] = round(db10(p[0]) - db10(p0[0]), 2)
        out["thd_db"] = round(db10(sum(p[1:])) - db10(p[0]), 1)
        out["rms_change_db"] = round(
            db10(float(np.mean(a**2))) - db10(float(np.mean(b**2))), 2
        )
        late = slice(int((PRE + 1.0) * rate), int((PRE + hold) * rate))
        early = slice(int((PRE + 0.05) * rate), int((PRE + 0.3) * rate))
        out["gain_early_vs_late_db"] = round(
            db10(float(np.mean(a[early] ** 2))) - db10(float(np.mean(a[late] ** 2))), 2
        )
    elif fx == "chr":
        keep = np.flatnonzero(e > pk - 30)
        sl = slice(int(keep[0] * hop * rate), int((keep[-1] + 1) * hop * rate))
        seg_ = res[sl]
        out["audible_s"] = round(len(seg_) / rate, 2)
        m = ac.modulation_rate(seg_, rate, F0)
        out["mod_phase_hz"] = round(m["phase_hz"], 2)
        out["mod_phase_dev_rad"] = round(m["phase_dev"], 2)
        for c, lab in enumerate("lr"):
            f = ac.inst_freq_dev(seg_[:, c], rate, F0)
            out[f"fm_{lab}"] = {k_: round(v, 2) for k_, v in f.items()}
        st = ac.segment_stats(seg_, rate, [0.0], len(seg_) / rate - 0.001)[0]
        out["wet_centroid_hz"] = (
            None if math.isnan(st["centroid_hz"]) else round(st["centroid_hz"])
        )
        out["wet_bands_db"] = {
            k_: round(v, 1) for k_, v in st.items() if k_.startswith("band_")
        }
    return out


def sf_delta(fx: str, res: ac.F, ref: ac.F, rate: int, hold: float) -> dict[str, Any]:
    """What a send parameter added to the effect: res - ref (both wet signals)."""
    n = min(len(res), len(ref))
    dl = res[:n] - ref[:n]
    hop = 0.005
    t, e = ac.envelope(dl, rate, hop * 1000)
    pk = float(e.max())
    off = PRE + hold
    out: dict[str, Any] = {
        "delta_rms_db": round(db10(float(np.mean(dl**2))), 1),
        "delta_peak_db": round(pk, 1),
        "delta_onset30_s": round(ac.first_above(t, e, 30) - PRE, 3),
        "delta_after_off_db": {
            f"{dt:g}s": round(float(e[int((off + dt) / hop)]), 1)
            for dt in (0.3, 1.0, 2.0, 3.0)
            if int((off + dt) / hop) < len(e)
        },
    }
    sp, corr = ac.autocorr_spacing(e, hop, 0.1, 2.5, rises=True)
    out["delta_spacing_s"] = None if math.isnan(sp) else round(sp, 4)
    out["delta_spacing_corr"] = round(corr, 2)
    below = np.flatnonzero((t > off + 0.2) & (e < pk - 45))
    t1 = float(t[below[0]]) if len(below) else float(t[-1])
    rs = ac.rt60_slope(t, e, off + 0.2, t1)
    out["delta_rt60_slope_s"] = None if math.isnan(rs) else round(rs, 2)
    st = ac.segment_stats(dl, rate, [off + 0.3], 1.0)[0]
    out["delta_tail_bands_db"] = {
        k: round(v, 1) for k, v in st.items() if k.startswith("band_")
    }
    return out


def sf_lines(res: dict[str, dict[str, Any]]) -> list[str]:
    """One block per effect: the swept parameter's read-back value and the key
    measures, in sweep order."""
    keys = {
        "del": (
            "wet_rms_db",
            "wet_onset30_s",
            "spacing_s",
            "n_echoes",
            "db_per_repeat",
            "echo_lr_db",
            "echo_centroid_hz",
            "between_echo_db",
            "tail_end_db",
            "wet_corr",
        ),
        "rev": (
            "wet_rms_db",
            "rt60_s",
            "rt60_slope_s",
            "onset35_s",
            "onset25_s",
            "level_after_off_db",
            "tail_centroid_hz",
            "tail_bands_db",
        ),
        "mst": ("fund_change_db", "rms_change_db", "thd_db", "gain_early_vs_late_db"),
        "chr": (
            "wet_rms_db",
            "wet_lr_rel_db",
            "wet_corr",
            "mod_phase_hz",
            "fm_l",
            "fm_r",
            "wet_centroid_hz",
            "wet_bands_db",
            "audible_s",
        ),
    }
    lines = []
    for n, r in res.items():
        m = r.get("sf")
        if not m:
            continue
        fx = sf_name(n)[0]
        rb = r.get("readback_vs_ref") or {}
        lines.append(f"{n}: readback {dict(rb)}")
        lines.append(
            "    " + ", ".join(f"{k}={m[k]}" for k in keys[fx or "del"] if k in m)
        )
    return lines


def monotone(v: list[float]) -> str:
    d = np.diff(v)
    if len(d) and bool(np.all(d >= -1e-9)):
        return "increasing"
    if len(d) and bool(np.all(d <= 1e-9)):
        return "decreasing"
    return "not monotone"


def series(res: dict[str, dict[str, Any]]) -> list[str]:
    """One line per family and measure: the values over the settings and whether
    they move monotonically."""
    lines = []
    for fam in (*HOLD_FAMILIES, *SEND_FAMILIES):
        rows = sorted(
            (
                (r["tf"]["setting"], r)
                for n, r in res.items()
                if r.get("tf", {}).get("family") == fam
                and not r["tf"]["routing_1"]
                and not n.endswith("-hold")
                and "-flt" not in n
                and r["tf"]["setting"]
            ),
            key=lambda t: t[0],
        )
        if len(rows) < 2:
            continue
        keys = {
            "br": [("bits", "diff_db"), ("bits", "thd_db"), ("bits", "resid_db")],
            "over": [("tf", "h3_db"), ("tf", "h5_db"), ("tf", "fund_change_db")],
            "srr": [("bits", "diff_db"), ("bits", "resid_db")],
            "del": [("tf", "wet_rms_db"), ("tf", "wet_peak_db")],
            "rev": [("tf", "wet_rms_db"), ("tf", "rt60_s")],
            "chr": [("tf", "wet_rms_db"), ("tf", "mod_phase_hz")],
        }[fam]
        for grp, key in keys:
            vals = [r[grp].get(key) for _, r in rows]
            if any(not isinstance(v, (int, float)) or v is None for v in vals):
                continue
            lines.append(
                f"{fam} {key} @ {[s for s, _ in rows]}: "
                f"{[round(v, 1) for v in vals]} {monotone(vals)}"
            )
        if fam == "srr":
            lines.append(
                f"srr fs' @ {[s for s, _ in rows]}: "
                f"{[r['tf']['fs_candidates'][:2] for _, r in rows]}"
            )
    return lines


def pcm_observations(x: ac.F, reference: ac.F | None, name: str) -> dict[str, Any]:
    """Exact NumPy comparisons for external oracle consumers; no signal approximation."""
    return {
        "shape": list(x.shape),
        "mono_equal": bool(np.array_equal(x[:, 0], x[:, 1])),
        "reference": name,
        "reference_shape": list(reference.shape) if reference is not None else None,
        "reference_max_abs_diff": float(np.abs(x - reference).max())
        if reference is not None and x.shape == reference.shape
        else None,
        "reference_equal": bool(np.array_equal(x, reference))
        if reference is not None
        else None,
    }


def json_measurements(value: Any) -> Any:
    """Undefined/nonfinite measurements are null at the JSON interchange boundary."""
    if isinstance(value, float) and not math.isfinite(value):
        return None
    if isinstance(value, dict):
        return {key: json_measurements(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [json_measurements(item) for item in value]
    return value


def check(
    runs: list[pathlib.Path],
    ref_name: str | None,
    plots: bool,
    image: pathlib.Path = DEFAULT_IMAGE,
) -> dict[str, dict[str, Any]]:
    loaded = {}
    for d in runs:
        for name, (sc, wav) in load_run(d).items():
            rate, x = ac.load(wav)
            loaded[name] = (d, sc, rate, x)
    names = name_maps(image) if image.exists() else None
    res: dict[str, dict[str, Any]] = {}
    evs = scenario_events()
    decoded: dict[str, dict[str, int]] = {}
    for name, (_d, sc, _rate, _x) in loaded.items():
        pk = sc.get("peek") or {}
        decoded[name] = decode_arrays(pk, names) if names else {}
    refs: dict[str, str] = {}
    for name in loaded:
        hold = hold_seconds(evs.get(name, []))
        refs[name] = ref_name or reference_for(name, hold)
    refw_cache: dict[str, dict[str, Any]] = {}
    for name, (d, sc, rate, x) in loaded.items():
        t0 = onset(x, rate)
        pk = sc.get("peek") or {}
        ref = loaded.get(refs[name])
        is_ref = name == refs[name]
        rx = ref[3] if ref and not is_ref else None
        r0 = onset(rx, rate) if rx is not None else 0.0
        pui = {k: ui(v) for k, v in pk.items() if k not in ("snd", "fx")}
        dec = decoded[name]
        if dec:
            pui.setdefault("br", snd_ui(dec, BR_IDX))
            pui.setdefault("dsend", snd_ui(dec, DSEND_IDX))
        row: dict[str, object] = {
            "run_dir": str(d),
            "peek_ui": pui,
            "analysis": ac.analyse(x, rate),
            "onset_s": t0,
            "decoded": dec,
            "pcm": pcm_observations(x, ref[3] if ref else None, refs[name]),
        }
        if dec and ref and not is_ref:
            row["readback_diff"] = readback_diff(dec, decoded[refs[name]])
        sfx = sf_name(name)[0]
        if dec and sfx and f"sf-{sfx}-ref" in decoded and name != f"sf-{sfx}-ref":
            row["readback_vs_ref"] = readback_diff(dec, decoded[f"sf-{sfx}-ref"])
        row["bits"] = bit_features(x, rate, t0, rx, r0)
        if (
            rx is not None
            and (pui.get("dsend") or 0) != 0
            and not name.startswith("tf-")
        ):
            row["delay"] = delay_features(x, rx, rate, t0, r0)
        hold = hold_seconds(evs.get(name, []))
        if rx is not None and hold:
            row["vs_dry"] = diff_vs_dry(x, rx, rate, t0, r0, hold)
        if name.startswith("tf-") and rx is not None and hold:
            rn = refs[name]
            if rn not in refw_cache:
                h = ac.tone_harmonics(window(rx, rate, r0), rate, F0)
                refw_cache[rn] = {"p1": h["power"][0]}  # type: ignore[index]
            m = tf_measures(name, x, rate, t0, rx, r0, hold, refw_cache[rn])
            part = loaded.get(name.removesuffix("-rt1"))
            if tf_name(name)[2] and part:
                m["vs_routing_0"] = routing_compare(x, part[3], rate, t0, hold)
            row["tf"] = m
            if plots:
                plot_tf(x, rate, t0, _mkdir(d / f"plots-{name}"), m)
        if (
            name.startswith("sf-")
            and rx is not None
            and hold
            and sfx in ("del", "rev", "chr", "mst")
        ):
            sm = sf_measures(sfx, x, rx, rate, t0, r0, hold)
            row["sf"] = sm
            if plots:
                plot_tf(x, rate, t0, _mkdir(d / f"plots-{name}"), sm)
        res[name] = row
        if plots:
            ac.plots(x, rate, _mkdir(d / f"plots-{name}"))
    for name, row in res.items():
        fx, param, _ = sf_name(name)
        ref_row = res.get(f"sf-{fx}-ref")
        if "sf" in row and param in ("rsend", "dsend") and ref_row and "sf" in ref_row:
            rate = loaded[name][2]
            sfm: dict[str, Any] = row["sf"]  # type: ignore[assignment]
            refm: dict[str, Any] = ref_row["sf"]
            row["delta"] = sf_delta(
                fx or "",
                sfm["_res"],
                refm["_res"],
                rate,
                hold_seconds(evs.get(name, [])) or 0.3,
            )
    for d in runs:
        mine = {k: v for k, v in res.items() if v["run_dir"] == str(d)}
        for v in mine.values():
            if "tf" in v:
                v["tf"].pop("_res", None)
            if "sf" in v:
                v["sf"].pop("_res", None)
        (d / "fxcheck.json").write_text(
            json.dumps(json_measurements(mine), indent=2, default=str, allow_nan=False)
            + "\n"
        )
    return res


def _mkdir(p: pathlib.Path) -> pathlib.Path:
    p.mkdir(exist_ok=True)
    return p


def table(res: dict[str, dict[str, Any]]) -> str:
    cols = (
        "scenario  br dsend peak   rms    dc     L-R(dB) levels  "
        "fund    thd  resid  diff  bits"
    )
    lines = [cols]
    for name, r in res.items():
        a: dict = r["analysis"]
        b: dict = a["basic"]
        p: dict = r["peek_ui"]
        k: dict = r["bits"]
        lr = a["lr_diff"]["rel_db"]
        diff = k.get("diff_db")
        lines.append(
            f"{name:<14}{p.get('br')!s:>3}{p.get('dsend')!s:>6} "
            f"{b['peak']:.3f} {b['rms']:.4f} {max(map(abs, b['dc'])):.0e} "
            f"{'identical' if lr is None else f'{lr:7.1f}':>9} "
            f"{k['n_levels']:>6.0f} "
            f"{k['fund_db']:6.1f} {k['thd_db']:6.1f} {k['resid_db']:6.1f} "
            f"{'-' if diff is None else f'{diff:5.1f}'} "
            f"{'-' if k.get('implied_bits') is None else f'{k["implied_bits"]:4.1f}'}"
            f"{' NONFINITE' if b['nonfinite'] else ''}{' GROWTH' if b['growth'] else ''}"
        )
    for name, r in res.items():
        d = r.get("delay")
        if d:
            p = r["peek_ui"]
            lines.append(
                f"{name}: time {p.get('dtime')} fb {p.get('dfb')} mix {p.get('dmix')} "
                f"echoes at {d['echo_onsets_s']} s, spacing {d['spacing_s']}, "
                f"levels {d['echo_level_db']} dB, ratios {d['echo_ratio_db']} dB"
            )
    for name, r in res.items():
        v = r.get("vs_dry")
        if v:
            lines.append(
                f"{name}: vs dry note {v.get('note_diff_db', float('nan')):.1f} dB "
                f"(dry {v.get('note_dry_db', float('nan')):.1f}), tail "
                f"{v.get('tail_diff_db', float('nan')):.1f} dB "
                f"(dry {v.get('tail_dry_db', float('nan')):.1f}), tail onsets "
                f"{v['tail_onsets']}, echo {'YES' if v['tail_echo'] else 'no'}"
            )
    for name, r in res.items():
        rd = r.get("readback_diff")
        if rd is not None:
            lines.append(
                f"{name}: changed {dict(rd)}"
                if rd
                else f"{name}: no stored parameter differs"
            )
    for name, r in res.items():
        m = r.get("tf")
        if m:
            show = {
                k: v for k, v in m.items() if k not in ("family", "setting", "_res")
            }
            lines.append(f"{name}: {show}")
    lines += sf_lines(res)
    lines += series(res)
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("runs", nargs="+", type=pathlib.Path)
    ap.add_argument(
        "--ref", default=None, help="dry reference scenario (default by name)"
    )
    ap.add_argument("--device", choices=device_files.shorts(), default="dn2")
    ap.add_argument("--image", type=pathlib.Path, default=None)
    ap.add_argument("--no-plots", action="store_true")
    a = ap.parse_args(argv)
    configure(a.device)
    print(table(check(a.runs, a.ref, not a.no_plots, a.image or DEFAULT_IMAGE)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
