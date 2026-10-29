#!/usr/bin/env python3
"""patchkit proof: the PLUCK machine end to end on Digitakt II (core/dsp-voice + core/machines + core/params +
test/patched-pluck), in the emulator, through the panel.

    uv run python -m tools.patchkit.pk_proof_pluck [--fw 1.17] [--out DIR] [--only a,b,c,d,e,f,g] [--quick] [--jobs N]

Drives one patched image per part on coupled daemons through the harness (`tools/patchkit/pk_harness.py` Session: panel inputs,
health asserted after every input) and reads back:

  a  select    FUNC+SRC lists PLUCK (8th entry), YES commits type 9 (its permanent number; 7 and 8 are gap rows, not listed). The machine's descriptor, read from the running
               firmware, names the five rows (descriptor field -> relocated parameter table row -> name pointer): TUNE
               and LEV are SAMPLE's rows, DECAY, PICK, BRIGHT and COLOR the module's. After the first trig the engine
               slots hold the module's defaults (and the TX frame words follow).
  b  knobs     encoders A..H on the SRC page change exactly the slots the descriptor maps them to (finding 02) in the
               Sound object and in the TX frame; the empty fields (D, G) change nothing. Control: stock SAMPLE.
  c  tuning    TRIG 1 and keyboard keys: the fundamental of the rendered note against the pitch word the ColdFire sent.
  d  params    DECAY, BRIGHT, COLOR, PICK set through the encoders change the sound the way the DSP routine documents:
               T60 (DECAY), spectral tilt (BRIGHT), excitation spectrum (COLOR) and the comb notch at the 4th harmonic
               (PICK = 1/2).
  e  sequencer four quarter-note trigs plus PLAY: four plucks.
  f  persist   save the project with edited PLUCK values, set the track to SAMPLE, load it: type 9 and the four values
               are back. The card's writes are laid over the card of the STOCK firmware: the track loads as SAMPLE.
  g  mod       MOD page, DEST knob: the SRC group lists PLUCK's own names (frames differ from stock SAMPLE's).

Parts are independent and run in parallel processes (`--jobs`, tools/patchkit/pk_proofkit.py; `--only b` names a group). The
full tier splits b (b1, b2, b3) and c (c1, c2) over several daemons and keeps d whole (its PICK measure depends on the
noise state the earlier notes leave); e plays its two runs side by side.

`--quick` (about 60-80 s wall, 4 parts in parallel): aq (select, descriptor, defaults, the DECAY encoder moves its slot
only, tuning of two notes), dq1 (DECAY: decay slope at two settings), dq2 (PICK: comb against a reset noise state) and
f (persistence). A sounding note costs about 6 s of wall per emulated second (before the code-watch fix of
findings 18: 35 s; the patch bars stock AOT regions), an idle second about 3 s, so the quick parts turn DECAY down right after the first trig, play 0.33 s notes and measure windows of
0.1-0.2 s. dq1 and dq2 reset the excitation noise (`_pk_rng`) before each note: the notes then differ only by the
parameter under test (with the parameter ignored they would be bit-identical). Teeth: tools/patchkit/pk_break.py makes broken
images (`PK_PROOF_IMAGE`); the limits and the margins are in the checks.

Output (PNG frames, WAV, card images) goes under OUT (default out/runs/pluck-proof) and is never committed. Exit 0 only
when every check passes. Addresses are the DT2 1.16/1.17 ones of findings 02 and 04.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.emulator import card_overlay  # noqa: E402
from tools.patchkit import hook_ab  # noqa: E402
from tools.patchkit import pk_dn2_common as C  # noqa: E402
from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proof_machines as M  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402
from tools.patchkit.pitch import f0_from_crossings  # noqa: E402

PLUCK_TYPE = 9  # core/machines types.toml, "test/patched-pluck:pluck" on dt2
MODS = ["core/dsp-voice", "core/machines", "core/params", "test/patched-pluck"]
PLUCK_DOWNS = (
    7  # presses of DOWN from the top of the machine list to PLUCK (the 8th entry)
)
# descriptor field -> (mirror index, encoder) of the SRC page, finding 02
SLOTS = {
    0: (25, 1),
    1: (26, 2),
    2: (27, 3),
    3: (28, 4),
    4: (31, 5),
    5: (32, 6),
    6: (33, 7),
    7: (34, 8),
}
# module defaults of the DK_PARAM declarations in tests/fixtures/modules/patched-pluck/src/module.c
DEFAULTS = {25: 0x4000, 26: 0x4000, 27: 0x2000, 31: 0x5000, 32: 0x2800, 34: 0x6400}
DECAY, PICK, BRIGHT, COLOR = 26, 27, 31, 32
ENC = {DECAY: 2, PICK: 3, BRIGHT: 5, COLOR: 6}
PITCH_WORD = (
    0x80005348  # TX frame word 0x02 of track 1 sits in the low half of this long
)
T1_BASE = {
    "1.17": 0x4020F59C,
}  # stock table; patched images read it back from the code


def semitone_hz(note: float) -> float:
    return 440.0 * 2 ** ((note - 69) / 12)


# ----- analysis -------------------------------------------------------------------------------------------------


def left_of(pcm: np.ndarray) -> np.ndarray:
    return pcm[0::2].astype(np.float64)


def onset(left: np.ndarray, thresh: float = 1e-4) -> int:
    act = np.flatnonzero(np.abs(left) > thresh)
    return int(act[0]) if len(act) else -1


def f0_of(left: np.ndarray, start: int, want_hz: float, n: int = 30000) -> float:
    return f0_from_crossings(left[start + 300 : start + 300 + n], want_hz)


def t60_of(left: np.ndarray, start: int, drop_db: float = 30.0) -> float | None:
    """T60 from the slope of the 512-sample RMS (dB) over the first DROP_DB of decay after the peak."""
    n = (len(left) - start) // 512
    if n < 8:
        return None
    rms = np.array(
        [
            np.sqrt(np.mean(left[start + 512 * k : start + 512 * (k + 1)] ** 2))
            for k in range(n)
        ]
    )
    db = 20 * np.log10(np.maximum(rms, 1e-9))
    k0 = int(np.argmax(db))
    below = np.flatnonzero(db[k0:] < db[k0] - drop_db)
    k1 = k0 + (int(below[0]) if len(below) else len(db) - k0 - 1)
    if k1 - k0 < 3:
        return None
    t = (np.arange(k0, k1 + 1) * 512 + 256) / 48000.0
    slope = float(np.polyfit(t, db[k0 : k1 + 1], 1)[0])
    return -60.0 / slope if slope < 0 else None


def band_db(left: np.ndarray, a: int, n: int, lo: float, hi: float) -> float:
    """Level (dB) of the band LO..HI Hz in the window A..A+N."""
    x = left[a : a + n] * np.hanning(n)
    sp = np.abs(np.fft.rfft(x)) ** 2
    f = np.fft.rfftfreq(n, 1 / 48000.0)
    return 10 * math.log10(float(np.sum(sp[(f >= lo) & (f < hi)])) + 1e-30)


def harmonic_db(left: np.ndarray, a: int, n: int, f0: float, k: int) -> float:
    """Level of the k-th harmonic: the strongest bin within +-2 % of k * f0 (the window leaks, the string detunes
    slightly with damping)."""
    return band_db(left, a, n, 0.98 * k * f0, 1.02 * k * f0)


def onsets(left: np.ndarray) -> list[int]:
    """Sample indexes of new plucks: the excitation is noise, so the energy above the 2nd-difference corner jumps by
    more than 5 dB within 10 ms (a ringing string only decays there)."""
    h = np.diff(left, 2)
    w = 480
    e = np.array([np.sum(h[i : i + w] ** 2) for i in range(0, len(h) - w, w)])
    out: list[int] = []
    for i in range(1, len(e)):
        if (
            e[i] > 1e-4
            and e[i] > 3.2 * max(e[i - 1], 1e-12)
            and (not out or i * w - out[-1] > 4800)
        ):
            out.append(i * w)
    return out


# ----- the proof ------------------------------------------------------------------------------------------------


class PluckProof(M.Proof):
    tool = "pluck"

    def pluck(self, s: H.Session) -> None:
        """A fresh start with PLUCK selected on track 1 and one trig, so the engine slots hold their defaults."""
        self.fresh(s)
        self.select(s, PLUCK_DOWNS)
        s.do_hold("TRIG 1", 200)
        s.run_ms(300)

    def play(
        self,
        s: H.Session,
        name: str,
        key: str = "TRIG 1",
        hold: int = 300,
        total: int = 2200,
    ) -> tuple[np.ndarray, int]:
        """Hold KEY with PCM capture; returns (left channel, pitch word read 100 ms into the note)."""
        assert s.emu is not None
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        s.emu.button(H.button(key), True)
        s.run_ms(100)
        pitch = (s.emu.peek32(PITCH_WORD) or 0) & 0xFFFF
        s.run_ms(hold - 100)
        s.emu.button(H.button(key), False)
        s.run_ms(total - hold)
        s.emu.call("pcm.tap", action="capture_stop")
        s.health(f"play {name}")
        return left_of(hook_ab.read_wav(wav)), pitch

    def descriptor_names(self, s: H.Session) -> list[tuple[str, str]]:
        """(long name, short name) of the nine fields of PLUCK's descriptor, read from the running firmware: the
        machines registry row and the relocated table are found through the plan's symbols, each field is a row of the
        table, its name pointers are C strings."""
        assert s.emu is not None and s.image is not None
        plan = json.loads(Path(str(s.image)[: -len(".syx")] + ".plan.json").read_text())
        sym = {k.split(":", 1)[1]: v for k, v in plan["symbols"].items()}
        row = (
            sym["machines.rows"]
            + (sym["machines.pluck.index"] - sym["machines.first"])
            * sym["machines.row_size"]
        )
        desc, t1 = row, sym["pf_table"]
        peek = s.emu.peek32

        def cstr(addr: int) -> str:
            raw = b"".join(
                (peek(addr + i) or 0).to_bytes(4, "big") for i in range(0, 40, 4)
            )
            return raw.split(b"\0")[0].decode("ascii", "replace")

        out = []
        for n in range(9):
            code = peek(desc + 8 + 4 * n) or 0
            if code == 0:
                out.append(("", ""))
                continue
            row = t1 + 60 * code
            out.append((cstr(peek(row + 0x28) or 0), cstr(peek(row + 0x30) or 0)))
        return out

    def reseed(self, s: H.Session, seed: int = 0x2545F491) -> None:
        """Quick tier: reset the pluck excitation's noise state (module word _pk_rng) so that notes are repeatable."""
        assert s.image is not None and s.emu is not None
        plan = json.loads(Path(str(s.image)[: -len(".syx")] + ".plan.json").read_text())
        addr = next(v for k, v in plan["symbols"].items() if k.endswith(":_pk_rng"))
        s.emu.patch(text=f"DM {addr:x} {seed:08x}")

    # -- quick tier. A sounding PLUCK note costs ~6 s of wall per emulated second while it rings (35 s before the
    # code-watch fix, findings 18), an idle second ~3 s: every quick part turns DECAY down right after the first trig (that
    # shortens the ringing note too) and measures windows of 0.15-0.2 s.
    QUICK_DECAY = 0x1800  # T60 well under 0.26 s (0x2000 is 0.26 s in the full tier); a note costs ~3 s of wall

    def quick_start(self, s: H.Session, decay: int | None = None) -> None:
        """PLUCK selected on track 1, the SRC page open, one trig (the engine slots get their defaults) and DECAY
        turned down at once: the first note rings at the default 1 s T60 otherwise (several seconds of wall)."""
        self.fresh(s, 600)
        self.select(s, PLUCK_DOWNS)
        s.do_tap("SRC")
        s.run_ms(200)
        s.do_hold("TRIG 1", 40)
        s.run_ms(60)
        s.turn_burst(ENC[DECAY], -4, 60)  # to the bottom (T60 << 0.1 s) in 0.2 s
        if decay is not None:
            s.turn_to(ENC[DECAY], DECAY, decay)

    def part_aq(self) -> None:
        """Quick: select and type, descriptor rows, defaults, the DECAY encoder moves its slot only and an empty field
        none, tuning of TRIG 1 and one keyboard key (0.2 s windows)."""
        with self.session(MODS) as s:
            self.fresh(s, 600)
            self.select(s, PLUCK_DOWNS)
            t = self.type_now(s)
            self.check(
                f"PLUCK commits type {PLUCK_TYPE}",
                t == PLUCK_TYPE,
                f"track object type {t}",
            )
            names = self.descriptor_names(s)
            self.check(
                "the descriptor's rows carry the module's names",
                [n[1] for n in names[1:3] + names[4:6]]
                == ["DECAY", "PICK", "BRIGHT", "COLOR"]
                and [n[0] for n in names[1:3] + names[4:6]]
                == ["Decay", "Pick position", "Brightness", "Excitation colour"]
                and names[3] == ("", "")
                and names[6] == ("", ""),
                str(names),
            )
            s.do_tap("SRC")
            s.run_ms(200)
            s.do_hold("TRIG 1", 40)
            s.run_ms(60)
            got = {i: s.mirror(i) for i in DEFAULTS}
            self.check(
                "engine slots hold the module's defaults",
                got == DEFAULTS,
                {i: hex(v) for i, v in got.items()},
            )
            tx = {i: s.tx_word(i) for i in DEFAULTS}
            self.check("the TX frame carries them", tx == DEFAULTS, "")
            # an empty field (encoder 4, field D) changes nothing; DECAY (encoder 2) changes its slot only
            before = {i: s.mirror(i) for i, _ in SLOTS.values()}
            s.turn_burst(
                ENC[DECAY], -4, 60
            )  # also ends the first note (default T60 1 s) early
            s.turn_to(ENC[DECAY], DECAY, self.QUICK_DECAY)
            after = {i: s.mirror(i) for i in before}
            changed = sorted(i for i in before if after[i] != before[i])
            self.check(
                "encoder 2 moves slot [26] only",
                changed == [DECAY],
                f"changed {changed}",
            )
            # tuning: 0.2 s windows. 3 cents at 262 Hz over 0.2 s is a 1.6 ms drift over 52 cycles; the crossing
            # estimator's error is printed (the full tier measures 0.6 s with the same limit)
            before = {i: s.mirror(i) for i, _ in SLOTS.values()}
            s.turn_burst(4, -4, 12)
            s.turn_burst(4, 4, 12)
            after = {i: s.mirror(i) for i in before}
            self.check("encoder 4 (empty field) moves no slot", after == before, "")
            for name, key, want in (
                ("TRIG 1", "TRIG 1", 0x3C00),
                ("keyboard TRIG 5", "TRIG 5", None),
            ):
                if key == "TRIG 5":
                    s.do_tap("KEYBOARD")
                    s.run_ms(300)
                left, pitch = self.play(
                    s, f"aq-{key.replace(' ', '').lower()}", key, hold=100, total=330
                )
                st = onset(left)
                if want is not None:
                    self.check(
                        f"{name} plays note 60",
                        pitch == want,
                        f"pitch word {pitch:#06x}",
                    )
                if st < 0:
                    self.check(f"{name}: sounds", False, f"pitch word {pitch:#06x}")
                    continue
                note = pitch / 256
                f = f0_of(left, st, semitone_hz(note), 4000)
                cents = 1200 * math.log2(f / semitone_hz(note))
                self.check(
                    f"{name}: note {note:g}, f0 {f:.3f} Hz",
                    abs(cents) < 3,
                    f"{cents:+.4f} cents",
                )
            tx_type, dsp_type = self.wire(s)
            self.check(
                f"TX frame machine word and DSP type cache are {PLUCK_TYPE}",
                tx_type == PLUCK_TYPE and dsp_type == PLUCK_TYPE,
                f"{tx_type} {dsp_type}",
            )
            s.health("end of aq")

    # -- a: select and defaults
    def part_a(self) -> None:
        with self.session(MODS) as s:
            self.fresh(s)
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(200)
            for _ in range(PLUCK_DOWNS):
                s.do_tap("DOWN")
                s.run_ms(80)
            self.snap(s, "a-menu")
            s.do_tap("YES")
            s.run_ms(300)
            s.do_tap("YES")
            s.run_ms(300)
            t = self.type_now(s)
            self.check(
                f"PLUCK commits type {PLUCK_TYPE}",
                t == PLUCK_TYPE,
                f"track object type {t}",
            )
            names = self.descriptor_names(s)
            print("     descriptor fields:", names)
            self.check(
                "the descriptor's rows carry the module's names",
                [n[1] for n in names[1:3] + names[4:6]]
                == ["DECAY", "PICK", "BRIGHT", "COLOR"]
                and [n[0] for n in names[1:3] + names[4:6]]
                == ["Decay", "Pick position", "Brightness", "Excitation colour"]
                and names[3] == ("", "")
                and names[6] == ("", ""),
                str(names),
            )
            s.do_hold("TRIG 1", 200)
            s.run_ms(300)
            s.do_tap("SRC")
            s.run_ms(500)
            self.snap(s, "a-src")
            got = {i: s.mirror(i) for i in DEFAULTS}
            self.check(
                "engine slots hold the module's defaults",
                got == DEFAULTS,
                {i: hex(v) for i, v in got.items()},
            )
            tx = {i: s.tx_word(i) for i in DEFAULTS}
            self.check(
                "the TX frame carries them",
                tx == DEFAULTS,
                {i: hex(v) for i, v in tx.items()},
            )
            tx_type, dsp_type = self.wire(s)
            self.check(
                f"TX frame machine word and DSP type cache are {PLUCK_TYPE}",
                tx_type == PLUCK_TYPE and dsp_type == PLUCK_TYPE,
                f"{tx_type} {dsp_type}",
            )
            s.health("end of a")

    # -- b: knobs
    def part_b(
        self,
        labels: tuple[str, ...] = ("pluck", "sample"),
        fields: tuple[int, ...] = tuple(SLOTS),
    ) -> None:
        for label, mods, downs in (("pluck", MODS, PLUCK_DOWNS), ("sample", [], 0)):
            if label not in labels:
                continue
            with self.session(mods) as s:
                self.fresh(s)
                if downs:
                    self.select(s, downs)
                s.do_hold("TRIG 1", 200)
                s.run_ms(300)
                s.do_tap("SRC")
                s.run_ms(500)
                for field_n, (idx, enc) in SLOTS.items():
                    if field_n not in fields:
                        continue
                    if label == "sample" and field_n in (1, 2, 3):
                        continue  # PLAY and SAMP are modes and CFADE is not on the stock SRC page
                    before = {i: s.mirror(i) for i, _ in SLOTS.values()}
                    s.turn_burst(
                        enc, -4, 12
                    )  # down first: LEV, TUNE and the knobs at the top would clip
                    low = {i: s.mirror(i) for i, _ in SLOTS.values()}
                    s.turn_burst(enc, 4, 40)
                    high = {i: s.mirror(i) for i, _ in SLOTS.values()}
                    changed = sorted(
                        i for i in before if low[i] != before[i] or high[i] != before[i]
                    )
                    s.do_hold("TRIG 1", 100)  # a frame carries the values to the DSP
                    s.run_ms(100)
                    tx_changed = [i for i in changed if s.tx_word(i) == high[i]]
                    empty = label == "pluck" and field_n in (3, 6)
                    want = [] if empty else [idx]
                    self.check(
                        f"{label}: encoder {enc} moves slot(s) {want}",
                        changed == want and (not changed or tx_changed == changed),
                        f"changed {changed}, in the TX frame {tx_changed}",
                    )
                s.health(f"end of b {label}")

    # -- c: tuning
    def part_c(
        self,
        trig1: bool = True,
        keys: tuple[str, ...] = ("TRIG 2", "TRIG 3", "TRIG 5", "TRIG 13", "TRIG 16"),
        total: int = 2200,
    ) -> None:
        with self.session(MODS) as s:
            self.pluck(s)
            if trig1:
                left, pitch = self.play(s, "c-trig1", "TRIG 1", total=total)
                st = onset(left)
                note = pitch / 256
                self.check(
                    "TRIG 1 plays note 60", pitch == 0x3C00, f"pitch word {pitch:#06x}"
                )
                f = f0_of(left, st, semitone_hz(note))
                cents = 1200 * math.log2(f / semitone_hz(note))
                self.check(
                    f"note {note:g}: f0 {f:.3f} Hz",
                    abs(cents) < 3,
                    f"{cents:+.4f} cents",
                )
            s.do_tap("KEYBOARD")
            s.run_ms(300)
            for key in keys:
                left, pitch = self.play(
                    s, f"c-{key.replace(' ', '').lower()}", key, total=total
                )
                st = onset(left)
                if st < 0:
                    self.check(
                        f"{key}: sounds", False, f"silent, pitch word {pitch:#06x}"
                    )
                    continue
                note = pitch / 256
                f = f0_of(left, st, semitone_hz(note))
                cents = 1200 * math.log2(f / semitone_hz(note))
                self.check(
                    f"{key}: note {note:g}, f0 {f:.3f} Hz",
                    abs(cents) < 3,
                    f"{cents:+.4f} cents",
                )

    # -- d: parameter effects
    def part_d(
        self,
        sections: tuple[str, ...] = ("decay", "bright", "color", "pick"),
        quick: bool = False,
    ) -> None:
        with self.session(MODS) as s:
            if quick:
                self.quick_start(s)
            else:
                self.pluck(s)
                s.do_tap("SRC")
                s.run_ms(300)

            def setp(idx: int, value: int) -> int:
                return s.turn_to(ENC[idx], idx, value)

            def note(name: str, total: int = 2200) -> tuple[np.ndarray, int]:
                if quick:
                    self.reseed(
                        s
                    )  # the same excitation noise for every note: only the parameter differs
                left, _ = self.play(
                    s, name, "TRIG 1", hold=100 if quick else 300, total=total
                )
                return left, onset(left)

            if quick:
                setp(DECAY, self.QUICK_DECAY)  # shortens the ringing first note
            reached = setp(BRIGHT, 0x7F00)
            self.check("BRIGHT set to its top", reached >= 0x7C00, f"{reached:#x}")
            if "decay" in sections and quick:
                # quick: the slope of the decay (dB/s of the 512-sample RMS, 0.05 s after the peak to 0.25 s) at two
                # DECAY settings. Full tier: T60 0.26 s and 1.02 s (0x2000, 0x4000), so the slopes differ by 3.9x;
                # 0x3000 gives about 2x. The check wants the longer setting's slope to be less than 0.75 of the short's.
                slope = {}
                for label, value in (("short", 0x1800), ("mid", 0x2800)):
                    setp(DECAY, value)
                    left, st = note(f"d-decay-{label}", 330)
                    slope[label] = C.db_slope(left, st)
                    print(f"     DECAY {value:#x}: {slope[label]:.1f} dB/s")
                ratio = slope["mid"] / slope["short"]
                self.check(
                    "DECAY lengthens the note (slope at 0x2800 < 0.75 x slope at 0x1800)",
                    ratio < 0.75,
                    f"{slope['short']:.1f} dB/s -> {slope['mid']:.1f} dB/s, ratio {ratio:.2f}",
                )
                setp(DECAY, self.QUICK_DECAY)
            elif "decay" in sections:
                res = {}
                for label, value, total in (
                    ("short", 0x2000, 2200),
                    ("mid", 0x4000, 3200),
                    ("long", 0x5000, 6000),
                )[: 2 if quick else 3]:
                    setp(DECAY, value)
                    left, st = note(f"d-decay-{label}", total)
                    res[label] = t60_of(left, st)
                    print(f"     DECAY {value:#x}: T60 {res[label]}")
                self.check(
                    "DECAY lengthens the note (T60 short < mid < long)",
                    all(res.values())
                    and all(
                        a < b
                        for a, b in zip(
                            list(res.values()), list(res.values())[1:], strict=False
                        )
                    ),
                    str({k: round(v, 3) if v else v for k, v in res.items()}),
                )
                want_mid = 1.02
                self.check(
                    "DECAY 0x4000 with BRIGHT at the top gives T60 about 1.02 s",
                    res["mid"] is not None and abs(res["mid"] / want_mid - 1) < 0.2,
                    f"{res['mid']}",
                )
                setp(DECAY, 0x4000)
            if "bright" in sections:
                # brightness: energy above 3 kHz 0.2 s into the note, relative to the note's body
                hf = {}
                for label, value in (("dark", 0x0800), ("bright", 0x7F00)):
                    setp(BRIGHT, value)
                    left, st = note(f"d-bright-{label}")
                    hf[label] = band_db(left, st + 9600, 4096, 3000, 12000) - band_db(
                        left, st + 9600, 4096, 100, 1000
                    )
                    print(f"     BRIGHT {value:#x}: HF/LF {hf[label]:.1f} dB")
                self.check(
                    "BRIGHT raises the high frequencies late in the note",
                    hf["bright"] - hf["dark"] > 6,
                    f"{hf['bright'] - hf['dark']:.1f} dB",
                )
            if "color" in sections:
                # colour: the excitation's lowpass; the first 4096 samples after the onset
                setp(BRIGHT, 0x7F00)
                col = {}
                for label, value in (("white", 0x0000), ("soft", 0x7F00)):
                    setp(COLOR, value)
                    left, st = note(f"d-color-{label}")
                    col[label] = band_db(left, st + 200, 4096, 4000, 20000) - band_db(
                        left, st + 200, 4096, 100, 1000
                    )
                    print(f"     COLOR {value:#x}: HF/LF {col[label]:.1f} dB")
                self.check(
                    "COLOR (excitation lowpass) lowers the high frequencies",
                    col["white"] - col["soft"] > 6,
                    f"{col['white'] - col['soft']:.1f} dB",
                )
            if "pick" in sections:
                # pick position: plucked at the fraction x = PICK / 4 of the string the harmonics m scale as |sin(pi m x)|
                # (the comb y[k] - y[k - p], p = x N); at PICK = 1/2 (x = 1/4) the 2nd and 6th are at their peak, the 4th and
                # 8th in a notch. The notch is as exact as p is an integer, so the measure sums two harmonics each.
                setp(COLOR, 0x2800)
                comb = {}
                for label, value in (("none", 0x0000), ("half", 0x3F80)):
                    setp(PICK, value)
                    left, st = note(f"d-pick-{label}", 330 if quick else 2200)
                    f0 = f0_of(left, st, semitone_hz(60), 4000 if quick else 30000)
                    a, n = (0, 4096) if quick else (400, 8192)
                    lev = {
                        m: 10 ** (harmonic_db(left, st + a, n, f0, m) / 10)
                        for m in range(1, 13 if quick else 9)
                    }
                    if quick:
                        comb[label] = 10 * math.log10(
                            (lev[2] + lev[6] + lev[10]) / (lev[4] + lev[8] + lev[12])
                        )
                    else:
                        comb[label] = 10 * math.log10(
                            (lev[2] + lev[6]) / (lev[4] + lev[8])
                        )
                    print(f"     PICK {value:#x}: (H2+H6)/(H4+H8) {comb[label]:.1f} dB")
                self.check(
                    "PICK = 1/2 notches the 4th and 8th harmonics",
                    # quick tier: the excitation noise is reset before each note (`reseed`), so the two notes differ
                    # only by the comb (with PICK ignored they would be bit-identical: difference 0 dB). H2+H6+H10
                    # against H4+H8+H12 in the first 4096 samples. Measured over three seeds 8.3 .. 10.9 dB;
                    # the limit is 4 dB. The full tier (decay 1 s, 8192 samples at 400) has half > 10 dB and > 8 dB
                    # over none, with the engine's own noise state.
                    (comb["half"] - comb["none"] > 4)
                    if quick
                    else (comb["half"] > 10 and comb["half"] - comb["none"] > 8),
                    f"{comb['half']:.1f} dB against {comb['none']:.1f} dB without",
                )
            s.health("end of d")

    # -- e: sequencer
    def sequence_run(self, name: str, place: bool) -> np.ndarray:
        """PLUCK on track 1; with PLACE, grid recording (RECORD) puts a trig on step 1 (the same waits without); PLAY
        for 3.6 s with PCM capture (the pattern is 2.35 s long)."""
        with self.session(
            MODS, f"seq-{name}", pool=False
        ) as s:  # edits the pattern: own daemon
            self.fresh(s)
            self.select(s, PLUCK_DOWNS)
            if place:
                s.do_tap("RECORD")
            s.run_ms(300)
            if place:
                s.do_tap("TRIG 1")
            s.run_ms(300)
            if place:
                s.do_tap("RECORD")
            s.run_ms(1500)  # the audition of the trig dies away
            wav = self.out / f"{name}.wav"
            assert s.emu is not None
            s.emu.call("pcm.tap", action="capture", path=str(wav))
            s.do_tap("PLAY")
            s.run_ms(3600)
            s.emu.call("pcm.tap", action="capture_stop")
            s.do_tap("STOP")
            s.run_ms(300)
            s.health(f"sequence {name}")
            return left_of(hook_ab.read_wav(wav))

    # -- e: sequencer
    def part_e(self) -> None:
        # the factory pattern plays the other tracks too: the same run without the trig is the baseline, the engine
        # is deterministic, so the difference is track 1
        base, full = self.parallel(  # two daemons side by side
            lambda: self.sequence_run("e-seq-base", place=False),
            lambda: self.sequence_run("e-seq", place=True),
        )
        n = min(len(base), len(full))
        diff = full[:n] - base[:n]
        on = onsets(diff)
        self.check(
            "the sequencer plucks the string on both passes of the pattern",
            len(on) >= 2 and 2.0 < (on[1] - on[0]) / 48000 < 2.7,
            f"onsets at {[round(o / 48000, 3) for o in on]} s",
        )
        if on:
            f = f0_of(diff, on[0], semitone_hz(60))
            self.check(
                "the first sounds at note 60",
                abs(1200 * math.log2(f / semitone_hz(60))) < 3,
                f"{f:.3f} Hz",
            )

    # -- f: persistence
    def part_f(self) -> None:
        overlay = self.out / "card.overlay"
        want = {DECAY: 0x2000, BRIGHT: 0x6000}
        with self.session(
            MODS, "persist", pool=False
        ) as s:  # saves a project: own daemon
            if self.quick:
                self.quick_start(s)
            else:
                self.pluck(s)
                s.do_tap("SRC")
                s.run_ms(300)
            for idx, v in want.items():
                s.turn_to(ENC[idx], idx, v)
            keep = {i: s.mirror(i) for i in DEFAULTS}
            self.save_project(s)
            self.select(s, 0, ups=PLUCK_DOWNS + 2)  # SAMPLE
            self.check(
                "track set to SAMPLE", self.type_now(s) == 0, f"{self.type_now(s)}"
            )
            self.load_project(s, lambda: self.type_now(s) == PLUCK_TYPE)
            self.check(
                f"project reload brings type {PLUCK_TYPE} back",
                self.type_now(s) == PLUCK_TYPE,
                f"{self.type_now(s)} after {self.load_s:g} s",
            )
            if self.quick:
                # the project is still loading for a moment after the type is back: trig until the slots are
                for _ in range(8):
                    s.do_hold("TRIG 1", 40)
                    s.run_ms(260)
                    if {i: s.mirror(i) for i in DEFAULTS} == keep:
                        break
            else:
                s.do_hold("TRIG 1", 200)
                s.run_ms(300)
            back = {i: s.mirror(i) for i in DEFAULTS}
            self.check(
                "and the engine slot values",
                back == keep,
                f"{ {i: hex(v) for i, v in back.items()} } vs { {i: hex(v) for i, v in keep.items()} }",
            )
            s.health("reloaded")
            r = s.emu.call("card.overlay", path=str(overlay))
            self.check(
                "card writes exported", r["sectors"] > 0, f"{r['sectors']} sectors"
            )
            card = s.card
        types = card_overlay.sound_types(overlay.read_bytes())
        self.check(
            f"the saved project stores type {PLUCK_TYPE} in its sound records",
            types.get(PLUCK_TYPE, 0) > 0,
            f"{types}",
        )
        base = Path(card or "")
        out_img = self.out / "card-after-save.img"
        if not base.is_file():
            self.check("card image found", False, str(base))
            return
        n = card_overlay.apply(base, overlay, out_img)
        self.check("card image with the saved project written", n > 0, f"{n} sectors")
        with self.session([], "saved-card", pool=False) as s:
            s.card = str(out_img)
            s.load(s.syx, settle=False)
            s.emu.drain_events()
            s.run_ms(300)
            s.emu.pcm_start()
            s.pcm_mark = 0
            s.health("stock with the saved card")
            self.load_project(s)
            t = self.type_now(s)
            self.check(
                "STOCK loads the project: track 1 is SAMPLE", t == 0, f"type {t}"
            )
            s.run_ms(500)
            s.do_hold("TRIG 1", 300)
            s.run_ms(300)
            s.health("stock after loading the patched project")

    # -- g: modulation destination picker
    def part_g(self) -> None:
        with self.session([]) as s:
            stock = self.dest_frames(s, 0, upto=40)
        with self.session(MODS) as s:
            got = self.dest_frames(s, PLUCK_DOWNS, upto=40)
            self.snap(s, "g-mod")
        self.check(
            "PLUCK's destination list differs from SAMPLE's (its own names)",
            got != stock,
            f"{sum(a != b for a, b in zip(got, stock, strict=False))} of {len(stock)} frames differ",
        )


P = PluckProof
PARTS = [
    K.Part("a", "select: type, descriptor rows, defaults", P.part_a, False, 60),
    # b: the knobs. The full tier splits it in three daemons; the quick tier checks DECAY and an empty field
    K.Part(
        "b1",
        "knobs on the SRC page: PLUCK fields 0-3",
        lambda p: p.part_b(("pluck",), (0, 1, 2, 3)),
        False,
        100,
        group="b",
    ),
    K.Part(
        "b2",
        "knobs on the SRC page: PLUCK fields 4-7",
        lambda p: p.part_b(("pluck",), (4, 5, 6, 7)),
        False,
        100,
        group="b",
    ),
    K.Part(
        "b3",
        "knobs on the SRC page: stock SAMPLE as the control",
        lambda p: p.part_b(("sample",)),
        False,
        100,
        group="b",
    ),
    K.Part(
        "aq",
        "quick: select, defaults, DECAY and an empty encoder, tuning of two notes",
        P.part_aq,
        True,
        50,
        full=False,
    ),
    # c: tuning
    K.Part(
        "c1",
        "tuning: TRIG 1, keyboard TRIG 2 and 3",
        lambda p: p.part_c(True, ("TRIG 2", "TRIG 3")),
        False,
        150,
        group="c",
    ),
    K.Part(
        "c2",
        "tuning: keyboard TRIG 5, 13 and 16",
        lambda p: p.part_c(False, ("TRIG 5", "TRIG 13", "TRIG 16")),
        False,
        150,
        group="c",
    ),
    # d: the whole chain in one daemon (the pick measure depends on the noise state the earlier notes leave)
    K.Part(
        "d",
        "parameters set through the encoders change the sound",
        P.part_d,
        False,
        450,
    ),
    K.Part(
        "dq1",
        "parameters: DECAY (two notes)",
        lambda p: p.part_d(("decay",), True),
        True,
        100,
        full=False,
        group="d",
    ),
    K.Part(
        "dq2",
        "parameters: PICK comb (two notes)",
        lambda p: p.part_d(("pick",), True),
        True,
        100,
        full=False,
        group="d",
    ),
    K.Part("e", "sequencer: a trig on step 1 plucks the string", P.part_e, False, 200),
    K.Part(
        "f", "persistence (patched save and load, then stock load)", P.part_f, True, 150
    ),
    K.Part("g", "MOD page destination list", P.part_g, False, 120),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--fw", default="1.17", choices=("1.17",))
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or (
        ROOT
        / "out"
        / "runs"
        / ("pluck-proof" if a.fw == "1.17" else f"pluck-proof-{a.fw}")
    )
    M.FW = a.fw
    return K.run_tool(__file__, PluckProof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
