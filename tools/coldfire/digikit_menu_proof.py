#!/usr/bin/env python3
"""Emulator proof of SETTINGS > DIGIKIT (modules/core/apps/digikit-menu, docs/findings/28-digikit-menu.md).

    uv run python -m tools.coldfire.digikit_menu_proof run [--image dt2-1.16] [--update] [--jobs N] [--regress] [--only NAMES]

Patches the stock firmware with core/digikit-menu (which requires core/runtime and core/ui), builds a card from a `plus_drive`-style
folder with `digikit/modules/dyn-demo.dkm` (packed by `patchkit pack`), `bad-sha.dkm` and `bad-import.dkm`, boots in the
native emulator (regions_probe) and presses the panel keys of tools/coldfire/panel_script.py. The gate script checks dyn-demo in
MODULES, answers APPLY & RESTART? with YES, and after the restart shows ABOUT with one module loaded and the list with the row checked and
loaded. Judged per run:

  * the firmware's stores and DMA never touch the module RAM `cf.expansion`, except the module's own code (the
    write-watch with ALLOW_PC = the module; the module's read buffers come from the firmware heap), and no flush hit
    (DIGI_FLUSH_CHECK=1);
  * the OLED frames named by the script have the SHA-256 pinned in tests/golden/digikit_menu_frames.json
    (`--update` writes them after a look at the PNGs);
  * the menu's state after the run, read back with `--peek`: the store was read (STORE=1, three entries), the checked rows and the
    loaded flags, the last dialog's text;
  * `--regress` also runs the stock menu scripts of finding 25 on the patched image against their pinned frames.

PNGs of the DIGIKIT screens go to out/runs/lanes/r1-shots/ (a contact sheet per script).
"""

from __future__ import annotations

import argparse
import contextlib
import json
import pathlib
import re
import shutil
import subprocess
import sys

from tools import python_env
from tools.coldfire import arena_safety, panel_script
from tools.device.dkproto import SETTINGS_SECTORS
from tools.firmware import device as device_files

ROOT = pathlib.Path(__file__).resolve().parents[2]
GOLDEN = ROOT / "tests" / "golden" / "digikit_menu_frames.json"
OUT = ROOT / "out" / "runs" / "lanes" / "r1-run"
SHOTS = ROOT / "out" / "runs" / "lanes" / "r1-shots"
CARDS = ROOT / "out" / "runs" / "lanes" / "r1-card"

DEVICE = {
    "dt2-1.16": ("dt2", "Digitakt_II_OS1.16.syx"),
    "dt2-1.17": ("dt2", "Digitakt_II_OS1.17.syx"),
    "dn2-1.11": ("dn2", "Digitone_II_OS1.11.syx"),
    "dn2-1.12": ("dn2", "Digitone_II_OS1.12.syx"),
}
MENU = "core/digikit-menu"
RUNTIME = "core/runtime"
DYN_MAGIC = 0x44594E4D  # "DYNM"
FINI_MARK = 0xF1F10001


REGRESS_SCRIPTS = [
    "save_project", "save_quick", "load_project", "new_project", "manage_delete", "manage_rename", "format_pool",
    "os_update", "check_filesystem", "pattern_temp",
]  # fmt: skip


def _has_path_fs(image: str) -> bool:
    """True when the device of `image` has a folder filesystem on its card (the stock `check_filesystem` script runs there)."""
    return device_files.by_image(image).capabilities.get("card") == "path-fs"


def sh(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    r = subprocess.run(
        cmd, cwd=ROOT, env=python_env(), capture_output=True, text=True, **kw
    )
    if r.returncode:
        raise SystemExit(
            "command failed: %s\n%s%s"
            % (" ".join(cmd), r.stdout[-2000:], r.stderr[-2000:])
        )
    return r


def patch(
    image: str, modules: list[str], name: str
) -> tuple[pathlib.Path, pathlib.Path]:
    _, fw = DEVICE[image]
    syx = ROOT / "out" / "patch" / f"{name}.syx"
    sh(
        ["uv", "run", "python", "-m", "tools.patchkit.patchkit", "patch", "--firmware",
         f"firmware/{fw}", "--modules", ",".join(modules), "-o", str(syx.relative_to(ROOT))]
    )  # fmt: skip
    return syx, syx.with_suffix(".plan.json")


def make_card(image: str) -> pathlib.Path:
    """A card with digikit/modules/{bad-import,bad-sha,dyn-demo}.dkm and digikit/readme.txt (not a module)."""
    import tempfile

    from tools.patchkit import pk_dkrt

    dev, fw = DEVICE[image]
    folder = CARDS / dev
    shutil.rmtree(folder, ignore_errors=True)
    (folder / "digikit" / "modules").mkdir(parents=True)
    tools = pk_dkrt.Tools()
    pk_dkrt.pack(
        tools, "test/dyn-demo", folder / "digikit" / "modules" / "dyn-demo.dkm"
    )
    with tempfile.TemporaryDirectory(prefix="dkmenu-") as work:
        pk_dkrt.broken_variants(
            pathlib.Path(work), tools, "test/dyn-demo", folder / "digikit"
        )
    good = (folder / "digikit" / "modules" / "dyn-demo.dkm").read_bytes()
    other = (
        2 if dev == "dt2" else 1
    )  # the other device's id (dkrt-sdk DEVICE_DT2 = 1, DEVICE_DN2 = 2)
    (folder / "digikit" / "modules" / "foreign.dkm").write_bytes(
        forge(good, device=other)
    )
    (folder / "digikit" / "modules" / "future.dkm").write_bytes(
        forge(good, api_major=99)
    )
    (folder / "digikit" / "readme.txt").write_bytes(b"readme")
    img = CARDS / f"{dev}.img"
    img.unlink(missing_ok=True)
    sh(
        ["cargo", "run", "--release", "--locked", "--offline", "-p", "plusdrive-format", "--bin",
         "plusdrive", "--", "build", str(folder), "-o", str(img), "--syx", f"firmware/{fw}", "--device", dev]
    )  # fmt: skip
    return img


def forge(
    good: bytes, device: int | None = None, api_major: int | None = None
) -> bytes:
    """A copy of a good `.dkm` with a header field changed and the file sha256 made right again: register records for another
    device (an empty `DKRG` record, 16 bytes appended) or another `api` contract major. The menu reads only these header fields."""
    import hashlib
    import struct

    b = bytearray(good)
    if device is not None:
        b += b"\0" * (-len(b) % 16)
        struct.pack_into(">I", b, 0x90, len(b))
        struct.pack_into(">I", b, 0x94, 16)
        b += struct.pack(">4sIII", b"DKRG", 1, device, 0)
    if api_major is not None:
        b[0x38] = api_major
    struct.pack_into(">I", b, 0x08, len(b))
    b[0x10:0x30] = bytes(32)
    b[0x10:0x30] = hashlib.sha256(bytes(b)).digest()
    return bytes(b)


class Plan:
    def __init__(self, path: pathlib.Path | dict) -> None:
        self.syms = (path if isinstance(path, dict) else json.loads(path.read_text()))[
            "symbols"
        ]

    def addr(self, name: str) -> int:
        """A module symbol by its name (`module@version:NAME`)."""
        for k, v in self.syms.items():
            if k.endswith(":" + name):
                return v
        raise KeyError(name)


def peek(facts_dir: pathlib.Path, addr: int) -> int | None:
    for line in (facts_dir / "probe.stdout").read_text().splitlines():
        m = re.match(r'\{"peek":"(0x[0-9a-f]+)","value":(?:Some\((\d+)\)|None)\}', line)
        if m and int(m.group(1), 16) == addr:
            return None if m.group(2) is None else int(m.group(2))
    return None


def peek_ats(facts_dir: pathlib.Path) -> list[int | None]:
    """The values of the `--peek-at` reads, in time order."""
    out = []
    for line in (facts_dir / "probe.stdout").read_text().splitlines():
        m = re.match(
            r'\{"peek_at":"0x[0-9a-f]+","t":\d+,"value":(?:Some\((\d+)\)|None)\}', line
        )
        if m:
            out.append(None if m.group(1) is None else int(m.group(1)))
    return out


def peek_ats_bytes(facts_dir: pathlib.Path) -> bytes:
    """The longwords of the `--peek-at` reads of a text, in time order, as bytes (digikit_errors reads the first popup)."""
    return b"".join((v or 0).to_bytes(4, "big") for v in peek_ats(facts_dir))


def peek_bytes(d: pathlib.Path, addr: int, n: int) -> bytes:
    out = b""
    for k in range(0, n, 4):
        v = peek(d, addr + k)
        out += (v or 0).to_bytes(4, "big")
    return out[:n]


def cstr(b: bytes) -> str:
    return b.split(b"\0")[0].decode("latin1")


def pin(image: str, script: str, facts: dict, goldens: dict, update: bool) -> list[str]:
    bad = []
    for f, sha in facts["frame_sha256"].items():
        key = f"{image}/{script}/{f}"
        if update:
            goldens[key] = sha
        elif goldens.get(key) != sha:
            bad.append(
                f"frame {f}: {sha[:12]} is not the pinned {str(goldens.get(key))[:12]}"
            )
    return bad


def shots(image: str, script: str, d: pathlib.Path, frames: list[str]) -> None:
    from PIL import Image

    SHOTS.mkdir(parents=True, exist_ok=True)
    ims = []
    for f in frames:
        p = d / f"{f}.png"
        if p.exists():
            shutil.copy(p, SHOTS / f"{image}-{script}-{f}.png")
            ims.append(Image.open(p))
    if ims:
        w, h = ims[0].size
        sheet = Image.new("RGB", (w * 2, h * ((len(ims) + 1) // 2)))
        for i, im in enumerate(ims):
            sheet.paste(im, ((i % 2) * w, (i // 2) * h))
        sheet.save(SHOTS / f"{image}-{script}-sheet.png")


def stack_env(image: str, plan: Plan | None = None) -> dict[str, str]:
    """DIGI_STACK_WATCH on the UI task's stack (tools.coldfire.task_stacks)."""
    from tools.coldfire import task_stacks

    _, ts, pop = task_stacks.tasks(image)
    ui = task_stacks.ui_task(ts, pop)
    return {"DIGI_STACK_WATCH": "0x%x-0x%x" % (ui.base, ui.top)}


def run_one(
    image,
    name,
    script,
    syx,
    plan,
    card,
    goldens,
    update,
    peeks,
    watch=None,
    env_extra=None,
):
    out = OUT / image / name
    extra = []
    for a in peeks:
        extra += ["--peek", hex(a)]
    facts = arena_safety.run(
        image,
        script,
        out,
        card=card,
        syx=syx,
        allow_pc=arena_safety.plan_allow_pc(plan),
        watch=watch,
        extra=extra,
        env_extra=env_extra,
    )
    facts["name"] = name
    facts["bad"] = pin(image, name, facts, goldens, update)
    if not facts["pass"]:
        facts["bad"].append(
            "run failed: errors %s write events %s flush hits %s"
            % (facts["errors"], facts["write_events"], facts["flush_hits"])
        )
    return facts, out


def report_stack(name: str, facts: dict) -> None:
    st = facts["stacks"][0]
    print(
        f"  stack of {st['size']} bytes: deepest {st['depth']} (pc {st['pc']}); margin {st['size'] - st['depth']}"
    )


# ---- the boot proof: tick, apply, restart; safe mode (finding 38) -----------------------------------------------------------


def _wait_restart(rig, timeout_s: float = 300.0) -> None:
    """After the menu's `dkrt_restart`: step the coupled daemon until the emulator's soft reset has cold-booted the firmware again
    (the same wait as `BootRig.restart`, without the mailbox command)."""
    import time

    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout_s:
        try:
            rig.emu.step(400_000)
        except Exception:  # noqa: BLE001  a cold boot runs on its worker: steps are refused until it is done
            time.sleep(0.5)
        for ev in rig.emu.drain_events():
            if ev.get("kind") == "cpu.reset" and ev.get("restart") == "unsupported":
                raise RuntimeError("the emulator cannot restart this build")
            if ev.get("kind") == "firmware.loaded":
                if not ev.get("ok"):
                    raise RuntimeError(f"restart: the cold boot failed {ev}")
                rig.s.emu.watchdog(rig.s.cell.stall_ms)
                rig.s.emu.pcm_start()  # the harness's taps read the PCM report of the new emulator
                rig.s.run_ms(1500)
                rig.wait_dsp()
                return
    raise RuntimeError("restart: no new boot")


class BootProof:
    """The menu on a card with modules, in the coupled emulator with the RCM soft reset: the checks are made on what the menu and
    the runtime hold in memory and on the card (the firmware's popup text, the pending selection, the OFFER flag, boot info), and
    on pinned OLED frames."""

    def __init__(self, image: str, update: bool, work: pathlib.Path) -> None:
        from tools.patchkit import pk_dkrt_boot as B

        self.B, self.image, self.update = B, image, update
        self.env = B.Env(image, work)
        self.checks: list[tuple[str, bool, str]] = []
        self.goldens = json.loads(GOLDEN.read_text()) if GOLDEN.exists() else {}
        self.new_goldens: dict[str, str] = {}
        self.plan: Plan | None = None
        self.writes: set[int] = set()
        self.card_sectors: set[int] = set()
        self.watch = {"write_events": 0, "flush_hits": 0}

    def check(self, name: str, ok: bool, detail: str = "") -> None:
        self.checks.append((name, bool(ok), detail))
        print(f"{'ok  ' if ok else 'FAIL'} {name} {detail}", flush=True)

    def setup(self) -> None:
        e = self.env
        e.pack("test/dyn-demo", "dyn-demo.dkm")
        good = (e.folder / "digikit" / "modules" / "dyn-demo.dkm").read_bytes()
        bad = bytearray(good)
        bad[-1] ^= (
            0x5A  # the image is the last part: the header sha256 no longer matches
        )
        (e.folder / "digikit" / "modules" / "bad-sha.dkm").write_bytes(bad)
        other = 2 if self.env.device == "dt2" else 1
        (e.folder / "digikit" / "modules" / "foreign.dkm").write_bytes(
            forge(good, device=other)
        )
        self.img = e.build_image(
            f"{self.B.RT},{MENU}", "menu"
        )  # the boot rig reads the voice rows: dynreg and dyndsp too
        self.plan = Plan(self.img.plan)

    # -- reads
    def w(self, rig, name: str, k: int = 0) -> int:
        return rig.w(self.plan.addr(name) + 4 * k)

    def text(self, rig, name: str, n: int = 48) -> str:
        return cstr(
            b"".join(self.w(rig, name, k).to_bytes(4, "big") for k in range(n // 4))
        )

    def bytes_of(self, rig, name: str, n: int) -> list[int]:
        raw = b"".join(
            self.w(rig, name, k).to_bytes(4, "big") for k in range((n + 3) // 4)
        )
        return list(raw[:n])

    # -- panel
    def tap(self, rig, key: str, gap_ms: int = 350) -> None:
        rig.s.do_tap(key)
        rig.s.run_ms(gap_ms)

    def downs(self, rig, n: int) -> None:
        for _ in range(n):
            self.tap(rig, "DOWN", 200)

    def to_digikit(self, rig) -> None:
        self.tap(rig, "NO", 400)
        self.tap(rig, "SETTINGS", 600)
        self.downs(rig, 8)
        self.tap(rig, "YES", 700)

    def frame(self, rig, name: str) -> None:
        digest, raw = rig.s.frame()
        from tools.control.digi_client import oled_png

        SHOTS.mkdir(parents=True, exist_ok=True)
        (SHOTS / f"{self.image}-boot-{name}.png").write_bytes(oled_png(raw))
        key = f"{self.image}/boot/{name}"
        self.new_goldens[key] = digest
        if not self.update:
            want = self.goldens.get(key)
            self.check(
                f"frame {name}",
                want == digest,
                f"{digest[:12]} pinned {str(want)[:12]}",
            )

    def finish(self, rig) -> None:
        try:
            self.writes |= set(rig.card_store_writes())
            from tools.coldfire import arena_safety as A

            ov = rig.work / f"{rig.tag}.all-overlay.bin"
            rig.emu.call("card.overlay", path=str(ov))
            self.card_sectors |= set(A.parse_overlay(ov.read_bytes()))
        except Exception as ex:  # noqa: BLE001
            print(f"card.overlay: {ex}", file=sys.stderr)
        w = rig.watch_summaries()
        self.watch["write_events"] += w["write_events"]
        self.watch["flush_hits"] += w["flush_hits"]

    # -- scenarios
    def apply_scenario(self) -> None:
        """No selection: NO on MODULES asks nothing; check dyn-demo; NO asks APPLY & RESTART? (nothing is saved), YES saves and restarts; the next
        boot loads it; ABOUT shows it; the list shows it checked and loaded and NO asks nothing."""
        B, e = self.B, self.env
        with e.boot(self.img, e.card(None, tag="apply"), "apply", watch=True) as rig:
            self.to_digikit(rig)
            self.frame(rig, "digikit")
            self.downs(rig, 3)  # ABOUT, HEALTH, MEMORY, MODULES
            self.tap(rig, "YES", 900)  # MODULES
            self.frame(rig, "list_empty")
            self.check("rows: bad-sha, dyn-demo, foreign", self.w(rig, "NENT") == 3)
            self.check(
                "foreign is not compatible", self.bytes_of(rig, "NOK", 3) == [0, 0, 1]
            )
            self.downs(rig, 1)
            self.tap(rig, "YES", 500)  # check dyn-demo
            self.check(
                "pending selection: dyn-demo",
                self.bytes_of(rig, "TICK", 3) == [0, 1, 0],
            )
            self.check(
                "the card's selection is still empty", rig.sel_records() == [None, None]
            )
            self.frame(rig, "list_ticked")
            n0 = self.w(rig, "DK_POPUPS")
            self.tap(rig, "NO", 700)  # changes pending: the list asks and stays
            self.check(
                "NO with a change pending asks APPLY & RESTART? and saves nothing",
                self.w(rig, "DK_POPUPS") == n0 + 1
                and rig.sel_records() == [None, None]
                and self.text(rig, "DK_POPUP") == "APPLY & RESTART?"
                and self.text(rig, "DK_POPUP_TEXT") == "1 CHANGE",
                f"{self.text(rig, 'DK_POPUP')} / {self.text(rig, 'DK_POPUP_TEXT')}",
            )
            self.frame(rig, "confirm_apply")
            rig.emu.drain_events()
            self.tap(rig, "YES", 300)  # YES: save and restart
            self.check(
                "dkrt_selection_set answered 0", self.w(rig, "DK_APPLY_SET") == 0
            )
            _wait_restart(rig)
            b = rig.boot_info()
            self.check(
                "the next boot loaded the selection",
                b["count"] == 1
                and b["loaded"] == 1
                and b["results"] == [0]
                and b["safe"] == 0,
                str(b["results"]),
            )
            recs = rig.sel_records()
            self.check(
                "the record in use holds it (the boot counter writes the other copy)",
                B.current(recs) is not None
                and B.current(recs)["paths"] == ["modules/dyn-demo.dkm"],
                str(recs),
            )
            self.to_digikit(rig)
            self.tap(rig, "YES", 900)  # ABOUT, the first row
            self.frame(rig, "about_after_boot")
            self.tap(rig, "NO", 500)
            self.downs(rig, 3)
            self.tap(rig, "YES", 900)  # MODULES
            self.frame(rig, "list_after_boot")
            self.check(
                "after the boot: checked, marked loaded",
                self.bytes_of(rig, "TICK", 3) == [0, 1, 0]
                and self.bytes_of(rig, "LOADED", 3) == [0, 1, 0],
            )
            self.tap(rig, "NO", 500)
            self.check("after the boot: NO asks nothing", self.w(rig, "DK_POPUPS") == 0)
            self.finish(rig)

    def safe_scenario(self) -> None:
        """A card with a selection and a boot counter of 2: this boot is in safe mode and loaded nothing. The menu says so (a dialog
        when DIGIKIT opens, a row, the reason) and keeps the selection checked; NO on MODULES asks to restart, which is the way out."""
        e = self.env
        card = e.card(["modules/dyn-demo.dkm"], counter=2, tag="safe")
        with e.boot(self.img, card, "safe", watch=True) as rig:
            b = rig.boot_info()
            self.check(
                "safe mode by the counter",
                b["safe"] == 1 and b["reason"] == 1 and b["loaded"] == 0,
                f"{b['safe']} {b['reason']}",
            )
            self.to_digikit(rig)
            self.check(
                "a dialog names safe mode and why",
                self.w(rig, "DK_POPUPS") == 1
                and "SAFE" in self.text(rig, "DK_POPUP")
                and "BAD BOOTS" in self.text(rig, "DK_POPUP_TEXT"),
                f"{self.text(rig, 'DK_POPUP')} / {self.text(rig, 'DK_POPUP_TEXT')}",
            )
            self.frame(rig, "safe_popup")
            self.tap(rig, "NO", 500)
            self.frame(rig, "safe_main")
            self.downs(rig, 4)  # past the SAFE row, ABOUT, HEALTH and MEMORY to MODULES
            self.tap(rig, "YES", 900)
            self.frame(rig, "safe_list")
            self.check(
                "the selection is kept checked",
                self.bytes_of(rig, "TICK", 3) == [0, 1, 0],
            )
            self.tap(rig, "NO", 700)  # a restart leaves safe mode: the list asks
            self.check(
                "NO in safe mode asks APPLY & RESTART? (LEAVES SAFE MODE)",
                self.text(rig, "DK_POPUP") == "APPLY & RESTART?"
                and self.text(rig, "DK_POPUP_TEXT") == "LEAVES SAFE MODE",
                f"{self.text(rig, 'DK_POPUP')} / {self.text(rig, 'DK_POPUP_TEXT')}",
            )
            rig.emu.drain_events()
            self.tap(rig, "YES", 300)
            _wait_restart(rig)
            b = rig.boot_info()
            self.check(
                "the way out: the next boot is normal and loads the selection",
                b["safe"] == 0 and b["loaded"] == 1 and b["results"] == [0],
                str(b["results"]),
            )
            self.finish(rig)

    def bad_scenario(self) -> None:
        """A selection with the bad-sha module: the boot reports its result, ABOUT shows the text."""
        e = self.env
        card = e.card(["modules/bad-sha.dkm"], tag="bad")
        with e.boot(self.img, card, "bad", watch=True) as rig:
            b = rig.boot_info()
            self.check(
                "boot result of bad-sha is E_SHA",
                b["count"] == 1 and b["results"] == [-4],
                str(b["results"]),
            )
            self.to_digikit(rig)
            self.tap(rig, "YES", 900)  # ABOUT, the first row
            self.frame(rig, "about_bad")
            self.finish(rig)

    def run(self, quick: bool) -> bool:
        self.setup()
        self.apply_scenario()
        self.safe_scenario()
        if not quick:
            self.bad_scenario()
        from tools.patchkit import pk_device

        store = pk_device.store_of(self.image[:3])
        sel = {store.sel_a, store.sel_b}
        self.check(
            "card store writes only in the selection sectors",
            self.writes <= sel and self.writes,
            str(sorted(self.writes)),
        )
        self.check(
            "0 stray writes, 0 flush hits",
            self.watch["write_events"] == 0 and self.watch["flush_hits"] == 0,
            str(self.watch),
        )
        if self.update:
            g = {
                k: v
                for k, v in self.goldens.items()
                if not k.startswith(self.image + "/boot/")
            }
            g.update(self.new_goldens)
            GOLDEN.write_text(json.dumps(dict(sorted(g.items())), indent=1) + "\n")
        return all(ok for _, ok, _ in self.checks)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cmd", choices=["run"])
    ap.add_argument(
        "--no-boot",
        action="store_true",
        help="skip the boot proof (apply, restart, safe mode)",
    )
    ap.add_argument("--only-boot", action="store_true", help="only the boot proof")
    ap.add_argument("--image", default="dt2-1.16")
    ap.add_argument("--update", action="store_true", help="pin the frames of this run")
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument(
        "--regress",
        action="store_true",
        help="also the stock menu scripts, stock against patched image",
    )
    ap.add_argument(
        "--stack",
        action="store_true",
        help="also measure the UI task's stack high-water (DIGI_STACK_WATCH)",
    )
    ap.add_argument(
        "--quick",
        action="store_true",
        help="the quick tier: no bad-module scenario",
    )
    ap.add_argument(
        "--stack-stock",
        action="store_true",
        help="the UI stack high-water of the stock SETTINGS and project scripts (save, load, new, manage, format) on the patched image",
    )
    ap.add_argument("--only", help="comma-separated script names to run (default: all)")
    ap.add_argument("--only-regress", action="store_true", help="skip the DIGIKIT runs")
    a = ap.parse_args(argv)
    image = a.image
    if a.only_boot:
        work = OUT / image / "boot"
        work.mkdir(parents=True, exist_ok=True)
        return 0 if BootProof(image, a.update, work).run(a.quick) else 1
    syx, plan_path = patch(image, [MENU], f"dkmenu-{image}")
    plan = Plan(plan_path)
    card = make_card(image)
    goldens = json.loads(GOLDEN.read_text()) if GOLDEN.exists() else {}
    if a.update and not a.only_regress:
        goldens = {k: v for k, v in goldens.items() if not k.startswith(image + "/")}
    state = [plan.addr(n) for n in ("NENT", "STORE", "TICK", "DK_POPUP", "DK_POPUPS")]
    st = plan.addr("DKRT_STATE")
    # the whole of cf.expansion except the runtime's staging buffer: the card read lands there by DMA, by design
    lo, hi = arena_safety.expansion(image)
    stage = plan.addr("DKRT_STAGE")
    secbuf = plan.addr(
        "DKRT_SECBUF"
    )  # the selection record's 512-byte read buffer: a DMA target as well
    holes = [(stage, 64 * 1024), (secbuf, 512)]
    for name in (
        "DKRT_STORE_PEEK",
        "DKRT_STORE_FP",
        "DKRT_SETTINGS_HDR",
    ):  # the store's header and fingerprint sectors, the settings record's header sector: DMA targets too
        with contextlib.suppress(Exception):  # an image without them
            holes.append((plan.addr(name), 512))
    with contextlib.suppress(
        Exception
    ):  # the settings record's RAM copy: the boot's card read of the settings area
        holes.append((plan.addr("DKRT_SETTINGS"), SETTINGS_SECTORS * 512))
    holes.sort()
    ranges, at = [], lo
    for start, size in holes:
        if start > at:
            ranges.append("0x%x-0x%x" % (at, start))
        at = start + size
    ranges.append("0x%x-0x%x" % (at, hi))
    watch = ",".join(ranges)
    runtime_state = [st + 4 * k for k in range(0, 12)]
    jobs = [
        ("digikit_main", panel_script.SCRIPTS["digikit_main"]()),
        ("digikit_about", panel_script.digikit_about_script()),
    ]
    if a.only:
        jobs = [j for j in jobs if j[0] in a.only.split(",")]
    from concurrent.futures import ThreadPoolExecutor

    def go(j):
        name, script = j
        peeks = []
        for sym_addr in state:
            peeks += [sym_addr + 4 * k for k in range(0, 12)]
        peeks += runtime_state
        return (
            j,
            *run_one(
                image,
                name,
                script,
                syx,
                plan_path,
                card,
                goldens,
                a.update,
                peeks,
                watch,
                stack_env(image, plan) if a.stack else None,
            ),
        )

    with ThreadPoolExecutor(max_workers=a.jobs) as pool:
        results = [] if a.only_regress else list(pool.map(go, jobs))
    ok = True
    for j, facts, out in results:
        name, script = j
        bad = list(facts["bad"])
        nent = peek(out, plan.addr("NENT"))
        store = peek(out, plan.addr("STORE"))
        info = {"entries": nent, "store": store}
        if name == "digikit_about" and store not in (0, None):
            bad.append(f"the store was read by ABOUT: {store}")
        if a.stack:
            report_stack(name, facts)
        shots(image, name, out, [f for f, _ in script.frames])
        print(f"{name:18s} {'PASS' if not bad else 'FAIL'}  {info}")
        for b in bad:
            print("    " + b)
        ok &= not bad
    if a.update:
        GOLDEN.parent.mkdir(parents=True, exist_ok=True)
        GOLDEN.write_text(json.dumps(dict(sorted(goldens.items())), indent=1) + "\n")
    if a.stack_stock:
        env = stack_env(image)
        names = [
            n for n in REGRESS_SCRIPTS if n != "check_filesystem" or _has_path_fs(image)
        ]
        with ThreadPoolExecutor(max_workers=a.jobs) as pool:
            runs = list(
                pool.map(
                    lambda n: arena_safety.run(
                        image,
                        n,
                        OUT / image / ("stack-" + n),
                        card=card,
                        syx=syx,
                        allow_pc=arena_safety.plan_allow_pc(plan_path),
                        env_extra=env,
                    ),  # fmt: skip
                    names,
                )
            )
        for n, f in zip(names, runs, strict=True):
            m = f["stacks"][0]
            print(
                f"stack {n:16s} deepest {m['depth']} of {m['size']} bytes (pc {m['pc']})"
            )
    if a.regress:
        # the stock SETTINGS scripts of finding 25 (project, system and format menus, saves): the same card, stock image
        # against patched image; every dumped frame and the card sectors the firmware wrote must be equal
        names = [
            n for n in REGRESS_SCRIPTS if n != "check_filesystem" or _has_path_fs(image)
        ]
        stock = arena_safety.suite(
            image, names, OUT / image / "regress-stock", card, a.jobs, False
        )
        patched = arena_safety.suite(
            image, names, OUT / image / "regress-patched", card, a.jobs, False,
            run_kw={"syx": syx, "allow_pc": arena_safety.plan_allow_pc(plan_path)},
        )  # fmt: skip
        for st, pa in zip(stock, patched, strict=True):
            same_frames = st["frame_sha256"] == pa["frame_sha256"]
            same_card = st["card_ranges"] == pa["card_ranges"]
            good = (
                same_frames
                and same_card
                and pa["write_events"] == 0
                and not pa["flush_hits"]
                and not pa["errors"]
            )
            print(
                "regress %-16s %s  frames %d equal=%s  card ranges equal=%s  writes %s flush %s"
                % (st["script"], "PASS" if good else "FAIL", len(pa["frame_sha256"]), same_frames, same_card,
                   pa["write_events"], pa["flush_hits"])
            )  # fmt: skip
            ok &= good
    if not a.no_boot and not a.only_regress:
        work = OUT / image / "boot"
        work.mkdir(parents=True, exist_ok=True)
        ok &= BootProof(image, a.update, work).run(a.quick)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
