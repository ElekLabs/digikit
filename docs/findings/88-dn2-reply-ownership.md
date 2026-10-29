# DN2 reply-channel ownership before runtime sample integration

2026-10-29, DN2 OS 1.12. **[D]** inspected current-image evidence; **[O]** unresolved.
This follows [streaming preparation](87-pcm16-streaming.md) and the
[sampler plan](../plans/dsp-sdk-and-sampler.md). An independent evidence agent
checked the decision-critical paths against original bytes; no device was contacted.

## Occupied row channel

**[D]** Current section 7 is 836,956 bytes, SHA-256
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`,
matching the current port and database. The common-tail anchor at `0x1c9e5f`
matches original bytes. The tail reloads reply I4 from `DM(I6-2)` at `0x1c9e4f`
and writes stock word 0 at `0x1c9e62`. Reply selection at `0x1ca03a..45` is
`0x2c49d0 + (DM(0x2c0450) << 12)`.

**[D]** CF reply copy is `0x800053a4`, with rows starting at `+0x1c`.
The existing ACK at `+0x60..67` occupies row 0 bytes `+0x44..4b`; diagnostics
also use these offsets in later rows. In the mode-6 branch of ISR `0x40025e36`,
the row pointer `0x800053c0` is passed to `0x40003118`. That routine computes
row addresses with stride `0x54` and copies every byte of each row into outgoing
records at `+0x38`. This establishes an actual current-image consumer path for
the ACK/diagnostic bytes. It does not independently establish historical channel
numbers or measured audible consequences.

**[D]** The stock DSP row writer receives reply R12 into I13 at `0x1c2479`,
restores that pointer at `0x1c26a5`, then forms reply `+0x1c` at `0x1c26af`.
The packing loop at `0x1c26b2..ec` writes the rows. Its writes positively exclude
the header. Extending live sample transport must not assume the row area is free.

## Header candidate and counterevidence

**[D]** Original loader block 40 initializes both reply headers to zero at aliases
`0x282c49d0` and `0x282c59d0`; this establishes startup contents only.
A known halfword writer at `0x1c3117` targets header `+0x16`.
Descriptors built at `0x1ca46a` receive length `0xabc` from `0x1ca812`, derive
count `0x55e`, and install both reply addresses at `0x1ca513..552`. CF driver
`0x400cf922` copies `0xabc` bytes from DMA staging `0x80001a20` to the reply copy
before rearming DMA; header `+0x08` is included.

**[D]** Raw CF code outside known database function boundaries at `0x400cfbe0`
reads halfwords `0x80001a28/2a`, combines them into a 32-bit result, and returns.
This is a stock accessor for staging reply `+0x08`. No direct reference or literal
function pointer was found, which establishes neither live use nor dead code.
The neighboring accessor `0x400cfbf4` reads `+0x16` and has an actual caller at
`0x40070412`. Whole-main reference scanning covered 96.61% of bytes; computed
addresses/calls and asynchronous DMA are outside that proof.

**[O]** No exclusive header `+0x08` ownership or safe second ACK word is established.
Remaining edges are writers between construction and DMA consumption and the
reachability of `0x400cfbe0`. Empty references and repeated zero snapshots cannot
prove exclusivity. A compact one-word acknowledgement is only a conditional
candidate, not an approved allocation.

## Probe and implementation boundary

**[D]** Existing `dklog peek` cannot read the relevant area: current DN2 PEEK
ranges in `devices/digitone-ii.toml` exclude `0x80000000` RAM, and
`modules/core/runtime/src/rs/src/upload.rs` enforces them. A possible bounded
observation-only probe would copy staging/reply headers into module-owned,
PEEK-readable RAM, without writing the reply. It has not been implemented or
run; uploading or running probe firmware on hardware requires a separate concrete
proposal and user authorization. Snapshots can detect contrary evidence but
cannot exclude asynchronous writers.

**[O]** Production integration must resolve this channel ownership rather than
inherit the diagnostic ACK layout. The existing spare-frame channel has 12 payload
words on DN2, at most 48 bytes per 32-sample block. At 48 kHz that is a theoretical
72,000 bytes/second ceiling, so 960,000 bytes require at least 13.33 seconds before
acknowledgement, framing or host overhead. This is a capacity bound, not a benchmark.
The full-frame diagnostic is faster in principle but substitutes audio commands
and mutes output in its emulator proof. The user confirmed per-track changes with unrelated audio preserved; no hardware performance claim follows from either route.


**[D]** Follow-up traces the row copies through `0x40003118` to its transfer
submission at `0x40003352..58`: a `0x3f4` descriptor on endpoint index 7,
identified by the existing USB findings as Overbridge EP3 IN. Thus these bytes
reach an outgoing USB stream. Historical DN2 1.11 live-row measurements in
[finding 31](31-dsp-link.md) identify the overlapping bytes as parts of
channels 22..25 and show playback-dependent content. That historical measurement
supports the risk; exact DN2 1.12 host interpretation and audible consequences
remain unmeasured. Copying the bytes does not itself prove how a host uses them,
but the existing link comment calling the region unused is insufficient for live
sample-loading ownership.
