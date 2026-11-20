# The eMMC write and erase entries as hook sites for the card write guard

Date: 2026-11-21. Static analysis of `st-1.42` (MAIN, load 0x40000400) compared with `dt2-1.17`, `dn2-1.12` and `dn2-1.11` (cfdb, `tools/coldfire/fw.py`, `tools/coldfire/liveness.py`). Context: [145](145-syntakt-store-write-bounds.md), [154](154-syntakt-card-capacity-store.md), plan [card-write-guard](../plans/card-write-guard.md). Marks: **[D]** read once here; **[O]** open. Nothing is **[V]**: a second agent has not yet checked the bytes. The anchors are declared with mark O.

## Result

The firmware's eMMC block write and erase are the only code of MAIN that issues a write or erase command to the user area, apart from the OS-upgrade finale's raw controller writes of the COKi copies (finding 145). Both functions are one hash on all four images, so one hook pair and one allow-list mechanism serve every device. The hooks are `before` hooks at offset 0 of each function (eight displaced bytes, no PC-relative operand); D0, D1, A0, A1 and the condition codes are dead there under the compiled-code convention, and the five absolute-address call sites that the Syntakt has were read to confirm it.

## 1. The two functions [D]

| | write `f(sector, bytes, buf)` | erase `f(sector, count)` |
| --- | --- | --- |
| hash (strict) | `0dd5a176b94c6e30225068661b58a898` | `925ab4297f7c5d4d24f735ebc22fea1c` |
| size | 514 | 232 |
| st-1.42 | 0x4010f7f8 | 0x4010f52a |
| dt2-1.17 | 0x4012e39c | 0x4012e0ce |
| dn2-1.12 | 0x4012ca60 | 0x4012c792 |
| dn2-1.11 | 0x4012c780 | 0x4012c4b2 |
| displaced bytes | `4fef ffec 48d7 043c` (`lea (-0x14,SP),SP; movem.l {D2 D3 D4 D5 A2},(SP)`) | `4fef fff4 48d7 040c` (`lea (-0xc,SP),SP; movem.l {D2 D3 A2},(SP)`) |
| anchors | `cf.storage.site.block_write.entry`, hook `cf.storage.hook.block_write.entry` | `cf.storage.fn.block.erase`, `cf.storage.site.block_erase.entry`, hook `cf.storage.hook.block_erase.entry` |

At the entry 0(SP) is the return address and the arguments follow. The write calls itself through `jsr (A2)` at entry + 0x34 for each 64 KiB piece of a larger request; the instruction after it, entry + 0x36 (0x4010f82e on st-1.42), is the return address of that nested call (anchor `cf.storage.site.block_write.split_ret`, the same offset on the other three). The erase has no capacity check at all: CMD35 starts at `sector`, CMD36 ends at `sector + count - 1`, so a count of 0 ends at `sector - 1`.

## 2. Registers at the sites [D]

`uv run python -m tools.coldfire.liveness st-1.42 --addr 0x4010f7f0 --len 0 --from 0x4010f7f8 --abi both` and the same for 0x4010f52a: under the compiled-code convention D0, D1, A0, A1 and the condition codes are dead at both entries and D2..D7, A2..A6 are live (the entries save them with `movem.l`). Under the strict reading D0, D1, A0 and A1 count as live only because the first call (`jsr (A2)` at 0x4010f82c, `jsr 0x40001918.l` at 0x4010f53c) is treated as a use of every register; the callee takes its argument on the stack. The hook shim calls one C function, which may change D0, D1, A0, A1 and the condition codes only.

The callers: write 8 sites (the factory console command `FUN_40080646`, `FUN_400bba38`, `MmcStreamWriter::vfunc_2`, `FUN_400bcadc`, `FUN_400bd608` twice, `FUN_40111628`, and its own recursion), erase 5 sites (`FUN_400b639a`, `SafeMmcStreamWriter::vfunc_2`, `FUN_400bd608` twice, `FUN_4011015e`). All are compiled functions with a frame or a `movem` prologue. For the five absolute-address call sites (0x400806e6, 0x400bba5e, 0x400bbd94, 0x400b6452, 0x400bc136 are the return addresses) the liveness tool from the return address gives D1, A0 and A1 dead, each written before it is read. The three register-indirect sites in `FUN_400bd608` (`lea (FUN_4010f52a).l,A2; jsr (A2)`) and the recursion were not decoded to that depth **[O]**: the caller list is from the cfdb, which includes the `lea`-then-`jsr (An)` form; `tools/coldfire/refscan.py` was not run on the decoded image **[O]**.

## 3. What the shim does [D]

The shim (`modules/core/adapters/card-guard/src/cf/hooks.s`) pushes (return address, second argument, first argument) and calls `int32_t dkcg_write(sector, bytes, ret)` or `dkcg_erase(sector, count, ret)`. 0: the displaced instructions run in the tail and the stock function continues. Non-zero: `rts` to the caller with D0 = -10, the driver's own "sector out of range" code, before the stock code has taken the ESDHC mutex or changed any state. A write refused with -10 reaches the Data API as "Failed to write to eMMC" (finding 145 section 4); the MmcFs callers test for non-zero.

The writer class is the return address: the runtime's raw write goes through `dkrt_raw_block_write` (`core/runtime` `tramp.s`), whose call of the firmware is followed by the global label `dkrt_raw_write_ret`. A caller with that return address is class `runtime`; every other one is `stock`. A flag is not used: the RTOS is preemptive, so a stock task could write while the flag was set. The patched Syntakt images were checked on the host: both hooks sit at the entries with the displaced instructions as above, the tails continue at entry + 8 and the label follows the trampoline's `jsr` (`packages/patchkit/tests/card_guard.rs`).

## 4. The stock window of the Syntakt [D]

MmcFs refuses a start above 0x17ffff and a length past `(0x180000 - sector) * 512` in `FUN_40111628`; every stock erase and the FORMAT end at 0x17ffff; the COKi copies are at 0x7a000..0x7dfff (finding 145 section 2). The `stock` row is `[0, 0x180000)`; the 256 MiB up to the store at 0x200000 are a guard band. `patchkit gen` checks that every `[card]` item of a device lies inside one stock row, so a too-tight row is a build error.

## 5. Image budget [D]

The Syntakt `device` set with the guard (patchkit 2026-11-21): container 2,079,872 bytes against `container_max` 2,096,640 (16.4 KB free); without the adapter 2,079,264 (17.0 KB free). The bootstrap margin stays at its floor of 8.0 KB (the stream is made with the floor, finding 152). The runtime grew by 340 bytes of code and 12 of bss, the adapter is 362 bytes of code, 100 of rows and 20 of bss.

## Not covered **[O]**

The OS-upgrade finale's raw controller writes (the COKi copies at 0x7a000/0x7c000 from a constant table) and the CMD6 setup writes of the card's EXT_CSD do not pass the two entries. A hook on the command issue would see them; it is a second design.
