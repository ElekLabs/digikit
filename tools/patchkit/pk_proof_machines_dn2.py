#!/usr/bin/env python3
"""patchkit core/machines on Digitone II 1.12 (test/dn2-clone: a clone of FM TONE under its own name).

    uv run python -m tools.patchkit.pk_proof_machines_dn2 [--out DIR] [--only a,b,c,c2,d] [--quick] [--jobs N]

Drives patched images on coupled daemons through the harness (`tools/patchkit/pk_harness.py` Session: panel inputs, health
asserted after every input), emulator only:

  a  select    FUNC+SRC lists FM CLONE in the synth group, YES commits; after the first TRIG the ColdFire's audio
               mirror holds type 5 (SRAM `0x8000487c`), the TX frame's type word (`0x80005e60 + 0x94`) holds the
               clone's original (0), the SYN/FLTR/AMP pages are FM TONE's (frame hashes equal) and an audition is
               bit-identical to stock FM TONE
  b  params    the same encoder turns on the SYN page give the same PCM on stock FM TONE and on the clone
  c  persist   save the project with FM CLONE, restart, load it: FM CLONE is current again; the card writes hold type 5
               in the sound records (byte 0xf4); the stock firmware loads the saved project and plays FM TONE
  d  sequence  the sequencer plays the step: PCM equals stock FM TONE

Parts are independent and run in parallel processes (`--jobs`, tools/patchkit/pk_proofkit.py). `--quick` runs a, b and c; the
control c2 and the sequencer d are full-tier only.

Output (PNG frames, WAV, overlays) goes under OUT (default out/runs/machines-proof-dn2), never committed.
Exit 0 only when every check passes. Addresses: finding 02, "core.machines on Digitone II".
"""

from __future__ import annotations

import argparse
import sys
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

MIRROR = 0x8000487C  # the audio side's copy of track 1's sound at +0xde: the type is the first byte
TX_TYPE = 0x80005E60 + 0x94  # TX frame type word: track 1 in the high half
CLONE_DOWNS = 4  # FM TONE, FM DRUM, WAVETONE, SWARMER, FM CLONE (before MIDI)
MODS = ["core/machines", "test/dn2-clone"]
READY = ROOT / "out" / "ready-fastweb-dn2"


class Proof(K.Proof):
    tool = "machines-dn2"
    device = "dn2"
    fw = "1.12"
    ready_env = {"DIGI_PK_READY_DN2_112": str(READY)}

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._turned: np.ndarray | None = None

    def fresh(self, s: H.Session) -> None:
        s.reset()
        s.run_ms(1500)

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

    def menu_row(self, s: H.Session, name: str) -> int:
        """The machine menu reopened, then closed: the visible row (0 = the first listed machine at the top of the
        window, counted from the first row on screen) whose marker is the big filled dot (the track's machine)."""
        from PIL import Image

        for _ in range(3):  # the first FUNC+SRC after a commit can be swallowed
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(400)
            self.snap(s, name)
            if "MACHINE SEL" in self.title(name):
                break
        s.do_tap("NO")
        s.run_ms(300)
        im = np.asarray(Image.open(self.out / f"{name}.png").convert("L")) > 127
        best, row = 0, -1
        for k, y in enumerate((57, 90, 122, 154, 186)):
            box = im[y - 14 : y + 14, 440:475]
            n = int(min(box.sum(), box.size - box.sum()))
            if n > best:
                best, row = n, k
        return row if best > 60 else -1

    def title(self, name: str) -> str:
        """A crude screen identity: 'MACHINE SEL' when the menu's inverted title bar is on screen."""
        from PIL import Image

        im = np.asarray(Image.open(self.out / f"{name}.png").convert("L")) > 127
        top = im[0:36, 60:440]
        return (
            "MACHINE SEL"
            if top.mean() > 0.55 and not im[40:60, 0:60].mean() > 0.9
            else "?"
        )

    def audition(self, s: H.Session, name: str) -> np.ndarray:
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        s.do_hold("TRIG 1", 500)
        s.run_ms(500)
        s.emu.call("pcm.tap", action="capture_stop")
        s.health(f"audition {name}")
        return hook_ab.read_wav(wav)

    def wire(self, s: H.Session) -> tuple[int, int]:
        """(audio mirror type byte of track 1, TX frame type word of track 1): meaningful after a TRIG."""
        m = ((s.emu.peek32(MIRROR) or 0) >> 24) & 0xFF
        t = ((s.emu.peek32(TX_TYPE) or 0) >> 16) & 0xFFFF
        return m, t

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

    def pages(self, s: H.Session, tag: str) -> list[str]:
        out = []
        for k in ("SYN", "FLTR", "AMP", "FX", "MOD"):
            s.do_tap(k)
            s.run_ms(300)
            out.append(self.snap(s, f"{tag}-{k.lower()}"))
        return out

    # -- the parts
    def part_a(self) -> None:
        pcm: dict[str, np.ndarray] = {}
        with self.session([], "stock") as s:
            self.fresh(s)
            stock_pages = self.pages(s, "stock")
            self.fresh(s)
            pcm["stock"] = self.audition(s, "stock-fmtone")
            self.check(
                "stock: FM TONE sounds", not analyse(pcm["stock"]).get("silent", True)
            )
            self.check(
                "stock: type 0 on the wire", self.wire(s) == (0, 0), f"{self.wire(s)}"
            )
        with self.session(MODS, "clone") as s:
            self.fresh(s)
            self.fresh(s)
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(300)
            for _ in range(CLONE_DOWNS):
                s.do_tap("DOWN")
                s.run_ms(80)
            self.snap(s, "clone-menu")
            s.do_tap("YES")
            s.run_ms(300)
            before = self.snap(s, "clone-committed")
            s.do_tap("YES")
            s.run_ms(300)
            row = self.menu_row(s, "clone-reopened")
            self.check(
                "the reopened menu marks FM CLONE (row 4) as the track's machine",
                row == 4,
                f"row {row} {before}",
            )
            self.fresh(s)
            self.select(s, CLONE_DOWNS)
            pcm["clone"] = self.audition(s, "clone")
            m, t = self.wire(s)
            self.check("the audio mirror holds type 5", m == 5, f"{m}")
            self.check("the TX frame sends FM TONE's type (0)", t == 0, f"{t}")
            self.same_sound("FM CLONE", pcm["stock"], pcm["clone"], "stock FM TONE")
            self.fresh(s)
            self.select(s, CLONE_DOWNS)
            got = self.pages(s, "clone")
            self.check(
                "SYN/FLTR/AMP pages equal FM TONE's",
                got[:3] == stock_pages[:3],
                f"{got[:3]} {stock_pages[:3]}",
            )
            s.health("end of a")

    def turned(self, s: H.Session, name: str, downs: int) -> np.ndarray:
        self.fresh(s)
        if downs:
            self.select(s, downs)
        s.do_tap("SYN")
        s.run_ms(300)
        for enc, by in (("ENC A", 20), ("ENC B", -15), ("ENC E", 25)):
            s.do_turn(enc, by)
            s.run_ms(100)
        return self.audition(s, name)

    def turned_ref(self) -> np.ndarray:
        """Stock FM TONE with the SYN page turns (memoized per process): the reference of b and c."""
        if self._turned is None:
            with self.session([], "stock-turn") as s:
                self._turned = self.turned(s, "turn-stock", 0)
        return self._turned

    def part_b(self) -> None:
        ref = self.turned_ref()
        with self.session(MODS, "clone-turn") as s:
            got = self.turned(s, "turn-clone", CLONE_DOWNS)
            plain = self.audition(s, "turn-clone-plain")
        self.same_sound(
            "FM CLONE with turned parameters",
            ref,
            got,
            "stock FM TONE with the same turns",
        )
        ok, _ = aligned_equal(plain, got)
        self.check("the turns changed the sound", not ok)

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

    def part_c(self) -> None:
        turned = self.turned_ref()
        overlay = self.out / "card.overlay"
        with self.session(
            MODS, "persist", pool=False
        ) as s:  # saves a project: own daemon
            self.fresh(s)
            self.select(s, CLONE_DOWNS)
            self.check(
                "FM CLONE selected before the save",
                self.menu_row(s, "persist-selected") == 4,
            )
            s.do_tap("SYN")
            s.run_ms(300)
            for enc, by in (("ENC A", 20), ("ENC B", -15), ("ENC E", 25)):
                s.do_turn(enc, by)
                s.run_ms(100)
            self.save_project(s)
            r = s.emu.call("card.overlay", path=str(overlay))
            self.check(
                "card writes exported", r["sectors"] > 0, f"{r['sectors']} sectors"
            )
            types = card_overlay.sound_types(overlay.read_bytes(), type_at=0xF4)
            self.check(
                "the saved project stores type 5 in sound records",
                types.get(5, 0) > 0,
                f"{types}",
            )
            s.reset() if False else None
            self.select(
                s, 0
            )  # the cursor starts on FM CLONE: UP x4 would be needed; use the top of the list
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(300)
            for _ in range(6):
                s.do_tap("UP")
                s.run_ms(80)
            s.do_tap("YES")
            s.run_ms(300)
            s.do_tap("YES")
            s.run_ms(300)
            self.check("track set to FM TONE", self.menu_row(s, "persist-fmtone") == 0)
            self.load_project(s)
            row = self.menu_row(s, "persist-loaded")
            self.check(
                "the loaded project has FM CLONE on track 1 again",
                row == 4,
                f"row {row}",
            )
            self.same_sound(
                "the loaded project",
                turned,
                self.audition(s, "persist-loaded"),
                "stock FM TONE with the same turns",
            )
            # the stock firmware loads the same project (the card state stays with the daemon)
            s.load(s.syx, settle=False)
            s.emu.drain_events()
            s.run_ms(2000)
            s.emu.pcm_start()
            s.pcm_mark = 0
            s.health("stock firmware")
            self.load_project(s)
            row = self.menu_row(s, "persist-stock")
            self.check(
                "STOCK loads the project: track 1 is FM TONE (the clone's type is unknown to it)",
                row == 0,
                f"row {row}",
            )
            p = self.audition(s, "persist-stock-audition")
            self.check(
                "STOCK plays the loaded track", not analyse(p).get("silent", True)
            )
            self.same_sound(
                "STOCK with the loaded project",
                turned,
                p,
                "stock FM TONE with the same turns",
            )
            s.health("end of c")

    def part_c_control(self) -> None:
        """Control: the same image with the persistence immediate put back (one byte): the type must NOT survive."""
        import subprocess
        import tomllib

        from tools.firmware import build as dbuild
        from tools.patchkit import port_resolve as pr

        src = ROOT / "out" / "patch" / "proof-dn2-clone.syx"
        r = subprocess.run(
            [sys.executable, "-m", "tools.patchkit.patchkit", "patch", "--firmware", str(config.stock("Digitone_II_OS1.12.syx")),
             "--modules", ",".join(MODS), "-o", str(src)],
            capture_output=True, text=True, cwd=ROOT, env=python_env(),
        )  # fmt: skip
        self.check("patch for the control", r.returncode == 0)
        port = tomllib.loads((ROOT / "devices" / "ports" / "dn2-1.12.toml").read_text())
        site = next(
            a
            for a in port["anchor"]
            if a["name"] == "cf.machine.site.sound_load_type_bound"
        )["addr"]
        main = bytearray(pr.sections_of(src)[3])
        at = site + 1 - 0x40000400
        self.check(
            "the persistence immediate is the patched one", main[at] == 7, f"{main[at]}"
        )
        main[at] = 6
        img = self.out / "proof-nopersist.syx"
        img.write_bytes(
            dbuild.rebuild(str(src), replacements={3: bytes(main)}, pack="aplib", max_container=dbuild.FLASH_CONTAINER_MAX)
        )  # fmt: skip
        with self.session(MODS, "nopersist", image=img, pool=False) as s:
            self.fresh(s)
            self.select(s, CLONE_DOWNS)
            self.check(
                "control: FM CLONE selected",
                self.menu_row(s, "nopersist-selected") == 4,
            )
            self.save_project(s)
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(300)
            for _ in range(6):
                s.do_tap("UP")
                s.run_ms(80)
            s.do_tap("YES")
            s.run_ms(300)
            s.do_tap("YES")
            s.run_ms(300)
            self.load_project(s)
            row = self.menu_row(s, "nopersist-loaded")
            self.check(
                "control: without the immediate the type falls back to FM TONE",
                row == 0,
                f"row {row}",
            )

    def part_d(self) -> None:

        def seq(s: H.Session, name: str, downs: int) -> np.ndarray:
            """Live-record a trig on track 1 (RECORD, PLAY, TRIG 1 held), stop, then play the pattern back."""
            self.fresh(s)
            if downs:
                self.select(s, downs)
            s.do_tap("RECORD")
            s.run_ms(200)
            s.do_tap("PLAY")
            s.run_ms(100)
            s.do_hold("TRIG 1", 200)
            s.run_ms(2500)
            s.do_tap("STOP")
            s.run_ms(500)
            s.do_tap("RECORD")
            s.run_ms(200)
            wav = self.out / f"{name}.wav"
            s.emu.call("pcm.tap", action="capture", path=str(wav))
            s.do_tap("PLAY")
            s.run_ms(2500)
            s.emu.call("pcm.tap", action="capture_stop")
            s.do_tap("STOP")
            s.run_ms(300)
            s.health(f"sequence {name}")
            return hook_ab.read_wav(wav)

        with self.session([], "seq-stock") as s:
            ref = seq(s, "seq-stock", 0)
        self.check(
            "stock: FM TONE plays from the sequencer",
            not analyse(ref).get("silent", True),
        )
        with self.session(MODS, "seq-clone") as s:
            got = seq(s, "seq-clone", CLONE_DOWNS)
        self.same_sound("FM CLONE (sequencer)", ref, got, "stock FM TONE")


PARTS = [
    K.Part("a", "select (stock FM TONE, then test/dn2-clone)", Proof.part_a, True, 150),
    K.Part("b", "parameters (SYN page encoders turned)", Proof.part_b, True, 120),
    K.Part("c", "persistence", Proof.part_c, True, 260),
    K.Part(
        "c2",
        "control (persistence immediate left at the stock value)",
        Proof.part_c_control,
        False,
        120,
    ),
    K.Part("d", "sequencer", Proof.part_d, False, 120),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or ROOT / "out" / "runs" / "machines-proof-dn2"
    return K.run_tool(__file__, Proof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
