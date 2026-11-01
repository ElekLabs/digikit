#!/usr/bin/env python3
"""The `fxroute` kind of the digikit scenarios: the send page of the track FX key (FX routing step 3; docs/findings/64-fx-send-page.md).

Run by `tools/patchkit/pk_digikit_scen.py` (scenario `fx-route-dt2`). The image is the runtime with `core/dynfx` and `core/fxpages`; the sound is the run-time machine `test/sine` on
track 1, the effect `test/dyn-fx-gain-both` (the same gain and page as `test/dyn-fx-gain`, route fields master plus send; and, for the limit, the same file under two more names). The route of an effect is a setting (`fx/route/<module id>`): it is put on the card
before the boot, as the runtime's settings copy A (the record format of step 2), so no restart is needed. Boots, each in its own daemon:

  base    an image without `core/fxpages`, the sine alone: the screenshot of the FX key and the PCM that the send page must not change.
  stock   the image with the page machinery, no effect: the same screenshot pixel for pixel, the same PCM bit for bit, the list's count cell 0.
  master  the effect on the master route (no setting): the FX key is the stock one again; CAPACITY SEND_SLOTS has the device's limit and none used.
  send    the effect on the send route: the FX key has two pages (screenshots), the second has the effect's knob and the level; turning the knob on track 1 shows `<tab> Send=40`; the
          Sound array index of the knob takes the value, nothing else in the Sound object moves; the effect's amount for track 1 follows into the DSP row and the settings record
          (written about 2 s after the last change); the PCM carries the send (amount squared, as the stock sends); another track shows and edits its own amount.
  kit     Load Kit with the amounts set: the amounts come back from the settings (the DSP row never sees a 0 during the load, the settings keep them); another pattern's kit object gets them when it goes live.
  limit   three effects, all on the send route: the device allows two, the third stays on the master (route word 0, no cell); CAPACITY SEND_SLOTS says 2 of 2.
  lock    boots that record a trig on track 1 and hold it while a knob is turned (nothing; a filter knob; a knob of the first FX page; the send knob), then play the bar and save the project: the held turns of
          the stock knobs are p-locks (words of the track's DSP frame change during the bar), the send knob's is none (the same words as the trig alone, no lock data in pattern 1); a project saved with an amount
          has the same kit and sound bytes as one saved without; no MIDI CC leaves the send knob. The lock rule is also read from the firmware's own data (the class flag of the knob's template).

Screenshots go to the scenario's work folder; `DIGIKIT_SHOTS=DIR` copies them to DIR.
"""

from __future__ import annotations

import argparse
import json
import tempfile
from pathlib import Path
from typing import Any

import numpy as np

from tools.coldfire import arena_safety
from tools.device.dksettings import decode as decode_record
from tools.device.dksettings import encode as encode_record
from tools.emulator import card_overlay
from tools.patchkit import pk_device
from tools.patchkit import pk_digikit_project as P
from tools.patchkit import pk_dkrt_machine as M
from tools.patchkit.pk_digikit_debug import uploader
from tools.patchkit.pk_digikit_fx import SINE, chain_rows, scaled
from tools.patchkit.pk_digikit_fxpage import Shots, profile, u16
from tools.patchkit.pk_digikit_fxpersist import KIT_AREAS, Run
from tools.patchkit.pk_digikit_fxsend import GAIN, added_factor
from tools.patchkit.pk_digikit_settings import DELAY_MS, sector_bytes
from tools.patchkit.pk_harness import button

FX = "test/dyn-fx-gain-both"  # `route = "master"`, `routes = ["master", "send"]`: test/dyn-fx-gain allows the master only (finding 65), so a stored `send` would not hold
W_ROUTE, W_AMT0 = 18, 19  # row words of the send route (core/fx module.toml)
ENC_A, ENC_B = "ENC A", "ENC B"
SOUND_ARRAY = 0x14
AMOUNT = 40  # the amount the scenario turns to: the display says `Send=40`
CLASSES = {
    "dt2": 0x4293C040,
    "dn2": 0x4243425C,
}  # the second per-code table (value-class objects of 0x44 bytes; the accessor of core/params' anchor cf.param.fn.value_class)


def route_key(name: str) -> bytes:
    return b"fx/route/" + name.encode()


def send_key(name: str) -> bytes:
    return b"fx/send/" + name.encode()


def dsp_amount(rig: Any, slots: int, row: int, track: int) -> int:
    """The amount byte of TRACK in the DSP row ROW (words 19 to 22, little endian)."""
    w = chain_rows(rig, slots)[row][W_AMT0 + track // 4]
    return (w >> (8 * (track % 4))) & 0xFF


def run_fxroute(c: Any) -> None:
    from tools.patchkit.pk_digikit_scen import RT

    t = c.table()
    hint = t.get("hint", 7)
    trig = t.get("trig_frame", M.TRIG_FRAME)
    slots = pk_device.limits(c.device).fx_slots
    send_slots = pk_device.limits(c.device).send_slots
    cfg = profile(c.device)
    IDX = cfg[
        "send_indices"
    ]  # the Sound array indices of the send cells ([fx] send_indices)
    st = c.store
    img = c.image_for(RT + ",core/dynfx,core/fxpages", "fxroute")
    img0 = c.image_for(RT + ",core/dynfx", "fxroute-base")
    sine = c.pack(SINE, "m0.dkm")
    names = [FX, f"{FX}-2", f"{FX}-3"]
    files = []
    for k, name in enumerate(
        names
    ):  # the same effect under other names: a module is loaded once under its name
        out = c.env.folder / "digikit" / "modules" / f"m{k + 1}.dkm"
        c.env.tools.patchkit_run(
            "pack", FX, "--device", c.device, "--name", name, "-o", out
        )
        files.append(f"modules/m{k + 1}.dkm")
    fx1 = files[0]
    pcm: dict[str, np.ndarray] = {}
    hashes: dict[str, str] = {}

    def seeded(entries: list[tuple[bytes, bytes]], mods: list[str]) -> Path:
        """A card with MODS and the settings record (copy A, seq 1) holding ENTRIES."""
        rec = encode_record(1, entries)
        pad = rec.ljust(((len(rec) + 511) // 512) * 512, b"\0")
        return c.card(mods, foreign={st.set_a: pad})

    card_sine = c.card([sine])
    card_master = c.card([sine, fx1])
    card_send = seeded([(route_key(FX), b"\x01")], [sine, fx1])
    card_limit = seeded([(route_key(n), b"\x01") for n in names], [sine, *files])

    def current_record(card: Path) -> Any:
        """The settings record the runtime reads from CARD: the valid copy with the higher seq."""
        copies = [decode_record(sector_bytes(card, x)) for x in (st.set_a, st.set_b)]
        valid = [r for r in copies if r is not None]
        return max(valid, key=lambda r: r.seq) if valid else None

    def tap(rig: Any, keys: Any, ms: int = 300) -> None:
        rig.s.do_tap(keys, 20)
        rig.s.run_ms(ms)

    def sound_base(rig: Any, track: int) -> int:
        """The Sound object of TRACK (0 ..): inline in the live kit object, `[fx] sound_off` and `sound_stride`."""
        return rig.w(cfg["kit_ptr"]) + cfg["sound_off"] + cfg["sound_stride"] * track

    def sound_word(rig: Any, track: int, idx: int) -> int:
        return u16(rig, sound_base(rig, track) + SOUND_ARRAY + 2 * idx)

    def sound_image(rig: Any, track: int) -> list[int]:
        return [
            u16(rig, sound_base(rig, track) + 2 * i)
            for i in range(cfg["sound_stride"] // 2)
        ]

    def amount_shown(rig: Any, track: int, cell: int = 0) -> int:
        """What the page shows for the knob: the raw halfword / 256."""
        return sound_word(rig, track, IDX[cell]) >> 8

    def set_amount(rig: Any, enc: str, track: int, want: int, cell: int = 0) -> None:
        """Turn ENC until the knob shows WANT: bursts of fast events for the distance, single detents at the end."""
        for _ in range(200):
            got = amount_shown(rig, track, cell)
            if got == want:
                rig.s.run_ms(40)
                return
            err = want - got
            if abs(err) > 12:
                rig.s.turn_burst(enc, 8 if err > 0 else -8, 2)
            else:
                rig.s.turn_burst(enc, 1 if err > 0 else -1, 1, gap_ms=40)
            rig.s.run_ms(20)
        raise RuntimeError(
            f"turning {enc}: shows {amount_shown(rig, track, cell)}, wanted {want}"
        )

    def select_track(rig: Any, n: int) -> None:
        tap(rig, ["TRK", f"TRIG {n}"], 400)

    def sound(rig: Any, wav: Path, frame: int | None = None) -> np.ndarray:
        return rig.note(wav, frame if frame is not None else trig)

    def stock_screens(rig: Any, shots: Shots, tag: str) -> str:
        tap(rig, "FX", 400)
        hashes[tag] = shots.take(rig, "fx1")
        return hashes[tag]

    # ---- base, stock, master: nothing changes without a send effect ----
    def part_base() -> None:
        with c.rig(img0, card_sine, "base") as rig:
            rig.boot_info()
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            stock_screens(rig, Shots(c.work, "base"), "base")
            tap(rig, "NO", 300)
            pcm["base"] = sound(rig, c.work / "base.wav")
            c.done(rig)

    def part_stock() -> None:
        with c.rig(img, card_sine, "stock") as rig:
            info = rig.boot_info()
            c.check(
                "stock: the sine loaded", info["results"] == [0], f"{info['results']}"
            )
            cnt = rig.w(M.sym(rig.plan, "core/fxpages", "fxp_send_count"))
            c.check(
                "stock: no send effect, the track FX key's count cell is 0 (the stock list)",
                cnt == 0,
                f"{cnt}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            stock_screens(rig, Shots(c.work, "stock"), "stock")
            tap(rig, "NO", 300)
            pcm["stock"] = sound(rig, c.work / "stock.wav")
            cap = uploader(rig).capacity()["SEND_SLOTS"]
            c.check(
                "stock: CAPACITY SEND_SLOTS has the device's limit and none used",
                cap == {"limit": send_slots, "used": 0, "largest": 0},
                f"{cap}",
            )
            c.done(rig)

    def part_master() -> None:
        with c.rig(img, card_master, "master") as rig:
            info = rig.boot_info()
            c.check(
                "master: the sine and the effect loaded",
                info["results"] == [0, 0],
                f"{info['results']}",
            )
            row = chain_rows(rig, slots)[0]
            c.check(
                "master: the effect is on the master route with no amount (words 18 to 22 are 0)",
                row[0] != 0xFFFF_FFFF and row[W_ROUTE : W_AMT0 + 4] == (0,) * 5,
                f"{[hex(x) for x in row]}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            stock_screens(rig, Shots(c.work, "master"), "master")
            cap = uploader(rig).capacity()["SEND_SLOTS"]
            c.check(
                "master: CAPACITY SEND_SLOTS has the limit and none used",
                cap == {"limit": send_slots, "used": 0, "largest": 0},
                f"{cap}",
            )
            c.done(rig)

    # ---- send: the page, the knob, the amount, the DSP, the settings, the PCM, tracks ----
    def part_send() -> None:
        with c.rig(img, card_send, "send") as rig:
            info = rig.boot_info()
            c.check(
                "send: the sine and the effect loaded",
                info["results"] == [0, 0],
                f"{info['results']}",
            )
            row = chain_rows(rig, slots)[0]
            c.check(
                "send: row 0 of the chain holds the effect on the send route (word 18 = 1) with no amount",
                row[0] != 0xFFFF_FFFF
                and row[W_ROUTE] == 1
                and row[W_AMT0 : W_AMT0 + 4] == (0,) * 4,
                f"{[hex(x) for x in row]}",
            )
            # the lock rule, structurally: the firmware's lock test (FUN_4006a3d0) reads word 0 of the value-class object of the parameter's template (bit 0 or bit 3 set: no p-lock);
            # a knob row added by the registry answers with its template's object (core/params `pf_t2`). The send knobs' template is the Delay class, a stock send's own code is lockable.
            inf = M.sym(rig.plan, "core/fxpages", "fxpages_info")
            dri = M.sym(rig.plan, "core/dynreg", "dynreg_info")
            reg_rows, reg_size, reg_first, tpl_off = (
                rig.w(dri + 4 * i) for i in (21, 22, 23, 28)
            )
            classes = CLASSES[
                c.device
            ]  # the second per-code table: value-class objects of 0x44 bytes (core/params)

            def lockable(code: int) -> bool:
                return rig.w(classes + 0x44 * code) & 9 == 0

            code0 = rig.w(inf + 4 * 42)
            tpl = rig.w(reg_rows + (code0 - reg_first) * reg_size + tpl_off)
            row = rig.w(M.sym(rig.plan, "core/fxpages", "fxp_send_row") + 8)
            c.check(
                f"send: the knob row's template is the Delay class (code {tpl:#x}), whose class object has the no-p-lock flag; the stock send row {cfg['send_template']:#x} is lockable (the control); the page's cell holds the row's code",
                tpl == cfg["template_percent"]
                and not lockable(tpl)
                and lockable(cfg["send_template"])
                and row == code0,
                f"class words: template {rig.w(classes + 0x44 * tpl):#x}, stock send {rig.w(classes + 0x44 * cfg['send_template']):#x}; cell {row:#x} code {code0:#x}",
            )
            cnt = rig.w(M.sym(rig.plan, "core/fxpages", "fxp_send_count"))
            c.check(
                "send: the FX key's count cell is 2 (the send page follows the stock page)",
                cnt == 2,
                f"{cnt}",
            )
            cap = uploader(rig).capacity()["SEND_SLOTS"]
            c.check(
                "send: CAPACITY SEND_SLOTS has the limit and one used",
                cap == {"limit": send_slots, "used": 1, "largest": 0},
                f"{cap}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            shots = Shots(c.work, "send")
            tap(rig, "FX", 400)
            p1 = shots.take(rig, "page1")
            tap(rig, "FX", 400)
            p2 = shots.take(rig, "page2-track1")
            c.check(
                "send: the FX key has two pages now (the first differs from the stock one by the page count), the second shows the effect's knob",
                p1 != hashes.get("base") and p2 != p1,
                f"{p1[:8]} {p2[:8]} stock {hashes.get('base', '')[:8]}",
            )
            tap(rig, "FX", 400)
            c.check(
                "send: FX again leads back to the first page",
                shots.take(rig, "page1-again") == p1,
            )
            tap(rig, "FX", 400)
            # the Sound object before the turn: only the knob's index may move
            before = sound_image(rig, 0)
            c.check(
                "send: the knob reads 0 (no amount stored) on both Sound indices of the page",
                all(before[SOUND_ARRAY // 2 + i] == 0 for i in IDX),
                f"{before[SOUND_ARRAY // 2 + IDX[0]]:#x}",
            )
            set_amount(rig, ENC_A, 0, AMOUNT)
            shots.take(rig, "turning")
            after = sound_image(rig, 0)
            moved = [
                i for i, (x, y) in enumerate(zip(before, after, strict=True)) if x != y
            ]
            c.check(
                f"send: turning the knob moves Sound[{IDX[0]}] only (the halfword at {SOUND_ARRAY + 2 * IDX[0]:#x}): no other word of the Sound object changed (the record slot 0 included)",
                moved == [SOUND_ARRAY // 2 + IDX[0]],
                f"moved {[hex(2 * i) for i in moved]}",
            )
            rig.s.run_ms(300)
            c.check(
                f"send: the effect's amount for track 1 is {AMOUNT} in the DSP row, the other 15 are 0",
                dsp_amount(rig, slots, 0, 0) == AMOUNT
                and all(dsp_amount(rig, slots, 0, k) == 0 for k in range(1, 16)),
                f"{[hex(x) for x in chain_rows(rig, slots)[0][W_AMT0 : W_AMT0 + 4]]}",
            )
            tap(rig, "FX", 400)  # page 1
            tap(rig, "FX", 400)  # page 2 again: the idle page shows the value
            at40 = shots.take(rig, "page2-track1-40")
            # a second track has its own amount
            select_track(rig, 2)
            shots.take(rig, "page2-track2-after-select")
            t2 = amount_shown(rig, 1)
            c.check(
                "send: track 2 has no amount (its knob reads 0) while track 1 has 40",
                t2 == 0,
                f"{t2}",
            )
            tap(rig, "FX", 400)
            tap(rig, "FX", 400)
            shots.take(rig, "page2-track2-0")
            set_amount(rig, ENC_A, 1, 100)
            shots.take(rig, "turning-track2")
            rig.s.run_ms(300)
            c.check(
                "send: track 2's amount (100) is in the DSP row; track 1's is still 40",
                dsp_amount(rig, slots, 0, 1) == 100
                and dsp_amount(rig, slots, 0, 0) == AMOUNT,
                f"{dsp_amount(rig, slots, 0, 0)} {dsp_amount(rig, slots, 0, 1)}",
            )
            select_track(rig, 1)
            tap(rig, "FX", 400)
            tap(rig, "FX", 400)
            back = shots.take(rig, "page2-track1-back")
            c.check(
                "send: back on track 1 the page shows track 1's amount",
                back == at40 or amount_shown(rig, 0) == AMOUNT,
                f"{back[:8]} {at40[:8]} shown {amount_shown(rig, 0)}",
            )
            # the settings record: written about 2 s after the last change, to copy B
            rig.s.run_ms(DELAY_MS + 600)
            ov = rig.work / "send.after.bin"
            rig.emu.call("card.overlay", path=str(ov))
            after_card = P.card_after(c, card_send, ov)
            rec = current_record(after_card)
            got = dict(rec.entries) if rec else {}
            want = bytes([AMOUNT, 100]) + bytes(14)
            c.check(
                "send: the settings record on the card holds the amounts (40 on track 1, 100 on track 2) and the route; it was written (seq above the seed's 1)",
                rec is not None
                and rec.seq > 1
                and got.get(send_key(FX)) == want
                and got.get(route_key(FX)) == b"\x01",
                f"{rec and rec.seq} {got}",
            )
            writes = set(rig.card_store_writes())
            c.check(
                "send: the store sectors the guest wrote are the selection copies and the two settings copies (the first sector of each: the record is small)",
                writes <= {st.set_a, st.set_b, 0x2E19FFE, 0x2E19FFF}
                and bool(writes & {st.set_a, st.set_b}),
                f"{[hex(x) for x in sorted(writes)]}",
            )
            # the PCM: track 1 plays; the send adds GAIN * (40/127)^2
            tap(rig, "NO", 300)
            pcm["send"] = sound(rig, c.work / "send.wav", rig.frames() + 300)
            c.done(rig)

    def part_dry() -> None:
        with c.rig(img, card_sine, "dry") as rig:
            rig.boot_info()
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            pcm["dry"] = sound(rig, c.work / "dry.wav")
            c.done(rig)

    # ---- kit: Load Kit gives the amounts back ----
    def part_kit() -> None:
        with c.rig(img, card_send, "kit") as rig:
            rig.boot_info()
            run = Run(c, rig, cfg)
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            tap(rig, "FX", 400)
            tap(rig, "FX", 400)
            set_amount(rig, ENC_A, 0, AMOUNT)
            run.save_kit(1, "k")
            select_track(rig, 2)
            tap(rig, "FX", 400)
            tap(rig, "FX", 400)
            set_amount(rig, ENC_A, 1, 100)
            rig.s.run_ms(
                DELAY_MS + 600
            )  # the record is on the card; the amounts: 40 and 100
            seen: list[tuple[int, int]] = []
            n0 = rig.w(M.sym(rig.plan, "core/fxpages", "fxp_nload"))
            run.kit_menu(0)
            run.tap("YES", 700)
            run.slot(1)
            emu = rig.emu
            emu.button(button("YES"), True)
            for ms in range(0, 700, 2):
                rig.s.run_ms(2)
                w0 = chain_rows(rig, slots)[0][W_AMT0]
                if not seen or seen[-1][1] != w0:
                    seen.append((ms, w0))
                if ms == 60:
                    emu.button(button("YES"), False)
            rig.s.run_ms(500)
            loaded = rig.w(M.sym(rig.plan, "core/fxpages", "fxp_nload")) != n0
            fxs = M.sym(rig.plan, "core/runtime", "DKRT_FXS")
            c.check(
                "kit: Load Kit went through the loader (the hook counted it) and the runtime wrote the amounts back",
                loaded and rig.w(fxs + 16) > 0,
                f"nload {loaded} restores {rig.w(fxs + 16)}",
            )
            c.check(
                "kit: after the load Sound[send index] of track 1 shows 40 and of track 2 100 again (restored from the settings, not zero)",
                amount_shown(rig, 0) == AMOUNT and amount_shown(rig, 1) == 100,
                f"{amount_shown(rig, 0)} {amount_shown(rig, 1)}",
            )
            c.check(
                "kit: the DSP row never held a 0 amount word during the load (the words seen: only the same value)",
                all(w == seen[0][1] for _, w in seen)
                and seen[0][1] & 0xFFFF == (100 << 8 | AMOUNT),
                f"{[(ms, hex(w)) for ms, w in seen]}",
            )
            # another pattern has a kit object of its own (made before the runtime existed, no amount in it): the amounts are written into it when it goes live
            run.tap("PTN", 800)
            run.tap("TRIG 2", 300)
            rig.s.run_ms(1500)
            c.check(
                "kit: pattern A02's kit (another kit object) shows the same amounts (restored into it when it went live), and the DSP row did not change",
                amount_shown(rig, 0) == AMOUNT
                and amount_shown(rig, 1) == 100
                and dsp_amount(rig, slots, 0, 0) == AMOUNT
                and dsp_amount(rig, slots, 0, 1) == 100,
                f"{amount_shown(rig, 0)} {amount_shown(rig, 1)} live kit {rig.w(cfg['kit_ptr']):#x}",
            )
            run.tap("PTN", 800)
            run.tap("TRIG 1", 1500)
            c.check(
                "kit: back on pattern A01 the amounts are there too",
                amount_shown(rig, 0) == AMOUNT and amount_shown(rig, 1) == 100,
                f"{amount_shown(rig, 0)} {amount_shown(rig, 1)}",
            )
            rig.s.run_ms(DELAY_MS + 600)
            ov = rig.work / "kit.after.bin"
            rig.emu.call("card.overlay", path=str(ov))
            after_card = P.card_after(c, card_send, ov)
            rec = current_record(after_card)
            got = dict(rec.entries) if rec else {}
            c.check(
                "kit: the settings still hold 40 and 100 after the load (the zeroes of the loader were not stored)",
                got.get(send_key(FX), b"")[:2] == bytes([AMOUNT, 100]),
                f"{got}",
            )
            c.done(rig)

    # ---- limit: three effects on the send route, the device allows send_slots ----
    def part_limit() -> None:
        with c.rig(img, card_limit, "limit") as rig:
            info = rig.boot_info()
            c.check(
                f"limit: the sine and {len(names)} effects loaded",
                info["results"] == [0] * (1 + len(names)),
                f"{info['results']}",
            )
            rows = chain_rows(rig, slots)
            routes = [r[W_ROUTE] for r in rows[: len(names)]]
            c.check(
                f"limit: the first {send_slots} effects run on the send route, the next stays on the master (route words {[1] * send_slots + [0] * (len(names) - send_slots)})",
                routes == [1] * send_slots + [0] * (len(names) - send_slots),
                f"{routes}",
            )
            cap = uploader(rig).capacity()["SEND_SLOTS"]
            c.check(
                f"limit: CAPACITY SEND_SLOTS is {send_slots} of {send_slots}",
                cap == {"limit": send_slots, "used": send_slots, "largest": 0},
                f"{cap}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            shots = Shots(c.work, "limit")
            tap(rig, "FX", 400)
            tap(rig, "FX", 400)
            shots.take(rig, "page2")
            set_amount(rig, ENC_A, 0, 20, 0)
            if send_slots >= 2:
                set_amount(rig, ENC_B, 0, 30, 1)
            rig.s.run_ms(300)
            a, b = dsp_amount(rig, slots, 0, 0), dsp_amount(rig, slots, 1, 0)
            c.check(
                (
                    "limit: the two knobs set their own effects' amounts (20 for the first, 30 for the second); the third row has none"
                    if send_slots >= 2
                    else "limit: the one knob sets its effect's amount (20); the second and third row have none (no cell: they stay on the master)"
                ),
                a == 20
                and (b == 30 if send_slots >= 2 else b == 0)
                and dsp_amount(rig, slots, 2, 0) == 0,
                f"{a} {b} {dsp_amount(rig, slots, 2, 0)}",
            )
            c.done(rig)

    # ---- lock: the send knob leaves no p-lock, no MIDI CC, and no Sound record byte ----
    results: dict[str, Any] = {}

    def part_lock(tag: str, turn: str) -> None:
        """TURN: "base" no trig; "none" a trig; "amount" no trig, the knob turned live to AMOUNT; "send" a trig held while the send knob turns."""
        with c.rig(img, card_send, f"lock-{tag}") as rig:
            rig.boot_info()
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            s = rig.s
            shots = Shots(c.work, f"lock-{tag}")
            tap(rig, "NO", 300)  # out of the machine list
            if turn in ("send", "amount"):
                tap(rig, "FX", 400)
                tap(rig, "FX", 400)
            shots.take(rig, "page")
            trig_held = turn in ("none", "send")
            if trig_held:
                tap(rig, "RECORD", 200)
                tap(rig, "TRIG 1", 200)
                s.emu.button(button("TRIG 1"), True)
                s.run_ms(100)
            rig.emu.midi_out()  # drain
            if turn == "amount":
                set_amount(rig, ENC_A, 0, AMOUNT)
            elif turn == "send":
                s.do_turn(ENC_A, 11)
            s.run_ms(300)
            shots.take(rig, "turned")
            cc = rig.emu.midi_out()
            if trig_held:
                s.emu.button(button("TRIG 1"), False)
                s.run_ms(300)
                tap(rig, "RECORD", 300)
            shown = amount_shown(rig, 0)
            tap(rig, ["FUNC", "SETTINGS"], 400)
            tap(rig, "YES", 700)  # the slot list: slot 001
            tap(rig, "YES", 500)  # the name of the new project
            rig.s.run_ms(3500)
            ov = (
                c.work / f"lock-{tag}.overlay.bin"
            )  # (P.export names the file by the boot's tag, which these boots share)
            rig.emu.call("card.overlay", path=str(ov))
            c.done(rig)
        results[tag] = (ov, cc, shown)

    parts: dict[str, Any] = {
        "base": part_base,
        "stock": part_stock,
        "master": part_master,
        "dry": part_dry,
        "send": part_send,
        "kit": part_kit,
        "limit": part_limit,
        "lock-base": lambda: part_lock("base", "base"),
        "lock-none": lambda: part_lock("none", "none"),
        "lock-amount": lambda: part_lock("amount", "amount"),
        "lock-send": lambda: part_lock("send", "send"),
    }
    only = t.get(
        "only"
    )  # a debugging aid: run these parts alone (the checks across boots are skipped)
    c.parallel(*[f for n, f in parts.items() if not only or n in only])
    extra = frozenset({st.set_a, st.set_b})

    def across() -> None:
        c.check(
            "stock: the FX key is pixel for pixel the key of an image without core/fxpages",
            hashes["stock"] == hashes["base"],
            f"{hashes['stock'][:8]} {hashes['base'][:8]}",
        )
        c.check(
            "master: with an effect on the master route the FX key is still the stock one, pixel for pixel",
            hashes["master"] == hashes["base"],
            f"{hashes['master'][:8]} {hashes['base'][:8]}",
        )
        r = scaled(pcm["base"], pcm["stock"], 1.0)
        c.check(
            "stock: the PCM equals the PCM of an image without core/fxpages, bit for bit from the onset on",
            bool(r["identical"]) and abs(r["shift"]) <= 128,
            f"onset shift {r['shift']}, max err {r.get('max_abs_err')}",
        )
        e = added_factor(pcm["dry"], pcm["send"])
        want = GAIN * (AMOUNT / 127) ** 2
        c.check(
            f"send: the PCM carries the send: the dry PCM plus {GAIN} x the track x ({AMOUNT}/127)^2 = {want:.5f} (measured {e:.5f})",
            abs(e / want - 1.0) < 0.02,
            f"added factor {e:.5f} want {want:.5f}",
        )

    if not only or {"base", "stock", "master", "dry", "send"} <= set(only):
        across()

    def lock_checks() -> None:
        touched: set[int] = set()
        for ov, _, _ in results.values():
            touched |= set(P.slot_sectors(ov))
        imgs = {
            k: P.slot_image(P.card_after(c, card_send, v[0]), touched)
            for k, v in results.items()
        }
        trig = P.pattern0_changes(imgs["base"], imgs["none"])
        c.check(
            "lock: the panel script records a trig (pattern 1 differs from a project saved with no trig: the control of the method)",
            len(trig) > 0,
            f"{len(trig)} bytes differ: {trig[:6]}",
        )
        snd = P.pattern0_changes(imgs["none"], imgs["send"])
        c.check(
            "lock: after a trig-held turn of the send knob pattern 1 is as the trig alone left it (no lock data saved; consistent with the class flag above, but no panel script of this harness produced a p-lock on a stock knob, so this is not a proof by itself [O])",
            len(snd) == 0,
            f"{len(snd)} bytes differ: {snd[:6]}",
        )
        # (a project also saves the view state: a byte of the pool says that the FX key was visited, so the project with the amount is compared with the one that visited it without)
        d = P.diff_slot(imgs["send"], imgs["amount"], c.device, P.STAMP)
        c.check(
            f"lock: the kits and sound records of a project saved with an amount of {AMOUNT} on track 1 are the same bytes as those of a project saved with none (the FX key visited in both; nothing of the amount is saved in an Elektron file)",
            results["amount"][2] == AMOUNT
            and d["differ_bytes"].get("kit", 0) + d["differ_bytes"].get("pool", 0) == 0,
            f"shown {results['amount'][2]}, {d['differ_bytes']}",
        )
        c.check(
            "lock: the send knob sends no MIDI CC (nothing leaves the MIDI output while it turns)",
            results["send"][1] == b"",
            f"{results['send'][1].hex()}",
        )

    if {"base", "none", "amount", "send"} <= set(results):
        lock_checks()
    c.common_checks(KIT_AREAS[c.device], store_extra=extra)


def lock_check(
    device: str,
    card: Path,
    base: Path,
    none: Path,
    amount: Path,
    send: Path,
) -> dict[str, Any]:
    """The legacy lock comparisons on complete base-card images with each overlay applied."""
    overlays = {"base": base, "none": none, "amount": amount, "send": send}
    touched: set[int] = set()
    for overlay in overlays.values():
        touched.update(P.slot_sectors(overlay))
    with tempfile.TemporaryDirectory(prefix="fxroute-lock-", dir=card.parent) as work:
        images = {}
        for name, overlay in overlays.items():
            after = Path(work) / f"{name}.img"
            card_overlay.apply(card, overlay, after)
            images[name] = P.slot_image(after, touched)
    return {
        "trig_changed_bytes": len(P.pattern0_changes(images["base"], images["none"])),
        "send_changed_bytes": len(P.pattern0_changes(images["none"], images["send"])),
        "amount_diff": P.diff_slot(images["send"], images["amount"], device, P.STAMP),
    }


def overlay_writes(overlay: Path, device: str) -> dict[str, Any]:
    """Written store sectors, including settings and selection copies, as in the legacy proof."""
    return {"store_sectors": sorted(set(arena_safety.card_store_hits(overlay, device)))}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Track FX routing offline comparison helpers"
    )
    commands = parser.add_subparsers(dest="command", required=True)
    lock = commands.add_parser("lock-check")
    lock.add_argument("--device", choices=("dt2", "dn2"), required=True)
    lock.add_argument("--card", type=Path, required=True)
    for name in ("base", "none", "amount", "send"):
        lock.add_argument(f"--{name}", type=Path, required=True)
    writes = commands.add_parser("overlay-writes")
    writes.add_argument("overlay", type=Path)
    writes.add_argument("--device", choices=("dt2", "dn2"), required=True)
    args = parser.parse_args(argv)
    try:
        result = (
            lock_check(
                args.device, args.card, args.base, args.none, args.amount, args.send
            )
            if args.command == "lock-check"
            else overlay_writes(args.overlay, args.device)
        )
    except (OSError, ValueError) as exc:
        parser.exit(1, f"error: {exc}\n")
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
