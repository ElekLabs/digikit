#!/usr/bin/env python3
"""patchkit Phase 3 proof: core/machines on Digitakt II 1.17 (test/machine-clone, a clone of SLICE).

    uv run python -m tools.patchkit.pk_proof_machines [--fw 1.17] [--out DIR] [--only a,b] [--quick] [--jobs N]

Drives patched images on coupled daemons through the harness (`tools/patchkit/pk_harness.py` Session: panel inputs, health
asserted after every input) and reads back:

  a  select      FUNC+SRC lists the machine, YES commits: the track object's type byte (0x4265338e, the word at
                 0x4265338c), the TX frame's machine word (0x80005348 + 0x94) and the DSP's cache (DM 0x255970) all
                 carry the type number; the SRC page opens. PCM of an audition of XClone equals stock MANUAL SLICE
                 (the DSP selector word of the clone is copied, finding 02 "core.machines").
  b  persist     save the project with an XClone track, set the track to SAMPLE, load the project: the type is back.
                 The card's writes are exported (`card.overlay`), laid over the card image (`tools/emulator/card_overlay.py`) and
                 the same project is loaded on the STOCK firmware: the track loads as SAMPLE, no fault.

  d  mod picker  the MOD page DEST list of XClone equals MANUAL SLICE's                          (full tier only)
  e  sequencer   PLAY: a step of the machine sounds like MANUAL SLICE
  b2 control     the persistence immediate put back: the type must not survive                  (full tier only)

Parts are independent (each builds its own references) and run in parallel processes (`--jobs`, tools/patchkit/pk_proofkit.py).
`--quick` runs a, b and e: select and type, sound, persistence, the sequencer. In
b the quick tier polls instead of waiting (the save until the card writes stop growing, the patched reload until the type
is back) and waits 3 s instead of 6 s for the stock load.

Output (PNG frames, WAV, card images) goes under OUT (default out/runs/machines-proof) and is never committed.
Exit 0 only when every check passes. Addresses are the DT2 1.16/1.17 ones of finding 02 and 04.
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools import python_env  # noqa: E402
from tools.emulator import card_overlay  # noqa: E402
from tools.firmware import config  # noqa: E402
from tools.patchkit import hook_ab  # noqa: E402
from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402
from tools.patchkit.pk_proofkit import aligned_equal, analyse  # noqa: E402

TRACK_OBJ_WORD = 0x4265338C  # the type byte is the third byte of this word (0x4265338e)
TX_TYPE = (
    0x800053DC  # TX frame 0x80005348 + 0x94: big-endian word, track 1 in the high half
)
DSP_CACHE = 0x255970  # DSP DM: a 16-bit type per track at +2t
SLICE_DOWNS = (
    4  # presses of DOWN from the top of the machine list to SLICE, XSLICE, XClone
)
XCLONE_DOWNS = 8  # the list position of XClone
XCLONE_TYPE = (
    8  # "test/machine-clone:xclone" in modules/core/adapters/machines/types.toml
)
FW = "1.17"  # --fw
MODS_ONE = ["core/machines", "test/machine-clone"]


class Proof(K.Proof):
    tool = "machines"
    device = "dt2"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.fw = FW
        self._ref: dict[str, np.ndarray] = {}

    def fresh(self, s: H.Session, settle_ms: int = 1500) -> None:
        """Back to the ready state, with time for the sample slot to finish loading (SETTLE_MS: a synth machine needs
        no sample)."""
        s.reset()
        s.run_ms(settle_ms)

    def select(self, s: H.Session, downs: int, ups: int = 0) -> None:
        """FUNC+SRC, DOWN x downs (the cursor starts on the track's machine; UP x ups first), YES (commits, the
        menu stays), YES (closes)."""
        s.do_tap(["FUNC", "SRC"])
        s.run_ms(200)
        for key, n in (("UP", ups), ("DOWN", downs)):
            for _ in range(n):
                s.do_tap(key)
                s.run_ms(80)
        s.do_tap("YES")
        s.run_ms(300)
        s.do_tap("YES")
        s.run_ms(300)

    def type_now(self, s: H.Session) -> int:
        word = s.emu.peek32(TRACK_OBJ_WORD) or 0
        return (word >> 8) & 0xFF

    def audition(self, s: H.Session, name: str) -> np.ndarray:
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        s.do_hold("TRIG 1", 500)
        s.run_ms(500)
        s.emu.call("pcm.tap", action="capture_stop")
        s.health(f"audition {name}")
        return hook_ab.read_wav(wav)

    def wire(self, s: H.Session) -> tuple[int, int]:
        """(TX frame machine word of track 1, DSP cache word of track 1): meaningful after an audition."""
        tx = ((s.emu.peek32(TX_TYPE) or 0) >> 16) & 0xFFFF
        dsp = int.from_bytes(
            bytes.fromhex(s.emu.dsp_peek(DSP_CACHE, 2)["hex"]), "little"
        )
        return tx, dsp

    def same_sound(
        self, name: str, ref: np.ndarray, got: np.ndarray, label: str
    ) -> None:
        a = analyse(ref)
        ok, shift = aligned_equal(ref, got)
        self.check(
            f"{name}: PCM equals {label}",
            ok and a.get("silent", True) is not True,
            f"{a.get('active_samples')} active samples, shift {shift}",
        )

    def sequence(self, s: H.Session, downs: int, name: str) -> np.ndarray:
        """The machine on track 1, TRIG 1 (audition and step 1), then the sequencer plays the step for 2 s."""
        self.fresh(s)
        self.select(s, downs)
        s.do_tap("TRIG 1")
        s.run_ms(
            1600
        )  # the audition (a 1 s sample) is over before the sequencer retriggers the track
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        s.do_tap("PLAY")
        s.run_ms(2000)
        s.emu.call(
            "pcm.tap", action="capture_stop"
        )  # before STOP: the release after STOP is timing, not sound
        s.do_tap("STOP")
        s.run_ms(300)
        s.health(f"sequence {name}")
        return hook_ab.read_wav(wav)

    def dest_frames(self, s: H.Session, downs: int, upto: int = 36) -> list[str]:
        """MOD page 1, the DEST knob (encoder D) turned one detent at a time: the frame hash after each turn from
        the 22nd on, where the destination list (grouped by owner: META, SRC, ...) comes up."""
        self.fresh(s)
        self.select(s, downs)
        s.do_tap("MOD")
        s.run_ms(300)
        out = []
        for i in range(upto):
            s.do_turn("ENC D", 1)
            s.run_ms(150)
            if i >= 24:
                # the title bar (the top 3 of 8 pages of the OLED: 3 * 128 bytes) animates and differs by timing, and
                # the focus moves into the SRC group during the 23rd turn: compare the settled frames
                out.append(hashlib.sha256(s.frame()[1][384:]).hexdigest()[:12])
        return out

    # -- the parts
    def stock_slice(self) -> np.ndarray:
        """The reference every part compares with: stock MANUAL SLICE, audition of TRIG 1 (memoized per process)."""
        if "stock_slice" not in self._ref:
            with self.session([]) as s:
                self.fresh(s)
                self.select(s, SLICE_DOWNS)
                self._ref["stock_slice"] = self.audition(s, "stock-slice")
        return self._ref["stock_slice"]

    def part_a(self) -> None:
        pcm: dict[str, np.ndarray] = {}
        with self.session([]) as s:
            self.fresh(s)
            self.select(s, SLICE_DOWNS)
            self.check(
                "stock: SLICE is type 6", self.type_now(s) == 6, f"{self.type_now(s)}"
            )
            pcm["stock_slice"] = self._ref["stock_slice"] = self.audition(
                s, "stock-slice"
            )
            self.check(
                "stock: SLICE sounds",
                not analyse(pcm["stock_slice"]).get("silent", True),
            )
        with self.session(MODS_ONE) as s:
            self.fresh(s)
            self.select(s, SLICE_DOWNS)
            self.check("patched: SLICE is still type 6", self.type_now(s) == 6)
            pcm["patched_slice"] = self.audition(s, "patched-slice")
            self.same_sound(
                "patched SLICE", pcm["stock_slice"], pcm["patched_slice"], "stock SLICE"
            )
            self.fresh(s)
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(200)
            for _ in range(XCLONE_DOWNS):
                s.do_tap("DOWN")
                s.run_ms(80)
            self.snap(s, "xclone-menu")
            s.do_tap("YES")
            s.run_ms(300)
            s.do_tap("YES")
            s.run_ms(300)
            t = self.type_now(s)
            self.check(
                f"XClone commits type {XCLONE_TYPE}",
                t == XCLONE_TYPE,
                f"track object type {t}",
            )
            s.do_tap("SRC")
            s.run_ms(300)
            self.snap(s, "xclone-src")
            s.health("SRC page")
            pcm["xclone"] = self.audition(s, "xclone")
            tx, dsp = self.wire(s)
            self.check(
                f"TX frame machine word is {XCLONE_TYPE}",
                tx == XCLONE_TYPE,
                f"{tx:#06x}",
            )
            self.check(
                f"DSP type cache (DM 0x255970) is {XCLONE_TYPE}",
                dsp == XCLONE_TYPE,
                f"{dsp}",
            )
            self.same_sound("XClone", pcm["stock_slice"], pcm["xclone"], "stock SLICE")
            s.health("end of a")

    def part_b(self) -> None:
        overlay = self.out / "card.overlay"
        with self.session(
            MODS_ONE, "persist", pool=False
        ) as s:  # saves a project: the card is dirty afterwards
            self.fresh(s)
            self.select(s, XCLONE_DOWNS)
            self.check(
                "XClone selected before the save", self.type_now(s) == XCLONE_TYPE
            )
            self.save_project(s)
            self.select(s, 0, ups=XCLONE_DOWNS + 2)  # to the top of the list: SAMPLE
            self.check(
                "track set to SAMPLE", self.type_now(s) == 0, f"{self.type_now(s)}"
            )
            self.load_project(s)
            t = self.type_now(s)
            self.check(
                f"project reload brings type {XCLONE_TYPE} back",
                t == XCLONE_TYPE,
                f"{t}",
            )
            s.health("reloaded")
            r = s.emu.call("card.overlay", path=str(overlay))
            self.check(
                "card writes exported", r["sectors"] > 0, f"{r['sectors']} sectors"
            )
            card = s.card
        types = card_overlay.sound_types(overlay.read_bytes())
        self.check(
            f"the saved project stores type {XCLONE_TYPE} in its sound records",
            types.get(XCLONE_TYPE, 0) > 0,
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
            self.snap(s, "stock-loaded")
            s.run_ms(500)
            s.do_hold("TRIG 1", 300)
            s.run_ms(300)
            s.health("stock after loading the patched project")
            self.check("stock keeps running (no fault, halt or stall)", True)

    def part_b_control(self) -> None:
        """Control: the same image with the persistence immediate put back (one byte): the type must NOT survive."""
        import tomllib

        from tools.firmware import build as dbuild
        from tools.patchkit import port_resolve as pr

        src = ROOT / "out" / "patch" / f"proof-xclone-{FW}.syx"
        r = subprocess.run(
            [sys.executable, "-m", "tools.patchkit.patchkit", "patch", "--firmware", str(config.stock(f"Digitakt_II_OS{FW}.syx")),
             "--modules", ",".join(MODS_ONE), "-o", str(src)],
            capture_output=True, text=True, cwd=ROOT, env=python_env(),
        )  # fmt: skip
        self.check("patch for the control", r.returncode == 0)
        port = tomllib.loads(
            (ROOT / "devices" / "ports" / f"dt2-{FW}.toml").read_text()
        )
        site = next(
            a
            for a in port["anchor"]
            if a["name"] == "cf.machine.site.sound_load_type_bound"
        )["addr"]
        main = bytearray(pr.sections_of(src)[3])
        at = site + 1 - 0x40000400
        self.check(
            "the persistence immediate is the patched one", main[at] == 9, f"{main[at]}"
        )
        main[at] = 8
        img = self.out / "proof-nopersist.syx"
        img.write_bytes(
            dbuild.rebuild(
                str(src),
                replacements={3: bytes(main)},
                pack="aplib",
                max_container=dbuild.FLASH_CONTAINER_MAX,
            )
        )
        with self.session(MODS_ONE, "nopersist", image=img, pool=False) as s:
            self.fresh(s)
            self.select(s, XCLONE_DOWNS)
            self.check("control: XClone selected", self.type_now(s) == XCLONE_TYPE)
            self.save_project(s)
            self.select(s, 0, ups=XCLONE_DOWNS + 2)
            self.load_project(s)
            t = self.type_now(s)
            self.check(
                "control: without the immediate the type falls back to SAMPLE",
                t == 0,
                f"{t}",
            )

    def save_project(self, s: H.Session) -> None:
        s.do_tap(["FUNC", "SETTINGS"])
        s.run_ms(300)
        s.do_tap("YES")
        if not self.quick:
            s.run_ms(3000)
            return
        # quick tier: until the card writes (`card.overlay`) stop growing, at most 3 s; the full tier waits 3 s
        prev, waited = -1, 0
        while waited < 3000:
            s.run_ms(500)
            waited += 500
            n = s.emu.call("card.overlay", path=str(self.out / "save-poll.overlay"))[
                "sectors"
            ]
            if n > 0 and n == prev:
                break
            prev = n
        self.save_s = waited / 1000

    def load_project(
        self, s: H.Session, until: Callable[[], bool] | None = None
    ) -> None:
        """Load the saved project. Quick tier with UNTIL: poll every 250 ms (up to the 6 s of the full tier) until it
        is true, then wait 250 ms more; the seconds waited are in `self.load_s`."""
        s.do_tap("SETTINGS")
        s.run_ms(200)
        s.do_tap("YES")
        s.run_ms(200)
        s.do_tap("YES")
        s.run_ms(500)
        s.do_tap(
            "YES"
        )  # the list entry: "Any changes will be lost. Proceed with loading?"
        s.run_ms(500)
        s.do_tap("YES")
        if self.quick and until is not None:
            waited = 0
            while waited < 6000 and not until():
                s.run_ms(250)
                waited += 250
            s.run_ms(250)
            self.load_s = (waited + 250) / 1000
        else:
            # quick tier without a signal to poll (the stock firmware: the track is SAMPLE before and after): 3 s, the
            # patched reload showed its type after 0.75 s
            wait = 3000 if self.quick else 6000
            s.run_ms(wait)
            self.load_s = wait / 1000

    def part_d(self) -> None:
        with self.session([]) as s:
            stock = self.dest_frames(s, SLICE_DOWNS)
        with self.session(MODS_ONE) as s:
            got = self.dest_frames(s, XCLONE_DOWNS)
            self.check(
                "XClone: the destination lists (with the SRC group) equal MANUAL SLICE's",
                got == stock,
                f"{len(got)} frames",
            )

    def part_e(self) -> None:
        stock = self.sequence_stock()
        with self.session(MODS_ONE) as s:
            got = self.sequence(s, XCLONE_DOWNS, "seq-xclone")
            self.same_sound("XClone (sequencer)", stock, got, "stock SLICE")

    def sequence_stock(self) -> np.ndarray:
        with self.session([]) as s:
            stock = self.sequence(s, SLICE_DOWNS, "seq-stock-slice")
        self.check(
            "stock: SLICE plays from the sequencer",
            not analyse(stock).get("silent", True),
        )
        return stock


PARTS = [
    K.Part("a", "select (stock, then test/machine-clone)", Proof.part_a, True, 90),
    K.Part(
        "b",
        "persistence (patched save and load, then stock load)",
        Proof.part_b,
        True,
        150,
    ),
    K.Part(
        "b2",
        "control (persistence immediate left at the stock value)",
        Proof.part_b_control,
        False,
        90,
    ),
    K.Part(
        "d",
        "modulation destination picker (MOD page, DEST knob)",
        Proof.part_d,
        False,
        130,
    ),
    K.Part("e", "sequencer: the step plays", Proof.part_e, True, 70),
]


def main() -> int:
    global FW
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--fw", default="1.17", choices=("1.17",))
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or (
        ROOT
        / "out"
        / "runs"
        / ("machines-proof" if a.fw == "1.17" else f"machines-proof-{a.fw}")
    )
    FW = a.fw
    return K.run_tool(__file__, Proof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
