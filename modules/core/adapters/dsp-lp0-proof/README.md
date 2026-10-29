# Private DN2 LP0 receive and reply-extension diagnostic

Version 0.3.0, explicit opt-in for the emulator, DN2 OS 1.12 only. Requires
`core/expansion`; conflicts with `core/dsp-link`, `core/runtime` and
`core/dsp-ddr-proof`. Absent from device module sets. Never use on hardware.
The earlier 0.1 memory-observed proof is retained in its committed checkpoints.

Both wire counts change **at boot only**, `0xabc` to `0xacc` (16 extra bytes).
The CF boot getter call is retargeted; the original getter is unchanged. DSP
initialization receives the new length before constructing all four SPI2
ping-pong descriptors (`XCNT=0x566`, stride two). No armed descriptor is resized.
Numerical buffer capacities admit this experiment; exclusive gap ownership and
hardware safety remain unproved. See [reply ownership](../../../../docs/findings/88-dn2-reply-ownership.md).

The stock CF memcpy still copies exactly `0xabc` bytes. Its destination ends
exactly where stock TX starts, so extending that copy would corrupt stock data.
A wrapper at that memcpy call preserves its arguments and return value, then
reads the separate extension from source `+0xabc` into module-owned storage
before the frame driver rearms DMA. No header, row or trailer bytes are reused.

The extension remains eight numeric 16-bit halves. For LP progress, in order:

| Half | Meaning |
| --- | --- |
| 0, 1 | Magic `0x4c50`, version 1 |
| 2, 3 | Status, error |
| 4, 5 | Pages, low half then high half |
| 6, 7 | Bytes, low half then high half |

Version 0.3 multiplexes the last reply kind into those same 16 bytes. CLOSE
replies are four numeric words `{0x00014241 | status<<24, id, track, epoch}`,
with status PENDING=1, DRAINED=2, REJECTED=3. Each word travels low numeric half
first, so header halves are `0x4241`, `0x0100*status+1`. Global driver ERROR
always overrides CLOSE and publishes the LP progress error kind. CF decodes
progress and CLOSE into **separate** sequence-protected snapshots; observing one
kind retains the other. Unknown/malformed matching CLOSE replies fail closed.

The DSP stores four words using the existing low-half-first wire convention.
It captures status/error/pages/bytes under the SDK local interrupt mask, clears
SIMD for scalar stores, and publishes at the common tail after setup/rearm work
has returned. Exact MODE1 and saved registers/DAG state are restored, bases
before indices. This CPU critical section does not establish general DMA ownership.

A CF ISR seqlock protects the local decoded snapshot `lp_ack`: sequence, valid,
status, error, pages, bytes (six u32 words). Identical observations retain the
sequence; it is a private local coherence counter, not a wire generation API.
Wrong/absent magic remains pending. Valid magic with unsupported version,
inconsistent progress or DSP error fails closed. Malformed/error observations
remain sticky even if a later reply looks valid. Old valid progress stays pending.

`lp_mail` remains four u32 words: state, error, tag, sent count. The host writes
all 1,024 numeric BE staging words and the tag, then publishes START=1 last.
Before sending, the UI requires a coherent CF ACK with status 2, error 0 and
pages/bytes exactly matching the current cursor. A send-once flag is set before
the stock sender. BUSY=2 retains staging through sender return and subsequent
ACK polling: DONE=3 now means **DSP acknowledged consumption**, not merely sender
return. Post-ACK must match the next page and byte count, status 2 for intermediate
pages or status 3 for terminal page 235 / 960,000 bytes. Version 0.3 rearms after
that terminal payload too, retaining status 3 on success, to receive CLOSE. This
intentionally supersedes 0.2 terminal-inactive behavior; old runs retain their
historical meaning. Invalid requests/errors
produce sticky ERROR=4. There is no teardown, retry, reopen or retirement on timeout;
errors require a fresh boot. The stock sender itself still has an unbounded
readiness wait, so the external runner must bound its owned emulator.

LP0 setup still requires live IMASKP, owner, LP0_CTL and DMA30_CFG zero before
mutation. Private aligned L2 state/descriptor/RX storage, sequential tags 0..234,
the fixed unpublished bank `0x80800000..0x808ea600`, final 1,536-byte clamp, and
both full 4 KiB canaries remain. `_lp_status/error/pages/bytes` are independent
emulator oracles; native test progress comes from the CF wire ACK.

Version 0.2.1 introduced independent RX alignment to 64 bytes within a
4,224-byte reservation, retained by 0.3.
The 4,100 DMA bytes (tag plus 4,096-byte payload) occupy a rounded 4,160-byte
footprint wholly owned by RX, disjoint from the descriptor and saved context. Driver state remains
128 bytes with 16-byte alignment; descriptor logic is unchanged. This layout is
necessary for whole-line ownership but supplies no cache coherence or maintenance.
The 0.2 native runs retain their historical addresses and evidence only. Version
0.3 requires a new native boot fixture bound to the exact new patched-image hash;
no new native execution is claimed.

## Private CLOSE command and diagnostic drain controls

LP0 tag `0x01014243` carries a fixed 1,024-word payload beginning
`{request_id, track, expected_epoch}`. CF uses a separate zero-initialized
staging buffer; unused padding has no DSP semantics. CLOSE never copies sample
payload or advances pages/bytes. IDs/epochs are positive signed 31-bit values;
tracks are 0/1. Lower IDs, conflicting identical IDs and higher IDs while pending
reject without replacing accepted history or changing controls. An exact retry
rechecks the live epoch and drain; a new accepted ID clears the prior drained
grant. No wrap is permitted.

Two private six-word diagnostic records expose open, ready, closed, count, fault
and epoch (`_lp_close_{a,b}_FIELD`). They initialize at epoch/open/ready 1 and
closed/count/fault 0. **No audio reader is connected to them.** SDK close/drain
macros close admission and check exact count/fault. The callback rearms before
its result becomes eligible for tail publication; rearm failure is global ERROR.
The AudioTask tail checks a pending accepted request once per frame under its
local mask, after scalar normalization and with full hook state restored on
exit. It never spins, calls a driver or resends. Full incoming MODE1/bank selectors
are retained; scalar status tests occur after disabling SIMD. Callback entry
uses the existing scalar stock calling convention and preserves its MODE1,
callee registers and stack while masking the SDK control operations.

`lp_close_mail` has six u32 words `{state,error,id,track,epoch,sent}`. Publish
START last and retain all request fields until DONE/ERROR. CF captures an
immutable tuple; coherent LP readiness status 2 or 3 permits one send. Sender
return and LP progress cannot complete CLOSE. Exact PENDING stays BUSY; only
exact DRAINED completes. Wrong ID/track/epoch is ignored. Matching malformed,
REJECTED/unknown status and a budget of 65,536 UI polls cause sticky quarantine;
late ACKs cannot recover it or authorize buffer reuse. ISR-visible active identity
is volatile and published last. A matching bad observation is latched in the ISR,
so a later valid frame cannot erase it before UI polling. On exact DRAINED, CF
first retires the active tuple (terminal claim), then checks sticky evidence before
publishing DONE. Subsequent tuple replies cannot change that terminal outcome.
This poll budget is not
a wall-clock bound; the stock sender still requires the external deadline.
CLOSE quarantine/global errors fail both mailboxes and retain buffers until boot.

One global sender owner serializes both paths. In-flight data finishes first;
otherwise queued CLOSE has priority over queued data. No data begins while CLOSE
is active. Separate bounded one-attempt CF seqlock reads and the IRQ-masked DSP
snapshot cover this diagnostic's publication; READY-last alone is not a coherent
multiword message protocol. No public ABI, physical cache or reader-retirement
integration follows from these diagnostic controls. The separate 22-case logical
fixture supplies actual PCM-reader interleavings, independently of this transport.

## Required private boot fixture

The stock `cold-idle.dsp` already contains four armed/queued `0x55e` descriptors.
Overlaying boot instructions onto it cannot rerun their initialization. A private
pre-init-derived cold-idle is mandatory; live resizing is forbidden.

After building/applying the exact current diagnostic to an ignored `reply.syx`,
prepare a private DSP patch with the existing tool:

```sh
uv run python -m tools.patchkit.pk_dsp_diff firmware/Digitone_II_OS1.12.syx PRIVATE/reply.syx PRIVATE/reply-dsp.patch
```

Run the existing `dsp_cold` executable under the shared build/measurement lock
and an external **120-second wall timeout**, with:

```text
DSP_DEVICE=dn2 DSP_PATCH=ABS/PRIVATE/reply-dsp.patch SHARC_FAST_REGIONS=""
dsp_cold ABS/STOCK/digi-audio-dn2-image.bin ABS/STOCK/cold.state ABS/PRIVATE/cold-idle.dsp 800000000
```

This uses the original pre-init `cold.state` (PC `0x1c12e2`, empty overlay), so the
patch is applied before execution. Reuse the verified existing generated core;
stop if a tool requests core generation. Exit code zero is insufficient: the tool
also writes a state after halt or limit. Require its actual `idle:` log marker,
matching exported PC in `[0xb88a49,0xb88abb]`, and inspect the new state **before
any CF exchange** for all four fully known XCNT values `0x566`:

| Direction | Descriptor root, host byte address | XCNT field, host byte address |
| --- | --- | --- |
| RX0 | `0x282c2960` | `0x282c296c` |
| RX1 | `0x282c297c` | `0x282c2988` |
| TX0 | `0x282c2998` | `0x282c29a4` |
| TX1 | `0x282c29b4` | `0x282c29c0` |

Clone the stock ready inputs into a private directory. Retain the original
CF-ready snapshot, ready DSP snapshot, packed DSP image and stock `SOURCE.json`.
Replace **only** the clone's `cold-idle.dsp` with the pre-init-derived result.
The packed DSP image must be a physical byte-identical copy inside the private
directory, not a symlink back to stock inputs: the daemon canonicalizes that image
path and finds `cold-idle.dsp` beside it. Other unchanged inputs may remain links.
Initial Session stock restore still uses the original ready pair; patched cold
startup uses the private cold-idle and matched patched CF boot length.

Set both `DIGI_PK_READY_DN2_112=ABS/PRIVATE` and
`DIGI_LP0_BOOT_FIXTURE=ABS/provenance.json`. The read-only schema is defined by
`BootFixture` in the [native test](../../../../tests/regression/runtime/sample-lp0-transfer.test.ts):

- `schema: 1`, absolute `readyDir`, `baseSha256`, exact `patchedSyxSha256`.
- `{path, sha256}` for `dspPatch`, `packedDspImage`, `originalReadyDspSnapshot`,
  `dspColdBinary`; all paths absolute and files present.
- `coldState: {path, sha256, pc: 0x1c12e2, overlayBytes: 0}`.
- `coldIdle: {path, sha256, pc}` naming `readyDir/cold-idle.dsp`.
- `preparation: {command: string[], environment: {DSP_PATCH, DSP_DEVICE: "dn2"},
  maxInstructions, wallTimeoutSeconds, idleReached: true, log: {path, sha256}}`.
  Command records the executable, image, input/output states and explicit limit.
- `preExchange.descriptors`: four `{rootHostByteAddress, xcntHostByteAddress,
  xcnt: 0x566, knownMask: 0xffffffff}` observations at the table above.

JSON numbers use decimal notation. Record actual commands, hashes and observations;
do not populate metadata from expected constants. The test hashes every input,
checks idle evidence/count audit, preserves the original ready snapshot identity,
and binds the exact new patch output SHA. It never builds or prepares this fixture.
When native opt-ins are enabled without it, collection fails clearly.

## Validation limits

Focused assembled checks: `uv run python -m pytest tests/patchkit/test_dsp_lp0_proof.py -q`.
All 73 focused cases pass, including 16 four-byte data-placement phases checking
RX alignment, complete footprint ownership and padding preservation.
Stand-ins test our guards/call frames, halfword decoding, stock-copy boundary,
seqlock and send-once completion behavior. They do not execute the real driver/ISR
or prove cache behavior, gap ownership, DMA bank ownership or hardware safety.
The native first-page, full-bank and held-stock-audio cases retain independent
bank/hash/canary checks and now require armed terminal receive under 0.3.
The additional opt-in `DIGI_LP0_CLOSE=1` selects the test named
`closes diagnostic reader control over real LP0 and drains without resend with held stock audio`.
It uses the same exact-hash boot-fixture validation and existing Session harness,
transfers the full bank, injects diagnostic A count=1 before CLOSE, observes
BUSY/PENDING/A closed/B unchanged, then clears count and waits for exact DRAINED
without resend. Bank, canaries and finite/non-silent held stock audio are checked
afterward. This is explicit diagnostic fault injection, not a paused native voice.
Do not reuse a 0.2/0.2.1 boot fixture. No 0.3 native run has been performed.
The historical 0.2 first-page and full-bank held-stock-audio
proofs pass with a verified private boot fixture; artifacts and remaining limits
are in finding 88. No public production completion contract follows from this proof.

Current 0.3 owner checks pass: focused Python tests and Ruff; scoped TypeScript
format/lint/types; generation, current-object and document path checks. Module-owned
CF/DSP objects re-decode successfully (450/727 instructions), built with the existing
patchkit under the shared build lock. No emulator/core build or native run occurred.
For a shared prebuilt CF test runner, explicitly set `DIGI_LP0_CF_CALL`; otherwise
the tests use `pk_cfcall.EXAMPLE` in the checkout and never implicitly build it.
