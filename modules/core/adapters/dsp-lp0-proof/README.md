# Private DN2 LP0 receive and reply-extension diagnostic

Version 0.2.1, explicit opt-in for the emulator, DN2 OS 1.12 only. Requires
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

The extension is eight numeric 16-bit halves, in order:

| Half | Meaning |
| --- | --- |
| 0, 1 | Magic `0x4c50`, version 1 |
| 2, 3 | Status, error |
| 4, 5 | Pages, low half then high half |
| 6, 7 | Bytes, low half then high half |

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
pages or status 3 for terminal page 235 / 960,000 bytes. Invalid requests/errors
produce sticky ERROR=4. There is no teardown, retry, reopen or retirement on timeout;
errors require a fresh boot. The stock sender itself still has an unbounded
readiness wait, so the external runner must bound its owned emulator.

LP0 setup still requires live IMASKP, owner, LP0_CTL and DMA30_CFG zero before
mutation. Private aligned L2 state/descriptor/RX storage, sequential tags 0..234,
the fixed unpublished bank `0x80800000..0x808ea600`, final 1,536-byte clamp, and
both full 4 KiB canaries remain. `_lp_status/error/pages/bytes` are independent
emulator oracles; native test progress comes from the CF wire ACK.

Version 0.2.1 aligns RX independently to 64 bytes within a 4,224-byte reservation.
The 4,100 DMA bytes (tag plus 4,096-byte payload) occupy a rounded 4,160-byte
footprint wholly owned by RX, disjoint from the descriptor and saved context. Driver state remains
128 bytes with 16-byte alignment; descriptor logic is unchanged. This layout is
necessary for whole-line ownership but supplies no cache coherence or maintenance.
The 0.2 native runs retain their historical addresses and evidence only. Version
0.2.1 requires a new native boot fixture bound to the exact new patched-image hash;
no new native execution is claimed.

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
All 47 focused cases pass, including 16 four-byte data-placement phases checking
RX alignment, complete footprint ownership and padding preservation.
Stand-ins test our guards/call frames, halfword decoding, stock-copy boundary,
seqlock and send-once completion behavior. They do not execute the real driver/ISR
or prove cache behavior, gap ownership, DMA bank ownership or hardware safety.
The native first-page, full-bank and held-stock-audio cases retain their independent
bank/hash/canary/terminal checks. The 0.2 first-page and full-bank held-stock-audio
proofs pass with a verified private boot fixture; artifacts and remaining limits
are in finding 88. No public production completion contract follows from this proof.
