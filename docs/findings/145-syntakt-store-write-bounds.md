# Syntakt store write bounds: who writes the card, and where a bound belongs

Marks: **[V]** checked against image bytes by a second agent; **[D]** read once, static, in the decompiled MAIN of OS 1.42 (`st-1.42`) and in the repository sources; **[O]** open. No emulator, no device. Context:
[138](138-syntakt-runtime-facts.md) "Storage", [142](142-syntakt-sample-storage.md) section 6, `modules/core/runtime/src/rs/src/storage/card.rs`. Plan item:
`docs/plans/syntakt.md` "Store write safety".

## Result

- Two separate writers can reach the digikit store range (sectors `0x800000..0x2e1a000`, `[card.store]`): our runtime, and the stock MidiRpc Data API.
- **The runtime is bounded, on all three devices, by shared Rust code.** The Syntakt is covered by the same code **[V]**.
- **The stock Data API is not bounded.** The host controls the byte count of each write; the start offset comes from the path, not from the host. Nothing
  limits the count to the target slot, and the eMMC driver checks only the card capacity **[V]**. This is the same on DT2 and DN2, and it is stock behaviour.
- The bound for the stock path belongs in a firmware hook on the eMMC write and erase entry, owned by `core/runtime`. It is not a Rust-only change.
  Until that exists, the bound is a host rule (below). Confidence: the facts high, the hook design medium.

## 1. The runtime path **[D]**

- One function writes the card: `card::block_write` (`storage/card.rs:339`). Every runtime write goes through it: the selection record and boot counter, uploads
  (`store_writer.rs`), the settings record, `CMD_CARD_PROBE`. The only other caller of the firmware's raw block write is the claim of a blank range in the same
  file (`card.rs:252`), after the same checks. `platform::raw_block_write` is documented as "Only `card::block_write` and `card::claim` call it".
- The gate `admit` first calls `Layout::write_allowed(sector, bytes)` (`sdk/src/store.rs:80`): `bytes` a non-zero multiple of 512, `sector >= first`, and the range
  ends inside `first + sectors`. Otherwise `E_RANGE`, counted in `DKRT_CARD.refused_range`. Then it checks the card state (ours, blank, foreign, too small).
- `first` and `sectors` are the device words `dkrt_store_first` and `dkrt_store_sectors` (`src/cf/<dev>/device.s`), assembled from `STORE_FIRST` and
  `STORE_SECTORS`, which `packages/patchkit/src/devgen.rs:1200-1205` generates from `[card.store]` of the device file. The Syntakt object `src/cf/st/device.s`
  has the same words. So the Syntakt range is `[0x800000, 0x2e1a000)` from `devices/syntakt.toml`, enforced by the same Rust as DT2 and DN2.
- A host cannot choose a sector on this path: the SysEx upload commands carry file names and sizes, and the store computes sectors from its own layout.
- A card that does not reach `first + sectors - 1` is "too small": no store, nothing read or written. The real part of a Syntakt is not known **[O]** (`SEC_COUNT`,
  finding 138). If the card is smaller than 0x800000 sectors (4 GiB) the runtime stays out of the store entirely, which is safe.

Verdict: no change in the runtime is needed for the runtime's own writes.

## 2. The stock path: MidiRpc Data API **[D]**

Types `0x53..0x5e`, dispatched by `FUN_4010bd9a` (called from the RPC dispatcher `FUN_40108fbe`, finding 142 "Dispatch").

| Step | Function | What the host controls |
| --- | --- | --- |
| Route | `FUN_400b9dfe` (samples), the `/projects` and `/soundbanks` handlers | The path string. For `/samples/N` the card byte offset is `N * 0x80000 + 0x380000` with N checked 1..64. The offset is **not** a request field. |
| WriteOpen | `FUN_4010b4de`, the sample branch at the line `local_f4 = *(u32 *)(data + 0x19)` | The u32 at +0x19 of the import file header becomes the descriptor size (`+0x10`), with no comparison. The declared size of the WriteOpen request is unused. The same handler serves the other routes. |
| Permission | `FUN_400b6204`: `(perm & 4) == 0` refuses with "Write: Permission denied" | Nothing; the route sets the word (`0x7e` for samples). |
| Writer | `FUN_400b60b6` builds `SafeMmcStreamWriter(offset, size, 0x10000)`; `MmcStreamWriter::vfunc_2` (0x400bbbe4) takes at most `size` bytes from the offset, upward | The data and its count. |
| eMMC | `FUN_4010f7f8` (write), `FUN_4010f52a` (erase; the writer erases the first block at start) | Nothing. They check `sector < SEC_COUNT` (-10), the byte count not negative, and the end not above the capacity (-13). |

Result: a header with a large size at +0x19, followed by that much data, writes upward from the slot into the neighbours and, from slot 64 at byte
`0x2400000` on, through the project pool, the COKi copies and (on the Syntakt layout, at sector `0x800000`, byte 4 GiB) the digikit store, up to the end of the card.
The start can never move below the slot. The same is true for the other two routes with their own descriptor sizes **[O]** (their size source was not traced).
The MmcFs guard (refuse sector `>= 0x180000`, `FUN_40111628`) is not on this path. `Clear` (`FUN_400b639a`) takes its range from the descriptor **[O]**.

The start offset is path-derived; the host controls the size (and the path).

DT2 and DN2: `FUN_4010bd9a` and its writers are the shared Elektron code (finding 142: the dispatcher shape is the DT2's), so the same exposure exists there. The
digikit store of those devices sits at the same sector `0x800000`. Whether the DN2 and DT2 chains differ in a bound was not compared function by function **[O]**.

## 3. Where the bound belongs

Options, with the reason each is or is not enough:

1. **Host rule (exists, keep).** The Syntakt sample backend sets +0x19 to `0x40 + 2 * length` (at most `0x80000`), sends exactly `31 + size + 12` bytes and writes only
   `/samples/1..64` (finding 142 section 6). It protects against our own client only. A test that pins the three limits belongs with the `stock-slots` backend.
2. **A change of the descriptor size in `FUN_4010b4de`** (clamp the +0x19 value to the slot size): one site, but it covers only the sample route, and it
   changes stock behaviour of a function the other routes share. Not recommended.
3. **A guard at the eMMC entry points (recommended).** Hook `FUN_4010f7f8` (anchor `cf.storage.fn.block.write` exists, `addr = 0x4010f7f8`) and `FUN_4010f52a`
   (no anchor on st yet; `cf.storage.fn.block.erase` would be new, rule `fw/storage:cf_func`). The hook refuses a range that overlaps `[store.first, store.end)`
   unless a flag word says the caller is the runtime (set around `platform::raw_block_write`, which is the only runtime caller). Every stock path (all three Data
   routes, Clear, the format worker) funnels through these two functions, so one pair of checks covers them. Location: `core/runtime`, Rust, in `storage/card.rs`
   (the range comes from the same device words); a `mode = "before"` hook that returns the firmware's `-10` is the simplest shape. The hook sites
   `cf.storage.site.block_write.clamp_entry` (0x4010f80c) and `clamp_chunk` exist but belong to `core/sdhc-large` as `replace` hooks; a second `before` hook on the
   function entry avoids that conflict **[O]**: confirm how patchkit stacks two hooks on one function before choosing.

The recommendation: option 3 in `core/runtime`, with option 1 kept as the first line. Cost: about a day of agent time including the host test and one emulator pass
(the stock Data API write of a slot with an oversize header must return the error and leave the store header intact). It benefits DT2 and DN2 too; the plan item
asks for it on the Syntakt first because the Syntakt is flashed next.

Is it a blocker for the first flash? No, if the first flash is followed only by our own clients (the workbench), which obey option 1, and if no Elektron Transfer
session is run against the device. With option 3 missing, a stock transfer program that sends a valid file cannot hurt the store either (its sizes are the slot size).
The hazard is a bug in a host client. State this in the checklist and run no Data API write on hardware before the guard exists, except one sample slot test with the
limits of option 1.

## Open items

- Size source of the `/projects` and `/soundbanks` writers and of `Clear`, `Copy`, `Move`, `Swap` (`FUN_4010acec`, `FUN_4010b016`, `FUN_4010b320`).
- How patchkit stacks two hooks on `FUN_4010f7f8`.
- The real card part of a Syntakt.
