# Stereo machine output and channel history

2026-10-30. DT2 OS 1.17 / DN2 OS 1.12. **[D]** source inspection and bounded
original-byte audits; **[C]** corrected interpretation; **[O]** integration open.
See the [sampler plan](../plans/dsp-sdk-and-sampler.md).

## Current SDK boundary

**[D]** Machine contract 1.1 has one 32-float output pointer in `dk_voice_in`.
The DT2 adapter renders the even voice and explicitly copies its output into
the paired odd voice at `_dv_odd` in
`modules/core/adapters/dsp-voice/src/dsp/dsp-voice.s`. DN2's adapter passes one
32-float slot buffer to the module before the stock mono processing chain.
The separate FX contract already has two 32-sample planes; that does not make
machine output stereo. A second field in the SDK input alone is insufficient.

## DN2 channel split: correction

**[C]** The historical description of `0x1c9aea` as 32-to-64 temporal upsampling
is contradicted by current original bytes. This includes the legacy description
in the DN2 adapter's source comment. The code creates **two 32-sample planes**
from one 32-sample mono block, multiplying each input by separate gain streams.
It does not double the temporal sample count.

**[D]** DN2 1.12 SYX SHA-256:
`7e5b8a733d4ab1fdf7e5d22757f7d8eddaa3cbf71d0858874a791844c02ceb35`.
Section 7 is 836,956 bytes, SHA-256
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`.
The auditor freshly extracted these bytes and checked the listed instructions.

Initialization at `0x1c14e1..0x1c1575` builds pointer table `0x258ee8`:
channel A for slot v is `0x257ee8 + 0x100*v`; channel B is A+`0x80`.
The table is installed through `0x1c2848/4b` and read by the engine at
`0x1c9add`. At `0x1c9af5` one input is loaded; `0x1c9af6/aff` multiply that
same sample by two coefficients and `0x1c9af9/0x1c9b0c` store the pair.
The loop repeats for the next input at `0x1c9b1b/21/2d`.

The gain sources include complementary tables `0x8052d790/0x8052d990`, with
endpoints `(1,0)` and `(0,1)` and midpoint `(0.70710683,0.70710683)`.
This supports pan-channel interpretation; physical connector ordering was not
separately traced. Downstream `0x1c1738` and `0x1c1f1e` read paired planes
32 words apart and process 32 sample pairs. Caller order is engine at
`0x1c3044`, then those consumers at `0x1c3054/5d`.

Initialization writes 48000.0 to `0x257e5c` at `0x1c1487` and block count 32
to `0x257e6c`. This establishes configuration and temporal-count preservation,
not a physical clock measurement. There is no 96 kHz conversion in this loop.

## Mono processing before that split

**[D]** Hook `0xb8251b`, called at `0x1c9459`, receives one slot buffer as
both R8 and R12. Buffers start at `0x804acf90`, stride `0x80` bytes. Scalar
gain and later stages process that buffer in place before the split. Two finite
stage audits establish independent audio histories without internal control
advancement:

| Stage / call | State per slot v | Coefficients read | Audio history written |
| --- | --- | --- | --- |
| `0xb80c39 → 0xb809f2`, call `0x1c94fd` | `0x24f320 + 0x70*v` | Words 21,22,23 | Words 24,25 |
| `0xb81368`, conditional call `0x1c959f` | `0x2533e0 + 0x20*v` | Words 2,3,4,5 | Words 6,7 |

Both process the same input/output buffer, with a normally configured count of
32 and sixteen slots. In these finite bodies, other accesses are sample I/O and
stack traffic; there are no global/RNG writes or coefficient/ramp updates.

The first stage is `y = a*x + b*x_previous - c*y_previous`, then updates the
two histories. The second stage, with coefficients `a,d,b,c` in words 2..5, is:

```text
Unew = a*U - d*x
Ynew = c*Unew - c*U + b*Y
```

The second stage loads coefficients once per block. Its separate float products
and subtraction order matter. Under fixed coefficients, its real-arithmetic
transfer function can be expressed as the SDK biquad with
`b0=-c*d, b1=c*d, b2=0, a1=-(a+b), a2=a*b`. That is **not** established as a
bit-identical replacement: a float32 example already differs by one ULP, and
state conversion depends on coefficients. Do not collapse the recurrence or
reuse one channel's history for both channels.

**[O]** These stages can share captured coefficients while retaining independent
left/right histories. This does not establish the same property for the entire
chain or exclude external coefficient changes. Remaining work is an ordered
stage/state inventory, shared control evolution once per block, independent
channel processing, and the existing pan/send path. A late stereo insertion
alone would bypass earlier mono processing and is not the selected solution.

No module ABI change, firmware patch, emulator run or hardware test is claimed
by this finding.

## Finite post-source inventory

**[D]** A further original-byte pass checked 722 aligned instructions in
`0x1c9448..0x1c9b33` and 99 local continuation/dispatch instructions in
`0x1c9b73..0x1c9c5d`. There are ten processing families. The two recurrences above were audited first;
the eight additional footprints are now classified below. Their placements are:

| Callee | State base + byte stride per slot | Placement |
| --- | --- | --- |
| `0x1cdb56` | `0x24fa20 + 0x60*v` | Conditional early or later scaled placement |
| `0xb8265b` | `0x250160 + 0x14*v` | Conditional early or late placement |
| `0xb82342` | `0x2505a0 + 0xcc*v` | Dispatch case 1 |
| `0xb81a16` | `0x251260 + 0x12c*v` | Dispatch case 2 |
| `0xb80f2e` | `0x252520 + 0xec*v` | Dispatch case 3 |
| `0xb806f5` | `0x2535e0 + 0x48*v` | Dispatch cases 4 and 6; existing ring bounds in finding 91 |
| `0xb81526` | `0x253a60 + 0xd4*v` | Dispatch case 5, scratch output then blend |
| `0xb80345` | `0x2547a0 + 0x2c*v` | After selected processing stages |

The seven-entry dispatch table at `0x8052dbc0` has case 0 as skip. Other cases
process the mono buffer in place except case 5, whose scratch destination comes
from `DM(0x254aa0)`; its continuation blends scratch and the original buffer.
Conditional placements use tables at `0x254ab4`, `0x254aa4`, `0x254ac4`.
Their full flag domains are not established, so early/late exclusivity is not
assumed for arbitrary values.

The ordered path is constant gain, conditional `0x1cdb56`, audited first-order
stage, conditional `0xb8265b`, conditional audited U/Y stage, selected dispatch,
late conditional placements, `0xb80345`, gain ramp, conditional fade, metadata
cleanup, then the two-channel split. Most control/coefficient generation is
before the source hook at `0x1c9045..93c6`. Some physically later code rejoins
that earlier control flow; it must not be counted as extra post-source stages.
The inline cleanup mutates shared flags and must not simply run twice.

**[O]** Live routing-table invariants, external state writers and reset/lifetime
coverage remain open. The local classifications below do not authorize duplicating
the whole chain's execution.


## Remaining eight state footprints

**[D]** Original DN2 1.12 section-7 byte audits classify all ten local bodies.
Offsets below are 32-bit words in the passed record; the placement strides above
are bytes. These are finite local read/write audits, not whole-engine ownership.

| Stage | Persistent reads | Persistent writes and interpretation |
| --- | --- | --- |
| `0x1cdb56` | 0,1,2,5,12 | 4,5,6 and 11,12,13: two shared old/current/step control groups; no audio history |
| `0xb8265b` | 0..4 | 1,2 held audio; 3,4 phase counters. Share captured timing, retain independent held samples |
| `0xb82342` | 3,4,5,6,7,12,14,26,28,33,35,40,42 | 3,4 recursive histories; 48,49,50 audio-dependent snapshots. Keep all five channel-local pending external-consumer audit |
| `0xb81a16` | 10,11; 12+7k and 14+7k, k=0..8 | 10,11 histories only; coefficient ramps evolve locally |
| `0xb80f2e` | 0,1,2,3,4,5,9,10,11,12,14,18,25,32,39,46,53 | Audio histories 5,9,10,11,12; shared block preparation writes {17,18,19}+7k, k=0..5 |
| `0xb806f5` | 0,2,4,10,13,14,15,16,17 | Audio history 0 and external ring; control old/current/step 3,4,5; read coordinate 13, write cursor 14, fill status 17 |
| `0xb81526` | 9,10; 11+7k and 13+7k, k=0..5 | 9,10 histories only; coefficient ramps evolve locally |
| `0xb80345` | 0..7,9,10 | 7 phase, 9 counter, 10 gain marker; audio-independent control evolution |

The nonlinear `0x1cdb56` uses captured control ramps and memoryless clipping/
shaping. Replaying it or `0xb80f2e` on already advanced shared state changes
its ramp. For example, `0xb80f79` copies word 4 to prior-value word 46; another
invocation immediately computes zero difference there. Shadow control changes
must survive repeated occurrences within the right-channel replay, then remain
separate from authoritative control state.

`0xb8265b` reads table `0x26e9c8`; its active path is fixed at 32 samples.
`0xb80f2e` reads `0x26b3a8` with adjacent dynamic indices; their full domain is
not audited. `0xb80345` reads tables `0x26a570/0x26a770/0x26a970`; upstream
index bounds remain open. Its unity paths assume in-place input/output. Its
stack arguments are control, byte control, then sample count, unlike a simple
single-count wrapper. No new mutable globals or RNG effects were found in
these bounded bodies apart from the external ring below.

`0xb806f5` word 16 points at `0x804ad790 + 0x8000*v`, established by
`0x1c8c05/0b/0d`. Constructor `0xb80575 → 0xb80546` clears each 32 KiB ring;
reset `0xb805d8` clears history/cursor/status without clearing the backing.
**A copied record retaining that pointer would share audio history.** Ownership
of an independent second-channel ring is not established. Initial coordinate
bounds and DAG/SIMD entry assumptions remain open. Case-5 scratch is
`0x804ace90`, installed through `DM(0x254aa0)` by `0x1c8c02/08`; consume or
retain each result before reusing it, including the continuation blend.

Arithmetic helper `0x1c06ba` has no memory accesses in its actual body. Its
returns at `0x1c06fa` and `0x1c0701` include delayed-return forms; later copy
code printed by the database is outside the helper. Its temporary MODE1
rounding-bit change is restored on the bounded normal path. The truncation
helper `0x1c07cc` likewise restores its temporary mode. These observations do
not cover exceptional floating-point inputs or arbitrary live contexts.

## Bounded shadow replay proof

**[D]** `tests/fixtures/dsp/stereo-shadow-replay.s` exercises a synthetic ordered
subset: `0x1cdb56 → 0xb80c39 → 0xb81368 → 0x1cdb56`. This is deliberately
not identified as a stock default route. Original firmware is loaded only at
runtime, hash-bound to the section above; no firmware bytes are in the fixture.

The assembled preparation copies initial shared controls and coefficients before
left processing, preserving independent right histories. Right control mutations
persist between repeated stages but are never published to the left record.
Three changing-control blocks with unequal channel inputs match two isolated mono
reference executions exactly within the configured numerical model. Explicitly
resetting both histories and controls reproduces block zero. Store checks bound
writes to declared state, audio and stack, with exact stage-write counts.

Six focused tests pass without skips. Negative controls detect copying controls
after left processing, clearing right histories between blocks, and restoring
initial controls between repeated occurrences. The harness also checks original
image preservation, linked-code overlay priority and per-image decode-cache
isolation. Independent source review found no issues.

**[O]** The combined test uses existing `approx_recips=True`: default execution
stops at `0x1c0706` on an unknown AV predicate after an unmodeled ordinary RECIPS
seed. Candidate and references use the same approximation. A separate three-block
recurrence-only test passes with approximation disabled. This establishes model
state-separation equivalence, not hardware-exact reciprocal arithmetic, complete
routing, pan/send integration, natural reset/retrigger/steal coverage, external
writer serialization, throughput or production stereo ABI.

**[D]** A reset candidate is `0xb8135a` (zeros words 6/7). The call at
`0x1c96aa` lies in a physically later pre-hook branch: `0x1c9343 → 0x1c9642`,
then `0x1c970e → 0x1c9346`, before source hook `0x1c9459`. Numerical address
order is not execution order.

An independent original-byte audit resolves the reset argument to
`0x2533e0 + R10`: the delayed call at `0x1c96aa` sets `R4=R5+R10` in its
`0x1c96ad` delay slot, overriding the earlier `R4=M9`. Caller `0x1c303e`
supplies engine base `0x241298`, propagated to I9 by `0x1c8f18/1d`.
`0x1c928b/93/96` computes R5 as that base plus `0x12148`. Complete bounded
callee audits (`0xb813e8`, `0xb8028c`, `0xb81d6d`, `0xb818c7`, `0xb81c8d`,
`0xb811f1`, `0xb82440`) preserve R5/R10 along this path. `0x1c93aa/ae`
increments R10 by `0x20`, matching the processing-record stride. Exact slot
ordinal mapping and the event-level meaning of the SV branch remain unaudited.

**[C]** An intermediate database-only interpretation of engine pointer
`0x257e6c` as this reset argument is contradicted by the delayed argument
setup. An apparent instruction at `0x1c96a9` is actually the final parcel of
`0x1c96a7`; a frame-slot propagation inference also used a value from before
its reload. Neither interpretation may be used to place a reset hook.

This establishes one concrete per-slot reset path, not complete initialization,
retrigger/steal/filter-change coverage or an implemented shadow reset hook.
The audit used original bytes with the project decoder, not a hardware trace.


## Additional filter replay proofs

**[D]** The replay fixture now covers the three isolated leaves `0xb82342`,
`0xb81a16` and `0xb81526` in
`tests/fixtures/dsp/stereo-filter-shadow-replay.s`. Each runs three unequal-channel
blocks with changing captured coefficients and independent nonzero histories.
The candidate matches two isolated mono sessions, keeps left state unchanged by
the right pass, and bounds stores to declared audio, audited mutable fields and
a 65-word stack window. Deliberately replacing right histories with left end-state
must change actual right audio, so silence or a bypass cannot satisfy the proof.
For `0xb82342`, all five audio-dependent fields, including snapshots 48..50, stay
private. These six new cases and the six existing replay cases pass without skips;
independent source review found no issues.

The new leaves use byte-space state/audio/stack pointers with explicit
`assume_nw32=True`, because their raw-byte MODIFY interiors are combined with
normal-word loads/stores. Reciprocal approximation is disabled. This context is
part of the proof, not an unqualified claim of arbitrary-mode equivalence. The
preparation fixture accesses equivalent normal-word aliases, while stock callees
receive byte pointers. No core semantics, public ABI or runtime code changed.

Six of the ten local stage families now have replay experiments. Case-5 scratch
and continuation blending, the remaining four families, complete routes, external
writers, reset integration and actual pan/send output remain outside these tests.
No hardware throughput conclusion follows from numerical leaf tests.


## Shared reset predicate and first-order reset

**[D]** A fresh original-byte audit closes another bounded lifecycle question.
`0x1c9336` loads byte `DM(I2-17)`; LEFTZ at `0x1c9338` and `IF SV` at
`0x1c933b` skip reset for zero. Otherwise `0x1c933e` loads `DM(I14+5)`;
LEFTZ at `0x1c9340` and `IF NOT SV` at `0x1c9343` select the reset sequence
for nonzero. Both inputs must therefore be nonzero. Public Programming Reference
printed p. 19-66 describes unary LEFTZ with SV set for zero input; the rendered
second operand R0 is unused. This is a zero test, not an arithmetic-overflow test.

The same sequence also resets the first-order histories. `0x1c966c/6f/71`
saves `I9+0xe088 = 0x24f320` in frame slot −3. `0x1c9681` loads R14 from
frame slot −18; `0x1c96c7/c9/cb` reloads the base and calls `0xb80b93`
with base+R14. The complete reset body, including return delays, zeros words
24,25,26,27 at `0xb80b97/99/9b/9d`. Frame −18 starts at zero (`0x1c9035`)
and advances by `0x70` at `0x1c9370/7b/86`, stored at `0x1c93a0`.
Complete bounded callees `0xb811f1`, `0xb82440`, `0xb8135a`, `0xb805d8`
with nested `0xb80e48`, and `0xb827d8` preserve R14 between load and reset.

Local loop flow initializes R3/R10 to zero, increments R3, tests `R3>>4`, advances
R10 by `0x20`, and loops while SZ. This supports sixteen physical record ordinals
with U/Y offset `0x20*v` and first-order offset `0x70*v`. It does not establish
user-facing voice identity. Local arithmetic suggests guard byte `0x254ba4+v`
and word `0x2547b4+0x2c*v`; the former depends on correctly decoding the byte
post-modifier at `0x1c911a`, which the listing's suffix omits.

**[O]** Upstream indirect dispatches `0x1c90b0/0x1c91ad` and their complete
register-preservation graphs are not exhausted. Keep full guard-address/ordinal
propagation conditional on that gap. Neither guard is identified as note-on,
retrigger, steal or filter change. Next bounded question: their actual producers.
This is a concrete common reset path, not complete lifecycle coverage.


## Held samples and shared block preparation

**[D]** `tests/fixtures/dsp/stereo-prepared-shadow-replay.s` extends the numerical
replay experiment to `0xb8265b` and `0xb80f2e`. Six new cases plus the previous
twelve pass without skips. Both run unequal channels over three changing-control
blocks; right histories stay independent and the left record matches an isolated
reference after right execution. Negative controls copy already advanced controls
or replace right histories with left state and must change output samples.

Held-sample tests use active table indices 32,48,64 with original finite periods
between 6 and 26 samples. Every block includes both repeated output and refresh;
the disabled copy path cannot pass. The preparation-stage tests retain five audio
histories and copy shared input/control fields before the left pass. All dynamic
table-load PCs are watched over the full address space, so a load outside the
128-entry or 1,024-entry tables fails rather than disappearing from the watch.
Canonical store bounds include only permitted state, audio and stack locations.

The held stage runs without reciprocal approximation. The preparation stage uses
the same explicitly qualified approximation as the earlier nonlinear proof after
confirming the known default-core stop at `0x1c0706`. Byte-space pointers and
32-bit normal-word accesses are explicit, as in the filter-leaf tests. These are
bounded captured-control cases, not complete parameter domains or live routes.
Eight of ten stage families now have replay tests; the ring-backed stage and
final gain stage, plus inline blend/pan/route/lifecycle integration, remain open.

## Reset guard producers: bounded follow-up

**[D]** Setter `0xb8028c` writes state word 5 at `0xb802f9` from stack argument
loaded at `0xb802d2`. Caller `0x1c92a5` pushes R15, loaded at `0x1c9285` from
`DM(I5-1)`; `0xb813e8` preserves R15 through its delayed restore. Visible frame
arithmetic supplies `0x2556dc + 0x234*v`. The frame builder writes that same
word at `0x1c2c14`, after arithmetically shifting the packed word read at
`0x1c2b00` right by 16 and then 8. Thus the guard receives a sign-extended high
byte from word `0x25c5ea + 0x92*v` (byte `0x25c5ed + 0x92*v` in the local
little-endian frame). Its event/parameter name is not established.

Helper `0x1c9c9d`, called at `0x1c31a7` with delayed arguments, writes M13
(normally zero) as a byte to `0x254ba4 + slot` at `0x1c9cc0`. It also writes
M13 to `0x254b94 + slot` and M14 to `0x254bb4 + slot`. The caller invokes it
when a signed-short frame value is nonzero. This is a concrete clear writer;
the writer that makes the guard nonzero remains unidentified. Empty targeted
reference results do not exclude other writers. Upstream indirect-preservation
and event-name caveats above still apply; no lifecycle hook is selected yet.


## Final gain replay

**[D]** Four more cases in `tests/fixtures/dsp/stereo-gain-shadow-replay.s`
exercise `0xb80345`: linear attack followed by exponential progression toward
0.5, and a second record with a 14-sample in-place unity hold followed by release
toward zero. Nine blocks include two uses of the actual entry reinitialization
control. All eleven words are captured before left processing; right output and
control state match an isolated mono reference, and right processing leaves the
left record unchanged. Copying advanced controls after left processing must
change the right output. No audio-history fields are introduced for this stage.

The full reachable body ends at `0xb80545`, beyond the database function boundary.
Its consumed-count calculation divides a byte-pointer difference by four, so the
proof uses byte-space audio pointers, explicit 32-bit normal-word access context
and in-place buffers. Table-load PCs are checked against 128-word table bounds;
state writes are restricted to words 7,9,10, plus audio and bounded stack stores.
The hold path may leave existing in-place audio untouched. Reciprocal approximation
is disabled and the trajectories avoid the phase-5 division helper.

The synthetic record begins with indices 7,1,7,4th-word float 0.5, then 7,1,0,5,
0,0,0. Constructor/setter provenance was separately inspected. Word 9 is explicitly
initialized: the constructor does not initialize it. The alternate record uses
word 6=1 and target word 3=0. The entry control rewrites phase/gain markers; this
is not a complete-record reset or a named note-on/retrigger event claim.

All 22 replay tests pass without skips. Independent source review found no issues;
the primary worktree received byte-identical tested files from the isolated gain
worktree. Nine of ten local stage families now have bounded replay experiments.
The ring-backed stage, complete ordered routing, scratch blend, pan/send integration,
external writer coverage and production lifecycle still remain open.


## Ring backing and source continuation replay

**[D]** The ring fixture `tests/fixtures/dsp/stereo-ring-shadow-replay.s`
executes the actual constructor at `0xb80575` at 48 kHz, checking all 8192
initialized words in each independent 32 KiB backing. Three blocks through
`0xb806f5` wrap the write position from 8190 with integer delay 32. Right audio,
state and backing match an isolated mono reference and leave the left backing
unchanged. Read and store bounds are checked; deliberately sharing the backing
fails the ring-store boundary check and changes left memory. Private record words
are 0,13,14,16,17; shared controls are captured before left execution. Word 17 has
byte-wide fill stores. This case uses the existing qualified reciprocal
approximation, avoiding the known floating endpoint at 8192. It does not establish
all delay values, lifecycle coverage or production allocation.

**[D]** The separate source continuation `0xb8251b` is outside the ten post-source
families. Original-image inspection found no persistent state stores, nested calls
or RNG in its reachable body. Its positive-control branch processes exactly 32
samples and uses companion SIMD registers as scratch. The fixture
`tests/fixtures/dsp/stereo-source-shadow-replay.s` exercises controls 0, 0.25, 0.5,
0.75 and 1, with unequal finite input in [-0.8,0.8], explicit byte/NW32 pointers,
linear address context and MODE1 initially zero. Both outputs match separate mono
references; bypass is exact, positive outputs change and remain finite, record
word 0 stays unchanged, and table/audio/store bounds hold. MODE1 returns to zero;
S0/S5/S9/S14 are treated as scratch, not promised preserved. Reciprocal
approximation is disabled. The positive controls cover depths 8,4,2,1; the depth-31
shift edge and full control domain are not covered.

All 29 integrated cases pass without skips. Independent source review of both
additions found no issues. All ten identified post-source families now have
bounded numerical examples, plus the source continuation. Complete ordered routing,
inline scratch blend, pan/send integration and production lifecycle remain open.


## Selected ordered-route replay

**[D]** `tests/fixtures/dsp/stereo-ordered-shadow-replay.s` composes the source
continuation, first-order filter, case-5 filter into private scratch, original
inline scratch blend, final gain and original output split. The blend runs from
`0x1c9c12` through its delayed final store at `0x1c9c44`, stopping at `0x1c98b9`;
the split runs from `0x1c9aea` and stops at `0x1c9b30`, before the outer-loop endpoint.
The synthetic route chooses unity scalar/later ramps and disables other optional
stages/fade. It does not establish that these routing flags occur together in
stock operation; shared cleanup remains outside the replay.

Four blocks compare all four intermediate buffers and both output planes from
each channel with independent mono references. Controls are captured before left
execution; first-order and case-5 histories remain private. Read/store bounds cover
state, stack, table, scratch and audio accesses. Gain traverses phases 2,2,1,1.
All values remain finite; no reciprocal approximation is enabled. Negative controls
catch shared histories, advanced gain state, scratch overwritten before blending,
and swapped or duplicated terminal planes. The scratch fault first verifies an
otherwise-correct filter result, then detects the numerical blend mismatch.

The proposed terminal pair takes plane A from the left input and plane B from the
right input, without summing cross-contributions or extra normalization. Both
private pairs are checked against mono references first. This is a candidate
balance mapping, not a finalized pan policy or public ABI. Stock DT2 stereo pan
behavior is being checked before adopting it. Send consumers, full engine entry
context, natural reset/retrigger/steal, all slots and production routing remain
outside this bounded proof. Independent source review found no issues.


## DT2 paired-input pan comparison

**[D]** Fresh DT2 1.17 section 7 (321,016 bytes, SHA-256
`0f514a12a2255f5c081e292c47f1f29462003177658da4bbae0a22fd737fffa2`)
shows independent scaling in the original pan fragment `0x1c711a..1c7155`.
Inputs are separate pointer-table entries 2t/2t+1 at `0x24ef2c`; destination
initialization at `0x1c1699/9c` with stride update `0x1c16d7` establishes entry
v at `0x252df8 + 0x80*v`. Each pair therefore has two distinct 32-float buffers.
The first sample is independently loaded/scaled/stored at `0x1c7124/25/2a` and
`0x1c7129/2a/34`; subsequent samples remain separate at `0x1c7140..1c7155`.
There is no cross-channel addition in this inspected fragment.

The 128-entry coefficient tables at `0x8055c440/0x8055c640` have endpoints
(1,0)/(0,1), with index 64 approximately (0.70710683,0.70710683). Each target
also includes its own level squared and factor approximately 0.35479999.
Control preparation clamps/interpolates separately and publishes start/target/
increment state before audio processing. The audio loop reads those streams;
conditional preparation can snap starts and zero increments. Consequently the
candidate DN2 diagonal left-A/right-B mapping is consistent with this stock DT2
boundary, without claiming identical complete processing or parameter policy.

This does not establish UI hard-pan mapping, physical connector identity or full
upstream/downstream routing. A zero coefficient removes its input here rather
than folding it into the opposite output. The existing module adapter's even-to-
odd copy is its current mono behavior, not evidence of native channel folding.

## Remaining local reset helpers and control dependencies

**[D]** Original DN2 1.12 bytes identify the remaining private-history resets.
Offsets in this table are words; record strides are bytes. Full helper bodies,
including return delays, were inspected under the usual SISD/linear/NW32 context.

| Audio family / record | Reset helper | Local engine calls | History behavior |
| --- | --- | --- | --- |
| `0xb82342`, `0x2505a0+0xcc*v` | `0xb81d6d..0xb81e1f` | `0x1c964b`, `0x1c9716` | Clears 3/4; preserves snapshots 48/49/50 |
| `0xb81a16`, `0x251260+0x12c*v` | `0xb81c8d..0xb81cc0` | `0x1c9684`, `0x1c9747` | Clears 10/11; also initializes coefficient groups |
| `0xb80f2e`, `0x252520+0xec*v` | `0xb811f1..0xb81278` | `0x1c9692`, `0x1c976f` | Clears 5..12; also initializes coefficient groups |
| `0xb8265b`, `0x250160+0x14*v` | `0xb82440..0xb82453` | `0x1c91c5`, `0x1c96a3` | Clears all words 0..4, including return-delay store at `0xb82450` |

Constructor paths also invoke those helpers: `0x1c8d2b → 0xb81fa4`,
`0x1c8d39 → 0xb81cc0`, `0x1c8d51 → 0xb81278`, and direct initialization at
`0x1c8d07 → 0xb82440`. The latter helper also initializes the separate source
record at `0x1c8d10`; helper address alone cannot identify the held-sample family.
The common reset branch alone also misses the other local calls in the table.
Delayed R4 arguments and the previously qualified upstream indirect preservation
must be respected; these addresses do not establish named lifecycle events.

For `0xb81d6d`, record writes are
{3,4,12,13,14,17,19,20,21,24,26,27,28,31,33,34,35,38,40,41,42,45}.
Only {3,4,14,21,28,35,42} are zeroed; others copy or compute controls.
`0xb81c8d` writes 10/11 and {12,13,14,17}+7k for k=0..8.
`0xb811f1` writes 5..12 and {17,18,19,22}+7k for k=0..5.
A generic full-record clear would not reproduce any of the first three helpers.

**[D]** The immediate case-1 coefficient preparation path
`0x1c9253 → 0xb82096 → 0xb81e1f` does not consume audio histories 3/4 or snapshots
48/49/50. The outer setter has no record loads; the nested setter reads only
13/27/34/41 and updates coefficient ramps. Inspected adjacent constructors and
setters also reveal no snapshot consumer or initializer. This finite result
supports the current local separation; it does not prove globally unused state,
exclude retained pointers/bulk copies, or authorize ignoring external consumers.

## AudioTask context and snapshot-stability gap

**[D]** The immediate caller chain is AudioTask entry `0x1c9fe7`, dispatch
`0x1c9d6b`, command-3 path `0x1c9f0f/0x1c9fbc`, frame builder `0x1c2712`, then
engine `0x1c8ef1` called at `0x1c3044`. The builder copies 672 words at
`0x1c302d..33`; engine preparation begins at `0x1c9045`. Source processing is
inside the sixteen-slot loop at `0x1c944e/59/60/61`.

No explicit IRPTEN clear or IMASK write was found in those inspected builder/
engine bodies; their explicit MODE1 changes concern SIMD only. The SEC wrapper
permits handler execution with interrupts enabled. SPI input DMA and selector
updates, SPORT callbacks and task notification remain concrete asynchronous
avenues. This bounded trace does not establish that they write processing records,
but AudioTask naming alone cannot prove serialization. Callee masks, scheduler
priorities, indirect writers, DMA and NMI remain outside the local proof.

Capture-before-left remains necessary. A short interrupt mask would only address
identified maskable CPU writers, not establish whole-route DMA/NMI stability.
The production snapshot/commit boundary is therefore still unresolved; numerical
replays with deliberately stable controls do not close that gap.

## Actual-helper reset replay

**[D]** `tests/fixtures/dsp/stereo-reset-shadow-replay.s` executes the four reset
helpers above on independent records, then compares reset state and subsequent
rendering with isolated mono references. Nine added cases cover correct mirroring,
a missed reset in each family, and blanket clearing of snapshots 48..50 in the
first family. Every missed reset changes subsequent audio. Blanket clearing is
caught at the reset boundary; the next audio call legitimately overwrites those
snapshots and could otherwise hide the mistake.

Right execution leaves left state unchanged. Record and stack accesses are bounded;
the first helper's 128-entry coefficient-table reads are bounded too. The held
helper is explicitly bound to its held-sample record, and its return-delay store
is included. Reset calls require no reciprocal approximation; only the previously
qualified prepared-stage rendering enables the existing approximation. Independent
source review found no issues. These tests establish finite helper behavior, not
natural event coverage, all asynchronous writers or an installed reset hook.
