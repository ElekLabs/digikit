# 21. patchkit core: codec, ports, linker, expansion loader, wasm

Plan: `docs/plans/patchkit.md` (Phase 1). Code: `packages/patchkit`, `tools/patchkit/pk_build.py`, `tools/patchkit/port_resolve.py`,
`tools/patchkit/patchkit.py`, `modules/`, `devices/ports/`. Marks as in AGENTS.md. Nothing here has been checked by a
second agent, so it is **[D]** (measured here once). No patched image has been flashed to hardware.

## Container layout: the stock images start their payloads at 0x90 **[D]** **[C]**

The four stock images (DT2 1.16, 1.17, DN2 1.11, 1.12) place the first section payload at container offset
`0x90` (the table area is `0x80` bytes and 16 more are left zero), each later payload on the next 16-byte
boundary, then 16-byte padding and the 32-byte HMAC trailer. `tools/firmware/build.py` starts at `0x80`, so
`build.rebuild(stock)` with no replacement is **not** the stock file: every section offset in the table is 16
bytes lower, the preamble checksum, the HMAC and 1.69 M of 1.88 M file bytes differ (it still passes
`roundtrip.check_authenticity`, which checks the container's own checksums and HMAC, not equality with stock). patchkit keeps the first
offset of the source image: `Firmware::parse` then `build` reproduces all four stock `.syx` files **byte for
byte** (framing, 8-in-7, packet checksums, content checksum, HMAC; `tests/patchkit/test_patchkit.py`). An image with a
patched section differs from stock only in that section, the table and the checksums.

## aPLib packer in Rust equals the Python packer **[D]**

`packages/patchkit/src/aplib.rs` is a line-for-line port of `tools/firmware/aplibpack.py` (same hash chains, same cost
model, same tie breaks). `tools/patchkit/pk_aplib_eq.py` packs every packed section (2, 3, 7, 8) of the four firmwares with
both and compares the stored bytes: 16 of 16 equal. Rust takes 1.0 s for MAIN OS where Python takes 24 to 27 s.

| image | section 3 stored | section 7 stored |
|---|---:|---:|
| DT2 1.16 / 1.17 | 1,107,873 / 1,108,851 | 187,017 |
| DN2 1.11 / 1.12 | 1,096,271 / 1,097,013 | 615,802 |

## Port lockfiles **[D]**

`devices/ports/{dt2-1.16,dt2-1.17,dn2-1.11,dn2-1.12}.toml`, made by `tools/patchkit/port_resolve.py` from the
`[[anchor_rule]]` entries of the modules and from `tools/coldfire/regions.py`. Seven anchors each (the whole MAIN OS image
`cf.main`, the reset-path calls and the two helper addresses, and the three DSP master-hook anchors) and three
regions (`cf.flash_cave`, `cf.expansion`, `dsp.l2`). They hold addresses and sha256 of the bytes, no firmware.
The two DSP functions of the master hook have the **same relocation-insensitive hash on DT2 and DN2** (the block
handler `0x1c75d8` / `0x1c9e76`, the Q31 converter `0x1c74a1` / `0x1c9d3f`), so one rule matches both devices; the
call is at +0x15c parcels, the converter's `I3 = buffer` literal at +0xf. DT2 and DN2 resolve to the addresses
of finding 06 ("Master hook").

## Selache listing quirk **[D]**

`seldump` lists the *unrelocated* object and splits Type 3a (`DM(I,M) = Ureg`, 48 bits in our decoder) into a
4-byte row plus a separate `0000` row (and then lists an even-aligned 48-bit `JUMP` starting at that `0000`). The
real extents are ours: `pk_sharc` now takes the instruction rows from our decoder and cross-checks Selache's
listing against them (same length, or a shorter listing whose missing parcels are zero) and each relocation
against our instruction starts.

## digikit/master-gain through patchkit **[D]**

Module `modules/fx/master-gain`: the spike 0.7 routine as Selache source, its gain word in its own data section,
the CALL of the Q31 converter retargeted by `patchkit` (op `retarget`), placed first-fit in `dsp.l2` (DT2
`0x2001ab80`, gain word `0x2001abb0`; DN2 `0x20088240`). Patched from the stock images, verified by
`patchkit verify` and `tools/firmware/roundtrip.check_authenticity`, injected hot (`firmware.load`, `start_used: hot-dsp`)
into a coupled daemon on the ready inputs of the stock firmware; each run starts from `restart`
(`tools/patchkit/pk_proof_gain.py`):

| device | note | unity (gain word 1.0 as built) | gain word 0.5 (`dsp.patch DM <gain> 3f000000`) |
|---|---|---|---|
| DT2 1.17, one-sample card | TRIG 1, 1.5 s, 133,580 active samples | bit-identical to stock | RMS -6.0206 dB, peak ratio 0.5, max error 2.3e-10 (0.5 ulp) |
| DN2 1.12 | TRIG 1 + PLAY, 3 s, 157,928 active samples | bit-identical | RMS -6.0206 dB, peak ratio 0.5, max error 2.3e-10 |

The Rust linker's block equals what `tools/patchkit/pk_sharc.py` links for the same bases and externs
(`test_rust_linker_equals_python_linker`).

## Oracles **[D]**

- DN2 master half as one `write` (`out/modules/oracle/dn2-master-half`, made by `tools/patchkit/pk_oracle_dn2_half.py`): every decoded section of the output equals
  `tools/patchkit/dsp_patch.py syx` on `dn2-1.11-master-half.patch` (compared with `--no-record`: the oracle must not
  carry the record block).
- XSLICE as raw writes (`out/modules/oracle/xslice-dt2-1-16`, made by `07e58305:tools/patchkit/pk_oracle_xslice.py`, removed with the 1.16 tests from the
  machinebuild plan): the plan's 51 writes overlap each other, so they are merged into 41 spans; patched section 3
  is `cdbd772639d22c8a06231958e00a600fd7fd29df3b057e33ff4d76f837ecb86a` and equals the machinebuild output.
  The module is refused on every other firmware. Both oracle modules hold firmware words (instruction and
  descriptor words), so they are generated under `out/modules/` and never committed; patchkit reads extra module
  roots from `--modules-dir` and `PATCHKIT_MODULES` (`tools/patchkit/patchkit.py` adds `out/modules`).

## core/expansion **[D]**

The loader (`modules/core/expansion/src/cf/loader.s`, 54 bytes, ISA_A) replaces the reset code's two calls
(`jsr copy; jsr clear` at `0x4000053e`) by `jsr loader; nop`. The segment chain behind the stock MAIN OS image is
`[u32 len][u32 dest][u32 bss][payload]...[0xffffffff]`; the loader copies each payload to its destination in
`cf.expansion` and zeroes the module's bss behind it, then runs the two original helpers. patchkit adds the
module whenever an object is placed in `cf.expansion` and puts the `DKPR` record in the first segment.

- Unit test with teeth: the loader runs on the ColdFire core with stub helpers over a region filled with `0xa5`;
  payloads are copied, bss is zero, nothing past it is touched, callee-saved registers are intact.
- `test/cf-demo` (code, read-only data, `.data`, bss) on DT2 1.17 and DN2 1.12: the patched image cold-boots to
  ready (CF-only call harness), `pk_demo_add(2,3)` = `0x1239`, the table reads back, the bss is zero.
- Coupled cold boot through `firmware.load` (`tools/patchkit/patched_audio_check.py`): DN2 1.12 reaches ready with DSP
  audio in 6.4 s (851,236,338 ColdFire instructions, 4,731 DSP frames), the note's PCM equals stock (`c0ddaa77..`);
  DT2 1.17 in 13.6 s (1,191,235,681 instructions), the 220 Hz note has the stock spectrum.

## Browser **[D]**

`apps/web/public/patchkit.wasm` (638 KB, gitignored, built by `tools/emulator/native_wasm.py`; it embeds no port and no
firmware hash) applies `digikit/master-gain` to DT2 1.17 and DN2 1.12 byte-identically to the command line, plan.json
included, and refuses what the command line refuses (`apps/web/tests/patchkit.wasm.test.ts`).

## Limits and open **[O]**

- `retarget` of SHARC code supports the direct `CALL` (`25a_direct`) only; ColdFire `jsr/jmp abs.l`, `jsr
  d16(pc)`, `bsr.w`, `bsr.l` are written but not exercised by a module yet.
- The record is read by nobody but `inspect` and `verify`; the DSP copy and the expansion copy are never touched
  by firmware, only the DSP block lies in a region the real SHARC boot ROM has not been shown to accept (finding 06).
- The reset-path loader and the `DKPR` chain are checked in the emulator only. Hardware: not tested.
- `core.expansion` is the id `core/expansion` (module ids are paths under `modules/`).
