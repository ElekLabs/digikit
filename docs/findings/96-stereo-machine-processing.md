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

A database-only reset candidate is `0xb8135a` (zeros words 6/7). The call at
`0x1c96aa` lies in a physically later pre-hook branch: `0x1c9343 → 0x1c9642`,
then `0x1c970e → 0x1c9346`, before source hook `0x1c9459`. Numerical address
order is not execution order. The actual passed record and trigger semantics of
that call remain unresolved; it is not yet a proven reset hook for our shadow.
