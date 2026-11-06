# 115. Loadable bootscreens: stock path, footprint and early-boot safety

2026-11-06. DT2 OS **1.17**, DN2 OS **1.12**. Research plus an unreleased,
opt-in recovery prototype; its focused coupled safety fixtures pass. Production
recovery acceptance remains incomplete. There
is no production player or hardware recovery proof. The immediate static startup
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
| Prior `4721bec8` checkpoint preflight and selection | 11 tasks passed, 48.4 s; merge selection explained, full merge gate not run |
| Follow-up affected preflight and selection | 11 tasks passed, 41.3 s; merge plan explained 65 affected tasks plus prerequisites, full merge gate not run |
| CLI/profile-base host tests; test-support host tests/types | 8 parser + 2 validated-base + 5 test-support tests passed; both affected TypeScript checks passed |
| Existing DT2 safe-mode/torn-selection and crash-capture files | 11 passed (9 + 2), 597.16 s, before startup-only host changes; device runtime unchanged, no early adapter |
| Current non-reset proof families | 10 passed (5 per device), 414.13 s; current runtime/startup seam, before isolated control addition |
| Current existing DN2 safe-mode/torn-selection file | 9 passed, 789.95 s; no early adapter |
| True-empty DN2 returning-callback control | 1 passed, 50.31 s: empty startup, normal UI, callback once, persisted counter 1 and 150 more DSP frames |
| Earlier new-proof iteration | DN2 6 passed; DT2 5 passed and reset readiness failed; these do not accept the strengthened current revision |
| Current DN2 reset case, concurrent then alone | Both failed in baseline reload before the new DSP/input assertions; 190.56 s and 199.30 s total |
| Initial-seeded DN2 first patched load over stock runtime | Valid empty selection, no second manual reload; failed in initial baseline before body assertions, 133.24 s total |
| Initial true-empty DN2 reset attempt | Empty startup observed, but the original 120 s baseline window expired before body assertions, 132.69 s total; superseded by the follow-up below |
| Follow-up DN2 reset acceptance | 1 passed, 135.92 s total / 123.34 s baseline, with the scoped 180 s three-boot budget and all original assertions |
| Follow-up DT2 reset acceptance | Passed after the DT2-only known-main gate: 57.15 s total / 42.266 s baseline; retained-image repeat also passed, 57.22 s / 42.372 s. All original roundtrip/IRQ/lifetime assertions retained |

The reset and its paired empty-start control use the existing `relaxFs` reset
contract, matching the existing safe-mode test; the ten normal cases do not
change their startup contract. The initial DT2 failure
had a null filesystem-verification field after a reset: the restored runtime
reconstructed a stricter readiness contract than the coupled reset path. A
single three-frame status was not evidence of a DSP stall. The underlying
readiness-contract serialization gap is outside this implementation.
MainPanel acceptance allows missing filesystem verification and rejects
explicit failure; it does not imply the stock filesystem worker is skipped.
The later DT2 captures show verification/loading continuing after snapshot.

The historical attempts below used the original 120 s baseline window. The
strengthened assertions retain the safe/counter/selection checks and add
150 frames of DSP progress plus SETTINGS/NO framebuffer response, interrupt and
drained-input checks. It explicitly resumes the daemon after record export
pauses it. At that checkpoint these added assertions had not run successfully: both
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
still red at that checkpoint. No DT2 true-empty reset attempt had followed. The failed
reset remains registered and unskipped; filtered passing cases do not accept it.

**[D/O]** The failed run's unique cache contains a CF/DSP pair. Source ordering
writes the main snapshot last, after cold `status.ready`, DSP quiet and save/
export; this does not verify boot-info counters or installation in the actor.
The unresolved stages are worker return/teardown, controller completion,
patched-audio restore, and actor install/reply. The next diagnostic should add
bounded phase markers there, rather than another startup or timeout variant.
The generic non-reset control passed, so no further startup seam is proposed.
This was the diagnostic hypothesis at the checkpoint, corrected below.
An earlier overly broad legacy title-filter run was aborted and counts as no
accepted result. The full merge gate has not run; this is not merge-ready.

### Follow-up: deadline ordering and device acceptance

**[C]** Three retained failed runs completed their cache pairs 4–6 s **after**
the original 120 s deadline. They do not establish a post-cache, transport or
DSP hang. In the refined run, the failure artifact was written at Unix
1793965094.447 s, the main snapshot at 1793965099.445 s and the final daemon
log at 1793965099.644 s. The late `poll.reply.sent` marker therefore did not
show that a client was still waiting when the reply was sent. The earlier
post-cache stall interpretation is withdrawn; older first/later-load failures
were not rerun or independently resolved by this result.

**[D]** Default-off `DIGI_TRACE_PATCH_BOOT=1` stderr markers now bracket cold
pair saving, worker return/send, controller completion, coupled restore and
actor installation/reply. They include Unix wall milliseconds and reset
generation where applicable. The existing baseline-error artifact now records
the pending operation, resolved load report if any, expected identity and
whether its saved status is before or after load. The refined failure retained
`phase: firmware.load`, `load: null`, `statusPhase: before-load`, epoch 0 and
no firmware. Artifact directories are `digi-session-myxJgM` and
`digi-early-boot-reset-recovery-dn2-QmqStv` under the session temporary root;
the preceding marker run is `digi-session-bIJNmT` / `digi-early-boot-reset-recovery-dn2-nNaFA9`.

**[D]** The reset family alone now uses `baselineTimeoutMs: 3 * 60_000`, passed
to both the existing baseline deadline and no-progress watchdog: one bounded
minute per expected cold boot. Other fixtures retain their existing 120 s
defaults/environment behavior; the 600 s hook backstop is unchanged. This is
an emulator workload budget, not a hardware boot-time claim. No engine, reset,
snapshot, readiness or firmware behavior was fixed or relaxed.

The exact DN2 1.12 reset case then passed: 135.92 s total, 123.34 s baseline.
All safe/counter/empty-selection, 150-frame DSP, SETTINGS/NO framebuffer,
interrupt/input-drain and lifetime assertions passed. Timestamped boot markers
showed roughly 38 s for each reset boot before the third safe boot; the first
two generations together took about 76 s. Passed session
`digi-session-flZ5hu` / `digi-early-boot-reset-recovery-dn2-NoHRt2` was cleaned up
by the harness. This accepts that DN2 emulator scenario, not physical bypass or
every running-runtime reload path.

The first DT2 1.17 follow-up failed. Its retained Vitest cache records
`failed:true`, 53.873 s file duration; raw terminal output/exit text was lost
when the tool result was truncated. The phase log shows two guest resets and
the baseline reply after 41.855 s (worker boots 9.268 s, 9.197 s and 20.792 s).
The retained images show `VERIFYING FILE SYSTEM...` over the initial main
screen, then a loading/main page rather than SETTINGS, then unobscured main.
Thus hash inequality was a false-positive SETTINGS-entry oracle, and the
full-hash return equality fails on the retained bytes. Downstream IRQ/input
assertions are unproved. Lifetime safety recorded `pass:true`; that does not
make the case pass. Root independently inspected all three rendered images.
Evidence: `/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-session-27q4z7`
and `digi-early-boot-reset-recovery-dt2-e3AUh0` in the same temporary root;
analysis PNGs are `/private/tmp/bootscreen-dt2-reset-oled/`.

**[D]** The reset case now gates DT2 input on the visually verified empty-card
stock main framebuffer hash
`f55c67e4d946992137a1381c0883f9a3e76991a650ef2ceb31996b5dab0df817`, using a
bounded 30 s framebuffer poll after the existing 150-frame DSP check. It
excludes verification/loading overlays; DN2 is unchanged. The original
SETTINGS difference, full main-return equality, IRQ/input and lifetime checks
remain intact. This is a known-frame oracle for this fixture, not a new
firmware state or global readiness rule.

The exact DT2 case passed with exit 0: 57.15 s total / 42.266 s baseline. Its
successful cleanup removed captures before review, so one unchanged repeat
retained the missing visual evidence and also passed: 57.22 s total / 42.372 s
baseline, 1 passed / 12 skipped. Raw outputs are
`/private/tmp/bootscreen-dt2-oracle-calibration.log` and
`/private/tmp/bootscreen-dt2-oracle-retained.log`. The repeat's announced
`digi-early-boot-reset-recovery-dt2-wUyAoq` directory was copied before normal
cleanup to `/private/tmp/bootscreen-dt2-settled-review/`. Root inspected its
`safe-main.png` (unobscured SMP view) and `safe-settings.png` (actual SETTINGS:
PROJECT, SONG, PATTERN, MIDI CONFIG, SYSTEM). The SETTINGS hash is
`dd320930a8c1d208fb3a210d66c1b2c237f8e0d9108ee5a1846534235187fe82`; it is
recorded evidence, not another menu-layout assertion. Both device reset cases
are now accepted for these emulator fixtures. The failed transient-modal run
remains in the history, with its downstream assertions unproved.

This remains an experimental checkpoint without push, PR or merge. The common
device runtime is unchanged from `4721bec8`; the earlier normal 10, legacy 20 and returning
control results retain their stated scope. No engine behavior fix, player or
asset work was added. Production blockers below remain open. Next work is
bounded storage/independent bypass and the outstanding safety cases, not more
timeout or startup variants.

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
versus the earlier DMA-semaphore wait and read-only recovery still need proof.
The following genuine early CPU-exception proof covers working-card emulator
conditions only, not hardware or blocked pre-arm I/O. The existing late-path
exception tests alone do not prove early timing. No player or service
architecture decision is implied.

## Acceptance still pending

### Genuine early CPU exception proof (2026-11-07)

**[D]** A separate linked illegal-instruction fixture exercises vector 4 at the
exact optional callback PC before the original intro installer. It does not
write SOFTRST. Both current-device emulator cases passed as recorded below.
Static inspection found that the existing controller
`cpu.exceptions` policy reached installed runtimes but not cold workers; the
bounded patch forwards it before guest execution and includes true mode in
the patched-pair cache key, preserving false-mode keys. The proof
requires valid retained CPU evidence, persisted `dk/fault`, two handler resets
and safe third boot with zero selected modules, plus DSP/UI/input/lifetime
checks. This does not replace the previously passing reset fixtures.

**[D]** The exact DN2 1.12 genuine-exception case passed (exit 0, 1 passed /
1 skipped): 135.58 s total, 123.623 s baseline. The retained version/CRC-valid
record reports vector 4 at the planned `dk_boot_early` entry `0x468046a4`,
no dynamic module (`0xffffffff`), pre-stable crash, RAM kept and `saved:1`.
The persisted fault decodes as CPU at the same PC with no module name. The
third boot is safe-counter mode with counter 2 and zero modules; the UI clears
the stored selection counter and preserves empty paths. DSP progress,
SETTINGS/NO response, IRQ/input and scenario lifetime assertions passed.
Written store sectors are exactly the two selection sectors and the first
sector of each settings copy (`0x2e19fde`, `0x2e19fee`, `0x2e19ffe`,
`0x2e19fff`). The fixture preallows only those settings sectors and requires
a settings-copy write, rather than accepting arbitrary observed writes.

Evidence is retained at `/private/tmp/digi-early-exception-evidence-dn2-Mtm7qy/`
(`early-exception.json`, `startup-empty.json`, `lifetime-safety.json`); full
output is `/private/tmp/bootscreen-dn2-early-exception.log`. The generic raw
lifetime report says `pass:false`: it is generated with the default policy
before CardRig filters the scenario's explicit settings-sector allowance.
Its only `invalid_store` entries are those two settings-copy sectors;
CardRig's final checks passed with zero write-watch/flush hits and no other
invalid writes. Do not label that historical raw report `pass:true`.

**[D]** The exact DT2 1.17 case then passed (exit 0, 1 passed / 1 skipped):
61.13 s total, 43.957 s baseline, with retained vector-4 CPU record and
persisted fault at planned callback PC `0x47f73de4`. It has the same validated
record, safe-counter/zero-module, four-sector-only write, DSP/UI/input and
lifetime evidence as DN2. Evidence is
`/private/tmp/digi-early-exception-evidence-dt2-ldET0h/`; full output is
`/private/tmp/bootscreen-dt2-early-exception.log`. A small reporting-only fix
before DT2 passes CardRig's existing explicit sector allowance into
`runtimeSafety` facts; no permission was broadened. Its retained report now
lists exactly `setA`/`setB`, has `pass:true`, zero write-watch/flush hits and
empty invalid-write lists. The existing write-safety host file passed 21 tests,
including default rejection, exact acceptance, adjacent-sector rejection and
unchanged firmware-area rules. DN2 was not repeated for this reporting fix.

The fixture is four bytes (`4afc60fe`, two re-decoded instructions), with the
callback entry at the illegal instruction. The two production pair-key host
tests, regression types and the daemon prerequisite (16.1 s) passed before
proof. After DT2, retention was made portable with `os.tmpdir()`; only
format/type checks apply to that output-path change, with no emulator repeat.
Both tests used the existing 180 s three-boot workload window and DT2 known-main
gate; neither global limits nor firmware/runtime handler behavior changed.
This establishes a real early guest exception and retained/persisted recovery
under working-card emulator conditions, not hardware or pre-arm-I/O safety.

Final phase checks: generated-module `--check` passed without regeneration;
affected merge plan explained 65 tasks plus prerequisites. Initial cheap
preflight completed 10/11 tasks in 42.9 s, with one unrelated ABOUT deadline
host failure (`expired:false` versus expected `true`). With no code/timeout
change, the isolated test passed; only the failed regression host owner was
then rerun, passing all 177 tests in 6.2 s. The other ten successes were
retained, not repeated. Formatting and diff checks passed. The full merge gate
was not run, and this remains an experimental checkpoint without push/merge.

**[O]** At checkpoint `70c0875d`, pre-arm work included selection reads, area probes/possible
1 MiB blank scan, settings/store loads and `at_boot` immediate fault flush.
A caller timeout cannot cancel blocked SDHC/DMA. Conservative deferral of a
pending KEEP crash to normal late handling, and unrelated settings load on a
clean boot, were design options only in that exception patch. The subsequent
validated deferral below supersedes this historical boundary. A bypass candidate needs separate received/held evidence;
absent freshness must deny optional work, and receipt is not hardware initial
scan proof. No independent early physical bypass is established. See the
[current handover](../handovers/HANDOVER-2026-11-07-early-exception.md).

**[D/O]** Historical `70c0875d` UART dispatch receipt candidate: hash
`8a1e573cfa9d7893e83dc64c6f6caeb3`, offset `0xa8`, len 4, resolves DT2
`0x4011e6f0` / DN2 `0x40120064`. Decoder entry also sees `core/input` injection;
the call site is the physical-receipt candidate. Optional permission would
require fresh channel-2 receipt and RECORD released, denying held/missing
receipt. Packet receipt and request `0x60,0x00` are not initial-scan proof.
Although `Emulator.button` can queue input before stepping, no coupled cold
initial-input bridge or hook is approved in this patch.

**[D]** Parallel primary-source storage research: NXP's
[MCF54418 Reference Manual, Rev. 5, May 2018](https://www.nxp.com/docs/en/reference-manual/MCF54418RM.pdf)
§25.3.9 (pp. 25-20–25-21) defines RSTD/RSTC/RSTA host resets. §25.3.19
(p. 25-38) describes `EXT_DMA_EN` suppressing requests, without documenting
quiescence of an already accepted eDMA transfer. §19.4.1 (pp. 19-6–19-7) and
§19.4.2 (p. 19-8) describe CX/ECX completing the current read/write sequence
before cancellation; §19.4.1 (p. 19-5) also restricts `EDMA_CR` writes to
inactive channels. **[O]** The apparent tension remains unresolved. No maximum
abort/reset latency or firmware semaphore/IRQ ownership contract has been
established, so these register semantics do not establish safe integration or
hardware recovery. No low-level cancellation implementation is approved.

### Historical first fresh-packet proof (before I/O deferral)

**[D]** The first fresh-packet gate revision was distinct from the RECORD
safe-mode check already on main. It observes only UART dispatch, captures
receipt/sticky RECORD-down before intro and denies missing/held input before
requesting runtime preparation. STOP down/up supplies real channel-2 packets
without a historical RECORD-down. The bounded coupled-cold logical-input seam
is implemented. The first exact DN2 fresh-released case passed, 72.06 s total /
67.293 s baseline: captured decision 1, closed observation, one runtime request
and one returned callback; zero modules, persisted counter 1, UI/DSP/lifetime
checks passed. This establishes UART receipt before intro for that emulator
sequence only; missing/held/sticky cases and DT2 had not run at that point. Raw output is
`/private/tmp/bootscreen-dn2-fresh-panel.log`, three named JSON files are in
`/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-early-panel-evidence-dn2-H2eSDV/`.
Receipt does not establish hardware scan completion or bounded storage recovery. See
the [packet-gate handover](../handovers/HANDOVER-2026-11-07-fresh-panel.md).

**[D]** Intermediate packet-gate object: 256 B text / 188 B BSS, versus
`70c0875d` 130 / 172 B; rodata/data zero. Actual plan placement is 444 B object
plus 16 B intro tail plus 20 B UART tail = 480 B, versus prior measured 320 B.
Runtime object sections/relocations are identical; only its generated-source
fingerprint changed. These are measured proof costs, not player, compressed
flash, stack or performance claims.

### Current fresh UART gate and I/O deferral

**[D]** The subsequent I/O-deferral substep is implemented and validated on both
current emulator devices. A pure volatile KEEP word-0 marker check denies early
preparation before selection I/O, leaving even corrupt marked records for
normal late validation. Invalid selection, safe-once or starting counter >= 2
defers after selection reads but before card/settings/crash side effects.
Clean early preparation skips unrelated settings load; main ensures settings
once, while the ordinary late path preserves settings-before-at_boot order.
Only a preparation already complete on main entry can take the early-arm
counter retry. No KEEP/settings format, persistent runtime state or driver
change is introduced. All 58 runtime host tests and revised regression types
passed; the five-task object build, generated and layer checks passed. The earlier packet/cost
measurement predates this runtime change.

The revised exception proof expects one real fault then pending-record
deferral: starting counter 0 with a selected healthy module returns through
normal late boot (counter-1 info), while seeded counter 1/empty selection
returns through late safe-counter 2. Seeded history is not a second fault.
The separate reset fixture retains its two actual reset expectations. Private
pure settings-loaded snapshots and direct late DKRT_SET observation verified
early 0/0 / late 1. Selection/probe
I/O, counter writes and ordinary late fault/settings I/O can still block;
cached probe errors, read-only recovery and physical bypass are still open.

**[D]** All 25 current focused cases passed: 13 early-boot, 8 panel and 4
exception cases. The separate DN2 pending-fault zero-counter run passed in
101.53 s (88.789 s baseline); the remaining three-file matrix passed 24 cases,
with only that prior case skipped, in 1015.06 s, exit 0. No retries or budget
changes. Raw output: `/private/tmp/bootscreen-dn2-fault-deferral-zero.log` and
`/private/tmp/bootscreen-focused-matrix.log`. Retained evidence indexes:
`/private/tmp/bootscreen-matrix-panel-summary.json` and
`/private/tmp/bootscreen-matrix-exception-summary.json`.

**[C]** At historical checkpoint `04f5ec86`, all four held/sticky fixtures observed late reason **NONE**, not KEY.
Decision 3 and zero runtime requests prove the new optional early-work bypass;
they do not establish global late safe mode or hardware RECORD behavior.
Functional REC access to global safe mode did not activate in those cold fixtures;
the subsequent current-key follow-up below corrects this emulator result. Missing
input also denies requests while selected-module late loading succeeds. Fresh
released cases show decision 1, one request/callback and settings loaded 0/0/1.
UART packet freshness is not hardware initial-scan completion.

**[D]** Revised exception cases retain exact vector 4 / PC DN2 `0x46804824`,
DT2 `0x47f73f64`, CRC-valid KEEP and persisted fault evidence, narrow settings/
selection writes, DSP growth, settled UI, actual SETTINGS/NO, IRQ/input and
lifetime checks. Counter-0 cases load the selected module after one real fault;
seeded counter-1 cases reach safe-counter 2 after one real fault. The separate
reset fixture still proves two real resets. The 180 s recovery window is unchanged.

**[D]** Final measured cost against `70c0875d`: adapter text 296 B (+166), BSS
196 B (+24), data/rodata zero; placed object 492 B plus 16 B intro and 20 B UART
tails =528 B (+208). Runtime text 157072 B (+172), actual placement 901456 B
(+176); runtime data/BSS unchanged. Retained small metadata:
`/private/tmp/bootscreen-deferral-section-costs.json` and
`/private/tmp/bootscreen-deferral-placement.json`. No stack, compressed-flash or
performance claim. See the authoritative
[current handover](../handovers/HANDOVER-2026-11-07-fresh-panel.md) for checks,
limits and future-only snapshot follow-up. Final `mise run test:changed` passed
all 13 tasks in 48.5 s (0 cache hits); generated-module and diff checks passed.
The full merge gate was not run.

### Resident DKBS interoperability proof

**[D]** The next RAM-only phase replaces procedural scene drawing with a pure C
resident DKBS decoder plus existing stock blit. Full validation reuses checked
forward playback then rewinds one aligned previous-frame buffer; no heap, card
I/O or public SDK/provider contract. Private valid/corrupt linked data providers
exercise RAW clearing, XOR and holds. Four differential C/Python host cases
passed on one unchanged retry after an initial child-process timeout (transient
cause unproved); header/CRC/bounds/expansion/redzone/no-prefix-output checks and
types/generated/layers pass. A compiler-generated memcpy link failure was fixed
by approved explicit volatile source-header field stores, without libc binding.
Actual adapter placement is 3,260 B (text 1,116 B, data 16 B, BSS 2,116 B, tail 12 B),
valid provider separately 2,184 B (text 18 B, immutable bytes 2,166 B); six-byte hook unchanged. First DN2
valid case passed (55.41 s total/51.969 s baseline): four callbacks/three decoded
records/status 1/validation 1/errors 0, exact final bitmap hash `630531…14a66`,
selected loading/counter 1, default 80 ms SETTINGS/NO full-hash roundtrip and
DSP/input/lifetime. All eight selected cases passed: remaining DN2 held/corrupt/
absent group 3 cases in 153.38 s, DT2 active/held/corrupt/absent 4 in 124.14 s,
both exit 0. Both valid assets show four calls/three records/exact final hash.
CRC failures deny publication with status 2; absent getter makes no C preparation;
both fall back to the stock black last drawing with normal late loading/counter 1.
Held REC denies early work and takes KEY safe mode, skips the module/counter 0.
All eight default 80 ms UI roundtrips, DSP/input/lifetime checks passed, one
baseline each. Missing-receipt cases stayed registered without repetition.
Final cheap preflight passed 14/14 tasks in 48.0 s (7 cached), with generated,
layer, owned-format and diff checks passed; full merge gate not run.
Small evidence index: `/private/tmp/bootscreen-resident-summary.json`. See the
[resident handover](../handovers/HANDOVER-2026-11-07-resident-dkbs.md).

**[V]** Stock blit at DT2 `0x40114168` / DN2 `0x40115adc`: an independent audit
matched all 131 instructions/376 raw bytes on both current images, SHA-256
`9ddd2f673c1ac024d2fb381131327c7646f667392302f99095283c177f988222`.
Source fields are width/height/bands at +4/+8/+0xc, pixels +0x10, mask +0x14;
copy is `dst ^= (src ^ dst) & mask`. There are no header/ownership writes,
outside calls or retained pointers, supporting the synchronous 28-byte stack
view with opaque mask used here. This is static ABI evidence, not hardware proof.

**[D]** Historical `f83518d7` procedural scene proof, superseded by the resident
decoder above: one opt-in core adapter implemented the table substitution, not
a generic player: the accounted early callback publishes a const three-frame
diagnostic table; a hash-resolved scene_setup entry hook substitutes only its
original stack argument, then executes the stock setup/task/completion. Drawing
uses the existing fill_rect and supplied Bitmap without ownership changes.
Independent raw audits establish the LEA/CLR hook window, callback ABI and pixels
pointer at +0x10 (auxiliary bitplane +0x14, ownership byte +0x18), correcting the
old blit declaration documentation.

The native observer captures a completed drawing at the stock callback return,
before stock presentation, replacing the full-pixel coverage heuristic. Existing
legacy tracker snapshot slots remain opaque serialized metadata unchanged.
Read-only framebuffer.get source:intro retains that latest callback bitmap with
the global display revision; no history or hardware display proof. Focused host,
resolver, snapshot, RPC/types and layer checks passed. First exact DN2 active case
passed (79.29 s total/60.550 s baseline): three callbacks in order, guard accounted
once, exact last index-2 bitmap hash
`63053142ed4218204f369f72b315a6294777a0b9180d5a158de4810b28814a66`, healthy selected
module/main/DSP/input/lifetime. Remaining five cases also passed: DN2 missing/held
2 cases in 276.82 s, DT2 active/missing/held 3 in 261.16 s, both exit 0. Both active
drawings match exactly; missing receipt leaves diagnostic inactive and stock
retained drawing available with normal selected loading; held REC leaves it
inactive and takes KEY2 with selected module skipped. All six lifetime reports
pass; one baseline per scope, no retries or budget changes. Evidence index:
`/private/tmp/bootscreen-ram-scene-summary.json`. Capture is completed drawing
before stock presentation, not physical display/flush proof.
Text 252 B, const table 16 B, BSS 20 B; actual placement 288 B object plus 12 B tail
=300 B, separate six-byte in-place site. No stack or performance claim.
Final cheap preflight passed 13/13 tasks in 1 min 41 s (0 cached), with final
generated-module, layer and diff checks passed; full merge gate not run.
See the [RAM scene handover](../handovers/HANDOVER-2026-11-07-ram-scene.md).

**[D]** A subsequent host-only asset prototype packs PNG manifests into provisional
DKBS v1 (RAW or bounded previous-frame XOR-RUN), decodes/validates them and creates
self-contained decoded-frame previews. Existing Pillow and oled_png are reused,
with explicit native/OLED layout conversion. No firmware/runtime/player/transfer
implementation or dependency change. The 64 KiB cap is chosen host policy, not
device staging or blocked-read proof. Original synthetic 180-tick moving dot:
5,715 B/180 records; constant dot: 42 B/one merged record; each represents
184,320 raw tick bytes, not a general compression/performance claim. Named host
tests (49), scoped Ruff and mypy passed. Cheap preflight passed all 13 tasks in
1 min 2 s (8 cached); diff and generated-preview Node syntax checks passed.
Interactive preview was not checked because no browser is available on this host.
See [format/design](../design/bootscreen-assets.md) and
[host handover](../handovers/HANDOVER-2026-11-07-host-assets.md).

**[V]** Subsequent current-state REC follow-up: independent audit matched all
50 key_down function bytes on both current images. Key 19 maps to channel 2,
bit 2 (mask 4); the query reads the current mask without queue/init/persistent
side effects, and its ABI matches the existing binding. **[D]** The runtime
now uses that query instead of scanning queued events, with no stale-event
fallback. This is not evidence that intro consumed the old queue or that a
physical initial scan arrives before main. Direct held/released acceptance
without boot-guard passed first on DN2: safe 1 / KEY 2 / count 1 / loaded 0,
unchanged selected c-demo/counter 0, DSP/UI and lifetime checks passed. Exactly
one baseline reload (59.599 s), 62.95 s total; raw
`/private/tmp/bootscreen-direct-rec-dn2-held.log`, retained JSON directory
`/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-direct-rec-evidence-dn2-oLSg4J/`.
All remaining direct/tightened panel cases also passed: held is KEY/selected
module skipped/counter 0; down/up is NONE/module loaded once/counter 1 on both
devices. Both panel sequences retain sticky early denial (decision 3, requests 0).
This corrects the prior emulator REC limitation, not hardware initial-scan timing
or independent escape from blocked I/O.

**[D]** This follow-up passed all 18 unique affected cases: prior DN2 held 1,
group A refactored absent/four probe cases 10 (426.42 s), group B remaining
direct/panel cases 7 (300.99 s), no retries. Raw group logs:
`/private/tmp/bootscreen-rec-refactored-group-a.log` and
`/private/tmp/bootscreen-rec-panel-group-b.log`. The ten refactored cases each
have exactly one baseline-reload trace, removing ten redundant baseline boots
with unchanged assertions and exact initial cards. The old first-span sum
281.769 s is a potential setup ceiling only, not a measured wall-time saving;
host load varies. Runtime 58 units, types, scoped object build, generated/layer
and format checks passed. Final cheap preflight passed 13/13 tasks in 42.4 s
(7 cached); final generated-module and diff checks passed. Exact commands,
retained evidence paths and remaining limits are in the
[REC follow-up handover](../handovers/HANDOVER-2026-11-07-rec-key-state.md).

### Remaining product and safety acceptance

The genuine pre-intro CPU-fault case above is accepted under working-card
emulator conditions on both devices. It does not accept the following
read/timeout/hang cases or establish physical exception recovery.

| Case | Required proof |
| --- | --- |
| Absent, invalid, corrupt or oversized asset/provider | Stock fallback, bounded validation, completion preserved |
| Power cut during upload | No partial asset becomes active; previous valid selection or stock remains |
| Read failure, timeout or hang | Bounded escape or independent bypass before existing protections; later selected-module behavior after early I/O denial still needs proof. `REC` is cached; counter-write retry units do not establish selection refresh after transient reads. |
| Repeated failed boots; bootscreen-only/no modules | Protection accounts for early work and eventually bypasses it |
| Read-only card | Explicit recovery independent of counter writes |
| Safe-once flag, held key, service/skip boot modes | Earliest behavior and queued-key timing proved; stock semantics preserved |
| Competing providers/hooks | Conflict refused before patching; safe mode leaves relevant service sites stock |
| Normal playback | Frame-boundary capture, timing, allocations and stack proved on DN2, then DT2 |
| Callback/buffer lifetime | Preserve bitmap flags, orientation, ownership and completed-frame publication; no provider cleanup until the callback returns and no stock table/function pointer references its memory |

No row has complete card-loadable bootscreen acceptance yet. Use bounded runs and the owning
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
pre-main-hook proof in those reused suites alone; the separate current
early-exception case supplies the working-card emulator evidence described above.

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


## Opt-in card provider (2026-11-07)

**[D]** A separate `core/boot-card-proof` adapter supplies the private optional
getter/discard pair from `bootscreens/active.dkbs`, without default patch
membership or selection/settings changes. It uses runtime stat, exact-size arena
allocation and the existing API's stage-fit/SHA-checked whole-file copy. Successful
bytes remain immutable until reset; invalid DKBS invokes discard before scene
publication and clears the decoder pointer. No callback-time freeing is allowed:
normal main-task allocation can overlap the intro task. Both current firmware
images link offline with the unchanged hook and no new firmware binding or libc
helper. Provider undefined symbols are only `dkrt_api` and `dkrt_store_stat`.

**[D]** Provider host tests passed 9/9; existing decoder tests passed 4/4. Scoped
object builds passed 7 tasks; regression types, generated metadata and layers
passed. The generic-store SDK owner test also passed with a new same-path
replacement commit-cut case. That case seeds separate complete payload extents,
then cuts metadata writes and checks one complete old/new entry, digest, payload
and table CRC. It does not model interrupted DATA or device flush timing.

**[D]** Offline placement on both devices: provider text 246 B, constant path
24 B, BSS 32 B, placed 304 B; scene text 1,152 B, data 16 B, BSS 2,116 B,
placed 3,284 B plus existing 12 B tail. The six-byte hook is unchanged. A valid
2,166 B file costs 2,192 arena bytes after alignment/header; the file cap costs
65,552 arena bytes. Decoder/mask remain additional retained state.

**[D]** Ten selected card cases passed on current firmware: all seven DN2
cases (valid/missing/corrupt/oversized/held/empty/counter), plus DT2 valid/corrupt/
held. There was one baseline per case, ten total. First DN2 valid: 1 passed/
13 skipped in 54.61 s (49.428 s baseline); remaining DN2 group: 6 passed/
8 skipped in 300.03 s; DT2 group: 3 passed/11 skipped in 90.60 s. Four additional
DT2 cases stay registered, not run. No old RAM matrix or broader reset sweep ran.

**[D]** Both valid cases and DN2 empty selection had attempt/stat/read 1/1/1,
owned bytes 2,166 and discard 0. Their aligned pointers lay outside staging;
all retained bytes matched the fixture after late loading (no module loaded in
the empty-selection case). Four callbacks, last index 3, three records, one
validation, status 1 and zero errors produced final bitmap hash
`63053142ed4218204f369f72b315a6294777a0b9180d5a158de4810b28814a66`.
Normal selection kept c-demo and counter 1; empty selection had count/loaded 0
and counter 1.

**[D]** Missing/oversized cases statted once, made no allocation/read and owned
zero bytes. CRC-corrupt cases read once, discarded once, owned zero bytes and
had invalid status 2 without publication/callbacks. All permitted failure cases
loaded c-demo normally and kept counter 1. Non-held fallback captures matched
stock's black last drawing exactly, not the full stock animation. Held REC and
seeded counter 2 made no provider calls and skipped selected loading. Counter
safe mode retained preboot counter 2 in BootInfo, then cleared the stored counter
to 0 on reaching the UI; held REC also left stored counter 0. All ten passed the
existing 80 ms SETTINGS/NO full-hash roundtrip, IRQ/input, DSP and lifetime
checks with zero write-watch/flush hits and no invalid writes.

Logs: `/private/tmp/bootscreen-card-dn2-valid.log`,
`/private/tmp/bootscreen-card-dn2-remaining.log`,
`/private/tmp/bootscreen-card-dt2.log`. The actual observation/evidence-directory/
baseline index is `/private/tmp/bootscreen-card-summary.json`. Retained boot evidence
is limited to three JSON files and an intro PNG where captured.

**[D]** A further DN2 USB transfer case passed: 1 passed/15 skipped in 50.55 s,
one 45.898 s baseline. Existing `h.usb`/DkSession PUT replaced the active path
with a 40 B valid black asset (the existing corrupt fixture with repaired body
CRC); fresh verifyFile returned true, LIST reported 40 B, DELETE removed the path
and the next LIST omitted it. Before replacement, after PUT/VERIFY/LIST and
after DELETE/LIST, the same pointer, all 2,166 source bytes, provider/scene
counters, final intro hash and selection paths/flags were unchanged. The owned
copy SHA was `5bef464c2a86fc5fbd508000d438b5ae6aa431c6247b146f070535a117f488a6`.
One c-demo remained loaded with zero faults; the existing 80 ms UI/input/DSP
checks passed. CardRig's existing `uploads: true` checks passed with no writes
outside the store/project policy, no invalid low writes and zero write-watch/
flush hits. Its raw default boot-only `lifetime-safety.pass` is false because the
five legitimate upload store sectors are listed as `invalid_store`; that raw
flag is not the upload-policy assertion. No hardware flush claim follows.

An initial transfer setup attempt failed before any USB mutation because the
existing paused transport rejected the running baseline. The explicit pause
fixed this; only the transfer case was retried. There are **11 successful unique
cases/11 successful baselines, 12 attempted baselines** including that failure.
Sixteen cases are registered (the prior fourteen plus transfer on both devices);
five DT2 scopes remain unrun. The transfer adds one bounded `transfer-proof.json`
to its retained evidence. Failed setup log:
`/private/tmp/bootscreen-card-dn2-transfer.log`; successful retry:
`/private/tmp/bootscreen-card-dn2-transfer-retry.log`; both are indexed in the
same summary JSON.

**[O]** No hardware, timing, cancellation, reset/power-cut or physical
presentation claim follows. See the [card contract](../design/bootscreen-assets.md#opt-in-card-provider-contract).


**[D]** Integrated Workbench/card phase: backend `6becb4ba`, reviewed UI
cherry-pick `1a13c0f5` (source `75a24fb8`). One explicit-base cheap gate
(`DIGI_GATE_BASE=38cb06a8 mise run test:changed`) passed 12/12 tasks in 48.5 s,
zero cache hits; final generated-module, layer and diff checks passed. No full
merge gate, hardware, push, PR or merge was performed.
