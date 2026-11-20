# Syntakt OS 1.42: device facts for the digikit runtime

All claims are **[D]** (read once, static) unless marked **[O]** (open). The storage section also holds one emulator observation; nothing was run on a device. Image: st-1.42 MAIN
(`out/sections/st-1.42/section_3_MAIN_OS.bin`, load 0x40000400, end 0x40348b90) and its bootstrap (section 2). DT2 values are from
`devices/digitakt-ii.toml` and the DT2 1.17 MAIN. Why these facts: [finding 137](137-syntakt-patch-dry-run.md) "Runtime target",
[device-data](../design/device-data.md). Container and CPUs: [finding 70](70-syntakt-1-42.md).

## Summary

| Fact | DT2 1.17 | st 1.42 | Evidence | Mark |
| --- | --- | --- | --- | --- |
| Console UART base | 0xEC070000 (UART8) | 0xEC070000 | The literal is at 0x40002794 (DT2 0x40002450). The function that holds it, 0x40002782, is the UART8 init: byte for byte the DT2 function 0x4000243e (decompile equal, `UART8_USR = 0xdd`, divider `132000000 / (baud << 5)`, eDMA TCD35). The TX function 0x400a5890 (waits `USR & 4`, writes `UART8_DAT`) is the same 30 bytes as DT2 0x400d9c28. | D |
| Console drain before a soft reset | `FUN_400023f6`: queues, then `USR & 8` | 0x4000273a: the same | Called at the start of the soft-reset function 0x400058f4 (DT2 0x40005608), which is the same code: `sr = 0x2700`, drain, `clr.l 0x48000000`, 25 nops, `0x80` to `RCM.RCR` 0xEC090000. | D |
| eSDHC base | 0xFC0CC000 | 0xFC0CC000 | PRSSTAT 0xFC0CC024 has 17 literals in both images, all in the same functions (st 0x400bd02a.., 0x4010ed1a..; DT2 0x400f0078.., 0x4012d8be..). `SD_BRINGUP_SIG` (packages/emulator/boot/src/common.rs) with its data mask is unique on st at 0x4010ebea (DT2 0x4012d78e): same size 1402, same strict hash, and it spins on `ESDHC_PRSSTAT & 3`. It is called from the boot thread 0x40082578 (DT2 0x400cc9c8). The literal 0xFC0CC000 itself occurs in neither image: registers are reached by offset. | D |
| DDR cached base, size | 0x40000000, 128 MiB | 0x40000000, 128 MiB | The SDRAM controller setup of the bootstrap (section 2, 512 bytes from file offset 0x2c0 on st, 0x2c6 on DT2) is byte-identical. MAIN sets CACR 0xa50ce100 and ACR0 0x4007e020 (base 0x40, mask 0x07: 0x40000000..0x47ffffff) in the reset path (`movec` at 0x4000055a and 0x40000564), identical on both. The reset SP is 0x48000000 (`movea.l` at 0x400004f2). | D |
| Uncached alias | 0x48000000 (+0x08000000) | 0x48000000 (+0x08000000) | `clr.l 0x48000000` in the soft-reset function (above). MAIN reaches its DMA data through `0x4d...` and `0x4f...` addresses only (476 operands in 0x48000400..0x50000000, most of them constants; the real ones are in "RAM map" below). That the high half mirrors the DDR is a chip property **[O]**. | D |
| Keep area 0x47fe0000 (+0x1000) | free | **in use** | See "RAM map": 0x47c00000..0x48000000 is a 4 MiB DMA ring that MAIN clears at boot. Proposal: keep 0x47bf0000 (+0x1000). | D |
| cf.expansion | 0x47e8b000..0x47fe0000 | **0x478c0000..0x47bf0000** (3.2 MiB) | "RAM map". The region of finding 137 (0x47e8b000..0x47fe0000) lies inside the ring. | D, O (no canary run) |
| Storage | eMMC, MmcFs + path filesystem | eMMC, MmcFs only (no path filesystem); the store is the Digitone II's (first sector 0x800000) | "Storage". | D |
| Safe key | 19 (RECORD, firmware key index = panel code) | firmware key index 39 (trig 16, panel code 16) | "Safe key". | D, O |
| PEEK ranges | `[0x40000000, 0x47e30000]`, `[0x47e8b000, 0x47fe1000]` | `[0x40000000, 0x45570000]`, `[0x478c0000, 0x47bf1000]` | "PEEK". | D |

## RAM map of MAIN 1.42

- Reset path, 0x4000045c and 0x400004b2 (same code as DT2): copy the init table 0x4033a000..0x40348b90 to SRAM, then clear
  **0x4033a000..0x45566490** (the bss end of finding 137). Nothing above the bss end is cleared by the reset path. The bootstrap has no loop
  over DDR: its `clr.l (a0)+` loops (file offsets 0x3358 and 0x3372) run over SRAM 0x800096f2..0x80009af2 only.
- Heap: the buddy allocator `FUN_401036d0` with descriptor 0x402d5324 holds `{0, 0x444e48c0, 0x44465004, 0x454e48c0, 0x14, 0x100000}`:
  16 MiB at 0x444e48c0..0x454e48c0, 16-byte blocks, bitmap 0x44465004 (128 KiB), mutex 0x44485004. It is inside the bss: it is below
  0x45566490. (DT2: 32 MiB, order 21.)
- DMA data through the alias, found by `refscan` over 0x48000400..0x50000000 and read in the code that uses it:
  - cached 0x45570000..0x45600000 and 0x45600000..0x45609000: nine 64 KiB and eight 4 KiB slots (`FUN_4009748c`, bases 0x4d570000 and 0x4d600000);
  - cached 0x45608000..0x4564f0a0: operands 0x4d608000..0x4d64f088 (boot thread and self-test code);
  - cached 0x4582f080..0x4583f280: the eMMC bounce buffers; EXT_CSD at 0x4d83f080 (SEC_COUNT is read at 0x4d83f154, `FUN_4010ebea` 0x4010f0fe);
  - cached 0x4584f2c0: the boot sample upload buffer (`FUN_40082334`, getter `FUN_40082a48`; finding 142), live until the sample transfer after the
    handshake; cached 0x4784f2c1..0x478af2cb: the section 7 transmit buffer (`0x4f84f2c1`, loop end `0x4f8af2c9`, last store `0x4f8af2ca`, at
    0x40082d20..0x40082d48). The peer image has no stage buffer: the NOR bytes go straight into the transmit buffer ([finding 151](151-syntakt-section7-pack.md)).
- **The delay ring.** `FUN_400966e0` (called from `FUN_400a2e90` through `FUN_40097722`) clears 0x400000 bytes at **0x4fc00000** with
  `memset`. `FUN_4009687a` and `FUN_4009748c` index it as `0x4fc00000 + 8 * (i & 0x7ffff)`, advance `i` by 0x20 per frame and give the
  address to eDMA TCDs (0xFC045540.., 0xFC0453C0..). So the cached range **0x47c00000..0x48000000** (the top 4 MiB of DDR, including the
  reset stack page and the keep area of DT2) is a ring of 0x80000 entries of 8 bytes that is cleared at every boot and written every frame.
  That it is the delay memory is **[O]**; that the range is in use is **[D]**.
- Cached references into 0x45566490..0x48000000 (`refscan`, 102 hits) are all data or float constants (0x47c35000 is 100000.0f, 0x47efffff the
  high word of a double, 0x46fe0000 is 32512.0f).

Correction to [finding 137](137-syntakt-patch-dry-run.md): the proposed `cf.expansion` 0x47e8b000..0x47fe0000 is above the bss end, but it is
inside the ring. Proposal from this scan: start above the last peer-buffer byte, 0x478af2cb, plus 64 KiB, aligned: **0x478c0000**; end below
the ring with 64 KiB margin: **0x47bf0000**. The keep area follows the DT2 rule (the first 4 KiB above `cf.expansion`): **0x47bf0000..0x47bf1000**.
A canary run in the emulator (regions.py `confirm` on st) must pass before the numbers are used **[O]**. There is also an unreferenced gap
0x458b0000..0x4784f2c1 (31 MiB); it is not proposed because the code reaches buffers by base plus index.

## Storage

The Syntakt has an eMMC on eSDHC0, as the DT2 and DN2:

- The driver is the DT2 driver: init function equal (above); erase uses CMD35/36/38 (`FUN_4010f52a`, 0x4010f52a); the capacity comes from
  EXT_CSD `SEC_COUNT` (bytes 0x154..0x157 of the buffer at 0x4d83f080, stored to `_DAT_444ba720`, 0x4010f106) or, when that word is 0,
  from the CSD (0x4010f10e..0x4010f164). Read `FUN_4010f612` and write `FUN_4010f7f8` reject a sector at or above the capacity (-10) and a
  range that ends above it (-13) (0x4010f874).
- The eMMC identity whitelist is the same table: four rows (`004G90`, `4FTE4R`, `TX2932`, `TS0A32`) with the same capacity words
  (0x760000/0x3b0000, 0x748000/0x3a4000, 0x3a4a000/0x2e1a000 twice) at 0x402d5b40 (DT2 0x402b4fc8); referenced from 0x4010f1a4 and 0x40113766.
  The table is not enforced at boot (finding 154). Which part a Syntakt carries is read from the driver at run time (INFO `card_sectors`); `total_sectors = 0x2e1a000`
  is only the card the emulator emulates.

The card layout of MAIN 1.42 (units are 512-byte sectors; the MmcFs object is the class `MmcFs`, vtable 0x4024324c):

| Sectors | Content | Evidence |
| --- | --- | --- |
| 0 | MmcFs header, magic 0xBEEFBACE, version 1, 8 bytes read | `FUN_401128d0` sets `0xBEEFBACE` (`-0x41104532`); `FUN_40111704` reads 8 bytes at 0 |
| 0x800 | table: 0x110 = 272 bytes (16 + 256: the write-protect bitmaps of projects and sounds; **no kit bitmap**) | `FUN_40111786`; the format zero-fills 0x10 and 0x100 bytes |
| 0x1000..0x2000 | sound presets: 2048 slots of 2 sectors (`(slot + 0x800) * 2`, `FUN_40110c3a`); same as DN2 1.11 | `MmcFs::vfunc_14` returns 0x800 slots |
| 0x2000..0x12000 | 64 sample slots of 0x400 sectors, magic `SAMP` 0x53414d50, `((slot + 8) & 0x1fff) << 10`; up to 0x7ffc0 bytes of data | `FUN_401132ec`, `FUN_401133ce` (slot < 0x40); header, limits and host path: finding 142 |
| 0x7a000 and 0x7c000 | two "COKi" copies of the persistent arena, 0x3e6714 bytes each, in 0x2000-sector slots; three more copies are read from offsets 0x400000, 0x800000 and 0xc00000 through `FUN_40116d7a` (NOR, as on the DT2) **[O]** | `FUN_400bd608`, `FUN_400bd74e`, table 0x401c1490 / 0x401c149c |
| 0x7e000..0x80000 | project slot 0x80 (the working project), 4 MiB | `FUN_40110746` (`param == 0x80`: 0x7e000) |
| 0x80000..0x180000 | 128 project slots of 0x2000 sectors (4 MiB), `(slot + 0x40) * 0x2000`; the whole pool is erased by the format | `FUN_40110746`, `FUN_401128d0` (erase of 0x100000 sectors) |

There is no path filesystem: the magic 0x656b4653 and the constants 0x5d8000 and 0x2818180 are absent from the code. The format worker
`FUN_401128d0` ("INITIALIZING +DRIVE...") erases only the two pools above. **MmcFs writes (`FUN_40111628`) refuse a sector at or above 0x180000**
(`0x17ffff < sector`). `[capabilities] card` is `slots` for this layout (as the DN2), not `unknown`.

Other writers of the card: the COKi mirror (sectors 0x7a000 and 0x7c000 only); a self-test write at 0x40080646 (0x400806e0: sector 0, length 0)
**[O]**; and the MidiRpc Data API (types `0x53..0x5e`: `FUN_4010bd9a`; stream handlers `FUN_400b639a` Clear, `FUN_400b60b6` writer, `FUN_400b8b8e`).
Its offset does not come from the request: a path (`/projects`, `/soundbanks`, `/samples/N`) selects a route handler that builds the descriptor
(card offset, size, permissions). The writers (`MmcStreamWriter`, `SafeMmcStreamWriter`) call the eMMC write directly and ignore the MmcFs limit above.
For `/samples/N` the offset is `N * 0x80000 + 0x380000` and the size comes from the host's file header: finding 142.

### Store location

The store is a fixed 512 MiB area: `first_sector = 0x200000`, `end_sector = 0x300000`, `selection_sectors = 2`, `settings_sectors = 32`,
`header_b_sector = 0x2fffdd` (`devices/syntakt.toml` `[card.store]`; `[capabilities] card = "slots"`). It is above everything stock 1.42 writes (below 0x180000) and below
the smallest capacity the firmware's card table lists (0x3a4000). The earlier choice, the DN2 store at 0x800000..0x2e1a000, was too big for a unit's card
(`CARD_TOO_SMALL`). Reasons, the capacity word and the risks: [finding 154](154-syntakt-card-capacity-store.md).

Emulator observation **[D]**: stock 1.42 booted ColdFire-only (no DSP, no runtime) on the default emulated card (the whitelisted 32 GB pSLC part,
0x2e1a000 sectors), with a log of every eSDHC command that writes or erases (CMD25 data with its sector, CMD35, CMD36, CMD38). The session used the
panel: boot, SAVE PROJECT AS (slot 001, name NEW PROJECT, SAVE OK), a new project (LOAD PROJECT, CREATE NEW), SAVE SOUND (A:001, DONE) and
FORMAT +DRIVE with PROJECTS+SOUNDS and SAMPLES ticked (SUCCESS). A sample could not be loaded from the panel (samples arrive over USB).

| Step | Card access | Sectors |
| --- | --- | --- |
| Boot | CMD25 | 0x7e000..0x7ffff (the working project, 64 writes of 128 sectors) |
| Save project as, slot 1 | CMD25 | 0x80000..0x81f7f (project slot 1 is 0x2000 sectors from 0x80000) and again the working project |
| Save sound A:001 | CMD25, 1 sector | 0x1000 (`(slot + 0x800) * 2`) |
| Format +Drive | CMD35/36/38, 546 erases of 0x800 sectors | 0x80000 up to **0x17ffff** (the last CMD36 end address), the erase of the whole project pool |

The largest sector written is **0x81f7f** and the largest sector erased is **0x17ffff**. No CMD24 and no write at or above 0x180000 occurred. This
agrees with the static result (MmcFs writes refuse a sector at or above 0x180000; the format erases only the two pools above). The store starts
at 0x800000, 5 times higher. A real Syntakt may carry a smaller part **[O]** (`SEC_COUNT` of a unit was not read).

The remote stream handler (**static [D]**, details in [145](145-syntakt-store-write-bounds.md)): the `MidiRpcData*` request handler `FUN_4010bd9a` dispatches
Read/Write/Clear/Copy/Move/Swap requests. The card offset comes from the path (`/samples/N`: `N * 0x80000 + 0x380000`; `/projects`, `/soundbanks` have their own
descriptors), not from a request field. The host controls the byte count: the u32 at +0x19 of the import header becomes the writer size with no comparison, and the
writer (`SafeMmcStreamWriter`, first-block erase `FUN_4010f52a`) writes upward from the slot with no limit tied to it. The eMMC driver checks only the card capacity.
So a bad size from a host client can overrun a slot into the project pool and the digikit store at 0x800000; the runtime's own writes are bounded separately
(`card::admit`, finding 145). This is stock behaviour of every device (DT2 and DN2 share the handler).

## INFO device id

The runtime writes the device id in a 4-byte field of the INFO reply (`dkrt_device_name`, NUL padded), so "st" is followed by two NULs. The hosts
parsed a NUL-terminated string and read the next field one byte early. They now skip the padding of a name shorter than 3 characters
(`tools/device/dkupload.py`, `packages/dkupload/src/proto.rs`, `packages/dk-session/src/dkupload-core.ts`).

## Safe key

- `cf.input.fn.key_down(k)` (0x40102f40) is `table[k]` at 0x401d7984 (firmware key index to wire position `channel * 8 + bit`), then the bit in the
  mask bytes at 0x44464ff4. The decoder 0x40103094 maps the wire position to the firmware key index with the table at 0x401d7a48. On the DT2 and
  DN2 both tables are the identity, so `SAFE_KEY_CODE` (a `key_down` argument) equals the panel code. **On the Syntakt they differ**: trig keys 1..16
  (panel codes 1..16, wire positions 0..15) are firmware key indices **24..39** (`0x401d7a48`: channel 0 = 24..31, channel 1 = 32..39).
  `[digikit] safe_key` for this device must hold the index, 39 for trig 16.
- MAIN has no boot-time key reader: the 79 callers of `key_down` are view and event handlers (popups, parameter pages, pattern grid, song mode
  and so on). The literals they pass are 1, 2, 3, 5, 7, 12, 13, 18 and 23 (16 sites pass a computed index). None is in 24..39 as a literal. The only
  other reader of the mask bytes is 0x40102fe0 (a handler of the key event). The boot thread 0x40082578 and the self-test 0x40080646 do not
  read keys. A boot combination handled in the bootstrap (section 2) is not excluded **[O]**; the DT2 bootstrap is not different.
- Recommendation: trig 16 (panel code 16, firmware key index 39). Trigs 1..12 also select the track; 13..16 do not. A key held at power-on queues
  a key-down event that the UI loop gets later, as on the DT2 with RECORD (finding 117): trig 16 may toggle a step on the first pattern **[O]**.

## PEEK

- First range: from the DDR base to the first DMA arena above the bss end: **`[0x40000000, 0x45570000]`** (image, `.data`, bss and heap;
  DT2 stops at its arena, 0x47e30000, and does not include it).
- Second range: the runtime region with the keep area, **`[0x478c0000, 0x47bf1000]`** (follows the proposed `cf.expansion` and keep area).
- The arenas, the peer buffers and the ring are excluded (they are DMA targets, as the DT2 arena).

## Open items

- Canary run of the new `cf.expansion` and keep area on st (regions.py `confirm`).
- Whether a soft reset keeps DDR on the Syntakt (same open question as DT2).
- The card part of a real Syntakt (`SEC_COUNT`).
- The size source of the `/projects` and `/soundbanks` writers (finding 145), and the self-test write at 0x400806e0.
- The ring's purpose; the bootstrap's own key reads.
