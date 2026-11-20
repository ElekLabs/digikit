# Plan: the eMMC write guard (core/card-guard)

Status: built for the Syntakt (`st-1.42`) behind a device-data switch; DT2 and DN2 carry the same code with the switch off. Host checks pass; no emulator or device run yet (the device test list is in section 8).
Hook sites, addresses and register facts: [finding 156](../findings/156-card-write-guard-sites.md). Sources: findings 142, 145, 146, 154, `modules/core/runtime/src/rs/src/storage/card.rs`, `devices/fw/storage.toml`, `devices/*.toml` `[card.guard]`.
Marks: **[D]** read once, static; **[O]** open. Nothing is **[V]**: a second agent must check the bytes first.

## 1. What is built

1. **One guard at the two eMMC entry functions**: a `before` hook at offset 0 of the block write `FUN_4010f7f8` (anchor `cf.storage.fn.block.write`) and the block erase `FUN_4010f52a` (anchor `cf.storage.fn.block.erase`).
   The two functions have the same strict hash on st-1.42, dt2-1.17 and dn2-1.12 (write `0dd5a176b94c`, erase `925ab4297f7c`), so one hook pair and one mechanism serve all three devices **[D]**.
2. **Its own adapter, `core/card-guard`**: the hook shims (`hooks.s`), the allow-list check as plain C with host tests (`guard.h`), the counters and the rows (`guard.c`, `st/device.s`). The runtime only exports one label.
3. **Allow-list from device data**: `[card.guard]` of the device file: the switch `enabled` and rows `{who, first, end}` in sectors. `who = "stock"`: every caller except the runtime's raw write; `who = "runtime"`: the runtime's raw write, which holds the store only.
   A request is allowed only if its whole range lies inside one row of its class; the gaps between rows are guard bands. Without rows the defaults are `stock = [0, store.first)` and `runtime = [store.first, store.end)`.
   The Syntakt sets the tight stock window `[0, 0x180000)` (the end of the project pool: MmcFs refuses more), so the 256 MiB up to the store are a guard band.
4. **Class of the caller by return address**: the runtime's raw write goes through `dkrt_raw_block_write` (`tramp.s`), whose call of the firmware is followed by the global label `dkrt_raw_write_ret`. A request with that return address is class `runtime`; every other caller is class `stock`.
   The recursive 64 KiB call inside the stock write (return address entry + 0x36, anchor `cf.storage.site.block_write.split_ret`) is skipped: it is a part of a request that was checked as a whole.
5. **Refusal**: return `-10` (the firmware's own "sector out of range" code) from the entry, with no lock taken and no state changed. Most callers test the result for non-zero or negative and stop with their own error (Data API: "Failed to write to eMMC"); five st-1.42 sites ignore it (finding 156 section 3), where a refusal is silent and safe because the request is not executed and those paths stay inside the stock window **[D]**.
   A refusal is counted (`dkcg_stats`): INFO appends six words after the card state, `DkrtHealth` has two fields (version 5), and the runtime's tick raises one log event `CARD_GUARD_REFUSED` (kind 58) for the refusals since the last event, at most one per five seconds (a stock writer that retries cannot flood the log). Nothing is shown on the device screen.
6. **The switch is enforced by patchkit**: `[module] needed_when = [{ switch = "card.guard.enabled", with = "core/runtime" }]` in the adapter's `module.toml`. On a device whose file turns the switch on, every patch set (checked by `patchkit gen`) and every module selection (checked by `apply`) that holds `core/runtime` must hold `core/card-guard`; a selection that holds the adapter where the switch is off is refused too.
7. The slot-level bound inside the stock window (a sample upload must not run into the next slot) is a separate, optional phase 2 at the Data API writer constructor (section 7). It protects the user's samples and projects, not the store. The store is already unreachable for every stock writer.

Cost: the adapter is 362 bytes of code, 100 of rows and 20 of bss; the runtime grew by 340 bytes of code and 12 of bss; the Syntakt `device` image has 16.4 KB of container left (limit 0x1FFE00) and keeps the bootstrap margin floor of 8.0 KB. Per call about 40 instructions at 264 MHz (under 0.2 microseconds) against 0.1 to 1 ms for one eMMC command; no paired A/B is claimed, the hardware timing item of section 8 is the check.

## 2. Every path that writes or erases eMMC sectors in st-1.42 MAIN 

The two entry functions are the only code in MAIN that issues write (CMD25 with `ESDHC_XFERTYP = 0x193a0022`) or erase (CMD35, CMD36, CMD38) to the user area,
**except** one raw path in the OS-upgrade finale (row 9) **[D]** (writers of `XFERTYP` at 0xfc0cc00c: `FUN_400bce58`, `FUN_400bcf7a`, `FUN_4010eb24` (command issue),
`FUN_4010ebea` (init), `FUN_4010f612` (read), `FUN_4010f7f8`, `FUN_4019e8b0`).

Entry facts **[D]** (decompiled; the DT2 and DN2 functions have the same hash):

- Write `FUN_4010f7f8(sector, bytes, buf)` (stack arguments `4(SP)`, `8(SP)`, `12(SP)` at entry). If `bytes > 0x10000` it calls itself in 64 KiB pieces (`jsr (A2)` at `0x4010f82c`,
  return address `0x4010f82e`). Then it takes the ESDHC mutex and checks `sector < capacity` (-10), `buf != 0` (-11), `bytes >= 0` (-12), `sector + ceil(bytes/512) <= capacity` (-13).
  `bytes == 0` returns 0 without a command. The capacity is the only bound.
- Erase `FUN_4010f52a(sector, count)`: CMD35 start `sector`, CMD36 end `sector + count - 1`, CMD38 argument 1. **No capacity check at all**; `count == 0` makes the end `sector - 1`.

| # | Writer | Entry chain | Sector range and how it is computed | Bounded? |
| --- | --- | --- | --- | --- |
| 1 | Data API WriteOpen/WritePartial, every route whose descriptor storage is the card (samples, projects, soundbanks) | `FUN_4010b4de` -> `FUN_400b6204` -> `FUN_400b60b6` -> `SafeMmcStreamWriter` (`0x400bbfa4`) -> `MmcStreamWriter::vfunc_2` (`0x400bbbe4`) -> write (RMW sectors `0x200` at a time; bulk chunks through `FUN_400bba38`) | Start = descriptor offset from the **path** (`/samples/N`: sector `((N-1)+8)<<10`, N 1..64; projects `(slot+0x40)*0x2000`; sounds `(slot+0x800)*2`). Size = the **u32 at +0x19 of the host's import header**, copied to descriptor `+0x10` with no comparison (`local_f4 = *(data+0x19)` at `0x4010b4de`; the handler is the same for all routes) | **No.** Upward from the slot to the card end |
| 2 | same, erase of the first sector | `SafeMmcStreamWriter::vfunc_2` (`0x400bc0ce`) -> erase(offset>>9, 1) | one sector at the slot start | Yes (path-derived) |
| 3 | Data API Clear | `FUN_4010b1d4` -> `FUN_400b639a` | card item: erase 1 sector at the item offset, or a zero write of `min(size, 0x200)` | Yes |
| 4 | Data API Copy | `FUN_400b65e4` -> `FUN_400b60b6` | destination size = `max(src size, dest size)` of the two **descriptors**; sample descriptor sizes are clamped `4..0x80000` by `FUN_400b9dfe` | Yes **[D]**; Move and Swap internals **[O]** |
| 5 | Data API Rename | `FUN_4010aeaa` -> `FUN_40111ed4` -> `FUN_40111628` | read-modify-write of the slot header sector | Yes (MmcFs check, row 6) |
| 6 | Stock UI saves (project, sound, sample header table, header, write-protect table) | `MmcFs` -> `FUN_40111628` (9 callers) -> write | `FUN_40111628` refuses a start above `0x17ffff` and a byte count above `(0x180000 - sector) * 512` | **Yes**, below `0x180000` (checked in the function itself) |
| 7 | Stock erases through MmcFs | `FUN_4011015e` (6 callers) -> erase | fixed: one sound slot (2 sectors), one sample slot (`0x400`), one project slot (`0x2000`), erase all samples `0x2000..0x11fff` in `0x800` pieces (`FUN_40112052`), FORMAT projects `0x80000..0x17ffff` (`0x100000` sectors in `0x800` pieces; `FUN_401128d0`, also run at boot as "INITIALIZING DRIVE", `FUN_4000a890`), FORMAT sounds `0x1000..0x1fff` | Yes, all below `0x180000` **[D]** |
| 8 | COKi arena copies | `FUN_400bd570` (the boot-record writer, anchor `cf.storage.fn.boot_record_write`), `FUN_400bd54e`, `FUN_400bd50e` -> `FUN_400bcadc` (`0x8000`-byte pieces); `FUN_400bd608` (restore from the NOR copy, erase `0x7a000`, `0x7c000`) | fixed table `{0x7a000, 0x7c000}` (`0x401c149c`) plus `0x3e6714` bytes (`0x1f34` sectors) | Yes, `0x7a000..0x7dfff` |
| 9 | OS-upgrade finale, **raw ESDHC, no call of the two entries** | `FUN_400bcf7a` (no direct caller; resets and re-initialises the card, `FUN_400bce58` DMA of `0x8000` bytes per step) | `(&0x401c149c)[idx] + n * 0x40`, idx checked `< 2`, `0x3e6714` bytes: the COKi copies again | Constant range **[D]**; the sector argument path is in assembly **[O]**. **Not seen by any hook** |
| 10 | Factory console command | `FUN_40080646` at `0x400806e0` | `write(0, 0, buf)`: sector 0, zero bytes, then prints DONE | No-op; **must stay allowed** (zero length) |
| 11 | Card configuration, not a sector write | `FUN_4010f3d8`: CMD6 EXT_CSD writes (`0x038d0000`, `0x038c0000`, `0x039c0100`, `0x039b0100`: enhanced-area size and attribute, partition setting complete) | one-time-programmable card setup (the pSLC conversion; finding 154 says the self-test only) | Not covered by a sector guard; irreversible **[O]** (who calls it on a unit) |
| 12 | The runtime | `card::block_write` -> `Platform::block_write_raw` -> `cf.storage.fn.block.write` | selection record, boot counter, uploads, settings, probe, claim; `admit` first checks `[card.store]` | **Yes** (finding 145 section 1) |

Not on the eMMC and out of scope here: NOR writers (OS upgrade stage, calibration record: finding 146; the same unbounded length-word pattern exists there and a sibling
guard with `[nor]` rows could reuse the table code, see section 9).

## 3. What a host can drive out of bounds 

- **Data API write on a card route (rows 1 and 2): yes.** The host picks the u32 at +0x19 and then sends that many bytes. The start cannot move below the slot, so the write walks
  **upward**, sector by sector, through the next slots, the project pool, the COKi copies (`0x7a000`) and the working project (`0x7e000`), then through the gap, the store, up to the card end **[D]**.
  Samples: from slot 64 (ends `0x12000`), projects: from slot 127 (ends `0x180000`). A `/projects` or `/soundbanks` route has its own descriptor but the same size source **[D]**;
  that no route clamps the size later was read for the sample route only (finding 142 section 6) **[O]** for the other two.
- Nothing else is host-driven: Clear, Copy and Rename are bounded by descriptors and MmcFs (rows 3 to 5). The stock UI saves and FORMAT (rows 6, 7) take their ranges from fixed formulas.
- A bug in a host client (ours or a third party) is the realistic cause; no attacker is needed. A stock Elektron Transfer sends the slot size, so it is safe.
- DT2 and DN2: the same write and erase functions exist, with `MmcStreamWriter::vfunc_2` and `SafeMmcStreamWriter::vfunc_2` among the callers (DT2 `0x400eef2c`, `0x400ef416`; DN2 `0x400f0908`).
  So the low level is exposed in the same way. Whether the Data API handler there also takes the size from the import header was not read **[O]** (finding 145 states it as the same code).
  DT2 has 27 callers of the write, mostly the +Drive folder filesystem (`FUN_40155*`, `FUN_4015a*`, `FUN_4015b*`); its sample route writes through that filesystem, not through a slot writer **[O]**.

## 4. Design options and trade-offs 

| Option | Sees | Strength | Weakness |
| --- | --- | --- | --- |
| A. Host rule only (exists: `0x40 + 2*length`, exact byte count, slots 1..64) | our clients | free | does not stop a client bug or another client |
| B. Hook on the Data API size load (`local_f4 = *(data+0x19)` in `FUN_4010b4de`) or on the `SafeMmcStreamWriter` constructor | Data API only | an exact slot bound is possible | misses every other writer and every future route; edits a function shared by all routes; needs per-type slot sizes; the site differs on DT2/DN2 (not shared) |
| **C. Hook at the two eMMC entries (built)** | **every caller**, including the runtime, the stock saves, FORMAT, the COKi writers, the console | one site pair, same hash on three devices, no Data API knowledge, `before` hook: stock path unchanged when the request is inside | cannot see the raw ESDHC finale (row 9) or the CMD6 setup (row 11); a coarse bound (stock window), not a slot bound; runs in stock task stacks |
| C+B. C now, B later as phase 2 | | slot integrity too | extra hook, per-type data |

Why the lowest level: the Data API writer is one of **seven** stock callers of the write and **four** of the erase (section 2). The guard that matters for the store is a property of
the card, not of one protocol. The hook also gives defence in depth for the runtime: its own range check (`card::admit`) and the hook check are independent.

### What a refused request does

- Write refused: `-10` from the entry. `MmcStreamWriter::vfunc_2` tests `-1 < result` and returns the error "Failed to write to eMMC" (`0x400bbf4c` path); `FUN_400bba38` returns -1, giving
  "Failed to write to eMMC from internal buffer"; the Data API job ends with an error to the host, with no retry loop **[D]**. MmcFs `FUN_40111628` tests `!= 0` and returns code 1.
  The COKi writer `FUN_400bcadc` ignores the result; `FUN_400bd608` returns -1 on a failed write.
- Erase refused: `-10`; `SafeMmcStreamWriter::vfunc_2` reports "Failed to erase first block"; `FUN_4011015e` reports an error code.
- Stock UI reaction for MmcFs errors (project save, FORMAT) is **[O]**: none is expected, because every stock range is inside the window (section 2, rows 6 to 8) and the emulator case
  (section 8) proves FORMAT and a save pass.
- An aborted sample upload leaves the slot empty (no `SAMP`), as an interrupted one does today (finding 142 section 3). Bytes already written into a neighbour slot before the refusal stay
  (phase 2 would prevent that).

### What must keep working

- Zero-length write at sector 0 (row 10): empty range, allowed.
- The 64 KiB self-split: the outer call is checked as a whole, the nested calls (return address `0x4010f82e`) are skipped.
- `core/sdhc-large` (DT2 and DN2): it replaces the compare at offsets `0x14` and `0x22` of the write; the guard patches offsets `0..8`; no byte overlap. With it a request can be up to `0x7fc00` bytes: still one check.
- FORMAT, boot-time "INITIALIZING DRIVE", COKi restore and project saves: all inside `[0, 0x180000)` on the Syntakt.

## 5. The check, precisely

Inputs: class (`stock` or `runtime`), `first` sector, `sectors`.

```text
write:  bytes as i32 < 0   -> pass to the stock code (it returns -12)
        bytes == 0         -> pass (stock returns 0)
        ret == split_ret   -> pass (nested call of a checked request)
        class  = runtime if ret == dkrt_raw_write_ret else stock
        sectors = (bytes + 511) >> 9            (u64 arithmetic)
erase:  count == 0         -> refuse (the end sector would be first - 1; no stock caller passes 0 [D])
        class = stock (the runtime does not erase)
allowed = exists row with row.who == class and first >= row.first and first + sectors <= row.end
```

No sum is formed: the row test is `first >= row.first && first < row.end && sectors <= row.end - first` in 32-bit arithmetic, so `sector + count` cannot wrap (`guard.h`). Rows with equal `who` that touch are merged by the generator.
On refusal the adapter's counters `refused writes` or `refused erases` rise and the last request (sector, sectors, class and kind) is stored (`dkcg_stats`). The log event `CARD_GUARD_REFUSED` is raised by the runtime tick from
these counters (not from the stock task: see the stack risk in section 10).

## 6. Device data, ownership, registers 

### Device data

A general region table in the device file; no device name in code. A Syntakt example:

```toml
# Which sectors each class of writer may write or erase. A request is checked as a whole.
# who = "stock":   firmware writers (every caller of the eMMC write and erase except the runtime's raw write)
# who = "runtime": the digikit runtime's raw card write
[[card.guard]]
who = "stock"
first = 0x0
end = 0x180000        # MmcFs refuses a sector at or above this; FORMAT ends at 0x17ffff (finding 154)
[[card.guard]]
who = "runtime"
first = 0x200000      # = [card.store]
end = 0x300000
```

Defaults (the DT2 and DN2 files carry `[card.guard] enabled = false` and no rows): the `runtime` row is `[card.store]` `first_sector..end_sector`; the `stock` row is `[0, store.first_sector)`.
For the DT2 that is `[0, 0x2df1000)` (the +Drive ends at `path_fs.end_sector = 0x2df0180`, its FORMAT erase ends there: 0x5d8000 + 0x2818180); for the DN2 `[0, 0x800000)` (nothing stock writes at
`0x458000` or above, finding 59). The Syntakt sets its row explicitly, because its default (`0x200000`) would leave `0x180000..0x200000` (256 MiB) writable for stock code.

`patchkit gen` checks, per device with the table (a failing check is a build error, not a test; host tests in `devgen.rs` `guard_tests`):
- at most 8 rows; `runtime` rows lie inside `[card.store]` and together equal it; no `stock` row overlaps a `runtime` row or the store;
- every `[card]` item (`header_sector`, `table_sector`, each pool slot range, each `[[card.regions]]`, `path_fs`) lies inside one `stock` row. This is the check that a too-tight row cannot break stock saves;
- a device without `[card.store]` cannot have the table.

The generator writes `CG_N`, `CG_FIRST_i`, `CG_END_i`, `CG_WHO_i` into the adapter's `build/gen/card-guard-<dev>.inc` (kind `gas-card-guard`); the per-device `src/cf/<dev>/device.s` lays them out as `dkcg_rows`. This does not change `DkrtDevice` (no ABI fingerprint change).

### Which module owns the hook

`core/card-guard`, an adapter with its own `[[op]] op = "hook"` entries (as `core/platform` and `core/sdhc-large`). The runtime exports only `dkrt_raw_block_write` and the label `dkrt_raw_write_ret`; it reads the adapter's counters through the optional symbol `dkcg_stats` (0 without the module) and knows nothing else of the guard (`storage/card_guard.rs`). The reason the first design gave for putting the hooks into the runtime (an adapter that is left out leaves the store open) is met by device data: the patch-set rule of section 1 item 6.

Files:
- `devices/fw/storage.toml`: `cf.storage.fn.block.erase`, the sites `block_write.entry`, `block_erase.entry`, `block_write.split_ret` and the hooks `cf.storage.hook.block_write.entry`, `cf.storage.hook.block_erase.entry` (`mode = "before"`); the ports of the four images carry them (mark O).
- `modules/core/adapters/card-guard/`: `module.toml`, `src/cf/guard.h` (the check), `guard.c`, `hooks.s`, `st/device.s` (rows), `tests/guard-host.c` with `tests/guard.test.ts`.
- `modules/core/runtime`: `src/cf/tramp.s` (`dkrt_raw_block_write`, `dkrt_raw_write_ret`), `storage/card_guard.rs` (INFO, HEALTH, the event), `platform.rs` (`block_write_raw` calls the trampoline).
- `packages/patchkit`: `devgen.rs` (`[card.guard]` rows and checks, the `gas-card-guard` kind, `check_needed_when`), `apply.rs` (the selection check), `devdata.rs` (`switches`).
- `packages/dk-session` (`info().card_guard`), `docs/design/digikit-upload.md` (INFO row), `sdk/sdk.toml` (log kind 58), `modules/core/runtime/abi.toml` (`DkrtHealth`).

Hook shim (write; the erase shim is the same with two arguments):

```text
dkcg_write_hook:             ; entered by jmp from the site; SP -> return address, 4(SP) sector, 8(SP) bytes
    move.l (%sp),-(%sp)             ; ret
    move.l 12(%sp),-(%sp)           ; bytes
    move.l 12(%sp),-(%sp)           ; sector
    jsr    dkcg_write               ; extern "C": 0 = pass, else the error code
    lea    12(%sp),%sp
    tst.l  %d0
    bne.b  1f
    jmp    cg_write_next            ; hook.<site>.tail: the displaced "lea (-0x14,SP),SP; movem.l ..." and back into the stock code
1:  rts                             ; d0 = -10; the lock was never taken
```

The runtime's trampoline: `dkrt_raw_block_write: push the three arguments again; jsr cf.storage.fn.block.write; dkrt_raw_write_ret: lea 12(%sp),%sp; rts`.
The class test is one compare of the saved return address with `dkrt_raw_write_ret`. Why not a "runtime is writing" flag: a stock task can run during the window of the flag (the RTOS is
preemptive), so the flag would let one stock write through; a return address cannot be forged by another task.

### Register and liveness facts at the sites **[D]**

`uv run python -m tools.coldfire.liveness st-1.42 --addr 0x4010f7f0 --len 0 --from 0x4010f7f8 --abi both` (and `0x4010f52a`): under the compiled-code call convention `D0, D1, A0, A1` and the condition
codes are dead at both entries; `D2..D7, A2..A6` are live (the entry saves them with `movem.l`; the shim only calls a C function, which preserves them). Under the strict reading they are live only because
the first call at `0x4010f82c` / `0x4010f53c` is counted as a use; the callee `FUN_40001918` (mutex lock) takes its argument on the stack (`pea`). Callers of both functions are compiled code (decompiled
call sites push arguments) **[D]**; an assembly caller that keeps a value in `D0`, `D1`, `A0` or `A1` across the call is not excluded **[O]** (run `tools/coldfire/refscan.py` and read the 6 + 4 call sites; the
liveness tool decides only the stock function's own reads). Displaced instructions: write `lea (-0x14,SP),SP; movem.l {D2 D3 D4 D5 A2},(SP)` (4 + 4 bytes), erase `lea (-0xc,SP),SP; movem.l {D2 D3 A2},(SP)`;
none is PC-relative, so the `before` hook copies them to the tail unchanged.

### Do DT2 and DN2 have the same exposure and the same hook sites?

The low level: yes **[D]**. Hashes are equal (write `0dd5a176b94c`, erase `925ab4297f7c`) and the function bodies have the same structure (recursion at 64 KiB, mutex, the same capacity checks).
Addresses: write DT2 `0x4012e39c`, DN2 `0x4012ca60`, st `0x4010f7f8`; erase DT2 `0x4012e0ce`, DN2 `0x4012c792`, st `0x4010f52a`. The stock windows differ by device (above) and come from device data.
The host-driven overrun on DT2 and DN2 depends on the Data API handler there **[O]**; the guard does not need the answer, because it bounds every caller.
The code is the same for all three devices; the switch `[card.guard] enabled` is on for the Syntakt and off for the DT2 and DN2, whose ports and sets take the adapter later.

## 7. Phase 2 (optional): slot bound for the Data API writer

Hook the constructor of `SafeMmcStreamWriter` (`0x400bbfa4`, arguments `this, offset, size, ring`; also see `FUN_400b60b6`) and clamp `size` to the end of the slot that contains `offset`.
The slot table is device data: add `[card.pools.samples]` (base `0x2000`, stride `0x400`, 64 slots, `slot_bytes 0x80000`) beside the existing `projects` and `sounds` pools; a general pool lookup then gives
`slot_end`. A larger body from the host then fails as "Footer was not processed" or "Content hash mismatch" (the extra bytes fall into the footer buffer), a clean error. This protects neighbouring slots and projects.
It is not needed for store safety, edits a stock constructor, and has no counterpart address on DT2/DN2 verified yet. It is done only when neighbour protection is wanted; finding 145 judged the
descriptor-size clamp alone as not worth it.

## 8. Tests 

Layers follow `AGENTS.md` "Test levels": the logic at the lowest layer, one case per seam in assembly, only firmware facts on the device.

**Host, done.**
- `core/card-guard` `tests/guard.test.ts` (C host test `guard-host.c`): inside, at the first and last sector of a row, one sector before and after, a range that spans two rows, the gap; class `runtime` in the store, below, above and straddling it; class `stock` in the store and in the gap; zero length (passed on), negative length (passed on), lengths not a multiple of 512, a sum that would wrap, a huge count; the nested call passes; erase (stock class, count 0 refused); counters and the last request.
- `patchkit` `devgen.rs` `guard_tests`: the Syntakt rows, the DT2 and DN2 defaults, a stock row too tight for a card item, rows that reach the store or leave it partly open, row shape, at most 8 rows, merging of touching rows, the constants.
- `patchkit` `tests/devdata.rs`: the patch-set rule (device and emulator sets without the adapter are refused, the adapter is refused where the switch is off or the table is missing, the resolved selection of `apply`).
- `patchkit` `tests/card_guard.rs` (real Syntakt image, skipped without it): both hooks sit at the entries with the displaced instructions, the tails continue at entry + 8, the adapter and the runtime label are placed, the split anchor is entry + 0x36; `apply` refuses a selection with the runtime and without the guard.
- `core/runtime:rust-tests` `storage/card_guard.rs`: no guard in the image reports zeros and no event; the words and their order; one event with the last request and the totals, none for an unchanged count. `packages/dk-session` `info-flags.test.ts`: the INFO fields.

**Emulator, done for the Syntakt:** `modules/core/runtime/tests/card-guard.device.test.ts` (project `core/runtime-tests`, record `devices: ["st"]`, Nx target `core/runtime-tests:test-device--card-guard--st`). It boots the Syntakt `emulator` set cold (blank card, so the stock drive initialisation and the working project writes run under the guard), reads the guard counters from INFO (0), uploads a module through the runtime (accepted) and a sample through the stock Data API (accepted, read back), and checks that no refusal event is in the runtime's ring. It then pokes the end of the stock row in `dkcg_rows` (RAM in the emulator) to the first sector of sample slot 2 and writes slot 2 again: the host gets an error, INFO counts one refused erase (the slot writer erases the first sector of the slot first) at that sector, class stock, one `CARD_GUARD_REFUSED` event names it, slot 2 stays empty, slot 1 and the store listing are unchanged, the card is still ours, and the runtime's own write into the store is still accepted. The `st` half is not cached (no daemon key, image hashes or patchkit key for this device in `dist/bytes`), and its daemon is the plain build of `tools.emulator.native_emu --daemon --build-only`. The numbered cases below are the first design; a stock project save and the DT2 and DN2 halves are not part of this test.

Design cases:
1. *Normal sample upload accepted*: the existing Data API sample upload of a slot (96,107 bytes, body ending on a chunk boundary, finding 142) still succeeds, reads back and the INFO guard counters (`refused_writes`, `refused_erases`) is 0.
2. *Oversized count refused*: a real walk from a sample slot to `0x180000` is about 760 MB of SysEx, so it cannot run in an emulator test. Use one of: (a) poke the `stock` row `end` in the loaded image
   to `0x2400` (end of slot 1; the table is rodata in RAM after the stock OS unpacks; the daemon's memory write, as the card tests peek `DKRT_CARD`) **[O]** confirm the daemon can write that region, then upload a header
   with size `0x100000` and a body of that length to slot 1: expect the host error, `refused_write == 1`, `last_sector == 0x2400`, slot 1 empty, slot 2 bytes unchanged; or (b) a test-only device-file variant
   with a small `stock` row (the fixture rule in `AGENTS.md`: a thin variant only where a test needs it). (a) is cheaper.
3. *Store untouched*: after case 2 with the row poked to end at the **store** boundary minus the gap, the store header sectors still read as before (a hash of the sectors `first`, `first + header_b`, `sel_a`, `sel_b`).
4. *FORMAT still works*: boot the Syntakt on a blank card image. The stock boot formats an uninitialised drive (`FUN_4000a890` -> `FUN_401128d0`, "INITIALIZING DRIVE", erases `0x80000..0x17ffff` and the
   sound area) **[O]** that a blank card takes this path; assert the MmcFs header magic at sector 0 afterwards, `refused_erase == 0`, and the runtime's state unchanged. A panel-driven SETTINGS > FORMAT scenario
   adds the menu path (an existing +Drive format scenario, finding 14, may do it **[O]**).
5. *A stock project save* (save project from the panel, existing scenario if any) and *the runtime's upload* (the store upload test) pass with the guard on.
6. *Runtime class*: `CMD_CARD_PROBE` (`card::probe`) writes inside the store: accepted; the `E_RANGE` refusal of `admit` still happens first for a sector outside the store, and the hook counters stay 0.

Test selection: the change reaches most device tests through the runtime image; use the named subset route (`mise run test:subset -- ...` plus `gate-smoke`, `gate-pins`) on the Syntakt and one of DT2/DN2,
not `test:full`; state the skips in the PR. The guard must not change the DSP, so SHARC golden hashes and PCM pins are not affected.

**Only hardware can show:**
- that the real card and the real stock firmware never write at or above `0x180000` in a path the emulator does not run (a project save while sampling is not a Syntakt feature; the OS upgrade, the
  self-test and the calibration flows; the first real FORMAT), read from the guard counters via INFO after a day of use, all zero;
- the stack headroom of every stock task that reaches the entries (Data API task, UI task, boot task, console task): measured high-water mark with the guard on;
- the timing of one guarded write against an unguarded one on the real eSDHC (expected below measurement noise);
- the real card part (finding 154 **[O]**: INFO `card_sectors`).

## 9. Not covered, and follow-ups

- Row 9 (the OS-upgrade finale's raw ESDHC writes) and row 11 (CMD6 EXT_CSD) bypass the entries. Row 9 writes only the COKi copies at `0x7a000/0x7c000` from a constant table **[D]**; the sector argument path
  was read only to the table lookup **[O]**. A hook on the command issue `FUN_4010eb24` would see CMD6, CMD24/25 and CMD35/36 of every path (and `FUN_4019e8b0` of the finale), but it works with the controller registers and runs under the
  controller lock; it is a second, larger design. Not part of this plan.
- A guard for the NOR writers (finding 146: a forged length word erases the calibration record) would use the same rows and a `[nor]` table; not part of this design.
- The Data API `storage == 1` (memory) branch builds `MemoryStreamWriter(+0xc, +0x10)` with the same host size: a RAM overrun path if any route has storage 1 **[O]**; not eMMC, listed because it shares the cause.

## 10. Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| A `stock` row that is too tight | stock saves or FORMAT fail with an error | generator check that every `[card]` item lies in a row; emulator FORMAT, save, upload cases; the Syntakt row follows MmcFs's own `0x180000` bound **[D]** |
| Stock code that writes above `0x180000` on 1.42 in an untested path | that feature fails with `-10`; counters name the sector | counters in INFO and HEALTH; first flash with no Data API write except a single slot test (finding 145) |
| Stack use in stock task contexts (the hook runs on the caller's stack) | stack overflow in a small task | the allow path is a leaf of about 24 bytes of arguments; the refusal path only stores counters; measure the high-water mark on hardware **[O]** |
| An assembly caller that keeps `D0/D1/A0/A1` across the call | corrupted state | read the 10 call sites; the compiled-code convention holds for the decompiled ones **[D]**, **[O]** for the rest |
| Hook install conflicts with `core/sdhc-large` | patch refused or both hooks broken | different offsets (`0..8` against `0x14` and `0x22`); one `patchkit` test with both modules on `dt2-1.17` and `dn2-1.12` |
| Port or hash drift (another OS version) | hook not installed | patchkit refuses an image whose anchor hash is missing; only 1.42, 1.17, 1.12 are supported |
| False sense of safety | rows 9 and 11 are outside | section 9 states it; the plan item says "eMMC write and erase entries" |
| Return-address class is wrong after a runtime change that inlines the write elsewhere | runtime writes refused (`-10`), runtime reports a failed write, no corruption | one trampoline, one label; a host test of the label; the assembly case; the runtime's own tests (store upload) fail at once |

Performance: ceiling about 40 instructions per call at 264 MHz (under 0.2 microseconds) against 0.1 to 1 ms for one eMMC command; one check per 64 KiB piece at most. No paired A/B is claimed;
the hardware timing item above is the check.

## 11. Scope and rules in force

1. Stock window: tight, `0x180000` on the Syntakt; the gap up to `store.first` is a guard band.
2. Writer class by return address (the runtime's raw-write trampoline label); not a union rule.
3. Phase 2 (slot clamp): its own item, later.
4. Stock sample and sound slots are written only through the workbench (bounded by `bytes_max`, `slot-samples.ts:137`) until the guard has been flashed.
5. Refusals: counters in INFO and HEALTH, one log event, nothing on the device screen.
6. Scope: the Syntakt, behind `[card.guard] enabled`; DT2 and DN2 follow after emulator proofs (a port of the adapter, `enabled = true`, the adapter in their sets).
7. Ownership: the hook pair lives in `core/card-guard`; patchkit refuses a patch set or selection that holds `core/runtime` on a device with the switch on and not `core/card-guard`.
