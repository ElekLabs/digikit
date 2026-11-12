# usb-fast transfer ceilings: eMMC, eSDHC and the raw card sweep

Code: `modules/core/adapters/sdhc-large`, `modules/core/runtime` (`store_writer`), `modules/tools/usb-fast`. Hardware runs of 2026-11-11 and 2026-11-12 on a DN2 (OS 1.12) and a DT2 (OS 1.17), with the card-info test (`test/card-info`) and a measurement image (branch `exp/measure`). Earlier ceilings: [124](124-dt2-drive-fast-close-ceilings.md). Marks: **[D]** measured or read once, **[O]** open. Nothing here is **[V]**.

## The eMMC (both devices) **[D]**

`test/card-info` reads the EXT_CSD of each device. The two copies are byte-identical.

- eMMC 5.1, 23.05 GiB.
- WR_REL_PARAM 0x15 (enhanced reliable write supported), WR_REL_SET 0 (no partition forces reliable writes).
- Cache 1 MiB, off. BKOPS off. OPTIMAL_WRITE_SIZE 32 KiB.
- The EXT_CSD copy in RAM is byte-reversed in each 32-bit word: the eSDHC runs in little-endian mode for its data port.

## Card clock and buses **[D]**

- Card clock 37.71 MHz: base 75.43 MHz (PLL 528 MHz divided by OUTDIV3 + 1 = 7) divided by 2, 4-bit bus, high-speed timing.
- The eSDHC has no 8-bit mode, so the bus cannot be widened.
- The USB controller's BURSTSIZE is already 0x1010. On the crossbar the USB OTG master has the highest priority.

## The firmware's card driver **[D]**

- The data moves by eDMA channel 59 to the eSDHC data port (DATPORT), 16 bytes per request. It is not ADMA (this corrects the wording of [120](120-dt2-plusdrive-c-file-api.md)).
- One command moves 64 KiB at most. The cause is the 64 KiB bounce buffer: the firmware uses it for buffers that are not 16-byte aligned and for reads that are not whole blocks, and it cuts every transfer at that size. The eDMA's CITER field limits one command to 1023 blocks (511 KiB); `core/sdhc-large` uses 1022.
- Every write is issued with CMD23 and the reliable-write bit.

## Raw sweep on the DN2 **[D]**

8 MiB, median of 3 runs, from the measurement image. Rates in MB/s.

| command size | write | read |
|---|---|---|
| 32 KiB | 11.4 | 13.8 |
| 64 KiB | 13.3 | 15.6 |
| 128 KiB | 14.4 | 15.8 |
| 256 KiB | 15.0 | 15.9 |
| 511 KiB | 15.3 | 16.1 |

- The reliable-write bit and the card cache change nothing within 2%.
- A copy in RAM runs at 18 MB/s. The hash (`hash32`, before the fast hash) ran at 14.6 MB/s on cached and 9.3 MB/s on uncached data.
- USB alone moves 20.9 MB/s, and 13.5 MB/s while the CPU copies.
- A normal 8 MB upload took 2.94 s.

## Ceiling of an upload with a full card read-back **[D]**

The upload writes the file once and reads it back once, so one second moves 1 / (1 / write + 1 / read) MB. At 37.7 MHz this gives about 7.2 MB/s with 64 KiB commands and about 7.8 MB/s with 511 KiB commands. A faster upload needs a faster card clock (open) or no read-back.

## Open

- **[O]** An upload after a run in discard mode hangs (experiment only, not in a product image).
- **[O]** The gain of the integrated build (large commands, read-back overlap, receive ring) on hardware: `node tools/devtools/wb.mjs samples put FILE --speed MASK` sets the four switches and prints the mask and the usb-fast STATUS words 18 (staging bytes) and 19 (ring slots).
