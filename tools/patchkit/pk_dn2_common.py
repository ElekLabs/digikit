#!/usr/bin/env python3
"""Shared pieces of the Digitone II machine proofs (tools/patchkit/pk_proof_pages_dn2.py, tools/patchkit/pk_proof_pluck_dn2.py).

Small on purpose: a session on the DN2 1.12 ready inputs, panel helpers (select a machine, hold a key, turn an encoder
until a voice-slot word of the TX frame reads a value), the voice-slot word read-back and the audio measures of the
PLUCK proofs (fundamental, T60, band levels). The proofs are independent parts with a quick mode so they can move onto
a shared proof library later.

Voice slots (finding 06, "DN2 voice slots"): the ColdFire allocates the 16 slots round robin from 0 after a restart, and
the TX frame (0x80005e60) carries, for slot v, the pitch word at 0x02 + 2 v, the velocity word at 0x34 + 2 v and the
sound-array words (slots 25..65) from 0xda + 0x92 v. A word follows a knob only while the voice's key is held.
"""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path
from typing import Any

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.patchkit import hook_ab  # noqa: E402
from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402
from tools.patchkit.pitch import f0_from_crossings  # noqa: E402

READY = ROOT / "out" / "ready-fastweb-dn2"  # Digitone II 1.12 (intc_force false)
TX = 0x80005E60
MIRROR = 0x8000487C  # the audio side's copy of track 1's sound at +0xde: the type is the first byte
SLOTS = 16


def semitone_hz(note: float) -> float:
    return 440.0 * 2 ** ((note - 69) / 12)


# ----- audio measures (the PLUCK proof of the Digitakt, on a 48 kHz left channel) ----------------------------------


def left_of(pcm: np.ndarray) -> np.ndarray:
    return pcm[0::2].astype(np.float64)


def onset(left: np.ndarray, thresh: float = 1e-4) -> int:
    act = np.flatnonzero(np.abs(left) > thresh)
    return int(act[0]) if len(act) else -1


def f0_of(left: np.ndarray, start: int, want_hz: float, n: int = 30000) -> float:
    return f0_from_crossings(left[start + 300 : start + 300 + n], want_hz)


def db_slope(left: np.ndarray, start: int, a: float = 0.03, b: float = 0.15) -> float:
    """Decay rate (dB/s, positive) of the 512-sample RMS from A to B seconds after the onset."""
    k0, k1 = int(a * 48000 // 512), int(b * 48000 // 512)
    rms = np.array(
        [
            np.sqrt(np.mean(left[start + 512 * k : start + 512 * (k + 1)] ** 2))
            for k in range(k0, k1 + 1)
        ]
    )
    db = 20 * np.log10(np.maximum(rms, 1e-9))
    t = (np.arange(k0, k1 + 1) * 512 + 256) / 48000.0
    return -float(np.polyfit(t, db, 1)[0])


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


# ----- the proof base --------------------------------------------------------------------------------------------


class Dn2Proof(K.Proof):
    """The DN2 1.12 proofs on the shared library (tools/patchkit/pk_proofkit.py): sessions come from `Proof.session` (pooled,
    shared boot cache), parts run in parallel processes."""

    device = "dn2"
    fw = "1.12"
    ready_env = {"DIGI_PK_READY_DN2_112": str(READY)}

    def __init__(self, *args: Any, **kwargs: Any):
        super().__init__(*args, **kwargs)
        self.slot = 0  # the voice slot the next note will use (round robin from 0 after a restart)

    def fresh(self, s: H.Session) -> None:
        s.reset()
        s.run_ms(1500)
        self.slot = 0

    def snap(self, s: H.Session, name: str) -> str:
        """Save the frame as PNG; return its hash."""
        from tools.control.digi_client import oled_png

        assert s.emu is not None
        _, raw = s.emu.frame()
        (self.out / f"{name}.png").write_bytes(oled_png(raw))
        return hashlib.sha256(raw).hexdigest()[:12]

    def select(self, s: H.Session, downs: int) -> None:
        """FUNC+SRC, DOWN x downs (the cursor starts on the track's machine), YES (commits), YES (closes)."""
        s.do_tap(["FUNC", "SRC"])
        s.run_ms(200)
        for _ in range(downs):
            s.do_tap("DOWN")
            s.run_ms(80)
        s.do_tap("YES")
        s.run_ms(300)
        s.do_tap("YES")
        s.run_ms(300)

    # -- DSP memory of the module (the plan's symbols)
    def dsp_sym(self, s: H.Session, name: str) -> int:
        """The address of a module symbol (for example `_dkstate_pluck`) from the plan of the patched image."""
        assert s.image is not None
        plan = json.loads(Path(str(s.image)[: -len(".syx")] + ".plan.json").read_text())
        for k, v in plan["symbols"].items():
            if k.split(":", 1)[1] == name:
                return int(v)
        raise KeyError(name)

    def dsp_words(self, s: H.Session, addr: int, n: int) -> np.ndarray:
        assert s.emu is not None
        raw = s.emu.dsp_peek(addr, 4 * n)["hex"].replace("??", "00")
        return np.frombuffer(bytes.fromhex(raw), dtype="<u4")

    # -- voice slot words
    def word(self, s: H.Session, v: int, slot: int) -> int:
        """The TX frame halfword of sound-array slot SLOT (25..65) of voice slot V."""
        return s._u16(TX + 0xDA + 0x92 * v + 2 * (slot - 25))

    def pitch_word(self, s: H.Session, v: int) -> int:
        return s._u16(TX + 0x02 + 2 * v)

    def wire_type(self, s: H.Session) -> int:
        return ((s.emu.peek32(MIRROR) or 0) >> 24) & 0xFF if s.emu else -1

    def hold(self, s: H.Session, key: str) -> int:
        """Press KEY (a note starts); returns the voice slot it uses."""
        assert s.emu is not None
        v = self.slot
        self.slot = (self.slot + 1) % SLOTS
        s.emu.button(H.button(key), True)
        s.run_ms(80)
        return v

    def release(self, s: H.Session, key: str) -> None:
        assert s.emu is not None
        s.emu.button(H.button(key), False)
        s.run_ms(50)

    def turn_held(
        self, s: H.Session, enc: str, slot: int, target: int, tol: int = 0x300
    ) -> int:
        """Hold TRIG 1 and turn ENC until the held voice's word of SLOT is within TOL of TARGET (the knob sets the
        track's sound; the word follows while the note is held). Returns the word reached."""
        v = self.hold(s, "TRIG 1")
        got = self.word(s, v, slot)
        for _ in range(80):
            err = target - got
            if abs(err) <= tol:
                break
            s.turn_burst(enc, 4 if err > 0 else -4, 3 if abs(err) < 0x1000 else 8)
            got = self.word(s, v, slot)
        self.release(s, "TRIG 1")
        return got

    def play(
        self,
        s: H.Session,
        name: str,
        key: str = "TRIG 1",
        hold: int = 300,
        total: int = 2200,
    ) -> tuple[np.ndarray, int, int]:
        """Hold KEY with PCM capture; returns (left channel, pitch word 100 ms into the note, voice slot)."""
        assert s.emu is not None
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        v = self.slot
        self.slot = (self.slot + 1) % SLOTS
        s.emu.button(H.button(key), True)
        s.run_ms(100)
        pitch = self.pitch_word(s, v)
        s.run_ms(hold - 100)
        s.emu.button(H.button(key), False)
        s.run_ms(total - hold)
        s.emu.call("pcm.tap", action="capture_stop")
        s.health(f"play {name}")
        return left_of(hook_ab.read_wav(wav)), pitch, v

    # -- project save / load (card writes stay with the daemon)
    def save_project(self, s: H.Session) -> None:
        s.do_tap(["FUNC", "SETTINGS"])
        s.run_ms(300)
        s.do_tap("YES")  # slot 001
        s.run_ms(500)
        s.do_tap("YES")  # the name, save
        s.run_ms(3000)

    def load_project(self, s: H.Session) -> None:
        """SETTINGS, PROJECT, LOAD PROJECT, slot 001, 'Any changes will be lost. Proceed?' yes."""
        s.do_tap("SETTINGS")
        s.run_ms(400)
        for _ in range(3):
            s.do_tap("YES")
            s.run_ms(500)
        s.do_tap("YES")
        s.run_ms(5000)
        s.health("project loaded")
        self.slot = 0

    def menu_row(self, s: H.Session, name: str) -> int:
        """The machine menu reopened, then closed: the visible row whose marker is the big filled dot (the track's
        machine); -1 when none is found."""
        from PIL import Image

        for _ in range(3):  # the first FUNC+SRC after a commit can be swallowed
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(400)
            self.snap(s, name)
            im = np.asarray(Image.open(self.out / f"{name}.png").convert("L")) > 127
            top = im[0:36, 60:440]
            if top.mean() > 0.55 and not im[40:60, 0:60].mean() > 0.9:
                break
        s.do_tap("NO")
        s.run_ms(300)
        best, row = 0, -1
        for k, y in enumerate((57, 90, 122, 154, 186)):
            box = im[y - 14 : y + 14, 440:475]
            n = int(min(box.sum(), box.size - box.sum()))
            if n > best:
                best, row = n, k
        return row if best > 60 else -1
