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
using `0x40000` iterations of two SIMD pair stores (`0xb84571/74`); before that,
32 SIMD pair stores (`0xb84521..5f`) clear the 256-byte workspace. Reset `0xb8439b` has its highest
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

**[D]** The next root `0x80459388` is bound by boot `0x1c168f/95` to
initializer `0xb84f51` and by frame code `0x1c1bd5/e1` to renderer `0xb84d5c`.
It has a four-byte header, two 2,048-byte rings
`[0x8045938c,0x80459b8c)` and `[0x80459b8c,0x8045a38c)`, two 128-byte work
buffers ending at `0x8045a48c`, and checked control accesses ending at
`0x8045a6c4`. The next root is `0x8045a6c8`: the four-byte remainder is not
claimed free. Ring clear count 256 and SIMD stores at `0xb84f39/3c` match the
ring extents; workspace stores at `0xb84e1a..58` clear 256 bytes. Nine-bit masks
at `0xb84ba1/ac/b2/b8` and `0xb84bef/f2` bound initialized, serialized read/write
indices to 0..511. Highest checked reset store `0xb84edf` ends at object offset
`0x133c`. Work-buffer bounds again depend on live frame count ≤32; caller
output-pointer arrays remain a separate obligation.

**[D]** The following data root `0x8045a6c8` is read at `0x1c297d` as
`base + 512 * group + 4 * index`. The signed selector loaded at `0x1c293a`
is clamped to group 0..4 at `0x1c2947/4c`. **[C]** The separate per-track load
at `0x1c2940` was initially described as an unbounded packed word. Raw Type3b
fields `l=1,w=0,x=1` instead establish a sign-extended 16-bit load, advancing
I5 by two bytes. Arithmetic right shift by eight at `0x1c2942` therefore bounds
the index to -128..127 regardless of CF validation. All resulting four-byte
reads lie within `[0x8045a4c8,0x8045b0c8)`. The source is frame halfword offset
`0x34 + 2 * track`, not a word in the `0x92`-stride parameter block.
The raw loop trip count at `0x1c2937` is 16. CF writer `0x40027510` copies
the halfword from `0x8000dd40 + 2 * track` into frame location
`0x80005e94 + 2 * track`; the upstream value producer is not needed for the
signed-load bound and remains unaudited.
**[O]** A nonnegative 0..127 range still needs the producer contract, but is not
needed to exclude arbitrary high-DDR addresses from this particular lookup.
Loader block 89 covers `[0x8045a6c8,0x80467cd0)`, which proves loaded extent,
not a runtime access maximum or free space.

**[C]** Pointer propagation also needs packed-access width handling. Original
instruction `0xb84597` has raw fields `l/w/x=0/0/0`, a byte access. Its `0xd3`
post-modifier advances from object offset `0x400119` to `0x4001ec`. Applying
blanket four-byte scaling would invent an overrun; raw access semantics control
the bound, not the pointer table's estimate.

## Output-pointer table initializer

**[D]** A targeted follow-up resolves the initializer of the previously open
table at `0x254a60`. The parent traced the computed field through current query
listings; an independent agent checked the original DN2 1.12 section-7 bytes
against the SHA above and the public Programming Reference.

Boot `0x1c14a2` passes base `0x241298` to `0x1c8e1c`. Its valid-rate branch at
`0x1c8e42` is delayed: both setup instructions at `0x1c8e45/48` execute before
continuation `0x1c8e67`, setting R11=0 and preserving the base in R5. The loop
calls `0x1c8b6d` at `0x1c8ecc` with that base and track index 0..15. Callee
register preservation and the increment/comparison maintain those values.

The callee forms the table address as `base + 0x1387c - 0xb4`. At `0x1c8bfd`,
the parallel memory operation stores incoming R2 while the compute operation
produces a new shifted R2 for later use. Normal-word premodify addressing scales
the track modifier by four without updating the table base. The resulting mapping is:

```text
table[track] at 0x254a60 + 4*track = 0x804acf90 + 128*track
track = 0..15
```

The table occupies `[0x254a60,0x254aa0)`. Sixteen SIMD pair stores at
`0x1c8c18..36`, with eight-byte advancement, clear 128 bytes per target.
The combined initialized output region is `[0x804acf90,0x804ad790)`, 2,048 bytes.
Public semantics: [SHARC+ Programming Reference](../sharc/SOURCES.md), Rev 1.5,
printed pp. 3-2, 3-22 and 14-12–15. The separate pointer assigned at
`0x1c8c05/0b/0d` is `0x804ad790 + (track << 15)`; this establishes pointer values,
not a 32-KiB usable allocation or every consumer's access bound.

**[C]** Two analysis traps matter here. A function-boundary label at `0x1c8e67`
does not make it independent of the earlier branch's delay-slot setup. Also,
the table has sixteen entries, not eight: `0x254a84` is slot 9, inside it.

**[O]** This closes initializer provenance, not the table's whole lifetime.
The database's resolved store candidate `0x1c9118 → 0x254a84` needs instruction
and caller-state tracing before treating it as either a real table mutation or
stale pointer propagation; packed access width and intervening I2 reloads matter.
Other computed writers and consumer bounds remain open. No production DDR
reservation or free-space conclusion follows from this initialization mapping.
