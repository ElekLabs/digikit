# Handover 2026-11-14: Syntakt on the integration branch

Start from docs/plans/syntakt.md (phases and the follow-up list). Findings: 70, 129 to 133. Main is at 682c0d6c; this branch has merged main up to that point.

## Branches and lanes
- `syntakt/runtime` in lane `~/src/digi/lanes/pages-deploy` is the integration branch with everything below. Work continues there.
- `device-registry` (lane ts-py) is draft PR 437. Its content is in `syntakt/runtime` too; merge the PR first or take the branch whole.
- Merged into `syntakt/runtime` and retired: `syntakt/bringup` (lane sweep-gen), `syntakt/peer-cpu`, `syntakt/patch`, `syntakt/perf`, `syntakt/runtime-device`, `syntakt/runtime-anchors` (lane fast-transfer), `syntakt/cf-aot` (lane ts-slots), `registry/s5-apps` (lane sharc-mode1), `registry/s6-python` (lane nx-nojob), `workbench/dsp-toggle` (lane zero-dsp), `emu/ips-instance` (lane usb-host).
- Em removes lanes and branches; none were deleted here.

## State
- Firmware query works for `st-1.42` and `st-1.42-peer` (`tools/coldfire/fw.py -i st-1.42`).
- The device registry (`devices/*.toml`, `@digi/devices`) drives apps, Python tools and patchkit; `tools/nx/device-literals.mjs` passes.
- The Syntakt emulator boots MAIN to the main screen with the second ColdFire CPU (the peer, section 7) running and with digital audio (finding 136). Native kernels (finding 140, env `DIGI_CF_NATIVE`) run the peer's voice oscillator: same WAV hash as the interpreter, 25 percent less peer wall time. Generated kernel code lives in `out/native/cf-native/NAME` (never committed); a build without `CF_NATIVE_DIR` has no kernels.
- The faceplate, and the workbench with the DSP toggle, work for the Syntakt.
- The digikit runtime, the debug module and Transfer work on the emulated Syntakt (finding 139). Patch set and facts: findings 137 and 138.
- patchkit: `verify` needs the stock base for a device with read-only sections (only a known stock image needs none); the web bundle may carry `[[firmware]]` hashes.
- Speed: 0.37 of real time with audio on this host.

## How to run it
`mise run workbench -- firmware/Syntakt_OS1.42.syx`, tick Runtime, leave DSP on. Headless: `panel_drive` (packages/emulator/boot/examples) with `tap:1,wait:200M,tap:37,wait:200M,wav:PATH`. Kernels: `cargo run -p coldfire --bin cfaot -- devices/cf-native/syntakt-1.42-peer.toml out/sections/st-1.42/section_7_BLOB.bin out/native/cf-native/syntakt-1.42-peer`, then build with `CF_NATIVE_DIR` set.

## Safety rules
- Never write sections 1, 6 or 7 (patchkit refuses; `verify` now proves it against the base).
- Never emulate or reconfigure the FPGA logic; the CPU bridge is a data mailbox only.
- Calibration data (SPI NOR 0x300000, "RBOZ") is never written to hardware; the emulator serves a synthetic record.
- No hardware flashing yet: first the open runtime items below, in the emulator or statically.

## Next, in order
1. Store write safety: bound the offsets that the remote stream handler takes from the host (finding 138).
2. Verify the `core/ui` HEALTH and MEMORY pages and `dk_input_turn` on the Syntakt.
3. Fix the Nx cache gap: `core/input:test-device--dt2` replayed after `modules/core/adapters/ui/build/ui.dko.json` changed (a missing input).
4. Speed: thread per CPU (about 1.7x), peer RAM fast path (about 1.25x), more kernels including MAIN's vector 191 mixer.
5. Emulator restructure: companion enum and crates, board options, one audio session, framed handoff, kernel sets, peer snapshot.
6. Design follow-ups: boot signatures as `devices/fw` rules, split `[emulator]`, Nx per-device targets from the registry (S1 to S3), one eDMA TCD helper, document the zero-time shortcuts.
7. Owners burn down the literal baseline growth from main's new code.
