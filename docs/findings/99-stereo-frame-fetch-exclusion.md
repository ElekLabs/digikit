# Private stereo-frame fetch exclusion experiment

**[D]** A private assembled fixture and 58 named model checks establish a smaller
reader exclusion scope for one pending maskable writer. They leave SDK kernels,
public ABI, current whole-block exclusion tests and production runtime unchanged.
This is not a shipping IRQ policy, a pool reservation or hardware acceptance.

## Reader and ownership boundary

Each of 32 stereo frames uses one masked scope for cached-generation admission,
epoch check, count acquisition, cursor/scalar copies and both channels' current
and clamped-neighbor packed words. The reader releases its count after the last
backing read, restores the exact incoming MODE1, then performs signed PCM16
conversion/interpolation from private copies, stores both channels and advances
the shared cursor once using `DK_SAMPLE_CURSOR_STEP`. Denial zeros every remaining
frame and does not relatch an epoch. Empty/completed/bounds cases read no PCM.

No-writer output and final cursor match the existing stereo kernel bit for bit
for forward, reverse, zero, fractional and extreme signed rates, terminal clamping,
empty/outside/completed states and same-plane sources. Computation order and
float32 model behavior are retained. The fixture uses scalar MODE1 `0x9000` or
incoming-disabled `0x8000`; it does not establish 40-bit arithmetic correctness.

**[O]** Cursor, output, save frame and tap scratch remain owned for the entire
callback, even while the backing count is zero. The test writer only closes
admission and reuses backing; assertions prohibit its stores to callback-private
storage. Backing drain alone does not authorize voice/module storage reuse or
prove callback quiescence. A production lifetime/serialization contract remains
necessary. No successor is published by this experiment.

## Actual DDR aliases and writer footprints

The reader uses NW32 DDR addresses in `[0x10000000,0x18000000)` mapping to byte
addresses `0x80000000+4*(w-0x10000000)`, through the existing addressing helpers.
This is explicitly different from `4*w`. Packed planes begin at physical byte
`0x8422c000` and `0x8422e000`; both descriptor bases may also select the same plane.
Each plane contains 65 PCM16 frames in 33 full words, so its owned footprint
includes the unused terminal high halfword.

The page writer affects exactly `[B+4096*p,B+4096*p+4096)`, with
B=`0x8422b9c8` and unsigned stock page bound p < `0x19200` (hexadecimal).
It is a private SISD 1,024-word fill of that actual footprint, not execution of the
stock page-copy instruction path. Page 0 overlaps only the left plane; page 2
only the distinct right plane. Any overlap with either packed-plane footprint
closes the whole binding before an exact-zero, fault-free drain and first store.
Nonwrapping DDR extents, stock page geometry, alignment and permitted lengths
are checked; rejected operands do not close admission or reach the guard helper.

The guard writer calls original DT2 1.17 helper `0x1c400f`, loaded at runtime from
section 7 with SHA-256
`0f514a12a2255f5c081e292c47f1f29462003177658da4bbae0a22fd737fffa2`.
For R4=start byte offset and R8=length byte offset, it reads ten bytes from
S=B+R4 and writes five two-byte values to S+R8+{0,2,4,6,8}. Source/destination
extents must be valid, aligned and nonoverlapping. The focused helper check
verifies exact writes, five load PCs (the watch layer records their ten individual
read bytes), frame/context preservation including I5, and matching physical aliases.
The guard starts at plane+130, overwriting the unused high half of the terminal
packed word that the clamped reader actually loads; its live low half is unchanged.
No original instruction bytes are embedded in the fixture or test.

**[C]** The preliminary `(SW)` DAG-store source forms were accepted but emitted
four-byte writes by the pinned assembler wrapper. This was an unsupported source
form in that route, not evidence of an emulator semantics bug. The original guard
helper resolves this experiment's exact-width requirement without an assembler
or core change. Byte-store source forms were not probed. A separate source-form
support/failure-reporting follow-up remains open.

## Complete arrivals and exact equivalence classes

For page and terminal-guard traces the test enumerates every instruction boundary,
including pending-before-entry and after-return arrivals: 5,173 and 5,205 boundaries,
respectively. It maps each arrival to its first model-eligible enabled boundary.
Distinct source planes test each one-plane writer; same-plane sources have one
physical footprint. These six configurations account for 31,134 arrivals.

The resulting deliveries form 33 exact classes per configuration, indexed by the
number of fully prefetched stereo frames. This is not sampled coverage: the full
trace checks that every descriptor/backing read is masked with count one and
every enabled boundary has count zero. Between fetches, only callback-owned
copies drive computation/store/cursor work. The writer mutates backing and close
controls only, uses a separate architectural stack and restores the entire saved
architectural context. Moving its delivery within one such interval therefore
commutes with that private work; the next admission denies. A prefetched but not
yet stored frame still finishes from old copies.

Each of the 198 representative executions checks equal old stereo prefixes then
zeros, cursor advancement by exactly that prefix and no torn frame. Resumed
callbacks are watched for any descriptor/backing load, including discarded loads;
stale callbacks after CLOSE read neither. The deliberate split-left/right lock
control tears the first stereo frame on an actual right-page overwrite despite
count zero. Count/fault, wrapping/sign-crossing source or destination, overlapping
source/destination, invalid page/alignment/length and exact nonoverlap boundaries
have focused controls. Rejected writer operands are watched for reads and writes
across DDR and their computed source/destination ranges; a helper-entry breakpoint
also proves no rejected source call occurs.

## Instruction observations and limits

| Reader fixture | Total model instructions | Largest masked span |
| --- | ---: | ---: |
| Per-frame, advancing/page scenario | 5,172 | 80 |
| Per-frame, terminal-clamped/guard scenario | 5,204 | 81 |
| Per-frame, incoming mask disabled | 5,172 | 5,172 |
| Existing whole-block represented-span reference | 3,485 | 3,462 |

These are measured instruction scopes, not cycle, IRQ-latency, speed or hardware
bounds. The bodies differ. Address-generation dependencies may stall on hardware;
the fixture also obeys the repository's four-intervening-instruction return-index
scheduling contract, checked from decoded instructions. The disabled incoming
mask stays disabled for the entire callback, so every pending arrival remains
pending until the caller later enables delivery.

The model exposes IRPTEN directly and preserves its existing bank pipeline.
The interleaving supplies one serialized, pending maskable writer, not physical
interrupt dispatch, DMA/NMI/cache exclusion, multiple producers or asynchronous
publication. Production geometry validation, complete writers, cached ownership,
callback lifetime and backing allocation remain open.

Sources: `tests/fixtures/dsp/sample-stereo-frame-fetch.s` and
`tests/patchkit/test_dsp_sample_stereo_frame_fetch.py`. Exact commands, immutable
tool identity, hashes, metrics and guard events remain under ignored
`out/runs/stereo-frame-fetch/`. The owner check passes 58 tests with no skips;
scoped Ruff, formatting and reference checks accompany this checkpoint.
The staged `mise run test:changed` fast checks also pass; deferred merge-gate
coverage remains separate.
