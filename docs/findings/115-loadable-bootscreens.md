# 115. Loadable bootscreens: stock path, footprint and early-boot safety

2026-11-06. DT2 OS **1.17**, DN2 OS **1.12**. Research plus an unreleased,
opt-in recovery prototype; its current coupled acceptance is incomplete. There
is no player or hardware recovery proof. The immediate static startup
logo is outside the established ownership boundary; replacing it remains **[O]**.

**[V]** below marks parent findings independently checked against current image
bytes by a second agent. **[D]** marks readings, arithmetic and session-local
build/emulator observations; **[O]** marks proposals or missing proof. Byte checks establish
instructions and stored values, not hardware timing or recoverability.

## Stock animation

**[V]** The animated intro renders a **128 × 64 monochrome bitmap**, 1,024 bytes.
Its packing is column-major, two big-endian 32-bit words per column:
`word = x * 2 + (y >> 5)`, `mask = 0x80000000 >> (y & 31)`.
The dither path reads row-major unsigned bytes and sets a pixel iff the source
value is **greater than 128**. These grayscale sources are not the panel format.

| Function | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| Intro installer | `0x400d1448` | `0x400d3920` |
| PIT3 ISR | `0x400d07cc` | `0x400d2ca4` |
| Intro task | `0x400d1a12` | `0x400d3eea` |
| Scene setup | `0x400d1832` | `0x400d3d0a` |
| Frame step | `0x400d18f0` | `0x400d3dc8` |
| Dither copy | `0x400d0bba` | `0x400d3092` |
| Bitmap constructor | `0x401124b6` | `0x40113e2a` |
| Pixel setter | `0x401124fc` | `0x40113e70` |
| Intro completion wait | `0x400d1502` | `0x400d39da` |

**[V]** Both installers program PIT3 PMR `0x2191` (8593), write PCSR `0x0936`,
then OR `9` (configured control value `0x093f`). **[D]** With the documented
132 MHz bus clock and PIT divisor `2^PRE`, this is
`132000000 / (512 * 8594) = 29.999127 Hz`, a 33.334303 ms period.
This is the configured animation cadence, not measured display throughput.
The earlier 15 fps interpretation is corrected in
[finding 07, timer correction](07-emulator.md#timers-at-the-device-rate-prescaler-held-ticks-idle-skip).

**[V]** The task selects one of five tables. Each has one callback/count pair
and a null terminator; each frame step invokes the callback and advances its
counter. **[D]** The scene callbacks offer a possible reuse boundary for a
player, but replacement and completion behavior still require proof.

| DT2 table / callback | DN2 table / callback | Frame steps |
| --- | --- | ---: |
| `0x402a1b64` / `0x400d1512` | `0x4028c694` / `0x400d39ea` | 175 |
| `0x402a1b74` / `0x400d0dce` | `0x4028c6a4` / `0x400d32a6` | 180 |
| `0x402a1b84` / `0x400d117c` | `0x4028c6b4` / `0x400d3654` | 180 |
| `0x402a1b94` / `0x400d11f0` | `0x4028c6c4` / `0x400d36c8` | 180 |
| `0x402a1ba4` / `0x400d120c` | `0x4028c6d4` / `0x400d36e4` | 170 |

## Identified stock footprint

**[D]** The identified footprint is **68,172 bytes (66.57 KiB)** in uncompressed
MAIN on each device. It counts the local code envelope, local tables/descriptors,
and the union of graphics payload ranges referenced by all five variants. It is
not one animation file, guaranteed removable space, or a complete accounting of
every related instruction/string. Some artwork belongs to the shared graphics
collection. The firmware container compresses MAIN collectively; no exact
independent compressed bootscreen size has been established.

All ranges below are half-open.

| Component | Bytes | DT2 range | DN2 range |
| --- | ---: | --- | --- |
| Local code envelope | 4,860 | `0x400d07cc..0x400d1ac8` | `0x400d2ca4..0x400d3fa0` |
| Five scene tables with terminators | 80 | `0x402a1b64..0x402a1bb4` | `0x4028c694..0x4028c6e4` |
| Control scalar | 4 | `0x402a1bb4..0x402a1bb8` | `0x4028c6e4..0x4028c6e8` |
| Five PixelData descriptors | 60 | `0x402a1bb8..0x402a1bf4` | `0x4028c6e8..0x4028c724` |
| 128 × 64 grayscale source | 8,192 | `0x4020b260..0x4020d260` | `0x401f3f14..0x401f5f14` |
| 56 × 64 grayscale source | 3,584 | `0x4020d260..0x4020e060` | `0x401f5f14..0x401f6d14` |
| 32 × 24 bitmap and mask | 256 | `0x402d8548..0x402d8648` | `0x402c3050..0x402c3150` |
| 60 × 64 bitmap and mask | 960 | `0x402e681c..0x402e6bdc` | `0x402d1324..0x402d16e4` |
| 128 × 64 bitmap and mask | 2,048 | `0x40307b18..0x40308318` | `0x402f2620..0x402f2e20` |
| 19 frames and one shared mask | 20,480 | `0x4030079c..0x4030579c` | `0x402eb2a4..0x402f02a4` |
| 12 frames and one shared mask | 13,312 | `0x402f091c..0x402f3d1c` | `0x402db424..0x402de824` |
| 13 frames and one shared mask | 14,336 | `0x402fb31c..0x402feb1c` | `0x402e5e24..0x402e9624` |

The graphics union is **63,168 bytes**, including 11,776 bytes of grayscale
sources and 48,128 bytes of sequence planes. Sequence frames are all 128 × 64,
1,024 bytes each; one mask per sequence is counted once. Constructors in
`0x4015c2e8` (DT2) / `0x40151b94` (DN2) establish their dimensions and pointers.
The three standalone RAM bitmap objects are constructed from MAIN source
pointers: a RAM object address does not mean its pixels have no firmware cost.

The code envelope has 26 indexed functions totaling 4,704 bytes and 156 bytes
of unindexed gaps, including executable callbacks/destructors. Excluded: shared
math/rendering/allocation/RTOS and graphics-initializer code, ancillary strings
and constants, and RAM workspaces. Three PixelData descriptors point to
explicitly cleared 8,192-byte work buffers (24,576 bytes total); the default
variant also builds a 65,536-byte procedural lookup vector at runtime.

## Early hook candidate and stock bypass

**[D]** On ordinary boot the stock raw eMMC path is invoked before the intro:

| Stage | DT2 call / callee | DN2 call / callee |
| --- | --- | --- |
| Card initialization | `0x400ccc74` / `0x4012d78e` | `0x400cf0a8` / `0x4012be52` |
| DMA/IRQ completion setup | `0x400ccc7a` / `0x4012dd74` | `0x400cf0ae` / `0x4012c438` |
| Boot-record read | `0x400cccce` / `0x400f078c` | `0x400cf102` / `0x400f2168` |
| Chunking wrapper | `0x400efdf2` | `0x400f17ce` |
| Raw block reader | `0x4012e1b6` | `0x4012c87a` |
| Skip getter | `0x400ccce4` / `0x400cc992` | `0x400cf118` / `0x400cedc6` |
| Intro installer | `0x400cccf2` / `0x400d1448` | `0x400cf126` / `0x400d3920` |

The boot-record reader invokes the wrapper at sector `0x40000`, length 256,
optionally trying `0x48000`; the wrapper calls
[`cf.storage.fn.block.read`](../../devices/fw/storage.toml). The reader checks
controller readiness and sector bounds, takes a mutex and blocks on DMA/IRQ
completion semaphores. A mounted filesystem is not required for this raw path.
Readiness is set at DT2 `0x4012dcf0` / DN2 `0x4012c3b4`. The wrapper discards
read errors: reaching it does not prove successful storage access on every boot.

**[D]** Panel transport setup `0x4000243e` is called at DT2 `0x400cca82` / DN2
`0x400ceeb6`, followed by panel-protocol commands, before these storage calls.
Intro flush functions are DT2 `0x40133bea` / DN2 `0x40131fd2`; page writers
`0x40133b1c` / `0x40131f04` use that transport. **[O]** This supports an early
player candidate, not proven buffer ownership, presentation or hardware timing.

**[V]** Main entry waits for intro completion at DT2 `0x400337dc` / DN2
`0x4002ed50`. The current runtime main-entry hook precedes that wait, but the
intro was installed before main-task creation. Whether the hook runs before
the first visible frame is **[O]**; do not claim the intro is already finished
or that the existing hook necessarily replaces its beginning.

**[D]** The intro bypass is an internal boot mode. Reset entry `0x400004ec`
copies the incoming bootstrap argument at `4(sp)` into DT2 `0x4029eef0` / DN2
`0x40287a60`. The getter returns bit 5 (`0x20`). The same bit enables the service
console task at DT2 `0x400ccdfc` / DN2 `0x400cf206` and selects USB mode 4.
Boot-record updates OR `2`/`0x92` or `4`, leaving bit 5 unchanged. No normal
setting, front-panel gesture, or soft-restart cause for bit 5 was established;
its bootstrap origin is **[O]**. With bypass set the installer initializes its
semaphores and signals completion without starting animation. Bit 6 bypasses
the ordinary record/intro boot path entirely. Preserve both boot modes and the
completion contract; the bypass is not a proposed recovery shortcut.

## Safety and architecture constraints

**[D]** Existing protection occurs too late to assume it covers a new pre-intro
hook. [`boot.rs`](../../modules/core/runtime/src/rs/src/boot.rs) makes its safe
decision and increments the boot counter at main entry (lines 268–310), after
the proposed early hook. The increment is conditional on selected module count
or a nonzero existing counter, and requires a writable card. A bootscreen-only
setup with no modules must be covered explicitly. A read-only card has no
counter protection. The held-RECORD check reads queued events; its own comment
at line 194 says hardware arrival timing is unverified. The exception handler
is installed from the UI loop ([`crash.rs`](../../modules/core/runtime/src/rs/src/crash.rs),
lines 120–128; `lib.rs:1119`), not before early code.

**[O]** An early fault or hang could prevent reaching today's protections. The
stock blocking reader has no established recoverable deadline. A timeout in a
caller does not by itself interrupt a blocked mutex/semaphore wait. Prove an
independent early bypass, bounded failure behavior and recovery before loading
custom code or trusting an asset. Missing, corrupt or oversized content must
fall back to stock without breaking completion. The bootloader STARTUP recovery
path is separate; it is not a guarantee of zero risk from a new early hook.

The sole active service-refactor plan is
[`runtime-and-services.md`](../plans/runtime-and-services.md), currently a draft
with nothing built. Sections 5.1–5.5 propose dependency-loaded `.dks` providers
containing their adapter and hook table; safe mode leaves service sites stock.
Section 5.5 explicitly keeps hooks needed before main entry in the image and
requires them to be harmless without a service. This research does not amend
that plan or establish an implemented early-provider phase.

**[O] Decision:** a separate feature adapter with a minimal resident early gate
under the current plan, versus a generic early-provider phase whose safety has
first been proved. Preserve the user's preference for future on-demand services.
Keep feature-format parsing out of the runtime; do not move the full loader
earlier or add an SDK/boot phase as an incidental bootscreen change. Measure
adapter code/data/BSS/stack, resident gate, staging and provider costs before
choosing; none has been measured here.

Report permanent flash/core-stub bytes separately from provider file, code,
rodata, initialized data, BSS and stack; also report asset disk bytes, retained
RAM and the zero-provider path. The stock 66.57 KiB figure does not predict any
of those adapter costs.

## Asset/workbench proposal, not an implemented format

**[O]** Host authoring could use deterministic Canvas code plus SVG/PNG inputs,
exporting 128 × 64 PNG frames with timing and an APNG preview. Device output could
be packed 1bpp with frame holds, XOR deltas/RLE and a raw fallback. Define bounds,
validation and ownership before choosing the format. Two full framebuffers are
2,048 bytes. Six seconds at 30 fps is 184,320 raw bytes; 30 KiB/s is the theoretical
raw payload rate, not a measured decoder, storage or display benchmark.

**[D]** [`sdk.toml:991`](../../sdk/sdk.toml) limits current `.dkm` staging to
65,536 bytes. Generic `DkSession.put(path, data)` exists in
[`dkupload-core.ts:990`](../../packages/dk-session/src/dkupload-core.ts), but the
workbench module workflow targets `.dkm`; bootscreen sidecar packaging, selection,
upload UI and playback do not exist. Provider, decoder, staging and framebuffer
costs remain **[O]** rather than size guesses.

**[D]** The current emulator intro observer watches `setPixel` and completes a
frame after every pixel has been seen; bulk-copy/memcpy frames bypass it. See
[`runtime.rs`](../../packages/emulator/boot/src/runtime.rs), lines 1977–1992,
2338–2353 and 3060–3157. **[O]** A common completed-frame boundary must capture
both stock procedural frames and a copying player before screenshots can prove
equivalence. Lack of a tracked frame is not proof of absent device output.

## Opt-in recovery proof checkpoint (2026-11-06)

**[D]** Em authorized a safety-only implementation in the isolated bootscreen
worktree. It adds `core/boot-guard`, explicitly selected by the new
`core/early-boot-tests` owner; no default patch-set membership changed. This is
not a player, asset loader, `.dks` provider phase or SDK extension. Its optional
`dk_boot_early` callback is linked into the image by test fixtures.

The hash-resolved installer before-hook preserves the stock skip argument and
always continues through the original installer. Compiled assembly tests skip
and the optional symbol address before entering C; an absent callback performs
no early recovery preparation. Runtime preparation and module loading have
separate once-only latches, including an explicit preparation-in-progress state.
The private early gate requires a valid selection, an owned card without probe
read errors, a successful counter write (including zero selected modules), and
readback of the actual exception-hook word. The later main-task hook retains
the queued RECORD check and module loading. A denied early arm leaves the late
counter policy available for the cached selection; failed writes retry the same
intended counter. Preparation caches `REC`: refreshing a selection and loading
its modules after transient early read errors remains unproved. The host units
cover the counter-write transaction, not read recovery.

**[D]** Current validation is incomplete:

| Check | Observed result |
| --- | --- |
| Runtime host target | 55 passed, including real counter-transaction failure/retry/idempotence units |
| Generated current ports and bindings; layer check | Passed |
| Final affected preflight and selection | 11 tasks passed, 48.4 s; merge selection explained, full merge gate not run |
| CLI/profile-base host tests; test-support host tests/types | 8 parser + 2 validated-base + 5 test-support tests passed; both affected TypeScript checks passed |
| Existing DT2 safe-mode/torn-selection and crash-capture files | 11 passed (9 + 2), 597.16 s, before startup-only host changes; device runtime unchanged, no early adapter |
| Current non-reset proof families | 10 passed (5 per device), 414.13 s; current runtime/startup seam, before isolated control addition |
| Current existing DN2 safe-mode/torn-selection file | 9 passed, 789.95 s; no early adapter |
| True-empty DN2 returning-callback control | 1 passed, 50.31 s: empty startup, normal UI, callback once, persisted counter 1 and 150 more DSP frames |
| Earlier new-proof iteration | DN2 6 passed; DT2 5 passed and reset readiness failed; these do not accept the strengthened current revision |
| Current DN2 reset case, concurrent then alone | Both failed in baseline reload before the new DSP/input assertions; 190.56 s and 199.30 s total |
| Initial-seeded DN2 first patched load over stock runtime | Valid empty selection, no second manual reload; failed in initial baseline before body assertions, 133.24 s total |
| True-empty DN2 reset case | Empty startup observed, but first prepared baseline timed out before body assertions, 132.69 s total; no DT2 true-empty reset attempt |

The reset and its paired empty-start control use the existing `relaxFs` reset
contract, matching the existing safe-mode test; the ten normal cases do not
change their startup contract. The initial DT2 failure
had a null filesystem-verification field after a reset: the restored runtime
reconstructed a stricter readiness contract than the coupled reset path. A
single three-frame status was not evidence of a DSP stall. The underlying
readiness-contract serialization gap is outside this implementation.

The strengthened assertions retain the safe/counter/selection checks and add
150 frames of DSP progress plus SETTINGS/NO framebuffer response, interrupt and
drained-input checks. It explicitly resumes the daemon after record export
pauses it. These added assertions have **not run successfully**: both latest
running-runtime DN2 reload attempts hit the unchanged lifecycle deadline (about 130 s in reload), with
the 120 s no-progress watchdog unable to obtain asynchronous status. The saved
status is a paused, ready **pre-reload** epoch, not post-failure evidence. The
sequential failure rules out an explanation based solely on another owned
emulator running concurrently. A separate scenario then seeded the **initial**
fixture with a valid empty selection and used `h.ready(true)` without a
second `h.boot` or the normal-only `h.first` helper. This also failed once in
initial baseline establishment (about 130 s), before safe/counter/DSP/UI body
assertions. No exact reset count or completed lifetime proof was obtained for
that failure. **[C]** `Session.start` already loads the positional stock image
before the initial fixture; this was a first patched load over stock runtime,
not an empty coupled power-on. Both earlier patch-load failures remain open.

**[D]** A subsequent scoped startup seam genuinely leaves the daemon empty:
`SessionOptions.startEmpty` is restricted to reusable coupled sessions, omits
only the positional startup image, and retains explicit automatic audio-profile
and DSP inputs. Startup status was recorded with epoch 0, firmware null and no
ColdFire runtime. The first prepared coupled load resolves stock bytes from
the configured profile and checks their hash against the declared base before
the existing DSP-diff path; the loaded status must still report coupled audio
and exact prepared-image identity. Default startup behavior is preserved.

The one true-empty reset attempt also hit the unchanged baseline deadline
(about 130 s), before body assertions or a completed lifetime proof. A separate
DN2 returning-callback control then **passed** using the same empty startup,
valid empty selection and MainPanel contract: normal UI, one callback,
persisted counter 1, and 150 additional DSP frames were observed. Generic
empty coupled startup therefore works for that control; reset acceptance is
still red. No reset retry or DT2 true-empty reset attempt followed. The failed
reset remains registered and unskipped; filtered passing cases do not accept it.

**[D/O]** The failed run's unique cache contains a CF/DSP pair. Source ordering
writes the main snapshot last, after cold `status.ready`, DSP quiet and save/
export; this does not verify boot-info counters or installation in the actor.
The unresolved stages are worker return/teardown, controller completion,
patched-audio restore, and actor install/reply. The next diagnostic should add
bounded phase markers there, rather than another startup or timeout variant.
The generic non-reset control passed, so no further startup seam is proposed.
An earlier overly broad legacy title-filter run was aborted and counts as no
accepted result. The full merge gate has not run; this is not merge-ready.

**[D]** Built-object costs, before placement alignment:

| Component | Text | Read-only data | Data | BSS |
| --- | ---: | ---: | ---: | ---: |
| Common runtime delta against checkpoint HEAD | +520 B | 0 B | 0 B | +7 B |
| Opt-in adapter, including proof instrumentation | 130 B | 0 B | 0 B | 172 B |

The observed DN2 plan reserves 304 B for the adapter and 16 B for the hook tail
(14 B of tail code), 320 B total. The 8-byte installer site is replaced in
place. The adapter's two typed snapshots and counters are proof overhead, not
a player cost. The absent-callback path still reserves this adapter memory if
the adapter was explicitly included; default images do not include it. The
runtime trampoline's generated object changed only its source fingerprint.
Stack use and compressed-container/physical-flash delta were not measured.
There is no new asset, decoder, framebuffer allocation or executable card load.

**[O]** This remains blocked for production. Raw storage calls can block before
the counter is armed. Prior-crash replay can flush settings/store data before
arming: preparation is not read-only and cannot be described as selection-only
I/O. No positive initial panel-scan marker was established; a cleared cached
key state cannot prove that RECORD was sampled. Early physical bypass, DTOE
versus the earlier DMA-semaphore wait, read-only recovery, and an actual early
CPU exception still need proof. The existing late-path exception tests do not prove
the early hook's timing. No player or service architecture decision is implied.

## Acceptance still pending

| Case | Required proof |
| --- | --- |
| Absent, invalid, corrupt or oversized asset/provider | Stock fallback, bounded validation, completion preserved |
| Power cut during upload | No partial asset becomes active; previous valid selection or stock remains |
| Read failure, timeout, early fault or hang | Bounded escape or independent bypass before existing protections; later selected-module behavior after early I/O denial still needs proof. `REC` is cached; counter-write retry units do not establish selection refresh after transient reads. |
| Repeated failed boots; bootscreen-only/no modules | Protection accounts for early work and eventually bypasses it |
| Read-only card | Explicit recovery independent of counter writes |
| Safe-once flag, held key, service/skip boot modes | Earliest behavior and queued-key timing proved; stock semantics preserved |
| Competing providers/hooks | Conflict refused before patching; safe mode leaves relevant service sites stock |
| Normal playback | Frame-boundary capture, timing, allocations and stack proved on DN2, then DT2 |
| Callback/buffer lifetime | Preserve bitmap flags, orientation, ownership and completed-frame publication; no provider cleanup until the callback returns and no stock table/function pointer references its memory |

No row has a bootscreen implementation proof yet. Use bounded runs and the owning
acceptance targets; compare with stock and run one paired A/B through
`tools/devtools/coupled_ab.py` before performance claims.
Current modules remain loaded for the session; a future early provider's cleanup
or unload facility is not implemented or promised by this proposal.

Existing reuse points under owner target `core/runtime-tests:test-device` are
[safe-mode](../../modules/core/runtime/tests/safe-mode.device.test.ts)
(failed init, safe-once, early/late hangs and torn selection),
[crash-capture](../../modules/core/runtime/tests/crash-capture.device.test.ts),
[card-safety](../../modules/core/runtime/tests/card-safety.device.test.ts) and
[fault-trace](../../modules/core/runtime/tests/fault-trace.device.test.ts).
These prove behavior against emulator daemons. `test/dyn-fault` resets from
`dkm_init`, later than the proposed pre-intro hook; reuse does not automatically
cover the early phase. There is no existing held-RECORD hardware-timing or
pre-main-hook proof in these suites.

## Reproducing the static evidence

Read the firmware-query skill first. From the repository root, for example:

```sh
UV_CACHE_DIR=/private/tmp/digi-boot-query-uv uv run --no-sync python -m tools.coldfire.fw batch - <<'Q'
disasm 0x400d1448 -i dt2-1.17 --max 0
disasm 0x400d3920 -i dn2-1.12 --max 0
decomp 0x400d18f0 -i dt2-1.17 --max 0
decomp 0x400d3dc8 -i dn2-1.12 --max 0
dossier 0x400f078c -i dt2-1.17 --other dn2-1.12 --decomp 90
dossier 0x400efdf2 -i dt2-1.17 --other dn2-1.12 --decomp 30
dossier 0x400cc79e -i dt2-1.17 --other dn2-1.12 --decomp 30
fn 0x4015c2e8 -i dt2-1.17 --max 8
fn 0x40151b94 -i dn2-1.12 --max 8
Q
```

Use `tools.patchkit.machineprofile.image_reader(path, 0x40000400)` on the existing
`out/sections/<image>/section_3_MAIN_OS.bin` for guest-address scalar reads;
`struct.unpack('>II', read(table, 8))` reads a scene pair. Use
`tools.coldfire.disasm.disasm` on those bytes for an independent decode. For
listing checks, `tools.coldfire.fw.CF.func_row` and `read_dump` expose the existing
listing; compare each listed instruction's bytes with the raw reader. Do not
export graphics or firmware-derived blobs into documentation.

Session checks: all 4,704 indexed local-code bytes per image matched raw MAIN;
all 99,614 graphics-initializer instruction bytes per image matched. Additional
boot/storage/display checks covered 4,352 DT2 / 4,304 DN2 instruction bytes in
11 functions, plus the 74-byte console creator and 10-byte reset flag copy per
image. Scalar reads established the tables/descriptors and distinct plane ranges.
Raw refscan helped find shared initialization references; its DT2 whole-image
decode coverage was 96.81%, not an exhaustive proof about computed references.
