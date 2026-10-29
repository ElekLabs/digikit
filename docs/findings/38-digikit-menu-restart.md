# 38. The DIGIKIT menu for boot-time loading: selection, APPLY & RESTART, safe mode

Lane r1, 2026-10-17, native emulator (coupled daemon with the RCM soft reset), all four images (DT2 1.16, 1.17, DN2 1.11, 1.12). Everything
is **[D]**: measured here once; no second agent has checked it. Nothing ran on a device. Design: `docs/plans/digikit-runtime.md` decisions 9
to 11 and Phase 6; the runtime side is finding 37. Earlier menu: findings 28 and 34 (the live LOAD and UNLOAD rows of those are gone).

## What the menu does now

`modules/core/apps/digikit-menu` no longer calls `dkrt_load` or `dkrt_unload`. It calls `dkrt_selection_get`, `dkrt_selection_set`,
`dkrt_restart`, `dkrt_boot_info` (the C ABI of finding 37), `dkrt_status` and `dkrt_module`.

- **MODULES** lists the store's `.dkm` files. A row is `[X]` when it is in the pending selection, `[ ]` when not, `[-]` (greyed, cannot be
  ticked) when its header says another device or a newer runtime (finding 34). The fourth character is `*` when this boot loaded the
  module (`dkrt_module`), `L` while its DSP part is still going. The pending selection starts as the saved one
  (`dkrt_selection_get`), not as what is loaded; ticking edits only the pending selection. A saved path the store no longer has is counted
  and dropped by the next apply.
- **APPLY & RESTART** is a row only when the pending selection differs from the saved one (or a saved file is gone, or this boot is in safe
  mode). The first press shows a popup (the firmware's own PopupWindow, finding 34): `SAVE FIRST! RESTART LOSES UNSAVED. PRESS AGAIN`, and the row reads
  `CONFIRM RESTART`; any other action disarms it. The second press calls `dkrt_selection_set` and `dkrt_restart`. A refusal of either is
  shown as a popup and a message row (`NOT SAVED: <text>`, `NO RESTART: CARD BUSY`); `dkrt_restart` only returns when it refuses. The
  firmware's yes/no window (`ConfirmWindow`, used by "Repair filesystem") was not looked at: a second press needs no new anchors **[O]**.
- **CLEAR SELECTION** (when anything is ticked) unticks all; APPLY & RESTART then saves the empty selection.
- **Safe mode.** When `dkrt_boot_info` reports it, the DIGIKIT view has a first row `SAFE: <reason>` (`2 BAD BOOTS`, `KEY HELD`, `REQUESTED`), a
  popup `SAFE MODE: <reason>` opens once per boot when DIGIKIT is opened (the row shows it again), MODULES keeps the saved selection ticked
  and offers APPLY & RESTART (a normal boot loads the selection; the safe boot cleared the flag and the counter) or CLEAR SELECTION first.
- **ABOUT**: `RUNTIME ABI n`, `SAFE: <reason>` when it applies (`SAFE MODE: <reason>` before runtime 0.5.3), `BOOT: m OF n OK`, then for each module of the saved selection its
  name and `- <result>` (`OK`, `LOADING`, or the text of the code: tables `TEXT_RUNTIME`, `TEXT_REGISTRY`, `TEXT_DSP` and the new `TEXT_BOOT` for -40..-44;
  unknown codes `ERROR <n>`), then `FREE n KB`. The names come from the saved selection, which is what the boot loaded unless it was saved
  again without a restart **[O]** (`DkrtBootInfo` has no paths).

## Proof

`uv run python -m tools.coldfire.digikit_menu_proof run --image IMAGE [--quick] [--update]`: the two OLED scripts of the earlier proof (main, ABOUT,
`regions_probe`) and `BootProof` (coupled daemon of `pk_dkrt_boot.Env`/`BootRig`, write-watch and flush check on, the card of a store with
`dyn-demo`, `bad-sha` and a forged `foreign` module); `--only-boot` runs only the second, `--no-boot` only the first; `--quick` leaves out the
bad-module scenario. Checks read the menu's memory (`DK_OFFER`, `DK_POPUP`, `DK_POPUPS`, `DK_APPLY_SET`, `TICK`, `LOADED`, `NOK`), the runtime's
boot info and the card's selection sectors, and pin the OLED frames (`tests/golden/digikit_menu_frames.json`, keys `IMAGE/boot/NAME`).

| scenario | result |
| --- | --- |
| no selection: rows `bad-sha`, `dyn-demo`, `foreign` (greyed) | APPLY not offered (`DK_OFFER` 0) |
| tick dyn-demo | pending `[0,1,0]`, the card's selection still empty, APPLY offered |
| first press of APPLY & RESTART | one popup `SAVE FIRST! RESTART LOSES UNSAVED. PRESS AGAIN`, nothing saved |
| second press | `dkrt_selection_set` 0; the emulator's soft reset; the next boot: `count 1 loaded 1 results [0]`, the record in use names `modules/dyn-demo.dkm` |
| after the boot | ABOUT shows `BOOT: 1 OF 1 OK`, `DYN-DEMO.DKM`, `- OK`; the list shows `[X]*`, APPLY not offered |
| safe mode (record counter 2) | `safe 1 reason 1`; a popup `SAFE MODE: 2 BAD BOOTS` at DIGIKIT; the selection ticked, APPLY offered; two presses restart into a normal boot that loads it |
| a selection with `bad-sha` | the boot's result is -4; ABOUT shows `BAD SHA` |
| card, watch | store-range writes only in the two selection sectors (`0x3dffe`, `0x3dfff`); 0 write events over `cf.expansion` (staging and record buffers left out), 0 flush hits |

The firmware's own card writes outside the store range (`0x50000...`, boot housekeeping) are not part of that claim. The old `digikit_gate` and
`digikit_errors` scripts and the live LOAD/UNLOAD rows are removed with the hot path.

## Stack

With the boot loads on the UI task's stack, the deepest store over the two probe scripts is 5772 bytes of 160 KiB on every image (finding 34
measured 3256 for a hot load); the deepest PC is the runtime's `memcpy`. Margin 158 068 bytes.

## Open

- **[O]** The popup confirm is two presses, not the firmware's yes/no window.
- **[O]** The menu does not call the firmware's project save; the popup text tells the user to save first (a restart loses unsaved changes, finding 40).
- **[O]** Safe mode by the key (RECORD held) and by the record's flag were not driven from the menu proof (counter only); `SAFE_FLAG` and `SAFE_KEY` have texts.
- **[O]** The restart is the emulator's model; the refusal (`E_CARD_BUSY`) is not exercised there (finding 37).
- **[O]** More than 32 `.dkm` rows are not listed; more than 8 ticks (`SEL_MAX`) are dropped by apply without a message.
