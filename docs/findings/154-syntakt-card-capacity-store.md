# Syntakt 1.42: the card capacity, the stock card map and the digikit store area

Date: 2026-11-20. Static analysis of `st-1.42` (MAIN `out/sections/st-1.42/section_3_MAIN_OS.bin`, load 0x40000400) compared with `dt2-1.17` and `dn2-1.12`, and emulator runs. Context: [14](14-plus-drive-format.md), [138](138-syntakt-runtime-facts.md) "Storage", [142](142-syntakt-sample-storage.md), [145](145-syntakt-store-write-bounds.md). Marks: **[D]** read or measured once, **[O]** open. Nothing here is **[V]**.

## Result

A unit reported `CARD_TOO_SMALL` for the store of the first Syntakt build (the DN2 range up to sector 0x2e1a000, 24.8 GB). The store is now a fixed area 1 GiB .. 1.5 GiB (`[card.store]` `first_sector = 0x200000`,
`end_sector = 0x300000`), checked at boot against the capacity that the firmware's own eMMC driver reports.

## 1. The capacity [D]

- The driver keeps the capacity in sectors in one RAM word, set once by the eSDHC init in the boot thread (`FUN_4010ebea`, called from `FUN_40082578`), before the main-OS task exists: st word 0x444ba720
  (written at 0x4010f106 from EXT_CSD SEC_COUNT, or at 0x4010f144 from the CSD when SEC_COUNT is 0); DT2 0x44e3fea0.
- A 20-byte getter returns it, or 0xFFFFFFFF while the driver is not ready: st `FUN_4010f232` (0x4010f232), DT2 1.17 `FUN_4012ddd6`, DN2 1.12 0x4012c49a, DN2 1.11 0x4012c1ba. One strict hash
  (`f196b962...`) on all four: the anchor `cf.storage.fn.capacity` (mark O). The block read refuses a start at or past the capacity (-10), the write also a range that ends above it (-13).
- The runtime reads the getter (`Platform::card_sectors`), records it in `DKRT_CARD.sectors`, reports it in the CARD_CHECK event (fifth word) and in INFO (`card_sectors` and `card_state`, appended after the ABI
  fingerprint, and after the firmware identity when CAP_FW_ID is set) and shows `CARD x.x GB` on the HEALTH page.

## 2. The part table [D]

The identity table (st 0x402d5adc, 7 rows of 32 bytes; DT2 0x402b4a24) lists the parts the maker ships: manufacturer 0x11 (four `004G90`-class names): capacity 0x760000, or 0x3b0000 with the enhanced area set; 0x15
`4FTE4R`: 0x748000 / 0x3a4000; 0x70 `TS0A32`, `TX2932`: 0x3a4a000 / 0x2e1a000. **The result of the check is not enforced at boot on the Syntakt**: the boot thread stores the answer in a word that only the self-test reads
(on the DT2 it gates the mount). The pSLC conversion (enhanced user area) is made only by the self-test. Which part a given unit has: read it with INFO `card_sectors`; Em's unit reads 0x3a4000 (section 6).

## 3. The stock sector map (512-byte sectors) [D]

| Sectors | Content |
| --- | --- |
| 0, 0x800 | MmcFs header, write-protect table |
| 0x1000..0x1fff | 2048 sound presets of 2 sectors |
| 0x2000..0x11fff | 64 sample slots of 0x400 sectors |
| 0x7a000..0x7dfff | two COKi copies of the persistent arena |
| 0x7e000..0x7ffff | the working project, rewritten at boot |
| 0x80000..0x17ffff | 128 project slots of 0x2000 sectors (the format erases the whole pool) |

Nothing is sized from the capacity; MmcFs writes refuse a sector above 0x17ffff. FORMAT +DRIVE erases 0x80000..0x17ffff and 0x1000..0x1fff (with SAMPLES also 0x2000..0x11fff), not the whole card. Stock 1.42 touches only
sectors below 0x180000. Whether a later firmware uses more is **[O]**.

## 4. The decision

- **Area.** `first_sector = 0x200000` (0x80000 sectors, 256 MiB, above the last stock sector), `end_sector = 0x300000` (0xa4000 sectors below the smallest capacity in the table, 0x3a4000). 512 MiB;
  `header_b_sector = 0x2fffdd` and the settings and selection records are relative to the end, as on the other devices.
- **Fixed, not computed from the capacity.** The tail records move with the end of the area; a card whose capacity changes (a conversion, a swapped card) must not lose the store.
- **The check** (`card::check_with`, every device): capacity 0xFFFFFFFF (driver not ready): nothing decided, nothing read, writes refused this time, asked again at the next write; capacity below
  `first + sectors`: `CARD_TOO_SMALL`, no card access; otherwise the probes, the blank scan, the claim and the gate as before (a non-blank foreign area stays read-only).
- **Existing data.** None: `CARD_TOO_SMALL` made `admit` refuse every write before any sector was written, and `claim` runs only on a blank card.

## 5. Risks

| Risk | State |
| --- | --- |
| The area overlaps stock data | excluded by the map and the distance; the blank scan refuses a non-blank area |
| A later firmware writes at or above 0x180000 | the blank scan; re-run the map for a new version |
| Card smaller than the area | capacity check, `CARD_TOO_SMALL` |
| Capacity read before the driver is ready | getter returns 0xFFFFFFFF; state stays unknown |
| **The stock Data API overrun** (a host-declared size written upward from a sample slot, checked only against the capacity, finding 145) | **not excluded by the placement**; the write guard stays open (plan: "Store write safety") |
| Anchor drift | patchkit resolves by hash and refuses an image where it is missing; the hash is the same on four images |

## 6. The store on the unit [D]

Measured once on Em's Syntakt (st 1.42, `device` set): INFO reports `card_state` 1 (ours), `card_read_only` false and `free_bytes` 536851968 (the whole 512 MiB area) on a freshly flashed unit with no upload yet. The first `store put`
(49 bytes) writes the header pair; `store ls` and `store rm` work. HEALTH then shows STORE USED 1 KB and STORE FREE 511.9 MB. A content module runs end to end: `digikit/remote` installed from the workbench Transfer page into the
store, selected in MODULES, applied, restarted; the Remote pane's keys work. Before the first upload the store has no headers, so INFO reports the whole area free while MODULES and HEALTH showed no store.
INFO `card_sectors` is 3817472 (0x3a4000): the 0x15 `4FTE4R` row of the identity table with the enhanced area set, 1.82 GiB, which covers the store area (end 0x300000).

## Open

- **[O]** Whether the emulator should model a card capacity (today it ignores the size of `--card-image`).
- **[O]** The write guard against the Data API overrun.
