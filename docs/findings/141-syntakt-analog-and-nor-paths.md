# Syntakt analog, FPGA and NOR paths in MAIN

All claims are **[D]** (read once in the decompiled MAIN of 1.42 and the cfdb datarefs) unless marked **[O]** (open). Image: `st-1.42`.
DT2 1.17 and DN2 1.12 are compared in the last section. Purpose: which windows a content module must never touch
(pack-time guard, `[protect]` in `devices/*.toml`). Context: [70](70-syntakt-1-42.md), [136](136-syntakt-peer-cpu.md).

## Result in one table (Syntakt MAIN)

| Window | Meaning | Who writes it at run time |
| --- | --- | --- |
| FlexBus CS0, base `0x10000000`, 128 KiB | bridge to the peer CPU (the peer drives the sound engine) | boot peer link, per-frame eDMA |
| FlexBus CS1, base `0`, 128 KiB | PCB revision byte (and unknown else) | read only: boot task, self-test |
| FlexBus registers `0xFC008000` | CSAR/CSMR/CSCR of CS0 and CS1 | `FUN_400824ea`, once at boot |
| DSPI `0xFC05C000` (DSPI0 in the RM) | SPI NOR flash | the NOR driver `0x40116xxx-0x40117bxx` |
| DSPI `0xFC03C000` (DSPI1 in the RM) | frame bus: PCS1 register writes and a 0x830-byte eDMA frame per audio block; FPGA bitstream push at boot | `vector_191_handler` family, `FUN_400bcb4e` |
| DSPI `0xEC038000` (DSPI2, slave) | peer image stream and boot | `FUN_40082bc0`, `vector_149_handler` |
| GPIO `0xEC094000-0xEC0940FF` | chip selects, doorbell, FPGA INIT/DONE, resets | many small helpers |
| eDMA TCDs `0xFC0451E0`, `0xFC0455E0` | the two frame streams above | `FUN_401020f4`, `vector_191_handler` |

## FlexBus window at 0x10000000: the peer link, not the analog voices

- Setup `FUN_400824ea` (called from the boot thread `FUN_40082578` at 0x40082582): `_DAT_fc008000 = 0x10000000` (CSAR0), `_DAT_fc008004 = 0x10001`
  (CSMR0: valid, BAM 1 = address bit 16 ignored, so 128 KiB), `_DAT_fc008008 = 0x100780` (CSCR0); `_DAT_fc00800c = 0` (CSAR1), `_DAT_fc008010 = 0x10001`
  (CSMR1, 128 KiB at 0), `_DAT_fc008014 = 0xfd80` (CSCR1). It also sets GPIO pins (`0xEC094048..4A`, `0xEC094064..67`) and `DAT_fc04002d = 2`.
- cfdb datarefs into `0x10000000..0x1000ffff` are 12, all in three functions: `FUN_400828cc` (poll callback, reads `+8`), `FUN_40082f90`
  (sample chunk upload: `+2 = 2`, `+0xc` sequence, 2 KB copy to `+0x800`) and `vector_68_handler` (hello magic at `+0`, `+2`, `+4`, command `+2`, `+0xc`).
  That is the mailbox of [70](70-syntakt-1-42.md) "Peer link".
- Per audio block `vector_191_handler` (0x400a38ce, and the three other copies) arms eDMA TCD `0xFC0455E0` twice: read `0x500` bytes from
  `0x100001b0` to SRAM `0x80003d50` (lines 791-796, 1095-1100) and write the SRAM frame at `0x80002000` to `0x10000000` (lines 923-928);
  `DAT_fc04401e = 0x2f` starts channel 47. So the window carries sample data, the frame to the peer and the peer's answer.
- No writer of the analog voice hardware was found in MAIN. The analog voices are behind the peer or the board, not behind an MAIN address
  that this scan can name **[O]**. What the bridge FPGA does with the window is not visible **[O]**.

## FlexBus CS1 at 0: PCB revision

- CS1 maps 128 KiB at address 0 (CSAR1 = 0). MAIN only reads it (byte 0: [70](70-syntakt-1-42.md) "Emulated NOR"). No store to it was found. A module
  reading or writing low addresses reaches this device bus, not RAM: low addresses are protected for relocations (literal 0..0x1ffff is too common to scan).

## DSPI 0xFC03C000 and GPIO

- `FUN_400a37d2` (audio block start) and `FUN_400a3870` (block end) write `_DAT_fc03c034` (PUSHR, PCS1) through `FUN_400a1d34(index, value)`:
  `(2,1) (3,0xf8) (4,0x76) (7,0xff) (5,4) (2,0)` at the start, `(7,0)` at the end. The index argument is not sent by the decompiled code, so the
  frame layout is **[O]**. Who listens on PCS1 is **[O]**.
- `FUN_4010223e` arms eDMA TCD `0xFC0451E0` (source `0x800051ec + n * 0x830`, destination PUSHR `0xFC03C034`; TCD built by `FUN_401020f4`) and starts channel 15
  (`MCF5441X_MMIO::EDMA_SERQ = 0xf`), eight call sites in the four `vector_191_handler` copies.
- `FUN_400bcb4e` is the boot-time FPGA load: DSPI MCR `0x80030c00`, INIT/DONE on GPIO `0xEC09401C/D`, bitstream halfwords pushed as `0x90000000 | w`
  (2-byte steps from `0x43be315c`). No caller is known (reached by a vector or as a thread entry); `FUN_400bcf7a` is its twin with the eMMC and NOR branch.
- GPIO `0xEC094xxx` helpers set or clear single pins: `FUN_40114038` (port 6 bit 2, the peer pin), `FUN_401140a2` (port 6 bit 0), `FUN_4011407e` (port 7 bit 7),
  `FUN_4011405c` (port 8 bit 1), `FUN_40114288` (port 6 bit 4: the NOR chip select, argument 1 selects, 0 releases). Roles of the others: **[O]**.

## SPI NOR writes

- Hardware: DSPI `0xFC05C000` with a GPIO chip select (`FUN_40114288`, then `FUN_40114358`). Mutex `0x444db2d0`. Chip id read `FUN_40117360` (command byte 0x9f path)
  and sized by `FUN_40117448` (pages 0x100, sectors 0x10000; Spansion 0x40000 sectors).
- Primitives: write enable `FUN_401163f4` (cmd 0x06), status `FUN_401164d0`, clear status `FUN_40116524` (0x30), page write `FUN_40116ea2`
  (WREN, cmd and bytes, poll busy), sector erase `FUN_4011719a` and `FUN_4011721c`, read `FUN_40116d7a` (thunk `0x40117a6e`).
- Public wrappers: **program** `FUN_40117a72(addr, len, buf)` (splits at the page size); **erase** `FUN_40117aee(addr)` and `FUN_40117b58(addr)`
  (one 0x40000 block; split when the sector is smaller).
- Callers of program: `FUN_4006eb7e` (calibration record), `FUN_4011aad4` (OS write), `FUN_4007fcfc`, `FUN_4007fdd8`, `FUN_4007feb4`, `FUN_4007ff90`,
  `FUN_4008006c`, `FUN_40080646` (self-test and updater steps). Callers of erase: the same plus `FUN_400bd608`.
- **Calibration record (NOR 0x300000, "RBOZ")**: `FUN_4006eb7e(0x300000)` checks the RAM copy at `0x4060d970` (magic `0x52424f5a`, `+4 = 0x22d30`, `+8 = 1`),
  stores the checksum (`FUN_4006eb5c`) at `+0xc`, erases the block and writes `0x22d40` bytes. Callers: `FUN_4006ebec` (called from `FUN_4006ed76`, whose callers
  include `FUN_40080646`, `FUN_4008ee30`, `FUN_4009176c`, `FUN_400918e6`), `FUN_4006ec48` (from `FUN_4009176c`, itself called from `FUN_4006b2de` and `FUN_4006b756`,
  the analog calibration menu: "PERFORMING WARM-UP CALIBRATION" is used by `AnalogCalibrationMenuView::vfunc_5` at 0x4006b434). `FUN_4006ec6e` restores `0x300000`
  from the copy at NOR `0x340000` (reads `0x22d40` bytes, no caller known).
- **OS area (from 0x80000)**: `FUN_4011aad4` (string "Writing Flash" at 0x4026ca4f): erases from `0x80000` in `FUN_40117bd6()`-sized steps, writes the image in `FUN_40117bc2()`
  steps, prints "Success, Rebooting", resets (`DAT_ec090000 = 0x80`). Its only caller `FUN_400e1a78` has no known caller (an OS-upload path entered by vector or message).
- `FUN_400bd608` (from `FUN_400bd74e`, from boot `FUN_40082578`) copies a "COKi" persistent arena (0x3e6714 bytes) from one of three NOR offsets (table `0x401c1490`)
  to the card at `0x7a000` and then erases the NOR copy ([138](138-syntakt-runtime-facts.md)).
- The self-test `FUN_40080646` also writes NOR (erases `0x3c0000` via `FUN_40117672`, `FUN_40117794` is chip erase command `0xe4`, `FUN_4011794c` writes the status register). **[O]**: which cases a user can reach.

## DT2 1.17 and DN2 1.12 (brief)

- Same NOR driver on DSPI `0xFC05C000` (20 functions on each device). OS write `FUN_4013ac54` (DT2, string "Writing Flash" at 0x4025b01e) and `FUN_40139104` (DN2, 0x40232e5f):
  erase from `0x80000` (`FUN_401373e6`), program (`FUN_40137cbe`); called from `FUN_40107682` on DT2. Other program callers on DT2:
  `FUN_400ca63a` family (the 0x200-byte records at NOR `0x380000` and `0x3c0000`), `console_task_entry` `FUN_400caff0`.
- Neither device has the string "RBOZ" or a `0x300000` record writer; no analog calibration.
- FlexBus data window on both: `0x8C000000..0x8C00000F` (sample pages to link port 0: `FUN_400ccf04`, `FUN_400cd0d8`; DN2 `FUN_400cf368`, `FUN_400cf2dc`).
- DSPI2 `0xEC038000` is the frame link to the SHARC; DSPI `0xFC03C000` is used by four functions (`vector_182_handler`, `FUN_400cd5ee`, `FUN_400cc81c`, `FUN_400cc9c8` on DT2); role **[O]**.

## What the guard uses

`[protect] windows` in `devices/*.toml` lists the windows above that are **[D]**. See `sdk/README.md` "Modules never touch hardware".
