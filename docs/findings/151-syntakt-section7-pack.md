# Syntakt OS 1.42: section 7 stored packed, unpacked by MAIN (the peer thread, the hook site, the RAM)

Date: 2026-11-19. Image: `st-1.42` MAIN (`out/sections/st-1.42/section_3_MAIN_OS.bin`, load 0x40000400) and its bootstrap (section 2, load 0x800003ec).
Static read, a host-verified patch and an emulator proof (section 7). Context: [70](70-syntakt-1-42.md), [136](136-syntakt-peer-cpu.md),
[138](138-syntakt-runtime-facts.md), [142](142-syntakt-sample-storage.md), [146](146-syntakt-os-upgrade-receiver.md).

Marks: **[V]** checked against image bytes by a second agent. **[D]** read once, static. **[O]** open.

## Result

The patched Syntakt image was 32,672 bytes over the 2 MiB OS-upgrade stage limit (`container_max = 0x1FFE00`: container 2,129,312 bytes, limit 2,096,640).
Elektron stores section 7 (the second CPU's program, 383,760 bytes) raw. Stored packed it is 264,194 bytes. With it packed the `emulator` set builds a container
of 2,011,488 bytes, 85,152 bytes under the limit (the image of that date; see [151](152-syntakt-bootstrap-main-stage.md): the bootstrap's in-place unpack of MAIN also needs a margin, which the full `device` set only has with a literal tail, and the container is then 2,072,768 bytes). MAIN streams section 7 to the second CPU byte for byte, so MAIN has to unpack it before the stream starts.
One hook in the peer thread does that. No other code reads id 7. No new RAM is needed.

1. **Reader [V] for the sequence, [D] for the call chain.** The peer thread, entry 0x40082c04, reads id 7 in a loop. Call chain: boot thread `FUN_40082578`
   (call at 0x4008287e) -> `FUN_40082e9c` -> `FUN_40001640` creates the thread with entry 0x40082c04 (`pea 0x40082c04(pc)` at 0x40082ecc; stack 0x4000 bytes).
   The thread looks the id up with `FUN_40116246` (call at 0x40082d4e) and gets the 16-byte directory entry (+4 offset, +8 length). It checks the length, writes
   `0xAA` and the big-endian length to the transmit buffer and reads the payload with `FUN_40116d7a(offset + 0x80000, length, 0x4f84f2c6)` (`jsr (a2)` at
   0x40082da2). There is **no separate stage buffer for id 7**: the NOR bytes go straight into the transmit buffer. The only length check is at 0x40082d66:
   `length + 5 <= 0x60000`, else status 3 (BUFSIZE, "DSP BOOT FAILURE (BUF)") **[V]**. Id not found gives status 1 ("DSP BOOT FAILURE", 0x40082e84).
   Id 6 is read first (0x40082d3a) into 0x4f8af2c9 behind eight 0xFF bytes. It has no length check.
2. **Hook site [V].** The 6-byte `lea.l 0x40116d7a.l,A2` at 0x40082c24 (`45f9 4011 6d7a`). A2 holds the NOR read routine and is used only by the two
   `jsr (a2)` calls at 0x40082d3a (id 6) and 0x40082da2 (id 7) **[D]**. The adapter replaces the 6 bytes with `jmp hook`; the hook loads A2 with its own wrapper
   (the same stack signature) and jumps back. No instruction is displaced.
3. **Other readers [D].** None of id 7. `FUN_40116246` has four call sites: the peer thread (ids 6 and 7), `FUN_400bd74e` (id 1, 0x400bd75e) and `FUN_400bdfba`
   (id 8, 0x400bdfd0). The other users of the NOR read `FUN_40116d7a` read the header (`FUN_401161d0`) and the calibration records (0x380000 and up). The self-test and
   the OS-upgrade receiver do not read id 7.
4. **Timing [D].** The timeout counter starts after the NOR reads and after the DMA is armed (SERQ at 0x40082e5c, callback at 0x40082e76), so the unpack adds no
   timeout risk. The timeout is about 5.1 s (19 calls of 266.7 ms, see section 4). The packed read is 119,566 bytes shorter than the stock read.
5. **FPGA [D].** Section 7 is not involved. MAIN configures the FPGA from section 1 (finding 70, "FPGA"). Section 7 holds a second bitstream that the peer loads
   itself, so the peer must receive the exact stock bytes.
6. **Depacker contract [V].** `FUN_401181ec(src, dst)` (hash 44162d8e3387, the same function as DT2 1.17 0x40138438) skips the 8-byte header that patchkit writes
   (`[u32 BE body length][u32 BE byte sum][body]`) and returns the output length in D0. It has no output bound (**[D]**).
7. **RAM [V].** The id-7 buffer is 0x4f84f2c1 .. 0x4f8af2c1 (0x60000 bytes, uncached alias). The adapter reads the packed stream to `0x4f8af2c1 - length` (the tail of
   the buffer) and unpacks forward to 0x4f84f2c6. The in-place margin of the patchkit stream (264,194 bytes) is **9,444 bytes** [V] (Python
   `tools.firmware.aplibpack.inplace_margin` and the Rust port in `aplib.rs` give the same number). The id-6 buffer starts at the end of the id-7 buffer and was
   filled before, so the stream does not touch it.

Stock section 7: 383,760 bytes, SHA-256 `daf6451cf9587c0b628e901b7bb6b25f4e2633d448c534c0c181dd35ec783bc2` **[V]**. patchkit's packed section 7 unpacks to exactly this.

## 1. The reader, in detail

Thread body 0x40082c04 to 0x40082e9c (no function in the cfdb; `fw disasm` refuses it, so it was read with `python -m tools.coldfire.disasm`).
Entry: `a3 = 0x40001792` (semaphore wait), `a4 = 0x40116246` (directory lookup), `a2 = 0x40116d7a` (NOR read), `d3 = a7 + 0x14` (16-byte entry buffer).

| Address | Step | Mark |
| --- | --- | --- |
| 0x40082c3e | wait on semaphore 0x41bbf438. The stack argument of this call is not popped here; the `lea 0xc(a7)` at 0x40082d0c pops it. | D |
| 0x40082c46..c68 | clear status 0x41bbf448, state 0x41bb32b0 and counters. | D |
| 0x40082ce0 | install vector 68 handler 0x40083034 (EPORT IRQ4) at 0x40000110. | D |
| 0x40082d0a | `FUN_40116246(6, d3)`: id 6. Fail: status 1 at 0x40082e90. | D |
| 0x40082d16..d26 | write 8 bytes 0xFF at 0x4f8af2c1..c8. | D |
| 0x40082d3a | `jsr (a2)`: `FUN_40116d7a(off6 + 0x80000, len6, 0x4f8af2c9)`. No length check. | D |
| 0x40082d48 | store 0x0f at 0x4f8af2ca: byte 1 of the section 6 payload is changed from 0x01 to 0x0f in the buffer. Not part of the hook. | D |
| 0x40082d4e | `FUN_40116246(7, d3)`: id 7. Fail: status 1 at 0x40082e84. | V |
| 0x40082d5e..d70 | `length = hdr+8`; `if (length + 5 > 0x60000) status = 3`. | V |
| 0x40082d84, d9c | `[0x4f84f2c2] = length` (4 bytes, big endian); `[0x4f84f2c1] = 0xAA`. | V |
| 0x40082da2 | `jsr (a2)`: `FUN_40116d7a(off7 + 0x80000, length, 0x4f84f2c6)`. | V |
| 0x40082dd4..e5c | arm eDMA channel 29 (TCD 0xFC0453A0) for id 6: source 0x4f8af2c1, count `((len6 - 4) & ~3) + 0x10` = 1756, INTMAJOR + DREQ. SERQ at 0x40082e5c. | D |
| 0x40082e62 | `FUN_40082bc0`: DSPI2 slave setup (MCR 0x10c00). | D |
| 0x40082e76 | register the poll callback 0x400828cc with mask 0x20 (`FUN_40002de8`). | D |

`FUN_40116d7a(src, len, dst)` is a byte loop on DSPI 0xFC05C000 (a 3-byte address read). It takes the NOR mutex 0x444db2d0.
`FUN_40116246(id, out)` loads the header with `FUN_401161d0` once (checks "ELE3" and 0x4a), walks the entries from 0x80020 and copies the 16-byte entry to `out`.

Stream to the peer: `vector_149_handler` (0x400831d6), run at the end of the id-6 transfer, re-arms channel 29 with source 0x4f84f2c1, NBYTES 1, SOFF 1,
CITER = BITER = 0x6000, CSR 0 (no DREQ). The channel stays enabled, so the stream continues past 0x6000 bytes for as long as the peer clocks bytes out. The peer
reads `0xAA`, the length, then exactly `length` bytes (finding 136) **[D]**. That the limit 0x60000 is `0x6000 * 16` by coincidence is **[O]**.

Buffers (uncached alias; cached = minus 0x08000000):

- id 7: 0x4f84f2c1 (AA), 0x4f84f2c2 (length), 0x4f84f2c6 (payload), limit 0x4f8af2c1 (`0x60000` bytes) **[V]**.
- id 6: 0x4f8af2c1 (eight 0xFF), 0x4f8af2c9 (payload), ends near 0x4f8af99a **[D]**.
- Both lie in cached 0x4784f2c1 to 0x478b0000, below cf.expansion (0x478c0000).

0x4d84f2c0 (cached 0x4584f2c0) is the boot sample upload buffer (`FUN_40082334`, finding 142; getter `FUN_40082a48`). `FUN_40082f90` sends it in 2 KB chunks
through the FlexBus window in state 3, after the handshake. It is live until then and is not the stage buffer of the peer image: that image has none **[D]**.

## 2. Hook site and registers

Site: the 6-byte `lea.l 0x40116d7a.l,A2` at 0x40082c24 **[V]**. The thread has no cfdb function, so the anchor `cf.peer.site.nor_read_lea` (`devices/fw/peer.toml`) is a
`cf_words` rule: the thread's first five words (40 bytes of code up to the first address literal), `offset = 0x20`, `len = 6`. The rule resolves to exactly one place
(`port_resolve`). The other anchors of the adapter: `cf.peer.fn.nor_read` (0x40116d7a, `cf_func`, hash 7def601760b3), `cf.peer.fn.depack` (0x401181ec, `cf_func`,
hash 44162d8e3387) and `cf.peer.site.status_fail` (0x40082e84, `cf_words`, same row, `offset = 0x280`, `len = 2`). All four have mark O until a second check.

The `hook` op (mode `replace`) writes `jmp.l peer_hook` over the 6 bytes. `peer_hook` is `lea peer_read,A2; jmp <site end>`. The registers the hook may change are
A2 (the displaced instruction wrote it anyway) and the flags (the next instruction is a `move.b`) **[D]**.

Wrapper contract at the `jsr (a2)` calls: stack `[ret][src][len][dst]`, the caller pops. At 0x40082da2 the live registers are D2 (id 6 length, used at 0x40082dfe and
0x40082e2e) and D3 **[D]**. `peer_read` copies the three arguments for a C function (`peer_load`), which keeps D2-D7 and A2-A6 as the m68k C ABI says.
The id-6 call uses the same A2 and passes through unchanged: `peer_load` tests `dst == 0x4f84f2c6`.

## 3. Other readers of id 7 [D]

`refscan` over MAIN (95.9% byte coverage, 1,021,683 instructions):

- `FUN_40116246` (0x40116246): 4 hits: `lea` 0x40082c1e, `jsr` 0x40082d4e, 0x400bd75e, 0x400bdfba. Callers by id: 6 and 7 (peer thread), 1 (`FUN_400bd74e`), 8 (`FUN_400bdfba`).
- `FUN_40116d7a` (0x40116d7a): 5 hits: `lea` 0x40082c24 and 0x40116272, `jsr` 0x400be00e, 0x401161e8, and the thunk 0x40117a6e (`bra.w`). The thunk's callers read NOR records.
- The literal 0x80020 (first directory entry) occurs once, at 0x40116264 inside `FUN_40116246`.
- The OS-upgrade receiver checks checksums over the RAM stage only (finding 146). It does not parse sections. The bootstrap loads section 3 only. A section 7 stored
  packed changes nothing for them.
- The DT2 checks of the section 7 stored and depacked size belong to the DT2 DSP loader. They have no counterpart in the Syntakt thread.
- The packed-section pattern already exists on the Syntakt for ids 1 and 8: read into a heap block, unpack, free.

Not covered: a read of the NOR directory by a computed id. All four call sites pass an immediate id **[O]**.

## 4. Timing

- The counter `0x41bbf3bc` is advanced by the poll callback `FUN_400828cc` only in status 0. Above 0x12 it sets status 2. The callback is registered at 0x40082e76,
  after the NOR reads and after SERQ **[D]**. The second argument of `FUN_40002de8` is a tick mask: mask 0x20 means every 32 base ticks.
- Base tick: PIT2 (PCSR 0x0636, PMR 0x4323, the values of finding 07); with the corrected prescaler the tick is 8.33 ms, so 19 calls of 32 ticks are **5.07 s**
  **[O]** on a Syntakt unit.
- The unpack and the hash run before the DMA is armed. The peer is not started by MAIN until then, so they use no timeout time **[D]**.
- Cost of the depacker (estimate from the loop of 0x401181ec): 14 to 20 instructions per output byte, 5.4 to 7.7 million instructions for 383,760 bytes; every
  output byte is a store to the uncached alias, so the budget is 0.15 to 0.3 s.
- Cost of the SHA-256 in `core/peer-image`: a rolled loop, about 40 cycles per byte for 383,760 bytes is 15 million cycles, about 60 ms at 264 MHz, plus about
  15 ms for the uncached reads. The byte sum of the packed stream is about 3 ms. Total added boot time: about 0.25 to 0.4 s (estimate **[O]**).
- The stock read of 383,760 bytes is a byte loop on DSPI with a poll per byte. Its time is not measured **[O]**; the packed read is 31% shorter, so the unpack and
  hash are partly paid for.
- Whether the main task needs status 6 before it uses the peer is **[O]**: the stock path already waits for the NOR reads, so a delay of 0.3 s before the
  doorbell is of the same kind.

## 5. Depacker contract

- patchkit `pack_section` (`packages/patchkit/src/aplib.rs`): `[u32 body length BE][u32 byte sum of body BE][body]` **[V]**. The Python packer
  (`tools.firmware.aplibpack.pack_section`) gives the same stored size for section 7: 264,194 bytes (8 header bytes included).
- `FUN_401181ec` (size 356; prologue `lea -0x14(sp)`, `movem.l d2-d5/a2`): `addq.l #8,A0` skips the header (0x40118202). It ends on the token with `D3 == 0x2ff`
  (0x40118286) and returns `A1 - dst` in D0 (0x40118342). Scratch registers: D0, D1, A0, A1. It saves D2 to D5 and A2 **[D]**.
- No output bound: the loop writes with `move.b (A2)+,(A1)+` and has no limit register. The unpacked size is not in the header. So the wrapper bounds the stream
  before it unpacks: the header length (`stream length + 8 == directory length`) and the byte sum of the body (264 KB add loop). A stream built by patchkit passes
  both; a corrupt NOR read fails the sum before the unpack writes anything **[D]**.

## 6. What was built

- **Device data.** `devices/syntakt.toml` `[container] repack = ["cf2"]`: the second CPU's section is read-only in content and may be stored packed.
- **patchkit.** A module declares `[[repack]]` (role, `dest`, `stage_end`, stock `len` and `sha256`) in its `module.toml`. patchkit stores the section packed only when a
  module of the set claims it, and refuses: a claim on a section the device does not list, two claims on one role, a stock section that differs from the declared
  length and hash, a stream that does not fit the buffer, and an in-place margin of 0 or less. `verify` accepts a repacked section only when its decoded bytes equal
  the stock bytes. Details: [PATCHING.md](../PATCHING.md) "Repacking a read-only section".
- **Adapter `core/peer-image`** (`modules/core/adapters/peer-image`). The hook of section 2, the entry stub `peer_read` and the C function `peer_load`: id 6
  (`dst != 0x4f84f2c6`) goes to the stock NOR read; id 7 is read to `0x4f8af2c1 - length`, checked (header length, byte sum), unpacked by the firmware's depacker,
  checked (output length 383,760, SHA-256 of the stock section) and the unpacked length is stored big endian at 0x4f84f2c2. Any failed check pops the return
  address and the three arguments (`lea 0x10(sp),sp`) and jumps to 0x40082e84, the path of a missing section: status 1, the DMA is never armed, the second CPU gets
  no bytes and MAIN shows its boot failure screen ("OOPS! SOMETHING WENT WRONG, SWITCH OFF/ON TO BOOT AGAIN." in the emulator). The SHA-256 is carried in the adapter (rolled, 16-word schedule, 1.2 KB of code and tables); the stock MAIN has none
  (finding 146) and the runtime's `sha256` is not reachable from an adapter under the layer rules.

- **Hardware sets.** `devices/syntakt.toml` `[patch_sets]`: `device` = the `emulator` modules (the runtime and every core module with a Syntakt port, container
  2,011,472 bytes; the workbench Firmware page flashes it, as on the other devices) and `device-peer` = `core/peer-image` alone (1,882,784 bytes, no runtime: the
  adapter does not require `core/runtime`), an optional first flash from the command line. Both pass `patchkit preflight`. After a flash whose image has no runtime
  the workbench and `patchkit flash` confirm the restart with the stock identity (Ping) and skip the service refresh and the default modules. How to flash:
  [PATCHING.md](../PATCHING.md) "Flashing a Syntakt".

## 7. Emulator proof [D]

Three headless boots with `panel_drive` (`packages/emulator/boot/examples/panel_drive.rs`, the device's default second-CPU model `peer = "coldfire"`, which runs
section 7 on a second core), each `--after 800M --max 1500M --steps peek:0x41bbf448:4,peek:0x41bb32b0:4,peer,frame:NAME`, release build, 5 to 19 s each:

| Image | Main UI at | Status / state | Images the second CPU took | Second CPU | Screen |
| --- | --- | --- | --- | --- | --- |
| `emulator` set with `core/peer-image`, section 7 packed (container 2,011,488 bytes) | 867 M | 6 / 6 | 1,748 bytes (section 6 with byte 1 = 0x0f); 383,760 bytes, SHA-256 `daf6451c…ec783bc2` (stock section 7) | runs, 0 faults | main screen |
| stock 1.42 (control) | 866 M | 6 / 6 | the same two images and hashes | runs, 0 faults | the same main screen |
| the same patched image with section 7 replaced by a valid packed stream of section 7 with one byte changed (header length, byte sum and unpacked length correct; in-place margin +9,444) | 866 M | 1 / 0 | none (stream bytes 0, no doorbell edge, no IRQ4) | never started | "OOPS! SOMETHING WENT WRONG, SWITCH OFF/ON TO BOOT AGAIN." |

So the hook runs in the peer thread before the stream (the cf.expansion code is installed by then), the second CPU gets exactly the stock bytes, and a wrong
result is refused by the SHA-256 check with no byte sent: not even section 6 goes out, because the DMA is armed after the id-7 read.

## Open

- **Limit of the pre-unpack guard.** The wrapper checks the stream's header length and a 32-bit additive byte sum before it unpacks. A byte sum is kept by byte
  reorderings, so a corrupt NOR read could pass it, and the depacker has no output bound: it could then write past the output buffer before the SHA-256 refuses the
  result. A stream built by patchkit is safe: `patchkit` checks its header, byte sum and in-place margin at build time and in `verify`. The stock MAIN streams section 7
  to the second CPU with no integrity check at all, so the adapter is stricter than stock on the result, not on the read. No code change planned.

- **[O]** The real time of the stock NOR read, the unpack and the SHA-256 on hardware (estimates in section 4). The emulator run of section 7 shows no timeout.
- **[O]** Syntakt PIT2 values were read from the code. The tick of 8.33 ms depends on the prescaler reading of finding 07.
- **[O]** Whether the main task needs status 6 before another consumer runs: not read in full (consumers at 0x4000a81e, 0x4000af72, 0x4000b36a, 0x40081ef8).

- **[O]** Whether stage 1 of the peer times out on a missing stream. The design never streams a corrupt one.
- **[O]** Whether a Syntakt flashed with this image keeps its recovery: the OS-upgrade receiver and the bootstrap do not parse sections, so a packed section 7 changes
  nothing for them **[D]**, but no hardware run exists.
