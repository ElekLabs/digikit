# DN2 DDR geometry and reservation boundary

2026-10-29, DN2 OS 1.12. **[D]** inspected evidence; **[O]** unresolved ownership.
An independent evidence agent checked original boot-stage bytes and public
[SHARC references](../sharc/SOURCES.md), for the [sampler plan](../plans/dsp-sdk-and-sampler.md).

**[D]** Boot INIT entry `0x120230` calls `0x120510`, then `0x120353`.
At `0x120520`, the initialization table is `0x242c4c`; the store at `0x120362`
writes its first word, `0x0af00622`, to DMC0_CFG (`0x31070040`). The table was
read from the original initialization-stage block before final loading overwrites
that memory. Using final loaded memory here would lose the configuration evidence.

**[D]** ADSP-2156x Hardware Reference Table 9-6 (printed pp. 9-17–9-18) decodes
low fields `0x622` as one external bank, 4-Gbit SDRAM, 16-bit device/interface:
512 MiB configured geometry. This is firmware configuration, not a hardware
capacity measurement or an allocation of that capacity to samples.

**[D]** Boot blocks 84–93 end their DDR loaded/fill extent at `0x8052fbe0`.
Stock code at `0x1c4664` uses that same address as a scratch base; the SIMD
clear loop at `0x1c4673..76` and subsequent normalization helper `0x1c45de`
read/write beyond the loaded extent. Therefore even the end of the loaded image
does not establish the start of free DDR.

**[D]** Audited allocator initialization `0xb89c53` constructs a free-list arena
`[0x2d08b8,0x2dd0b0)`, 51,192 bytes in L1 normal-address space, with metadata
at `0x2dd0b8..d8`. Its `0x80000000` constant is an allocation flag, not a DDR
base. This allocator cannot be used as evidence for a larger sample pool.

**[O]** The five-root pass establishes no general DDR allocator, maximum dynamic
scratch extent or production reservation hook. The existing emulator bank and a
larger high-DDR candidate still need a lifetime/ownership account. The next
question is runtime pointer provenance and bounds, rather than aperture size.
No DT2 project-pool or waveform-workspace addresses were transplanted to DN2.

**[D]** Original DN2 1.12 section-7 SHA-256:
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`.
No emulator run, hardware contact, binary changes or firmware bytes are part of
this finding.


## Bounded runtime-pointer follow-up

**[D]** Original-byte inspection bounds the audited scratch loops to
`[0x8052fbe0,0x80530be4)`, 4,100 bytes: 512 SIMD pair stores at eight-byte stride,
then one word. Reusers at `0x1c4745`, `0x1c4875` and `0x1c4999` share the window;
normalizer `0x1c45de` operates on its first 4,096 bytes. The recorded startup
call path is `0x1ca76b → 0x1c1445 → 0x1c8e1c → 0x1c8e67 → 0x1c463b`.
Missing direct audio-task edges do not exclude indirect use.

**[D]** In-memory graph analysis of current database edges and store sites found
3,229 stores in the audio-task direct graph, 729 resolved addresses, and 1,136
unresolved stores after excluding obvious stack accesses. Those unresolved sites
are provenance gaps, not evidence of high-DDR writes. The pointer table can also
retain stale values through floating-point index arithmetic: an apparent high
address at `0xb811a5` resolves on instruction inspection to a table at `0x8052dbe0`
with index clamped to 0..1023. Neither pointer-table absence nor a raw address-shaped
literal is sufficient to reserve a pool.

**[O]** Candidate high DDR `[0x9e000000,0x9f000000)` is within configured geometry,
but no reservation is established. A bounded experiment would need an explicit
owner/extent, checked destinations and stock CPU/DMA access guards; production
ownership still requires the remaining provenance/lifetime account.

## Fixed FX object follow-up

**[D]** A further original-byte audit ties allocation entry `0xb89c9c` to the
known L1 heap: its initialization pointer is `0x2dd0c4`, lazy initialization at
`0xb89de1` calls `0xb89c53`, and task creation calls it at `0xb88853/69`.
This does not establish a reservable DDR heap or prove every allocator was found.

**[D]** Boot passes `0x80000018` to `0xb83dc1`; its valid-rate continuation
`0xb83dfa` clears `0x59114` bytes through `0x1c0bde`, ending exactly at the next
object root `0x8005912c`. Initializer `0xb848cb → 0xb8450e/b84604` establishes
this next object's layout:

| Region | Half-open byte range |
| --- | --- |
| Ring 0 | `0x80059130..0x80259130` |
| Ring 1 | `0x80259130..0x80459130` |
| Work 0 | `0x80459130..0x804591b0` |
| Work 1 | `0x804591b0..0x80459230` |
| Control tail | `0x80459230..0x80459388` |

The next boot object begins at `0x80459388`. The initializer clears both rings
using `0x40000` iterations of two SIMD pair stores (`0xb84571/74`), then 32
SIMD pair stores clear the 256-byte workspace. Reset `0xb8439b` has its highest
explicit control store at `0xb8440a`, ending at that next root. Ring index masks
at `0xb846ce/d6`, `0xb8431b` and `0xb84071/77` are 19-bit masks, bounding
valid-state accesses within the two 2 MiB rings. Render entry is reached through
`0x1c1c4a → 0xb8482c`.

**[O]** Workspace loops consume live frame count at `0x257e6c`; boot initializes
it to 32 at `0x1c14a8`. A universal runtime count bound and the ownership of
caller-provided output pointers remain separate obligations. These object-local
bounds do not establish a free production pool.

**[C]** A pointer-table row suggesting `0xb823b3` writes the frame count retains
stale R12 provenance. Although caller `0x1c3038` passes `0x257e6c` into
`0x1c8ef1`, the relevant dispatch reloads R12 at `0x1c958c` from a runtime
output-pointer table rooted at `0x254a60`. Switch-table entry 1 reaches
`0x1c98ab`, which calls `0xb82342`; its output store `0xb823b3` uses that
reloaded pointer. This is not positive evidence of a frame-count write. The
pointer table is loader-zero and its populated values still need tracing; no
universal no-alias claim follows.
