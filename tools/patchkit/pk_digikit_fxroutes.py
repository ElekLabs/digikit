#!/usr/bin/env python3
"""The `fxroutes` kind of the digikit scenarios: SETTINGS > DIGIKIT > MODULES > side panel > ROUTE... and the manifest's route fields (FX routing step 4; docs/findings/65-fx-routes.md).

Run by `tools/patchkit/pk_digikit_scen.py` (scenarios `fx-routes-dt2` and `fx-routes-dn2`). The image is the runtime with `core/dynfx`, `core/fxpages` and `core/digikit-menu` (and `core/ui` that it requires); the sound is the run-time machine
`test/sine`, the effects are `test/dyn-fx-gain` (no route fields: master only, the module before the fields), `test/dyn-fx-gain-send` (`route = "send"`: allowed send and master, default
send) and `test/dyn-fx-gain-sendonly` (`routes = ["send"]`), and the first again under another name. The settings record (`fx/route/<module id>`) is put on the card before a boot,
as the runtime's settings copy A (step 2's record format), where a part needs a stored route. All five files are in the card's store, so MODULES has five rows (`m0` to `m4`, in name order).
The side panel of a loaded effect has the row ROUTE..., which opens a choice of the routes that the effect allows (SEND, MASTER) and that the free send slots leave; the menu keeps the panel's rows
(`DK_PANEL`), the routes the choice offered (`DK_ROUTE_ITEM`) and the pending routes (`DK_FXR_PEND`). Boots, each in its own daemon:

  dry       the sine alone: the reference PCM.
  none      no effect: no row of MODULES has a ROUTE... panel row (screenshot).
  list      a master-only and a send-default effect, no stored route: the send one starts as a send effect (route word, FX key page, CAPACITY); Function + FX lists the send effect before the master one (the
            list the runtime hands the firmware, and the screenshots); the pending routes are MASTER and SEND; ROUTE... on the master-only one offers MASTER only and changes nothing; on the other it
            offers SEND and MASTER, MASTER is a pending change (NO asks APPLY & RESTART? with 1 CHANGE, NO answers it and drops the change), SEND again is no change.
  sendonly  a send-only effect: ROUTE... offers SEND only, the pending route does not change.
  fallback  stored routes that the effect does not allow (send for a master-only one, master for a send-only one): the default holds.
  full      one effect more than the device has send slots (three on the Digitakt II, two on the Digitone II): ROUTE... on the last offers MASTER only (no free slot, nothing changes); the pending routes count, so moving a send effect to the master frees the slot.
  apply     a send-default effect: PCM = dry (send, no amounts); ROUTE... to MASTER, NO, YES on APPLY & RESTART? (the settings are written and the device restarts); after the restart the
            route is the master (row word, no send page on the FX key, PCM = dry x gain) and `patchkit store dump` shows the key.
  back      the same effect with a stored `master`: the other way, to SEND, after a restart (row word, FX key page 2, `patchkit store dump`, PCM = dry). A boot of its own: two restarts inside the first
            30 s of a session are two unstable boots in a row, and the third boot would be safe mode.

Screenshots go to the scenario's work folder; `DIGIKIT_SHOTS=DIR` copies them to DIR (`step4-<name>.png`).
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
from pathlib import Path
from typing import Any

import numpy as np

from tools.coldfire import digikit_menu_proof as D
from tools.control.digi_client import oled_png
from tools.device import dksettings
from tools.device.dksettings import encode as encode_record
from tools.patchkit import pk_device
from tools.patchkit import pk_digikit_project as P
from tools.patchkit import pk_dkrt_machine as M
from tools.patchkit.pk_digikit_debug import uploader
from tools.patchkit.pk_digikit_fx import SINE, chain_rows, scaled
from tools.patchkit.pk_digikit_fxpage import Shots, open_send_fx, page_down
from tools.patchkit.pk_digikit_fxpage import profile as fx_profile
from tools.patchkit.pk_digikit_fxpersist import KIT_AREAS
from tools.patchkit.pk_harness import text_rows

W_ROUTE = 18  # row word of the route (core/fx module.toml)
# Boots differ in when the key press reaches the DSP: the onset moves in 64-sample sub-block steps between separate boots
# (measured 0 to 192 on DN2 for one tree). The checks are about bit-exact PCM after alignment; this bound only catches a gross delay.
ONSET_JITTER = 256
GAIN, SEND, SENDONLY = (
    "test/dyn-fx-gain",
    "test/dyn-fx-gain-send",
    "test/dyn-fx-gain-sendonly",
)
SEND3 = SEND + "-3"
MASTER, SENDR = 1, 2  # the routes as the menu and DkrtFx hold them
# DKRT_FXS (core/runtime fxsend.rs `SendState`): 10 words, then `allow` and `dflt`, one byte per effect row
FXS_ALLOW, FXS_DFLT = 40, 44


def route_key(name: str) -> bytes:
    return b"fx/route/" + name.encode()


def run_fxroutes(c: Any) -> None:
    from tools.patchkit.pk_digikit_scen import RT

    t = c.table()
    hint = t.get("hint", 7)
    trig = t.get("trig_frame", M.TRIG_FRAME)
    slots = pk_device.limits(c.device).fx_slots
    send_slots = pk_device.limits(c.device).send_slots
    st = c.store
    img = c.image_for(RT + ",core/dynfx,core/fxpages,core/digikit-menu", "fxroutes")
    sine = c.pack(SINE, "m0.dkm")
    gain = c.pack(GAIN, "m1.dkm")
    snd = c.pack(SEND, "m2.dkm")
    so = c.pack(SENDONLY, "m3.dkm")
    out = c.env.folder / "digikit" / "modules" / "m4.dkm"
    c.env.tools.patchkit_run(
        "pack", SEND, "--device", c.device, "--name", SEND3, "-o", out
    )
    snd3 = "modules/m4.dkm"
    pcm: dict[str, np.ndarray] = {}

    def seeded(entries: list[tuple[bytes, bytes]], mods: list[str]) -> Path:
        """A card with MODS and the settings record (copy A, seq 1) holding ENTRIES."""
        rec = encode_record(1, entries)
        pad = rec.ljust(((len(rec) + 511) // 512) * 512, b"\0")
        return c.card(mods, foreign={st.set_a: pad})

    card_dry = c.card([sine])
    card_none = c.card([])
    card_list = c.card([sine, gain, snd])
    card_so = c.card([so, gain])
    card_fb = seeded(
        [(route_key(GAIN), b"\x01"), (route_key(SENDONLY), b"\x00")], [gain, so]
    )
    # the effects of the `full` boot: one more than the device has send slots (the second slot of the Digitakt II goes to the send-only effect)
    full_mods = [snd, so, snd3] if send_slots > 1 else [snd, snd3]
    full_routes = [1] * send_slots + [0]
    full_pending = [SENDR] * send_slots + [MASTER]
    card_full = seeded([(route_key(SEND3), b"\x00")], full_mods)
    card_apply = c.card([sine, snd])
    card_back = seeded([(route_key(SEND), b"\x00")], [sine, snd])

    def tap(rig: Any, keys: Any, ms: int = 300) -> None:
        rig.s.do_tap(keys, 20)
        rig.s.run_ms(ms)

    def sym(rig: Any, owner: str, name: str) -> int:
        return M.sym(rig.plan, owner, name)

    def pending(rig: Any) -> list[int]:
        w = rig.w(sym(rig, "core/digikit-menu", "DK_FXR_PEND"))
        return [(w >> (24 - 8 * k)) & 0xFF for k in range(4)]

    def popup(rig: Any) -> tuple[str, str]:
        """The title and the text of the last dialog the menu opened."""
        title = text_rows(rig.w, sym(rig, "core/digikit-menu", "DK_POPUP"), 1, 48)[0]
        text = text_rows(rig.w, sym(rig, "core/digikit-menu", "DK_POPUP_TEXT"), 1, 48)[
            0
        ]
        return title, text

    def popups(rig: Any) -> int:
        return rig.w(sym(rig, "core/digikit-menu", "DK_POPUPS"))

    def applies(rig: Any) -> int:
        """Times APPLY & RESTART was answered with YES."""
        return rig.w(sym(rig, "core/digikit-menu", "DK_APPLIES"))

    def to_modules(rig: Any) -> None:
        """SETTINGS > DIGIKIT (the ninth row) > MODULES (the fourth row)."""
        tap(rig, "NO", 400)
        tap(rig, "SETTINGS", 600)
        for _ in range(8):
            tap(rig, "DOWN", 200)
        tap(rig, "YES", 700)
        for _ in range(3):
            tap(rig, "DOWN", 200)
        tap(rig, "YES", 900)

    def goto(rig: Any, row: int) -> None:
        """The cursor on MODULES row ROW (the list has five rows)."""
        for _ in range(5):
            tap(rig, "UP", 150)
        for _ in range(row):
            tap(rig, "DOWN", 150)

    def panel_rows(rig: Any) -> list[str]:
        base = sym(rig, "core/digikit-menu", "DK_PANEL")
        return text_rows(
            rig.w, base, rig.w(sym(rig, "core/digikit-menu", "DK_PANEL_N")), 24
        )

    def open_panel(rig: Any, row: int) -> list[str]:
        """RIGHT on MODULES row ROW: the side panel; its rows."""
        goto(rig, row)
        tap(rig, "RIGHT", 400)
        return panel_rows(rig)

    def route_choice(rig: Any, row: int) -> list[int]:
        """The panel of an effect, ROUTE... (its second row): the choice opens; the routes it offers, in the order shown (1 master, 2 send)."""
        open_panel(rig, row)
        tap(rig, "DOWN", 250)
        tap(rig, "YES", 500)
        n = rig.w(sym(rig, "core/digikit-menu", "DK_ROUTE_N"))
        w = rig.w(sym(rig, "core/digikit-menu", "DK_ROUTE_ITEM"))
        return [(w >> (24 - 8 * k)) & 0xFF for k in range(n)]

    def pick(rig: Any, index: int) -> None:
        """Item INDEX of the open choice."""
        for _ in range(index):
            tap(rig, "DOWN", 250)
        tap(rig, "YES", 500)

    def leave_menu(rig: Any) -> None:
        """From the MODULES list without pending changes: back to the DIGIKIT menu, SETTINGS and the main screen."""
        for _ in range(3):
            tap(rig, "NO", 400)

    def routes(rig: Any, n: int) -> list[int]:
        return [r[W_ROUTE] for r in chain_rows(rig, slots)[:n]]

    def count_cell(rig: Any) -> int:
        return rig.w(sym(rig, "core/fxpages", "fxp_send_count"))

    def row_flags(rig: Any, n: int) -> tuple[list[int], list[int]]:
        """The routes each effect row allows and its default, as the runtime kept them from the records (1 master, 2 send)."""
        fxs = sym(rig, "core/runtime", "DKRT_FXS")
        a, d = rig.w(fxs + FXS_ALLOW), rig.w(fxs + FXS_DFLT)
        return (
            [(a >> (24 - 8 * k)) & 0xFF for k in range(n)],
            [(d >> (24 - 8 * k)) & 0xFF for k in range(n)],
        )

    def send_used(rig: Any) -> dict[str, int]:
        return uploader(rig).capacity()["SEND_SLOTS"]

    def sine_pcm(rig: Any, wav: Path) -> np.ndarray:
        rig.pick(rig.dyn_type(SINE, "sine"), hint)
        tap(rig, "NO", 300)
        return rig.note(wav, max(trig, rig.frames() + 300))

    def fx_page_order(rig: Any) -> list[int]:
        """The page numbers of the FX view's list (core/fxpages `fxpages_info`: list address, count cell), the stock pages first."""
        inf = sym(rig, "core/fxpages", "fxpages_info")
        first, lst = rig.w(inf + 4 * 5), rig.w(inf + 4 * 7)
        n = rig.w(rig.w(inf + 4 * 9))  # word 9 is the address of the count cell
        return [rig.w(lst + 4 * i) - first for i in range(n)]

    # ---- dry, none ----
    def part_dry() -> None:
        with c.rig(img, card_dry, "dry") as rig:
            rig.boot_info()
            pcm["dry"] = sine_pcm(rig, c.work / "dry.wav")
            c.done(rig)

    def part_none() -> None:
        with c.rig(img, card_none, "none") as rig:
            rig.boot_info()
            shots = Shots(c.work, "step4")
            to_modules(rig)
            rows = [open_panel(rig, r) for r in (0, 1, 2)]
            shots.take(rig, "none")
            c.check(
                "none: with no effect loaded no row of MODULES has a ROUTE... panel row",
                rows == [["LOAD", "INFO"]] * 3,
                f"{rows}",
            )
            c.done(rig)

    # ---- list ----
    def part_list() -> None:
        with c.rig(img, card_list, "list") as rig:
            info = rig.boot_info()
            c.check(
                "list: the sine and the two effects loaded",
                info["results"] == [0, 0, 0],
                f"{info['results']}",
            )
            allow, dflt = row_flags(rig, 2)
            c.check(
                "list: the records say what the manifests say: test/dyn-fx-gain (no route fields) allows the master only and defaults to it, test/dyn-fx-gain-send allows both and defaults to send",
                allow == [MASTER, MASTER | SENDR] and dflt == [MASTER, SENDR],
                f"allow {allow} default {dflt}",
            )
            c.check(
                "list: with no stored route the manifest's default holds: the master-only effect is on the master chain (route word 0), the send one starts as a send effect (route word 1)",
                routes(rig, 2) == [0, 1],
                f"{routes(rig, 2)}",
            )
            c.check(
                "list: the FX key's count cell is 2 (the send page exists) and CAPACITY SEND_SLOTS has one used",
                count_cell(rig) == 2
                and send_used(rig) == {"limit": send_slots, "used": 1, "largest": 0},
                f"{count_cell(rig)} {send_used(rig)}",
            )
            order = fx_page_order(rig)
            c.check(
                "list: the list of the FX view is the three stock pages, then the send effect's page (row 1), then the master effect's (row 0)",
                len(order) == 5 and order[3:] == [1, 0],
                f"{order}",
            )
            shots = Shots(c.work, "step4")
            open_send_fx(rig)
            shots.take(rig, "fxview-1")
            for k in range(2, 6):
                page_down(rig)
                shots.take(rig, f"fxview-{k}")
            leave_menu(rig)
            to_modules(rig)
            sine_row, gain_row, snd_row = 0, 1, 2  # the files m0, m1, m2
            p_sine = open_panel(rig, sine_row)
            tap(rig, "NO", 300)  # NO closes the panel
            p_gain = open_panel(rig, gain_row)
            tap(rig, "NO", 300)
            c.check(
                "list: the side panel of a loaded tool has UNLOAD and INFO, the panel of an effect has ROUTE... too",
                p_sine == ["UNLOAD", "INFO"]
                and p_gain == ["UNLOAD", "ROUTE...", "INFO"],
                f"{p_sine} {p_gain}",
            )
            c.check(
                "list: the pending routes are the routes that hold: MASTER for the master-only effect, SEND for the send-default one",
                pending(rig)[:2] == [MASTER, SENDR],
                f"{pending(rig)}",
            )
            n0 = popups(rig)
            items = route_choice(rig, gain_row)
            shots.take(rig, "master-only")
            c.check(
                "list: ROUTE... on the master-only effect offers MASTER only",
                items == [MASTER],
                f"{items}",
            )
            pick(rig, 0)
            tap(rig, "NO", 500)  # nothing changed: back to the DIGIKIT menu at once
            c.check(
                "list: choosing it changes nothing: NO goes back and asks nothing",
                pending(rig)[:2] == [MASTER, SENDR] and popups(rig) == n0,
                f"{pending(rig)} {popups(rig)} {n0}",
            )
            tap(rig, "YES", 900)  # MODULES again
            items = route_choice(rig, snd_row)
            shots.take(rig, "route-choice")
            c.check(
                "list: ROUTE... on the send-default effect offers SEND and MASTER",
                items == [SENDR, MASTER],
                f"{items}",
            )
            pick(rig, 1)  # MASTER
            c.check(
                "list: MASTER is a pending change; nothing is saved",
                pending(rig)[:2] == [MASTER, MASTER] and applies(rig) == 0,
                f"{pending(rig)} {applies(rig)}",
            )
            tap(rig, "NO", 700)
            shots.take(rig, "apply-question")
            c.check(
                "list: NO with the change pending asks `APPLY & RESTART?` (1 CHANGE)",
                popups(rig) == n0 + 1
                and popup(rig) == ("APPLY & RESTART?", "1 CHANGE"),
                f"{popups(rig)} {popup(rig)}",
            )
            tap(
                rig, "NO", 700
            )  # NO on the question drops the change and closes the list
            c.check(
                "list: NO on the question drops the pending route (SEND again)",
                pending(rig)[:2] == [MASTER, SENDR],
                f"{pending(rig)}",
            )
            tap(rig, "YES", 900)  # the DIGIKIT menu is on top: MODULES again
            route_choice(rig, snd_row)
            pick(rig, 1)
            route_choice(rig, snd_row)
            pick(rig, 0)  # back to SEND
            tap(rig, "NO", 500)
            c.check(
                "list: SEND again is no change: NO asks nothing",
                pending(rig)[:2] == [MASTER, SENDR] and popups(rig) == n0 + 1,
                f"{pending(rig)} {popups(rig)}",
            )
            c.done(rig)

    # ---- sendonly ----
    def part_sendonly() -> None:
        with c.rig(img, card_so, "sendonly") as rig:
            info = rig.boot_info()
            c.check(
                "sendonly: both effects loaded",
                info["results"] == [0, 0],
                f"{info['results']}",
            )
            allow, dflt = row_flags(rig, 2)
            c.check(
                'sendonly: test/dyn-fx-gain-sendonly allows send only (`routes = ["send"]`)',
                allow == [SENDR, MASTER] and dflt == [SENDR, MASTER],
                f"allow {allow} default {dflt}",
            )
            to_modules(rig)
            shots = Shots(c.work, "step4")
            c.check(
                "sendonly: the pending routes are SEND (the send-only effect), MASTER",
                pending(rig)[:2] == [SENDR, MASTER],
                f"{pending(rig)}",
            )
            n0 = popups(rig)
            items = route_choice(rig, 3)  # the file m3 holds the send-only effect
            shots.take(rig, "send-only")
            c.check(
                "sendonly: ROUTE... on the send-only effect offers SEND only",
                items == [SENDR],
                f"{items}",
            )
            pick(rig, 0)
            tap(rig, "NO", 500)
            c.check(
                "sendonly: choosing it changes nothing: NO asks nothing",
                pending(rig)[:2] == [SENDR, MASTER] and popups(rig) == n0,
                f"{pending(rig)} {popups(rig)}",
            )
            c.done(rig)

    # ---- fallback ----
    def part_fallback() -> None:
        with c.rig(img, card_fb, "fallback") as rig:
            info = rig.boot_info()
            c.check(
                "fallback: both effects loaded",
                info["results"] == [0, 0],
                f"{info['results']}",
            )
            c.check(
                "fallback: a stored `send` for the master-only effect and a stored `master` for the send-only one are not allowed: the defaults hold (route words 0 and 1)",
                routes(rig, 2) == [0, 1] and send_used(rig)["used"] == 1,
                f"{routes(rig, 2)} {send_used(rig)}",
            )
            to_modules(rig)
            c.check(
                "fallback: the pending routes are the routes that hold: MASTER, SEND",
                pending(rig)[:2] == [MASTER, SENDR],
                f"{pending(rig)}",
            )
            c.done(rig)

    # ---- full ----
    def part_full() -> None:
        with c.rig(img, card_full, "full") as rig:
            n = len(full_mods)
            info = rig.boot_info()
            c.check(
                f"full: the {n} effects loaded",
                info["results"] == [0] * n,
                f"{info['results']}",
            )
            c.check(
                f"full: {send_slots} effect{'s' if send_slots > 1 else ''} run on the send route and the stored `master` holds for the last (route words {full_routes}); CAPACITY SEND_SLOTS is {send_slots} of {send_slots}",
                routes(rig, n) == full_routes
                and send_used(rig)
                == {"limit": send_slots, "used": send_slots, "largest": 0},
                f"{routes(rig, n)} {send_used(rig)}",
            )
            to_modules(rig)
            shots = Shots(c.work, "step4")
            first, last = (
                2,
                4,
            )  # the files m2 (the first send effect) and m4 (the last effect)
            c.check(
                f"full: the pending routes are the routes that hold ({full_pending})",
                pending(rig)[:n] == full_pending,
                f"{pending(rig)}",
            )
            items = route_choice(rig, last)  # every slot is taken: no SEND
            shots.take(rig, "no-send-slot")
            c.check(
                "full: ROUTE... on the last effect offers MASTER only (no send slot is free)",
                items == [MASTER],
                f"{items}",
            )
            pick(rig, 0)
            c.check(
                "full: choosing it changes nothing",
                pending(rig)[:n] == full_pending,
                f"{pending(rig)}",
            )
            items = route_choice(rig, first)  # a send effect may always stay on send
            c.check(
                "full: ROUTE... on an effect that runs on send offers SEND and MASTER",
                items == [SENDR, MASTER],
                f"{items}",
            )
            pick(rig, 1)  # the first one to the master: a slot is free
            items = route_choice(rig, last)
            c.check(
                "full: the pending routes count: after the first goes to the master the last may go to send",
                items == [SENDR, MASTER],
                f"{items}",
            )
            pick(rig, 0)  # now the last takes it
            shots.take(rig, "pending-counts")
            c.check(
                "full: the pending routes are the first on the master and the last on send",
                pending(rig)[:n] == [MASTER, *full_pending[1:-1], SENDR],
                f"{pending(rig)}",
            )
            tap(rig, "NO", 700)
            c.check(
                "full: NO asks `APPLY & RESTART?` (2 CHANGES)",
                popup(rig) == ("APPLY & RESTART?", "2 CHANGES"),
                f"{popup(rig)}",
            )
            c.done(rig)

    # ---- apply ----
    def dump_route(rig: Any, base: Path, tag: str) -> str | None:
        """The value of `fx/route/<SEND>` that `patchkit store dump` shows on the card as the guest left it (None: no such key)."""
        ov = rig.work / f"{tag}.after.bin"
        rig.emu.call("card.overlay", path=str(ov))
        after = P.card_after(c, base, ov)
        text = c.env.tools.patchkit_run(
            "store", "dump", after, "--device", c.device, "--json"
        )
        d = json.loads(text)["settings"]
        cur = d["copies"][0 if d["active"] == "A" else 1]
        for e in cur["entries"]:
            if e["key"] == route_key(SEND).decode():
                return str(e["value"])
        return None

    def apply_flow(
        rig: Any, shots: Shots, tag: str, pick_index: int, want: int
    ) -> None:
        """On the MODULES list with one effect (the file m2): ROUTE... and the item PICK_INDEX, then NO asks APPLY & RESTART? (nothing is written yet),
        YES writes the route and restarts; waits for the restart."""
        items = route_choice(rig, 2)
        shots.take(rig, f"{tag}-choice")
        pick(rig, pick_index)
        c.check(
            f"apply {tag}: ROUTE... offered SEND and MASTER and the choice is a pending change",
            items == [SENDR, MASTER] and pending(rig)[0] == want,
            f"{items} {pending(rig)}",
        )
        done0 = applies(rig)
        tap(rig, "NO", 700)
        shots.take(rig, f"{tag}-question")
        c.check(
            f"apply {tag}: NO asks `APPLY & RESTART?` (1 CHANGE) and writes nothing",
            popup(rig) == ("APPLY & RESTART?", "1 CHANGE") and applies(rig) == done0,
            f"{popup(rig)}",
        )
        rig.emu.drain_events()
        tap(rig, "YES", 300)  # YES: put, flush, restart
        c.check(
            f"apply {tag}: YES wrote the route (the settings put and the flush answered 0)",
            applies(rig) == done0 + 1
            and rig.w(sym(rig, "core/digikit-menu", "DK_FXR_SET")) == 0
            and rig.w(sym(rig, "core/digikit-menu", "DK_FXR_FLUSH")) == 0,
            f"{applies(rig)}",
        )
        D._wait_restart(rig)

    def part_apply() -> None:
        with c.rig(img, card_apply, "apply") as rig:
            info = rig.boot_info()
            c.check(
                "apply: the sine and the send-default effect loaded",
                info["results"] == [0, 0],
                f"{info['results']}",
            )
            c.check(
                "apply: it starts as a send effect: route word 1, the send page's count cell 2, one send slot used",
                routes(rig, 1) == [1]
                and count_cell(rig) == 2
                and send_used(rig)["used"] == 1,
                f"{routes(rig, 1)} {count_cell(rig)} {send_used(rig)}",
            )
            pcm["send0"] = sine_pcm(rig, c.work / "send0.wav")
            shots = Shots(c.work, "step4")
            tap(rig, "FX", 400)
            fx_a = shots.take(rig, "apply-fx1-send")
            tap(rig, "FX", 400)
            fx_b = shots.take(rig, "apply-fx2-send")
            c.check(
                "apply: the FX key has a second page while the effect is routed send",
                fx_a != fx_b,
                f"{fx_a[:8]} {fx_b[:8]}",
            )
            to_modules(rig)
            shots.take(rig, "apply-start")
            c.check(
                "apply: the pending route is SEND",
                pending(rig)[0] == SENDR,
                f"{pending(rig)}",
            )
            apply_flow(rig, shots, "to-master", 1, MASTER)
            info = rig.boot_info()
            c.check(
                "apply: after the restart the module loaded again",
                info["results"] == [0, 0] and info["safe"] == 0,
                f"{info['results']}",
            )
            c.check(
                "apply: the route is the master now: route word 0, the count cell 0 (no send page), no send slot used",
                routes(rig, 1) == [0]
                and count_cell(rig) == 0
                and send_used(rig)["used"] == 0,
                f"{routes(rig, 1)} {count_cell(rig)} {send_used(rig)}",
            )
            c.check(
                "apply: `patchkit store dump` on the card as the guest left it shows `fx/route/test/dyn-fx-gain-send` = master",
                dump_route(rig, card_apply, "r1") == "master",
                f"{dump_route(rig, card_apply, 'r1')}",
            )
            pcm["master"] = sine_pcm(rig, c.work / "master.wav")
            tap(rig, "FX", 400)
            shots.take(rig, "apply-fx1-master")
            tap(rig, "FX", 400)
            shots.take(rig, "apply-fx1-master-again")
            to_modules(rig)
            shots.take(rig, "apply-master")
            c.check(
                "apply: the saved route is MASTER and NO asks nothing",
                pending(rig)[0] == MASTER,
                f"{pending(rig)}",
            )
            c.done(rig)

    def part_back() -> None:
        """The other way, on its own boot: a second restart within 30 s of the session would be the second unstable boot in a row, and safe mode."""
        with c.rig(img, card_back, "back") as rig:
            info = rig.boot_info()
            c.check(
                "back: the stored `master` holds: route word 0, no send page, no send slot used",
                info["results"] == [0, 0]
                and routes(rig, 1) == [0]
                and count_cell(rig) == 0
                and send_used(rig)["used"] == 0,
                f"{info['results']} {routes(rig, 1)} {count_cell(rig)} {send_used(rig)}",
            )
            shots = Shots(c.work, "step4")
            to_modules(rig)
            c.check(
                "back: the pending route is MASTER",
                pending(rig)[0] == MASTER,
                f"{pending(rig)}",
            )
            apply_flow(rig, shots, "to-send", 0, SENDR)
            info = rig.boot_info()
            c.check(
                "back: after the restart the module loaded again",
                info["results"] == [0, 0] and info["safe"] == 0,
                f"{info['results']}",
            )
            c.check(
                "back: the route is send now: route word 1, the count cell 2 (the send page), one send slot used",
                routes(rig, 1) == [1]
                and count_cell(rig) == 2
                and send_used(rig)["used"] == 1,
                f"{routes(rig, 1)} {count_cell(rig)} {send_used(rig)}",
            )
            c.check(
                "back: `patchkit store dump` shows the key as send (written to copy B, seq 2)",
                dump_route(rig, card_back, "r2") == "send",
                f"{dump_route(rig, card_back, 'r2')}",
            )
            pcm["send1"] = sine_pcm(rig, c.work / "send1.wav")
            tap(rig, "FX", 400)
            shots.take(rig, "back-fx1-send")
            tap(rig, "FX", 400)
            shots.take(rig, "back-fx2-send")
            c.done(rig)

    parts = {
        "dry": part_dry,
        "none": part_none,
        "list": part_list,
        "sendonly": part_sendonly,
        "fallback": part_fallback,
        "full": part_full,
        "apply": part_apply,
        "back": part_back,
    }
    only = t.get("only")
    c.parallel(*[f for n, f in parts.items() if not only or n in only])

    def across() -> None:
        r = scaled(pcm["dry"], pcm["send0"], 1.0)
        c.check(
            "apply: on the send route with no amount the PCM is the dry PCM, bit for bit from the onset on",
            bool(r["identical"]) and abs(r["shift"]) <= ONSET_JITTER,
            f"onset shift {r['shift']}, max err {r.get('max_abs_err')}",
        )
        r = scaled(pcm["dry"], pcm["master"], 0.5)
        c.check(
            "apply: after the restart on the master route the PCM is the dry PCM times the effect's gain 0.5 (the effect is in the master chain), within 2 ulps",
            r["rms_db"] is not None
            and r["max_err_ulps"] <= 2.0
            and abs(r["rms_db"] - 20 * np.log10(0.5)) < 0.001
            and abs(r["shift"]) <= ONSET_JITTER,
            f"rms {r['rms_db']}, max err {r['max_err_ulps']:.2f} ulps, shift {r['shift']}",
        )
        r = scaled(pcm["dry"], pcm["send1"], 1.0)
        c.check(
            "back: after the restart on the send route again (no amount) the PCM is the dry PCM again",
            bool(r["identical"]) and abs(r["shift"]) <= ONSET_JITTER,
            f"onset shift {r['shift']}, max err {r.get('max_abs_err')}",
        )

    if not only or {"dry", "apply", "back"} <= set(only):
        across()
    c.common_checks(KIT_AREAS[c.device], store_extra=frozenset({st.set_a, st.set_b}))


def profile(device: str) -> dict[str, Any]:
    """Device facts used by the FX routing acceptance scenarios (sectors, hi exclusive)."""
    limits = pk_device.limits(device)
    store = pk_device.store_of(device)
    return {
        "track_type_word": M.TRACK_OBJ_WORD,
        "fx_slots": limits.fx_slots,
        "send_slots": limits.send_slots,
        "settings": {"copy_a": store.set_a, "copy_b": store.set_b},
        "kit_areas": KIT_AREAS[device],
        "fx": fx_profile(device),
    }


def seed_card(card: Path, device: str, entries: Any) -> dict[str, Any]:
    """Seed copy A on an existing caller-owned card with empty settings copies."""
    if not isinstance(entries, list):
        raise ValueError("entries must be a JSON array of {key, value_hex} objects")
    pairs: list[tuple[bytes, bytes]] = []
    for entry in entries:
        if (
            not isinstance(entry, dict)
            or set(entry) != {"key", "value_hex"}
            or not isinstance(entry["key"], str)
            or not isinstance(entry["value_hex"], str)
        ):
            raise ValueError("each entry must contain string key and value_hex fields")
        try:
            key = entry["key"].encode("ascii")
            value = bytes.fromhex(entry["value_hex"])
        except (UnicodeError, ValueError) as exc:
            raise ValueError("entry needs an ASCII key and hexadecimal value") from exc
        if not dksettings.valid_key(key) or len(value) > dksettings.VALUE_MAX:
            raise ValueError("entry exceeds settings key or value limits")
        pairs.append((key, value))
    record = encode_record(1, pairs)
    reason = dksettings.why(record)
    if reason != "ok":
        raise ValueError(f"invalid settings record: {reason}")
    padded = record.ljust(((len(record) + 511) // 512) * 512, b"\0")
    store = pk_device.store_of(device)
    if len(padded) > store.set_copy_sectors * 512:
        raise ValueError("settings record exceeds the device's copy area")
    with card.open("r+b") as file:
        file.seek(0, 2)
        if file.tell() < (store.set_b + store.set_copy_sectors) * 512:
            raise ValueError("card is too small for the device settings copies")
        for sector in (store.set_a, store.set_b):
            file.seek(sector * 512)
            if any(file.read(store.set_copy_sectors * 512)):
                raise ValueError("card settings copies must be empty before seeding")
        file.seek(store.set_a * 512)
        file.write(padded)
    return {
        "card": str(card),
        "sector": store.set_a,
        "bytes": len(padded),
        "seq": 1,
        "entries": len(pairs),
    }


def render_shots(directory: Path) -> dict[str, Any]:
    """Render saved raw OLED frames, copying diagnostics as the legacy Shots helper does."""
    if not directory.is_dir():
        raise ValueError(f"not a shots directory: {directory}")
    frames = [(path, path.read_bytes()) for path in sorted(directory.glob("*.oled"))]
    for path, raw in frames:
        if len(raw) != 1024:
            raise ValueError(f"{path}: expected 1024 OLED bytes, got {len(raw)}")
    copy_dir = Path(out) if (out := os.environ.get("DIGIKIT_SHOTS")) else None
    if copy_dir is not None:
        copy_dir.mkdir(parents=True, exist_ok=True)
    files = []
    for path, raw in frames:
        destination = path.with_suffix(".png")
        destination.write_bytes(oled_png(raw, 3))
        if copy_dir is not None:
            copied = copy_dir / destination.name
            if copied.resolve() != destination.resolve():
                shutil.copy(destination, copied)
        files.append(str(destination))
    return {"files": files, "count": len(files)}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="FX routing scenario data helpers")
    commands = parser.add_subparsers(dest="command", required=True)
    facts = commands.add_parser("profile")
    facts.add_argument("--device", choices=("dt2", "dn2"), required=True)
    seed = commands.add_parser("seed-card")
    seed.add_argument("card", type=Path)
    seed.add_argument("--device", choices=("dt2", "dn2"), required=True)
    seed.add_argument("--entries", type=Path, required=True)
    shots = commands.add_parser("render-shots")
    shots.add_argument("directory", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "profile":
            result = profile(args.device)
        elif args.command == "render-shots":
            result = render_shots(args.directory)
        else:
            result = seed_card(
                args.card, args.device, json.loads(args.entries.read_text())
            )
    except (OSError, ValueError) as exc:
        parser.exit(1, f"error: {exc}\n")
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
