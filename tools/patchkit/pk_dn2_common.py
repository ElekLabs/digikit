#!/usr/bin/env python3
"""Shared pieces of the Digitone II machine proofs (tools/patchkit/pk_proof_pages_dn2.py).

Small on purpose: a session on the DN2 1.12 ready inputs, panel helpers (select a machine, hold a key) and the voice-slot
word read-back.

Voice slots (finding 06, "DN2 voice slots"): the ColdFire allocates the 16 slots round robin from 0 after a restart, and
the TX frame (0x80005e60) carries, for slot v, the pitch word at 0x02 + 2 v, the velocity word at 0x34 + 2 v and the
sound-array words (slots 25..65) from 0xda + 0x92 v. A word follows a knob only while the voice's key is held.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402

READY = ROOT / "out" / "ready-fastweb-dn2"  # Digitone II 1.12 (intc_force false)
TX = 0x80005E60
MIRROR = 0x8000487C  # the audio side's copy of track 1's sound at +0xde: the type is the first byte
SLOTS = 16


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

    # -- voice slot words
    def word(self, s: H.Session, v: int, slot: int) -> int:
        """The TX frame halfword of sound-array slot SLOT (25..65) of voice slot V."""
        return s._u16(TX + 0xDA + 0x92 * v + 2 * (slot - 25))

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
