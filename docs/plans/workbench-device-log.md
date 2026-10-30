# Workbench Log: the runtime log of the emulator and of the device (2026-10-30)

Status: built on lane `ft-sdk`, not committed; host checks, `cargo check` of the desktop app and the screenshots are done. Nothing ran against a real device [O].
Approved by Em on 2026-10-30. The web app is out of scope.

## What it does

The Log drawer of the desktop app (`apps/desktop/ui/src/LogPanel.tsx`) shows two more origins next to the app's own entries:

- **emulator**: the runtime log of the running emulator.
- **device**: the runtime log of the real device on the MIDI port last chosen in Transfer. It is followed also while Transfer is set to Emulator.

Header: `Log  N entries  [Source] [Errors only] [Follow] [Save] [Copy] [Clear]`.

- **Follow** reads both origins once a second. On by default, the choice is kept. It is disabled (title `The selected target has no digikit runtime`) when no origin can be followed.
- **Save** writes the entries shown in the file format of `tools/device/dklog.py follow`: a `# ` line for each entry and, for a runtime event, the event as one JSON object on the next line (with `host_time`).
- **Source** has the groups App (reload, build, firmware, card, transfer, reset), Emulator (all, emulator, runtime, each loaded module) and Device: MIDI port (all, runtime, each loaded module). A group with nothing to follow shows one disabled option.

## Rules

- A poll loop for each origin reads `logPage(from, quick)` from the last sequence number seen. It reads only an origin whose INFO has the LOG capability.
- Both loops use the shared sessions of `apps/web/src/transfer/target.ts` (`emulatorSession`, `lastMidiSession`), so their requests wait behind Transfer.
- Gaps, restarts and frozen devices are noted with the texts of `dklog.py` `Follower.poll`. No answer for 5 s makes one note and the loop asks every 5 s. A cancelled request makes no entry.
- The device loop pauses during a firmware flash (the Firmware page uses the port) and notes `paused during the firmware flash`.
- Rows: `KIND`, the module, then `name=value`. A module's own `MOD_LOG` row is the module, the message in quotes and `value=0x…`. The detail has the sequence number, the device time, the level, the source and the raw arguments.
- Debug state is read from LOGCFG when Follow starts and when a `DEBUG_SET` event shows a change (read only).
- The fault record `dk/fault` is read when Follow starts and after each restart. A record that differs from the last one shown makes one error entry; the `CRASH_REG`, `CRASH_STACK` and `CRASH_TAIL` events of that boot are folded into its detail.
- Caps: 2000 entries for each runtime log, 500 for the app.

## Steps

- [x] `patchkit gen` writes the kind table with argument names, the guard kinds and the flag layout into `gen_proto.ts` (`sdkgen.rs`).
- [x] `packages/dk-session`: `fault.ts` (record decoder, settings reader, ABOUT rows) and `runtime-log.ts` (event words, fault detail) replace the decoder in `REF:tests/regression/runtime/fault.ts` (the regression tests import them from the package); `DkSession.logConfig`.
- [x] `target.ts`: the last MIDI port and its session.
- [x] `log-core.ts` (origins, caps, filter, Source menu, Save text), `log-follow-core.ts` (the loop rules), `log-follow.ts` (sessions and timers), `LogPanel.tsx`.
- [x] Tauri command `log_save` (the system save dialog; no new plugin or capability).
- [x] Preview page `?page=log` over fake runtime logs, and scenes in `tools/devtools/ui_shots.mjs`.
- [x] Unit tests: formatter and fault rows (the cases of `tests/digikit/test_dklog.py`), follower rules, filter, menu, Save text.
- [x] Regenerated with `patchkit gen` (`mise run gen:modules`); the diff of generated files is only the new table in `gen_proto.ts`.
- [x] `mise run verify:host`, `mise run check:layers`, `cargo check -p digiemu-desktop`.
- [x] Screenshots looked at (preview scenes `log-*` in `tools/devtools/ui_shots.mjs`).
- [x] One bounded live check over a running emulator (`live.html?page=emulator&log=1`, scene `log-live`): the drawer shows the real boot events.

## Not in this plan

- No control for the log level or for debug logging (`dklog level` stays the CLI).
- Nothing reads a frozen device; the log keeps what came before.
- `dklog.py` stays as it is.
