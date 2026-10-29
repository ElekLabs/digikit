# Sampler capacity and replacement decisions

2026-10-29. Targets: Digitakt II OS 1.17 and Digitone II OS 1.12. **[D]** denotes local measurement or source
inspection; **[O]** denotes a proposed decision or unproved contract. No new [V] or hardware claim.

## Measured capacity

**[D]** Four serial emulator snapshots used the existing `pk_devreload` startup/reload path and
`dkupload.Uploader.modules()` with its CLI formatter. Baseline means zero dynamically loaded modules;
proof means only `test/sample-proof` v0.1.0, before sample-source acquisition. All owned daemons were stopped.
Warm build checks reported no module rebuilds; no source changes, core generation or hardware runs were needed.

All values below are bytes; largest is the largest currently allocatable block reported by the runtime.

| Current image / selection | CF arena free | CF largest | DSP area free | DSP largest |
| --- | ---: | ---: | ---: | ---: |
| DN2 1.12, no dynamic modules | 524288 | 524272 | 327680 | 327680 |
| DN2 1.12, proof only | 524032 | 523312 | 293696 | 293692 |
| DT2 1.17, no dynamic modules | 524288 | 524272 | 524288 | 524288 |
| DT2 1.17, proof only | 524032 | 523312 | 490304 | 490300 |

Both proof snapshots report CF image size **232 bytes**, DSP extent **33,984 bytes** at offset 4, assigned type 16,
zero faults and guard enabled. The CF free-total reduction is 256 bytes, including allocator rounding/header cost;
the largest-block reduction also reflects allocation placement. The DSP extent includes the 32,768-byte float32
bank plus code/state/descriptor. It is not all audio storage. The leading four bytes affect largest-block size.

The device profiles reserve a **512 KiB CF runtime arena on both devices**, and a **512 KiB DT2 / 320 KiB DN2
DSP module area**, shared by all loaded modules' code and state. These are runtime-owned pools, not measurements
of stock free RAM. The four snapshots do not establish arbitrary module coexistence or a production sample budget.
See [DT2 profile](../../devices/digitakt-ii.toml) and [DN2 profile](../../devices/digitone-ii.toml).

Artifacts remain under `out/runs/dsp-sdk-checks/capacity-discovery/`: `summary.json`, the four `*-snapshot.json`
records, `*-modules.txt` formatted reports, `*-reload.json`, `measure.log`, and the out-only orchestration script.
The snapshots retain full daemon status, module lists, ready-input paths and runtime image identifiers:

| Device | Runtime image id | Stock input SHA-256 |
| --- | --- | --- |
| DN2 1.12 | `c0e8bd18a4812349` | `7e5b8a733d4ab1fdf7e5d22757f7d8eddaa3cbf71d0858874a791844c02ceb35` |
| DT2 1.17 | `3bf7458c8463c435` | `26c22f6652625ac2cfd47f7ee970d388ed8b6427dae3563c0a6a2f2d334350d5` |

The input files are `firmware/Digitone_II_OS1.12.syx` and `firmware/Digitakt_II_OS1.17.syx`; ready inputs are
`out/ready-fix-dn2-112` and `out/ready-pk-dt2-117`. The historical DN2 DSP-core display label does not change
this current-image provenance; retain the qualification in [finding 79](79-sample-bank-proof.md).

## CF acquisition cost and actual lifetime

**[D]** The current source adapter allocates a sector-rounded complete native file plus 1,024 bytes of scratch,
then allocates decoded float32 output. A canonical N-frame native file occupies `2*N + 80` bytes. With 16-byte
arena headers and aligned payloads, the simultaneous allocation formula for a full-length source at a configured
N-frame acquisition limit is:

`align16(round512(2*N + 80) + 1024) + 16 + align16(4*N) + 16`.

At the implemented 8,192-frame limit this is **50,720 bytes**: 17,920 bytes input/scratch, 32,768 output and two
16-byte headers. Applying the same strategy to 16,384 frames would require **99,872 bytes**; that is arithmetic
for a possible extension, not an implemented or approved limit. These figures exclude other runtime allocations,
fragmentation, stack use and module storage. The input allocation uses the configured maximum even for a shorter
file, while decoded output follows actual frame count. See [source acquisition](../../modules/core/runtime/src/rs/src/sample_source.rs)
and [arena allocation](../../modules/core/runtime/src/rs/src/arena.rs).

**[C] [D]** Session pinning describes the DSP bank, not a permanent decoded CF copy. Input/scratch is freed before
source acquisition returns. The decoded CF buffer survives while transport may read it, then
[runtime sample processing](../../modules/core/runtime/src/rs/src/sample.rs) lines 160–169 takes and frees the source
when the job is no longer busy. Successful READY therefore does not retain this CF audio buffer. An ambiguous
accepted transfer must remain quarantined until terminal/quiescent; the
[job contract](../../modules/core/runtime/sdk/src/sample_job.rs) prevents `take_source()` while busy.

**[D]** The DSP bank remains pinned until restart in the current proof. Release exists for load rollback; there
is no successful live-unload/replacement path. A second load is refused. This is the ownership safety established
by [the bounded proof](79-sample-bank-proof.md), not a reusable live sample-pool contract.

## Replacement decision and next proof

**[D]** The user explicitly chose: **“Stop old notes on selection.”** The sample-length target question remains
pending; do not infer a duration or increase the current 8,192-frame limit (about 171 ms mono at 48 kHz).

**[O]** A single-bank replacement design can follow that choice: invalidate the old view and stop its notes;
obtain an acknowledged DSP quiescence barrier proving no consumer still reads the old bank; transfer the new
payload; transfer its descriptor; publish READY last. A consumer generation/epoch contract must prevent stale
voices or queued note-ons from reusing the old view during this sequence. Neither the barrier nor this generation
contract is implemented or proven. Publishing a new pointer or clearing READY alone cannot prove old voices have
stopped reading bytes that are about to be overwritten. Source-buffer lifetime and DSP-consumer lifetime require
separate acknowledgements. No replacement implementation is authorized by this note.

**[D]** Existing DSP-link acknowledgement proves that the receiver processed a chunk, not that every voice
reader is quiescent. The receive hook runs at the common block-handler tail; ordering against all voice renders
and freedom from preemption are not established. A READY=0 write acknowledgement is therefore not a reader
barrier. See [link bindings](../../devices/fw/dsp/link.toml),
[hook installation](../../modules/core/adapters/dsp-link/module.toml), and
[finding 31](31-dsp-link.md). The current four-word descriptor contains neither a generation nor reader
tracking, so the replacement protocol needs an explicit API/ABI design or extension.

**[O]** The next recommended bounded proof keeps the existing 8,192-frame float32 bank and proves stop-and-reload
with an explicit DSP reader-quiescence handshake and generation guard. Do not combine capacity increases,
format changes or a new pool with that lifecycle proof. Investigate a PCM16 reader for capacity after the lifetime
protocol is established; the sample-length target remains pending. The existing immutable-bank proof remains
valid and unchanged.

The following capacity ceilings spend the entire reserved DSP area on audio, **before code, descriptors,
alignment, other modules and safety headroom**. They are arithmetic bounds, not playable product limits.

| Next option | Raw resident ceiling DT2 / DN2 at 48 kHz mono | Required evidence / decision |
| --- | --- | --- |
| Float32 single bank | 131072 / 81920 frames (2.731 / 1.707 s) | Choose a smaller shared budget and length; prove stop/quiescence/generation replacement contract. |
| PCM16 resident bank | 262144 / 163840 frames (5.461 / 3.413 s) | Investigate packed signed-PCM16 reads/conversion with native numerical proof and a paired A/B before adoption. Same source precision, half the float32 storage; no speed claim. |
| Newly owned pool | Unknown | Establish ownership, bounds, all competing consumers and lifecycle before allocating. External RAM or a DT2 stock pool is not established as available. |

Memory aliases name the same physical storage and add no RAM. DT2 stock sample ownership does not establish an
available pool on DN2. A larger declared bank also competes with every other loaded module.

## Existing-link streaming ceiling

**[D]** Raw 48 kHz mono PCM16 consumes **96,000 bytes/s**, before framing or recovery. Current frame geometry
leaves at most 171 payload words on DT2 and 12 on DN2 after three header words. At 1,500 frames/s this gives
`171*4*1500 = 1,026,000` bytes/s and `12*4*1500 = 72,000` bytes/s respectively. These are ideal arithmetic
ceilings, not measured throughput: acknowledgements, retries, scheduling and contention reduce useful delivery.
The existing DN2 link cannot sustain that raw stream even at its ideal ceiling. Float32 would demand more.
See device profile tail sizes and [link framing](../../modules/core/adapters/dsp-link/src/cf/dsp-link.c).
Resident loading remains distinct from streaming; no throughput or loading-latency claim follows from the
capacity snapshots or the passing playback scenarios.
