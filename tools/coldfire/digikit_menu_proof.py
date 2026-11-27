#!/usr/bin/env python3
"""Emulator proof of SETTINGS > DIGIKIT (runtime/apps/digikit-menu, docs/findings/28-digikit-menu.md).

    uv run python -m tools.coldfire.digikit_menu_proof run [--image dt2-1.16] [--update] [--jobs N] [--regress]

Patches the stock firmware with core/runtime and core/ui (the menu is part of the system package, which the card carries), builds a card from a `plus_drive`-style
folder with `digikit/modules/dyn-demo.dkm` (packed by `patchkit pack`), `bad-sha.dkm` and `bad-import.dkm`, boots in the native emulator and presses the panel keys of
tools/coldfire/panel_script.py. The boot proof checks dyn-demo in MODULES, answers APPLY & RESTART? with YES, and after the restart shows ABOUT with one module loaded and the
list with the row checked and loaded. The menu's statics (the rows, the ticks, the dialog) are package statics: the proof reads them through the host directory of the runtime.
Judged per run:

  * the firmware's stores and DMA never touch the module RAM `cf.expansion`, except the module's own code (the write-watch with ALLOW_PC = the module; the module's read
    buffers come from the firmware heap), and no flush hit (DIGI_FLUSH_CHECK=1);
  * the OLED frames named by the script have the SHA-256 pinned in tests/golden/digikit_menu_frames.json (`--update` writes them after a look at the PNGs);
  * `--regress` also runs the stock menu scripts of finding 25 on the patched image against their pinned frames.

PNGs of the DIGIKIT screens go to out/runs/lanes/r1-shots/ (a contact sheet per script).
"""

from __future__ import annotations

import argparse
import json
import pathlib
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

from tools import python_env
from tools.coldfire import arena_safety
from tools.firmware import device as device_files
from tools.patchkit import pk_hostdir

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
# The statics of the menu (the C globals of runtime/apps/digikit-menu/src/menu.c) are package statics: found through the host directory of the runtime, by the ids of abi.toml.
MENU_IDS = {
    "TICK": "MENU_TICK",
    "LOADED": "MENU_LOADED",
    "DK_LOAD_N": "MENU_LOAD_N",
    "DK_POPUP": "MENU_POPUP",
    "DK_POPUP_TEXT": "MENU_POPUP_TEXT",
    "DK_POPUPS": "MENU_POPUPS",
}
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
        forge(good, runtime_major=99)
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
    good: bytes, device: int | None = None, runtime_major: int | None = None
) -> bytes:
    """A copy of a good `.dkm` with a header field changed and the file sha256 made right again: the DSP section for another
    device or another runtime major in the version the module needs. The menu reads only these header fields."""
    import hashlib
    import struct

    b = bytearray(good)
    if device is not None:
        # a DSP section of its own (64 zero bytes, appended) with the device word at offset 8: the menu reads only that word
        b += b"\0" * (-len(b) % 16)
        struct.pack_into(">II", b, 0x98, len(b), 64)
        b += b"\0" * 64
        struct.pack_into(">I", b, struct.unpack_from(">I", b, 0x98)[0] + 8, device)
    if runtime_major is not None:
        b[0x31] = runtime_major
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


def cstr(b: bytes) -> str:
    return b.split(b"\0")[0].decode("latin1")


def stack_env(image: str, plan: Plan | None = None) -> dict[str, str]:
    """DIGI_STACK_WATCH on the UI task's stack (tools.coldfire.task_stacks)."""
    from tools.coldfire import task_stacks

    _, ts, pop = task_stacks.tasks(image)
    ui = task_stacks.ui_task(ts, pop)
    return {"DIGI_STACK_WATCH": "0x%x-0x%x" % (ui.base, ui.top)}


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
            f"{self.B.RT},core/ui", "menu"
        )  # the boot rig reads the voice rows: dynreg and dyndsp too
        self.plan = Plan(self.img.plan)

    # -- reads
    def addr(self, rig, name: str) -> int:
        """The address of a static: a plan symbol, or (the menu's) a package static through the host directory."""
        if name not in MENU_IDS:
            return self.plan.addr(name)
        return pk_hostdir.read_package_address(
            rig.w, self.plan.addr("DKRT_HOST_DIR"), MENU_IDS[name]
        )

    def w(self, rig, name: str, k: int = 0) -> int:
        return rig.w(self.addr(rig, name) + 4 * k)

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
            self.check(
                "rows: bad-sha, dyn-demo, foreign", self.w(rig, "DK_LOAD_N") == 3
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
    ap.add_argument("--only-regress", action="store_true", help="skip the DIGIKIT runs")
    a = ap.parse_args(argv)
    image = a.image
    if a.only_boot:
        work = OUT / image / "boot"
        work.mkdir(parents=True, exist_ok=True)
        return 0 if BootProof(image, a.update, work).run(a.quick) else 1
    syx, plan_path = patch(image, ["core/ui"], f"dkui-{image}")
    card = make_card(image)
    ok = True
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
