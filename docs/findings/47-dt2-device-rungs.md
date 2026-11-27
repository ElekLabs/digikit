# 47. Digitakt II on the device: the flash rungs

Em's Digitakt II, OS 1.17, 2026-10-18. Images and steps: docs/PATCHING.md "Flashing a Digitakt II, rung by rung"; plan
`docs/plans/digikit-runtime.md` Phase 9. Marks: **[D]** measured once on the device, **[O]** open. The log lines are what
`tools/device/dklog.py follow --once` printed over USB (the runtime's LOG command, finding 46).

## Rung A: the runtime with no DSP change **[D]**

Image `out/patch/rungA-dt2-1.17.syx` (`core/runtime,core/digikit-menu,core/health`; sha256 `01e8cae4a713f5a7...`; preflight: section 3
only, sections 2, 4, 5, 7 and 8 identical to stock), flashed through SETTINGS > SYSTEM > OS UPGRADE with Transfer.

- It boots to the main screen; SETTINGS lists DIGIKIT; the log arrives over USB SysEx.
- `BOOT_START runtime=3 device=1`, `UI_FIRST blocks=4481` (2.99 s of firmware blocks after the hook; the coupled emulator: 5,972 on a
  blank card, finding 37), `STABLE blocks=49485` (the 30 s period ran out), no safe mode, nothing loaded, `DSP_ABSENT` (no `dl_diag`
  on this rung, as designed).
- **The module store range on this card is blank:** `CARD_CHECK state=blank first_nonblank=None read_errors=0` (all of sectors
  0x38000-0x3E000 read through the firmware's block read; finding 46). The runtime wrote nothing to the card (a blank card gets the
  record only when the selection changes).
- `RAM_KEEP kept=0 generation=0`: the first boot after a flash has no marker. Whether RAM survives a soft reset on this board is still
  **[O]** (the menu offers APPLY & RESTART only with a store; rung C).
- HEALTH, steady over 70 s with the device idle at the main screen: CPU 27.3-28.1 % (relative to the boot's idle floor, finding 41;
  the emulator reads 20-34 %), DSP 40.8 % average, 44.2 % maximum (EMUCLK block time at the assumed 1 GHz), heap free 27,448 KiB
  (emulator 26.7 MB). The DSP figure is about 1.3 times the emulator's 31 % on an idle sample card, inside the DT2 cycle factors of
  `devices/cycle-factors.json` (1.13 nominal, 1.41 high); with the same project on both sides it would calibrate the factor, or show the
  clock is not 1 GHz **[O]**.

## Rung B: with the DSP link and the voice hook, no modules **[D]**

Image `out/patch/rungB-dt2-1.17.syx` (rung A plus `core/rows,core/dyndsp`, which add `core/rows`, `core/rows`, `core/dsp-link`,
`core/dsp-voice`; sha256 `7d2851ee8f7460eb...`; preflight: sections 3 and 7). Em played a project with sample tracks for about a minute and
previewed a sample; `dklog follow` ran for 172 s.

- The DSP ran the whole session: no `DSP_STOPPED`; both heartbeats of `dl_diag` (finding 45) rose by exactly 1500 per second (the link
  hook's word 1 and the voice hook's word 2: the hook ran every block on stock voices).
- **The context registers at the voice hook on silicon equal the emulator's, all eleven**, constant over the session:
  M5 0, M6 1, M7 -1, M13 0, M14 1, M15 -1, I6 `0x2cb2c8`, I7 `0x2cb2c0`, B7 `0x2ca5a0`, L7 `0x3e2`, MODE1 `0x39011cf8`
  (`uv run python -m tools.patchkit.pk_ctxprobe --device dt2 --fw 1.17 --modules core/dsp-voice,core/dsp-link` gives the same words). So the
  2026-10 hardware report's two causes (M6 and M13 not having these values) did not hold at this hook on this device, which agrees with
  finding 45; what halted the SHARC there is still **[O]**.
- `RAM_KEEP kept=1 generation=1`: the keep marker that rung A's boot wrote survived the reboot that ends the OS update, so RAM above
  `cf.expansion` (`[digikit] keep_base`) is kept across that reset on this board. That the same holds for the runtime's own soft reset is
  likely but not yet seen.
- HEALTH: idle CPU about 30 %, DSP 41.5 % average idle (rung A: 40.8 %; the hook and link cost about 0.7 points), 55 % average and 59.6 %
  maximum while the pattern played; CPU 95-100 % for about 20 s while the project loaded. Heap free 27,416-27,449 KiB.
- The card check again reported the store range blank; nothing was written.

## Rung C: pluck loaded at run time, the SHARC halts **[D]**

Image `out/patch/rungC-dt2-1.17.syx` (sha256 `fb6caf04...`, with the breadcrumbs of finding 48), module `out/patch/dyn-pluck-dt2.dkm`
(sha256 `e66c4660...`). Logs `out/runs/device-logs/rungC-{1,2,3}.log` (not committed).

- **Upload over our SysEx works on the device:** `dkupload put` wrote the 4,484-byte module into the blank store range with the sha256
  checked on the device; `ls` lists it. The first card write by our code on this card.
- **The menu and boot load work:** MODULES tick, APPLY & RESTART; the next boot read `CARD_CHECK state=ours`, `BOOT_RECORD seq=1
  selected=1`, `MOD_LOAD result=1` and 0.1 s after the first UI pass `MOD_DSP result=0` (the DSP part transferred and acknowledged);
  ABOUT `BOOT: 1 OF 1 OK`.
- **RAM survives the runtime's own soft reset:** `RAM_KEEP kept=1 generation=3` after APPLY & RESTART (generation 2 was the update reboot).
- **Then the SHARC halted, without our routine ever being called.** Em selected PLUCK on track 1 and held TRIG 1: no sound. Both
  heartbeats froze at about 151.4 s of the session (link beat 221,802 at 143.4 s, then 233,838 and no further: 8.02 s later), and the
  watch logged `DSP_STOPPED` 4 s after. At the halt: `crumb=HOOK_ENTERED voice=31 type=0`, **`dispatches=0 returns=0` for the whole
  session**, the eleven context registers unchanged. So `core/dsp-voice` never saw type 18 on any voice and never called the pluck
  routine; its last breadcrumb is the one every normal block ends with (voice 31). The halt is outside the wrapper's machine path.
- **A halted core's reply records keep arriving** (valid header, frozen beats), so the breadcrumbs are readable after a halt on silicon:
  the DMA keeps sending (finding 48's open question, answered). The two records differ by one block (233,838 and 233,839); the watch
  read the larger one as a beat and logged a false `resumed=True` at 171 s, then stopped again (a watch bug: compare the larger of the two
  records). **[C]** Fixed in `dlog.rs`: after a stop the samples keep the larger heartbeat of the two records as the reference, and only a heartbeat
  16 blocks past it counts as running again (`RESUME_MARGIN`; a running DSP adds 1500 a second). **[D]** `debug-hang` checks that no `resumed=True` follows a stop
  (the emulator's two records differ too, 2 sets); the device case itself is not re-run.
- The ColdFire stayed healthy (UI, USB, log, HEALTH) after the halt, as in the 2026-10 hardware report.
- Reading **[O]**: the same flow passes in the emulator (scenario `rung-c`, where the hook dispatches type 18 thousands of times). On the
  device the type never reached the hook, which points at stock DSP code that routes or indexes by machine type before the voice hook,
  reading something the emulator has differently (for example a per-type table sized for the 7 stock types, read out of range, where the
  emulator's memory is zero and the device's is not). Not yet checked.
- **After the halt (Em, 2026-10-18, no log kept):** a power cycle with the module still selected and loaded, and the saved project still
  holding type 18 on track 1: Em set track 1 back to ONESHOT, then selecting a sample froze the whole device (the halted-SHARC sign of the
  2026-10 hardware report), so the DSP was most likely dead again before any note. After another power cycle, with track 1 on a stock
  machine and the module still loaded, SLICE and ONESHOT played and auditioned normally. So a track holding the added type is enough; a
  note and the routine are not needed. Whether the first halt came at the selection or at TRIG 1 is not known **[O]**.
- **Page 2 of PLUCK** drew almost blank on the device (the TUNE glyph and the bar indicators); encoders there show page 1's names. Cause
  and fix: finding 50 (a CF view chooses the page's centre widget by type with a mask for types 0-6; reproduced in the emulator).

## Rung C2: the bisect (added types skip the wrapper's call path) **[D]**

Image `out/patch/rungC2-dt2-1.17.syx` (sha256 `3a34dcf6...`; `DV_BISECT_C2`, finding 49 section 8), the same module and selection. Log
`out/runs/device-logs/rungC2.log` (not committed).

- `MACHINE_CHANGE track=1 old_type=0 new_type=18` at 18.3 s: selecting the machine alone changed nothing on the DSP (`dispatches` 0).
- TRIG 1 at about 33.5 s: from then on the hook saw type 18 every block (`crumb=STOCK_PATH_TAKEN voice=1 type=18`, `dispatches` rising
  3,000 a second: the two voices of track 1), for over 40 s; no `DSP_STOPPED`, heartbeats rising.
- **So the stock pipeline accepts a track on an added type, and the halt of rung C is inside core/dsp-voice's path for a registered
  added type before its stage-2 breadcrumb** (the part C2 removes: finding 49 sections 3 and 8). The type reaches the hook at the first
  note, as in the emulator (finding 49's open "earlier on the device" is not borne out by this run).

## Rung C3a: delayed CALL/RTS, fine breadcrumbs **[D]**

Image `out/patch/rungC3a-dt2-1.17.syx` (sha256 `56d1f994...`; `DV_C3_FINE,DV_C3_BFIRST,DV_C3_DB`, finding 49 section 9). Log
`out/runs/device-logs/rungC3a.log` (not committed). `MACHINE_CHANGE track=1 0 -> 18` at 14.6 s; at the first TRIG 1 (about 20 s) the
breadcrumb reached **`C3_TYPE_MATCHED voice=0 type=18`** and the SHARC stopped (`DSP_STOPPED` at 23.3 s, `dispatches=0`); no sound.
So the halt is **between fine stage 8 and stage 9**: the registered path's `MODIFY(I7, -40)` and its 35 frame stores (including
`R3 = DM(_dv_sv_l4)` and `R3 = MODE1`; finding 49 section 9.2, offsets 127-199). The `CALL` (after stage 10) is not reached, so the
delayed forms could not change the result; C3a does not test them.

## Rung C3d2: the 17 stores through R2 **[D]** (2026-10-19)

Image `out/patch/rungC3d2-dt2-1.17.syx` (sha256 `18d7a6d5...`; `DV_C3_FINE,DV_C3_BFIRST,DV_C3_VIAR`, finding 49 section 10), the same module (`dyn-pluck-dt2.dkm`,
`e66c4660...`) and selection. Log `out/runs/device-logs/rungC3d2.log` (not committed).

- Boot as before (`CARD_CHECK state=ours`, `RAM_KEEP kept=1`, `MOD_LOAD dyn-pluck-dt result=1`, `MOD_DSP result=0`).
- `MACHINE_CHANGE track=1 old_type=0 new_type=18` at 18.1 s. From the next report on: `crumb=EPILOGUE_DONE voice=1 type=18`, `dispatches` = `returns`
  (693, 15,693, 30,743, 45,799 at 22, 32, 42, 52 s: about 1,500 a second), heartbeats rising, no `DSP_STOPPED`. The context words stayed the rung B values.
- HEALTH: `dsp_avg_pct` 41.5 before the machine change, 41.6-42.2 while it played; `dsp_max_pct` 45.5 at most.
- Em: the pluck sounds on TRIG 1 and its parameters respond. Page 2 of SRC showed the knob page again (the interim fix of finding 50), which Em
  read as a bug; added machines now have one SRC page (finding 50, the section on one page; not yet on the device).
- So the halt of rung C was the excluded stores (finding 49 section 10.7).

## Rung C4: the default build and one SRC page **[D]** (2026-10-19)

Image `out/patch/rungC4-dt2-1.17.syx` (sha256 `cc9828c8...`; the default `core/dsp-voice`, no variant option, and the one-page SRC hook of finding 50), the
same module (`e66c4660...`, not re-uploaded). Log `out/runs/device-logs/rungC4.log` (not committed; it holds two boots, the second is the run).

- `MACHINE_CHANGE track=1 0 -> 18` at 9.6 s; `crumb=EPILOGUE_DONE voice=1 type=18`, `dispatches` = `returns` (691, 15,691, 30,744), no `DSP_STOPPED`.
- `MACHINE_CHANGE track=1 18 -> 0` at 42.6 s; the counts stop at 43,024 (= returns) and the stock path runs on (`HOOK_ENTERED type=0`, heartbeats rising).
- Em: the pluck plays and the switch back to ONESHOT works. So the default build matches C3d2 on the device.
