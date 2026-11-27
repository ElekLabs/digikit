# 17. Emulator control protocol and firmware injection

Design: `docs/design/emulator-control.md`. Code: `packages/emulator/control`,
`packages/emulator/sharc-peer/src/dsp_stream.rs`, `tools/control/digi_client.py`, `tools/patchkit/dsp_patch.py syx`.
Marks as in CLAUDE.md. Nothing here was re-checked by a second agent yet, so
the numbers are **[D]** (measured here, one machine, other lanes running).

## Hot DSP injection (DN2 1.11, ready inputs of 2026-10-06, fast regions on)

Scenario: `trig1-tap-release` events, 3 s of audio, DSP output hashed in the
controller (`pcm.tap`). Patch: `07e58305:tools/patchkit/dsp_patches/dn2-1.11-master-half.patch` (now `tools/patchkit/dsp_patches/dn2-1.12-master-half.patch`)
(`R2 = 0x1f` -> `0x1e` at 0x1c9d51, master Q31 scale, -6 dB), written into a
.syx with `tools/patchkit/dsp_patch.py syx` (one changed byte in section 7).

| run | rms | peak | PCM sha256 (prefix) | wall |
|---|---|---|---|---|
| reference (fresh restore) | 0.2096 | 0.6930 | `a0a0f42625dcb87e` | 5.0 to 6.5 s |
| same again after `restart` | 0.2096 | 0.6930 | `a0a0f42625dcb87e` | |
| patched .syx injected before the run | 0.1048 | 0.3465 | `6a7f5cecc41c9385` | 6.4 s |
| patched, then original injected, before the run | 0.2096 | 0.6930 | `a0a0f42625dcb87e` | |
| patched injected at frame 826 (0.55 s), mid-note | | | | window RMS 0.613 -> 0.489 -> 0.303 |

- The patched numbers equal the `--dsp-patch` start-time run of lane A
  (`6a7f5cec..`, rms 0.1048), so the live injection and the start-time overlay
  produce the same PCM. **[D]**
- Injecting the original .syx restores the reference PCM exactly (SHA-256
  equal). **[D]**
- `firmware.load` of the patched .syx takes 0.13 s (parse, compare 7 sections,
  diff the two DSP streams, apply between two frames). **[D]**
- The DSP keeps running at the same speed: the patch bars only the generated
  blocks and fast regions that touch the changed range (lane B), so 3 s of
  audio still render in about 5 s. Before lane B the same patch took 164 s for
  3 s with the fast tier off. **[D]**

## Patched MAIN (one byte of a string in section 3 of DN2 1.11)

- Cold CF boot to `ready` through `Emulator::new_patched`: 4.4 s wall,
  836,986,444 instructions. The cached snapshot loads in 0.13 s and 1M
  instructions later its state digest equals the cold run's (`8644d8a1e51f`).
  Injecting the stock .syx back restores the audio snapshot in 0.22 s. **[D]**
- A patched MAIN runs without DSP audio (no DSP state for it). **[C]** closed by "Coupled cold boot of a
  patched MAIN" below: with the launch's DSP inputs the patched MAIN now comes up with audio; the
  ColdFire-only boot above remains the fallback when there are none.

## Protocol checks (CF-only DN2 1.11, `tests/control/test_digi_client.py`)

- `step(N)` advances the ColdFire count by exactly N for N = 1, 7, 250000,
  250001 and 600000. An input with `at_instr` is recorded at exactly that
  count. Replaying `inputs.recorded` after `restart` gives the same state
  digest. **[D]**
- A render of `trig1-tap-release` (8.5 s) through the socket gives PCM
  `aa1dbd9e40e53443..`, the pinned `expect_sha256` of the scenario, and a second
  render after `restart` is identical. **[D]**
- `coupled_ready_exactness` fails in this merged tree: CF digest `75b6a52d..`
  (pinned `ab27c5c9..`) and PCM `849157bd..` (pinned `3b600f6d..`). The tree
  carries lane 0's uncommitted DSPI1 model (`board.rs` ch 14 completion, IRQ
  delivery in `runtime.rs`), which changes ColdFire state the gate hashes; the
  controller move touches no machine code and the scenario above still matches
  its pin. Not bisected against a tree without lane 0's change. **[O]**

## DSP patch files and the patch overlay **[D]**

`coupled-run --dsp-patch FILE` (also the desktop launcher and `firmware.load`) overlays
word-level edits on the imported DSP state after the snapshot import, because a `.snap.dsp`
carries its own copy of code and data. Format (`packages/emulator/sharc-peer/src/dsp_patch.rs`,
`tools/patchkit/dsp_patch.py check|show|apply|syx`): `BASE <sha256 of the unpatched .syx>`, then
`PM <pc> <hex> [was <hex>]` (a 16, 32 or 48-bit instruction at a short-word PC), `SW`,
`DM <addr> <hex32> [was ..]` and `BYTES <addr> <hex>`. `was` is checked against live memory.
A wrong BASE is refused. A no-op patch gives the identical PCM hash; `R2 = 0x1f -> 0x1e` at
sw 0x1c9d51 (the master Q31 scale) halves rms and peak exactly. A changed code word bars only
the AOT regions with an entry in a changed range (finding 18), and the fast tier verifies its
code words, so a patch no longer needs `SHARC_FAST_REGIONS` unset. Patch files hold firmware
words: keep them under `out/` and never commit them.

The ColdFire `step N` is exact for 1, 7, 250000 and 600000 instructions.

## Coupled cold boot of a patched MAIN (patchkit spike 0.6, 2026-10-13)

`firmware.load` of an image whose MAIN differs from the base now cold-boots the ColdFire **with the base's DSP
attached** and installs a runtime with DSP audio (`packages/emulator/control/src/controller.rs` `load_patched_coupled`;
`packages/emulator/sharc-peer/src/cold.rs` `boot_patched_to_ready`). Semantics and the cache layout are in
`docs/design/emulator-control.md` ("Firmware injection", item 3). Summary: the DSP is the ready inputs' image and
cold-idle state (the DSP section of the base .syx; a MAIN patch never changes it); the boot runs on a worker thread
until the panel is ready (DT2: until link port 0 is quiet, as `tools/emulator/ready.py` does) and writes the ready pair
(`digi-audio-m5.snap` and `.snap.dsp`) under `<run_dir>/patched/<MAIN sha16>-<key>/`; the key hashes the MAIN digest,
the DSP image file, the cold-idle file and the card file (path, size, mtime). A DSP section that differs from the
base's is diffed and injected hot after the install, so the cache does not depend on it. `start auto|snapshot|cold`
keep their meaning (`snapshot` without a pair is error 4, `cold` rebuilds the pair). `auto_no` (also on `restart`)
presses NO when the new runtime is ready, because a DT2 booted with a card waits at the FILE SYSTEM OK modal. Without
inputs, without `cold-idle.dsp`, or when `SOURCE.json` next to the DSP image names another .syx, the ColdFire-only
path of the section above runs and the report says why (`coupled_note`).

Measured through the socket, `tools/patchkit/patched_audio_check.py` (stock-size test image of `tools/patchkit/patched_image.py`, the
default project name UNTITLED becomes PATCHKIT; machine loaded by other lanes, load average 20 to 25, so wall times
are upper bounds). **[D]**

| | DN2 1.12 | DT2 1.16 and 1.17 (one-sample card) |
|---|---|---|
| uncached coupled boot to ready | 6.0 to 6.7 s (851,236,338 CF instructions, 4,731 DSP frames) | 12.5 to 13.5 s (1,191,235,681 instructions, 5,971 frames) |
| of that: DSP engine start | 0.05 s | 0.13 to 0.14 s |
| same boot, ColdFire only (before) | 4.8 s (847 M instructions) | 7.9 s (998 M; the coupled boot also waits for link port 0 to go quiet) |
| cached load, `restart` | 0.27 to 0.34 s | 0.45 to 0.49 s |
| stock firmware loaded back | 0.23 s | not timed |

Both are under the 30 s target. The floor is ColdFire interpretation (inside the coupled loop about 140 MIPS DN2 and 93 MIPS
DT2; ColdFire-only 175 and 126 MIPS); the DSP runs on its own thread (about 1.2 to 1.4 CPU seconds per
audio second, idle loop skipped) but the host waits for its frames every chunk, so the coupled boot costs about one
third more than ColdFire alone on DN2. The uncached time scales with the instruction count to ready, so a MAIN patch
that adds boot work adds its time. DN2 1.12 also took 11.6 s once while a 15 minute cargo build ran: the number is
CPU-bound.

Proof of audio: DN2 1.12 patched, TRIG 1 then PLAY: rms 0.0109, peak 0.0562, PCM SHA-256 `c0ddaa77..` for the stock,
the cold-booted, the cached and the restarted patched firmware (a string patch does not change the sound, so equal is
the expectation). DT2 patched with the card of `card.load` (a 220 Hz sine): TRIG 1 gives a peak at 222.66 Hz with 99.99 %
of the power, rms 0.0255, the same analysis as the stock firmware, after a cold boot, after a cached load and after
`restart`. 1.17 behaves like 1.16. Without `auto_no` the DT2 note is silent after the load (the modal is still up),
which is why the option exists. **[D]**

Tests: `tests/emulator/test_coupled_patched.py` (slow; DN2 1.12 and DT2 1.17 cold/cached/restart/audible, `start snapshot`
without a pair, `start cold` rebuild, hot inject of an appended block), the Rust unit test `patched_pair_key`, and
`tests/control/test_digi_client.py` (the ColdFire-only fallback and its note).

**ColdFire canary with the DSP running.** `tools/coldfire/regions.py confirm IMAGE --dsp READY_DIR` runs the spike 0.3 canary
probe with the ready directory's DSP on the DSPI2 link from reset (the expansion region filled with an
address-dependent pattern after the reset clear; digest compared with a zero-filled run). All PASS with
`bad_words=0`, equal digests and a running DSP (no halt): **[D]**

| image | scenario | CF instructions | DSP frames |
|---|---|---:|---:|
| dt2-1.17 | boot | 1,415,095,603 | 11,132 |
| dt2-1.17 | nav | 1,989,090,148 | 17,655 |
| dt2-1.17 | play | 2,556,093,945 | 24,098 |
| dt2-1.17 with the one-sample card | boot / play | 1,491,595,555 / 2,632,593,550 | 9,384 / 22,350 |
| dt2-1.16 | play | 2,556,093,945 | 24,098 |
| dn2-1.12 | boot, nav, play | 1,151,478,813 / 1,725,473,203 / 2,292,477,031 | 8,142 / 14,665 / 21,108 |
| dn2-1.11 | play | 2,292,477,031 | 21,108 |

(`play` presses PLAY, every trig, RECORD and a second PLAY/STOP cycle; `nav` every page, mode, arrow and encoder.)
Not covered: a card other than the one-sample card, and the DSP side of patched code (nothing new runs there yet).

## Hot-injected stream block, `dsp.peek`, and the ready-point trap **[D]**

- `dsp.peek {addr, len}` (new method, `Msg::Peek` on the DSP worker) reads DSP memory between frames: hex, `??` for a
  byte the engine does not know (never-written L2 reads as unknown, not zero). `coupled-run --dsp-peek
  NAME=ADDR:LEN[:BLOCK]` puts a SHA-256 (and per-block hashes) of a DSP range into each scenario's report.
- A .syx whose DSP stream has an extra block is injected hot as a diff of the loaded memory (`dsp_stream::diff` is
  address based, so streams with different block lists need no special case): DN2 1.12 + a 211,600-byte block,
  0.17 s, one `BYTES` op, `changed_bytes 211600`, the engine accepts the write into memory the image never had, the
  block reads back intact and the PCM is unchanged; injecting the stock .syx again reports `dropped_bytes 211600`
  (a patch cannot undo a write).
- Trap for later harnesses: `tools/emulator/ready.py` boots the ColdFire from `--syx`, and a container 0.8 MB larger (the
  stream-append image) reaches ready at a different instruction count, so the DSP clock at the snapshot is one frame
  (666,667 DSP instructions) earlier and every PCM hash changes, canary or not. Ready inputs for a DSP-only change
  must keep the stock ColdFire boot: `ready.py --syx STOCK --dsp-syx CHANGED` (new; the DSP image and cold state now
  come from the .syx's own section 7, byte-identical to the `out/sections` route for the stock files). A .syx of a
  known MAIN with another DSP stream is now identified by its MAIN digest (`Registry::boot_by_main`).


## Health events, heartbeat and the watchdog (patchkit Phase 2, 2026-10-13) **[D]**

Code: `packages/emulator/control/src/health.rs` (pure logic, 6 unit tests), `controller.rs` (`Actor::watch`), `tools/control/digi_client.py`
(`subscribe`, `drain_events`, `peek32`, `poke32`, `watchdog`).

- **`fault`** event (once per runtime): the ColdFire step error (`Status.error`, phase `fault`), with `error`, `pc`,
  `cpu_state`; `GUEST_FAULT` (error 6) is still the reply of the step that hit it. **`halt`** event (once): the DSP's
  `halted` reason, read every 16th chunk (it needs the audio session). **`stall`** event: the heartbeat stood still
  for `stall_ms` of emulated time (default 1000, `DIGI_STALL_MS`, `watchdog.config {stall_ms}`; 0 disables) while the
  CPU is `running` and the firmware is past the main panel; `stall.end` when it beats again. **`underrun`** event
  (2026-10-13, see below). Events are written by a socket thread, so a reply can arrive before the event it follows: a client that must
  be sure calls `status` once more, or reads the latched state (`status.cf.error`, `status.audio.halted`,
  `status.heartbeat.stall_count`).
- **`underrun`** event **[D]**: `{new, total}` when the audio sink's underrun counter (`PlaybackStats.underrun_events`,
  counted in the output callback when the ring runs dry; `Runtime::sink_underruns`, coupled-audio builds with a player
  only) rose since the last look; `total` counts since the runtime began, `status.heartbeat.underrun_count` holds it. The
  controller reads the counter every 16th chunk (like the DSP halt flag), so an event can follow its underrun by a few
  chunks and `new` can sum several. No player (no device, no sink capture, CF-only) means no event. The pure logic
  (`health.rs`, `underruns_are_reported_by_increase_and_a_new_runtime_rearms`) is unit tested; no test drives a real
  device underrun (needs an audio device). It is a **host** real-time signal: in a headless harness run the
  emulator is not paced, so it says nothing about the hardware's DSP load (finding 22 budget rule still applies).
- **Heartbeat** = `Status.mainloop_hits`, the passes through the resolved main-loop PC (the PC the readiness contract
  already counts; fused loops and the fast path stop at it, so the count is exact). New fields: `mainloop_hits`,
  `mainloop_pc`, `cpu_state`; `status.heartbeat {mainloop_hits, mainloop_pc, stalled, stall_count, stall_ms}`.
  Measured, DT2 1.17 at ready: 1 pass per about 4.0 M instructions (30 ms emulated, 33 Hz), DN2 1.12 similar, so the
  1000 ms default is about 33 missed beats. A `bra.s .` written after the loop's call gives a `stall` at exactly
  1000 ms (132.25 M instructions); an illegal instruction at the loop head gives `fault: exception vector 11`.
- **`mem.cf.peek {addr}` / `mem.cf.poke {addr, value}`** (SDRAM longwords, side-effect free, not recorded for replay)
  exist so tests can break the guest on purpose.
- **Watchdog**: the MCF5441x core watchdog is the SCM's CWCR (`0xFC040016`) and CWSR (`0xFC04001B`). No word in MAIN OS
  (1.16, 1.17, 1.11, 1.12) or in the bootstrap carries those addresses (the literal search for `fc040016` and
  `fc04001b` is empty **[V]**; the only SCM literals are `fc04002d`/`fc04002f`, PPMCR, and one `fc040024`), and the bus models no
  SCM register except the write-only PPMCR (`ppmcr_contract`). So the firmware does not configure the watchdog as far
  as a static read shows (computed addresses are not excluded), and the emulator would not model it. A hang
  therefore shows only as a `stall` here; on hardware the unconfigured CWT is off after reset (CWCR[CWE] cleared). **[D]**
- **DSP halt in a stock run** **[D]** **[C]**: DT2 1.17 stock, pattern 3 or 4 (`PTN` + `TRIG 3`/`4`) then PLAY halted the DSP
  on a conditional Type 7a (`native-trap: sharc_core.forms_dag._type_7a:63: stop: unsupported Type7a predicate at
  0x1cbc93`, `IF EQ modify(I4, M4)`). Fixed in `tools/sharc_dev/sharc_core` (finding 05, "Conditional Type 7a"); with the
  regenerated cores PLAY in patterns 3 and 4 runs clean on DT2 1.17 and 1.16
  (`test_play_in_patterns_3_and_4_runs_clean`), and `test_a_dsp_halt_is_a_failure` now provokes the halt with an
  undecodable word at that address (`dsp.patch`).
