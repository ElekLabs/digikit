# 131: The Syntakt runtime in the workbench path

Status: **[D]** measured once on Syntakt OS 1.42 in the emulator (2026-11-13); **[O]** marks the open cause.

## What the workbench path needs

- A patch set in the device file. `devices/syntakt.toml` has `[patch_sets] emulator = ["core/runtime"]` (core/expansion comes through `requires`).
  The other core modules have no Syntakt port, so the set is only the runtime. There is no `device` set: nothing is flashed to a Syntakt yet.
- A snapshot rule that accepts the device's own rate. The Syntakt runs at `[emulator] instructions_per_second` (189,928,000), not at the
  132 MHz base. A snapshot taken at the base rate keeps the magic `DT2SNP01` (no change for the other devices). One taken at another rate has
  `DT2SNP02` and the rate (u64) after the magic; a restore at a different rate is refused. A base-rate snapshot still restores at any rate.
- No boot cache while the second CPU runs. A snapshot does not carry the running peer CPU (`save_state` refuses), so the ready snapshot
  that `firmware.load` writes after a cold boot is skipped for a device with a running peer. An explicit `state.save` still reports the refusal.
  With the DSP switch off (`--no-dsp`: the mailbox responder instead of the peer program) the snapshot is written and restored.

## Result

- `digi-emud firmware/Syntakt_OS1.42.syx --runtime`: the runtime is patched, MAIN boots to the main screen (peer program running or responder),
  `dkupload --emu info` answers (`port: st`, runtime 0.7.0).
- With the responder (`--no-dsp`): `dkrun digikit/debug --emu` uploads the module, selects it, restarts, and the log shows
  `MOD_LOAD index=0 result=0 name=debug.dkm` and `UI_FIRST`. The ready snapshot is written on the first start and restored on the second
  (8 s instead of 40 s).
- The desktop launch (`native_emu --build-only firmware/Syntakt_OS1.42.syx --runtime`) builds the `cf-audio` app and passes the flags.

## Open: duplicate replies with the peer program running [O]

With the peer program running (the default) some replies of the runtime reach the host twice (about 1 request in 6; a longer upload
fails with `reply for cmd 0x11 seq 6, wanted 0x12 seq 7`). Evidence:

- The two captures of the USB transmit function have different instruction counts, about 68,000 apart, the same stack and the same bytes:
  the guest calls `dkrt_midi_rx` (or its reply) twice, not the capture.
- The runtime object of the commit before this work does the same; with the responder there is no duplicate.
- The hook site `cf.midi.site.rx` (0x40103f5e) is the second of the MIDI task's queue dispatches (queue at 0x44491554, arguments 4 or 2).
  Whether the stock task dispatches the message twice with the peer running, or the hook returns before the free tail in some timing, is not
  known. Next: log the message pointer and the `source` argument at both calls.
