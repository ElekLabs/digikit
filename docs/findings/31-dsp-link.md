# 31: the DSP code channel (core/dsp-link)

Phase 5 of the digikit runtime (docs/plans/digikit-runtime.md) needs run-time DSP code. Spike S4b (finding 23) found
the unused end of the per-block frame as a channel. This file is the module `core/dsp-link` that uses it: what it
does, the measurements, and what is still open. Marks: **[V]** verified here, **[D]** measured or read once,
**[O]** open, **[C]** corrects an earlier claim. Nothing here has run on a device; the emulator does not model the
SHARC caches or the real SPI timing (see "What the emulator does not show").

## Result **[D]**

Static module, patched in at patch time, all four images (DT2 1.16, 1.17; DN2 1.11, 1.12) from the same sources and
anchor rules. A ColdFire call `dl_send(dst_l2, buf, len)` moves `len` bytes into a reserved 128 KiB area of free DSP L2
while the device plays; after the last chunk the DSP has invalidated its instruction caches and reports done.
`mise run verify` routing and the proof: `uv run python -m tools.patchkit.pk_dsplink proof --device dt2|dn2 [--fw V]`.

| | DT2 1.16 / 1.17 | DN2 1.11 / 1.12 |
|---|---|---|
| tail of the frame | 174 words at ring byte 0x804 | 15 words at ring byte 0xa80 |
| payload per chunk (minus 3 words of header and check) | 171 words, 684 B | 12 words, 48 B |
| 4 KiB, blocks until done | 11 (6 chunks) | 91 (86 chunks) |
| 64 KiB, blocks until done | 102 (96 chunks), **964 KB/s** | 1373 (1366 chunks), **71.6 KB/s** |
| DSP L2 after the transfer vs the source | 4 KiB and 64 KiB byte exact | byte exact |
| DSP cost with no transfer (busy instructions per block, `pk_harness` budget) | +23 | +26 |
| PCM, 3 s note, transfer running vs idle image | bit-identical | bit-identical |
| PCM, hook active vs the same image with the hook site restored by `dsp.patch` | bit-identical | bit-identical |

Throughput is one chunk per block (1500 blocks/s) once the pipe is full; the acknowledgement of a chunk is seen about
four blocks later (2.7 ms): in the 4 KiB DT2 run the last chunk was sent four times for 11 blocks. DN2 is 6 % below
the S4b estimate (84 KB/s) because three of the 15 tail words are header and check. Numbers are the same on 1.16 and
1.17 (DT2) and on 1.11 and 1.12 (DN2): the DSP blobs and the ColdFire call sites are the same, the anchor rules
resolve on all four. The proofs ran in 25 to 60 s each.

## How it works

**ColdFire.** The block-frame sender is the vector 191 interrupt handler (DT2 `0x4002dd0c`, DN2 `0x40025e36` in 1.11
and 1.12), which calls the stock driver with `jsr abs.l` (DT2 `0x4002dd74`, DN2 `0x40025e9e`, 104 bytes into the
handler in all four images) after pushing four arguments: record copy, frame bytes (`0xabc`), data, data bytes (DT2
`0x802`, DN2 `0xa80`). Anchors `dl.cf.frame_call`, `dl.cf.frame_fn` (the driver, by masked hash) and `dl.cf.tx_lit`
(the literal of the driver's `lea <queue>,a0`, read at run time; no firmware address is in the module). The
`retarget` sends the call to `dl_frame_hook` (assembly), which saves d0/d1/a0/a1, calls the C function `dl_tick` with
the same four arguments and `jmp`s to the stock driver with the stack as the handler left it. The driver builds one
queue entry per frame halfword (a command halfword, then the data halfword) for `len/2` entries and only the command
halfword for the rest, so `dl_tick` writes a chunk into the data halves of the entries behind the data: the first tail
entry is `(len/2 + 1) & ~1` (so that the tail starts on a whole DSP word) and the geometry comes from the arguments,
not from constants. API: `int dl_send(u32 dst_l2, const void *buf, u32 len)` (0 started, -1 busy, -2 bad
destination or size) and `int dl_poll(DlStatus *)` (state IDLE 0, BUSY 1, DONE 2, ERROR 3; the struct has seven `u32`);
no struct results. `buf` is read in place until DONE or ERROR. 18 tests in `tests/patchkit/test_dsplink.py`
check the arguments, the chunk format against a Python model, the acknowledgement handling, the 900-block timeout and the
geometry error on the ColdFire core.

**[C]** The chunk header described next was widened in finding 32 (2026-10-17): `idx` 14 bits, a 2-bit window, `off` 17 bits, `xid` 5 bits, LAST bit 25, END bit 26, so that the area can be 512 KiB and the voices registry is a second window. The numbers of this file were measured with the 15-bit offset and 128 KiB area; the 4 KiB DT2 run was repeated with the new format (11 blocks, byte exact, PCM identical).

**Chunk format** (little-endian DSP words; the ColdFire writes the low halfword first): `w0` = magic `0xD15C` | idx << 16;
`w1` = off (words from the start of the area, 15 bits) | n (payload words, 8 bits) << 15 | LAST << 23 | END << 24 |
xid << 25; payload; `check` = s1 xor s2 over all words before it (s1 += w, s2 += s1). A chunk is taken when the magic,
xid and check hold and idx equals the number taken (cum); idx 0 with a new xid starts a transfer. The sender keeps up
to 12 chunks in flight, goes back to cum when the acknowledgement stalls (also if one is lost in the middle), and gives
up after 900 blocks (0.6 s) without progress. After DONE or an error it sends END chunks for six blocks (the DSP then
stops acknowledging) and zeroes the tail magic.

**DSP.** A hook (anchor `dl.dsp.block_tail`, `reloc_hash` offset `0xf4` in the block handler, the same hash on both
devices) replaces the 48-bit store of the ring toggle `DM(<toggle>) = R1`, the second last instruction of the handler's
common tail (DT2 `0x1c75c1`, DN2 `0x1c9e5f`). The stub in free L2 (`_dl_hook`, 143 instructions) reads the receive ring
the handler worked on from `DM(I6 - 3)`, checks magic, xid, idx, the limits (`n <= tail words - 3`, `off + n <= 0x8000`
words of the area; anything else sets status 3 and stops the transfer) and the check, copies the payload into
`_dl_area` (a `.PKBSS` of 128 KiB: reserved, not part of the boot stream), and after the LAST chunk runs `FLUSH CACHE`
and sets `ICAINV` in `SHL1C_CFG` (`0x3E000`) for 80 cycles before it writes the register back, then sets status 1.
Registers: R1, R2, I4, I6, I7, I12 are live at the site and untouched; R10, R11, R13 to R15 and I2, I3, I5 are reloaded
by the epilogue; R0, R3 to R6 are saved and restored. The displaced instruction runs in the patchkit tail.
`core/dsp-link` claims `dl.dsp.block_tail` and `dl.cf.frame_call`.

## Where the hook sits, and why not at the start of the handler **[C]**

S4b proposed a hook at the handler's entry (DN2 `0x1c9d9f`, DT2 `0x1c7501`, frame pointer in I3). That site is fine
for reading the tail but not for the acknowledgement: the DSP's reply record (`0xabc` bytes, two rings at DT2
`0x266220`/`0x267220`, DN2 `0x2c49d0`/`0x2c59d0`) is cleared and refilled in every block after that point. A marker
written into either ring at offset `0x200` is gone within 2 ms on both devices, and a run with markers over the whole
record showed that every byte of it is rewritten **[D]**. So the stub runs at the end instead, after the handler has
built the record and while the record pointer is still in I4 (`DM(I6 - 2)`); the acknowledgement is written in every
block while a transfer is active. Reading the chunk there is safe too: the receive rings alternate, so the next frame
lands in the other one (no corrupted chunk in any run: `dsp_nerr` 0).

**Acknowledgement.** Three halfwords at record offset `0x60` as the ColdFire reads them (big-endian halfwords of the
DSP's little-endian words): `0xA5D1`, cum, `xid | status << 8`, nerr. The ColdFire reads them from the record copy the
driver makes at the start of its next call (`param_4`). Status 0 running, 1 done, 3 destination refused.
Why `0x60`: the record is `0x1c` bytes of header, 32 slot rows of `0x54` bytes (14 entries of 6 bytes) and a
`0x20`-byte trailer at `0xa9c` that the ColdFire reads (the accessor `FUN_4002d36c` on DT2 1.16 indexes
`0x8000488c + 2 * (0x2a * slot + 3 * (n >> 1)) + {0x1c, 0x1e, 0x20}` with `n` = parameter - 14; callers take the
parameter from `DAT_800047a8/9`, which `FUN_4002d852` sets to 0, 1, `2 * (x + 1)`, `2 * y + 3`, or 40 and 41). The DSP
writes only entries 6 to 9 of each row (24 bytes at `0x40 + 0x54 k`, measured in a play run with six trigs) and
the ColdFire's default parameters 40 and 41 are entry 13. `0x60` to `0x67` are entries 11 and 12 of slot 0: written
by no DSP code and reachable only by parameters 36 to 39 (x = 17 or 18). That the ColdFire never asks for them is
**[O]**; the ack exists only during a transfer (a few hundred ms) and the PCM and the UI frames were unchanged in every
run. Header and trailer are not touched. On DN2 the record has no DSP data beyond the header in any run this finding
cites **[D]**, and the same offset is used. On the instrument the DN2's slot rows carry audio: see the next section.

## Digitone II: the slot rows are audio samples **[D]** (2026-10-22)

Measured on a DN2 unit (OS 1.11) by dn2_firmware_explore, with a read-only USB probe (PEEK of the ColdFire's record copy at
`0x800053a4`):

- **The layout** (2026-10-07, one note held at a time, 10 to 20 reads each, `waverider-pages34.md` in its docs folder). The 32 rows
  of `0x54` bytes are the 32 samples of the block. Each row is 28 channels of 24 bits. Channels 0 to 19 are tracks 7 to 16,
  one stereo pair each (track t on channels 2 (t - 7) and 2 (t - 7) + 1).
- **The other channels.** Channels 20 to 25 are three stereo buses that follow the effect sends. Channels 26 and 27 hold a
  small idle noise in every state. Tracks 1 to 6 reach the ColdFire in the SSI0 TDM stream instead.
- **Entries 11 and 12** (2026-10-22). Bytes `+0x44` to `+0x4b` of a row are bytes 68 to 75: the last byte of channel 22,
  channels 23 and 24, and the first byte of channel 25. A read of both words in rows 1 to 12, 100 times over 60 s, gave
  0 in every word while nothing played. While a project played (stock machines and a custom machine, unison, an arp,
  effects on), every one of the 24 words changed all the time.
- **Why the runs saw zeros.** The engine fills the rows outside the per-block render call that the runner and the
  coupled emulator run. So a run with no instrument sees the cleared record.

So on the DN2 these bytes are not free. Two writers in this repository use them:

- the acknowledgement at `DL_ACK_OFF` (`0x60` to `0x6b`, sample 0 of the effect buses), in every block while a transfer
  is active.
- `dl_diag`, when `core/dsp-voice` is in the image: 24 words at `+0x44` and `+0x48` of rows 1 to 12 (samples 1 to 12 of
  the effect buses), in every block.

What the ColdFire does with channels 20 to 25 (Overbridge or USB effect returns, or nothing) is **[O]**. A DN2 build that
writes there may put a fixed pattern into those channels at the block rate. The header words are a safer place on the
DN2. dn2_firmware_explore publishes its own diagnostics through reply word 2 (record offset `0x08`). No stock DSP code
writes that word. Words 0, 3, 4 and 5 are stock's.


## Byte order of the wire units **[D]**

The DSP's 32-bit read of the ring combines the first wire unit into the **low** half: word k = H(2k+1) << 16 | H(2k),
H(i) the i-th big-endian halfword the ColdFire sent (this is the `swap16` model of `sharc_harness/dma.py`). Seen in a
coupled run with a chunk in flight: the ColdFire queue holds `d15c` then `0001` in two consecutive data halves and the
DSP ring holds the bytes `5c d1 01 00`, the word `0x0001d15c`, on DT2 and DN2. The receive DMA delivers the whole
frame: DN2 RX `XCNT 0x55e` (finding 04); DT2 `FUN_1c7bd4` is called with `R4 = 0xabc` and stores `R4 / 2` as XCNT
(`0x1c80a0`, `0x1c7c06`) **[D]**. The tail halves of a stock frame are all zero in idle and while playing on both
devices (nothing writes them), so a magic of `0xD15C` with a check cannot occur in one; the proof reads the rings'
tail words idle (`pk_dsplink frame`).

## Proof details **[D]**

`pk_dsplink proof` (coupled daemon, a 3 s note after `restart`):
1. an image patched with `test/cf-demo` is the reference (a patched image boots cold, so its ready state is not the
   stock snapshot's; finding 17);
2. the same note on the dsp-link image with no transfer: **not equal** to the reference: one block earlier (64
   samples), with differences up to 9.0e-5 against a peak of 0.037. Moving the reference trigger by whole blocks does
   not make it equal (first nonzero sample matches at -88000 instructions, 87,852 samples still differ). This is the
   ColdFire hook's cost in every block interrupt (instructions) changing the note-on phase, not the module's content;
   reported, not checked. All 16 frame goldens of the smoke cell `dt2-1.16:core/dsp-link` equal the stock ones;
3. hook active vs the DSP site restored in the same image by `BYTES` patch (the original 6 bytes read from the reference
   daemon): bit-identical PCM;
4. a 4 KiB transfer to word offset 0x10 and a 64 KiB transfer to offset 0, started by writing the request fields of
   `dl_link` from the host (what `dl_send` stores; the daemon has no call method) with the state last: DONE, `dsp.peek` of
   the area equal to the source, PCM bit-identical to step 2;
5. a forged request whose first chunk ends beyond the area: the DSP sets status 3, the sender stops with error 3, the
   64 words around the area end are unchanged;
6. a lost chunk (the sender's position pushed ahead by 40): the DSP ignores the gap, the sender goes back once and the
   8 KiB arrive byte exact. The 64 KiB DN2 run also needed one natural go-back (`resent` 1).
`pk_harness` cells `dt2-1.16:core/dsp-link` and `dn2-1.11:core/dsp-link` (34 and 33 checks, smoke goldens recorded) and the
DSP budget gate pass.

## What the emulator does not show **[O]**

- **Caches.** The emulator does not model the SHARC L1 caches or the instruction-conflict cache: `FLUSH CACHE` and the
  `SHL1C_CFG` write are accepted and do nothing, so a stale-line bug cannot appear. From the manual
  (sc58x-2158x-prm, printed pages 4-50 and 8-1 to 8-13, and the `SHL1C_CFG` register page): the L1 cache stores L2 and L3
  code, `FLUSH CACHE` clears the instruction-conflict cache, `ICAINV` invalidates the I-cache in about 32 cycles, the
  firmware itself sets non-cacheable ranges (`0x200fa000` to `0x200fdfff`, `0x28240000` to `0x2839ffff`) and tests
  `SHL1C_CFG`, and a range invalidation (`CAINV` start and end, 64-byte lines, ends exclusive) must run from internal
  memory. The module uses the full invalidation; the range form is not used. Whether a read-modify-write of `SHL1C_CFG`
  with `ICAINV` set is the right sequence on a device (is the bit self-clearing?) is **[O]** until it runs there.
- **Data cache write-back (finding 91).** After the last chunk the hook now also writes the data cache back (write-back invalidation of DM and PM) before the ICAINV step, and the 12 instructions after each cache MMR write are 48-bit. Still no effect in the emulator; **[O]** on a device.
- **SPI timing.** The sender writes the tail whenever the driver is called. If the previous frame is still on the wire
  (the driver then skips its own frame) the chunk is lost or corrupted and the check or the go-back recovers it; the
  emulator's SPI never delays.
- **Hardware boot of the area.** The area is `.PKBSS`: the boot stream does not write it, so it is uninitialised L2
  (finding 06, ECC open point).
- That no ColdFire code reads record offsets `0x60` to `0x67` (above).

## Not done

The full-frame borrow (about 4 MB/s, silent blocks) of S4b is not built. The runtime wiring (`core/runtime` calling
`dl_send`, filling the voices registry from the loaded module) is the next Phase 5 item; the module's area is the only
destination `dl_send` accepts, so a module's DSP code and data must be linked for the area (`_dl_area`, 128 KiB; a
different size is the constant `DL_AREA_BYTES` and the `.PKBSS` in `dsp-link.inc`).
