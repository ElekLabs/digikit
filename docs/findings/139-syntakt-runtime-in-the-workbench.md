# 139: The Syntakt runtime in the workbench path

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

## Duplicate replies with the peer program running

Status: **[D]** traced on Syntakt OS 1.42 in the emulator (2026-11-13); fixed.

Symptom: with the peer program running about 1 host request in 6 got two replies, about 68,000 MAIN instructions apart, and an upload failed with
`reply for cmd 0x11 seq 6, wanted 0x12 seq 7`. With the responder (`--no-dsp`) there was no duplicate.

Cause: a capture artifact. The guest sent once. The emulator records a reply where the firmware's USB transmit function (0x40004a20) starts. It looked
at the PC at the top of the step, before the step's interrupt deliveries. When an interrupt took that step over (the peer's frame, the USB hand-over),
the task resumed at the same PC after the other tasks had run, and the same arrival was recorded again.

Trace (`DIGI_USB_TRACE=1`, `DIGI_USB_TRACE_PCS=40103f5e,40106784`, DSP on, `dkupload info` in a loop):

- One USB hand-over (`rx`), one execution of the hook site (0x40103f5e, so one `dkrt_midi_rx` call), two `tx` records with the same payload hash,
  the same SP, the same caller (0x40106784, inside the stock MIDI send function) and the same 90 stack words, 67,644 instructions apart.
- The address after the call (0x40106784) was reached once, after the second record only. The first call never ran its first instruction.
- The gap was the same (67,644) in every duplicate, for 56-byte and 220-byte replies.
- The emulator has no model of the USB MIDI controller on this path: it hands bytes to the firmware's receive callback and reads them at the
  transmit function. A completion timeout or a retransmit of the USB stack is not involved (hypothesis A), the runtime's reply path ran once (B),
  no descriptor is handed back (C). By the classification of the task: D, one send recorded twice.

Fix: the transmit capture (`observe_usb_tx`) runs just before the instruction executes, after the interrupt deliveries of the step, so a step that
an interrupt takes over records nothing. The callback unwind and the bulk dTD capture stay where they were. The bulk capture (EP2 IN queue
function) has the same ordering **[O]**: no duplicate seen there, not changed here, to keep the other devices' timing.
Unit test: `a_transmit_call_that_an_interrupt_preempts_is_recorded_once`.

After the fix, DSP on: 0 duplicates in 400 replies (120 and 60 INFO round trips, 20 uploads of 40,000 bytes with the hash checked on the device);
`dkrun digikit/debug --emu` and `dklog --emu follow` work; DSP off: 0 in 60.

The trace is off by default: `DIGI_USB_TRACE` logs each USB MIDI hand-over (`rx`) and each recorded transmit (`tx`: instruction count, length,
payload hash, caller) to stderr; `DIGI_USB_TRACE_PCS` adds addresses to log (instruction count, SP, stack top, d0, a0).
