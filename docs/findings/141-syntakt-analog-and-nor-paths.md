# Syntakt analog, FPGA and NOR paths in MAIN

All claims are **[D]** (read once in the decompiled MAIN of 1.42 and the cfdb datarefs) unless marked **[V]** (checked against image bytes by a second agent) or **[O]** (open). Image: `st-1.42`.
DT2 1.17 and DN2 1.12 are compared in the last section. Purpose: which windows a content module must never touch
(pack-time guard, `[protect]` in `devices/*.toml`). Context: [70](70-syntakt-1-42.md), [136](136-syntakt-peer-cpu.md).

## Result in one table (Syntakt MAIN)

| Window | Meaning | Who writes it at run time |
| --- | --- | --- |
| FlexBus CS0, base `0x10000000`, 128 KiB | bridge to the peer CPU (the peer drives the sound engine) | boot peer link, per-frame eDMA |
| FlexBus CS1, base `0`, 128 KiB | PCB revision byte (and unknown else) | read only: boot task, self-test |
| FlexBus registers `0xFC008000` | CSAR/CSMR/CSCR of CS0 and CS1 | `FUN_400824ea`, once at boot |
| DSPI `0xFC05C000` (DSPI0 in the RM) | SPI NOR flash | the NOR driver `0x40116xxx-0x40117bxx` |
| DSPI `0xFC03C000` (DSPI1 in the RM) | analog voice control: a 1000-byte frame per audio block (eDMA ch 15, PCS0), PCS1 byte writes every 70 blocks; FPGA bitstream push at boot ([143](143-syntakt-analog-frame.md)) | tick `vector_191_handler`, `FUN_401021b4`, `FUN_400bcb4e` |
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
  `0x100001b0` to SRAM `0x80003d50` (lines 791-796, 1095-1100) and write the SRAM frame at `0x80002000` (`0x1b0` bytes, up to `0x800021af`; the parameter block at `0x800021b0` is not sent) to `0x10000000` (lines 923-928);
  `DAT_fc04401e = 0x2f` starts channel 47. So the window carries sample data, the frame to the peer and the peer's answer.
- The analog voices are not behind this window. MAIN drives them through DSPI `0xFC03C000` (control frame, [143](143-syntakt-analog-frame.md)). What the bridge FPGA
  does with the window besides the peer link is not visible **[O]**.

## FlexBus CS1 at 0: PCB revision

- CS1 maps 128 KiB at address 0 (CSAR1 = 0). MAIN only reads it (byte 0: [70](70-syntakt-1-42.md) "Emulated NOR"). No store to it was found. A module
  reading or writing low addresses reaches this device bus, not RAM. The guard checks a window below 0x10000 against **absolute-symbol relocations
  only** (a relocation against a symbol with a fixed address, plus its addend). It does not check literals, data words, addends or code operands
  against it: a number below 64 KiB is an ordinary offset or constant, and checking them would refuse every module. So a module that builds
  a low address in a register is not caught.

## DSPI 0xFC03C000 and GPIO

- `FUN_400a37d2` (audio block start, every 70th block) and `FUN_400a3870` write 8-bit PUSHR words on PCS1 (`FUN_400a1d34(index, value)` sends only the value):
  `01 f8 76 ff 04 00`, later `00`. The listener is **[O]** ([143](143-syntakt-analog-frame.md)).
- `FUN_4010223e` arms eDMA TCD `0xFC0451E0` (source `0x800051ec + n * 0x830`, destination PUSHR `0xFC03C034`; TCD built by `FUN_401020f4`) and starts channel 15
  (`MCF5441X_MMIO::EDMA_SERQ = 0xf`), eight call sites in the four `vector_191_handler` copies. The buffers are filled by `FUN_401021b4` from the analog control frame
  `0x80003160` (502 PUSHR words, PCS0): [143](143-syntakt-analog-frame.md) gives the writer chain and the layout. The chain to eDMA ch 15, TCD `0xFC0451E0`, DADDR `0xFC03C034`, CITER and BITER `0x206` is **[V]**.
- `FUN_400bcb4e` is the boot-time FPGA load: DSPI MCR `0x80030c00`, INIT/DONE on GPIO `0xEC09401C/D`, bitstream halfwords pushed as `0x90000000 | w`
  (2-byte steps from `0x43be315c`). No caller is known (reached by a vector or as a thread entry); `FUN_400bcf7a` is its twin with the eMMC and NOR branch.
- GPIO `0xEC094xxx` helpers set or clear single pins: `FUN_40114038` (port 6 bit 2, the peer-link doorbell pin), `FUN_401140a2` (port 6 bit 0), `FUN_4011407e` (port 7 bit 7),
  `FUN_4011405c` (port 8 bit 1), `FUN_40114288` (port 6 bit 4: the NOR chip select, argument 1 selects, 0 releases). Roles of the other pins are **[O]**: [143](143-syntakt-analog-frame.md) "GPIO" labels them from their callers only (no analog enable or mute found).

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
  steps, prints "Success, Rebooting", resets (`DAT_ec090000 = 0x80`). Its only caller `FUN_400e1a78` is the body of the "OS Upgrade" background job that the OS UPGRADE screen starts (`FUN_400e1e1c`); [146](146-syntakt-os-upgrade-receiver.md) maps the receiver, the length bound and which callers can reach `0x300000..0x37ffff` (only `FUN_4006eb7e`, and the OS write with a forged length word).
- `FUN_400bd608` (from `FUN_400bd74e`, from boot `FUN_40082578`) copies a "COKi" persistent arena (0x3e6714 bytes) from one of three NOR offsets (table `0x401c1490`)
  to the card at `0x7a000` and then erases the NOR copy ([138](138-syntakt-runtime-facts.md)).
- The self-test `FUN_40080646` also writes NOR (erases `0x3c0000` via `FUN_40117672`, `FUN_40117794` is chip erase command `0xe4`, `FUN_4011794c` writes the status register). **[O]**: which cases a user can reach.

## DT2 1.17 and DN2 1.12

Addresses are DT2 1.17 with the DN2 1.12 pair after a slash. Source: `datarefs` of cfdb (`image` `dt2-1.17-emac`, `dn2-1.12-emac`) by target
range, and `tools/coldfire/refscan.py` over `section_3_MAIN_OS.bin` (96.8% and 96.6% byte coverage). All **[D]**.

| Window in `devices/digitakt-ii.toml`, `digitone-ii.toml` | Evidence | Role |
| --- | --- | --- |
| DSPI `0xFC05C000` (page) | 19 functions touch `0xFC05C000-0xFC05CFFF` on each device, 14 of them are NOR primitives that call the GPIO helper below (`FUN_401367e6`, `FUN_401368ae`, ... / `FUN_40134bce`, ...), one is `FUN_400effd8` (/ `FUN_400f19b4`). The OS write `FUN_4013ac54` (/ `FUN_40139104`, string "Writing Flash") erases from `0x80000` and programs. | SPI NOR flash controller |
| GPIO `0xEC094000-0xEC0940FF` | Each of the 14 NOR primitives calls one helper first, `FUN_401344dc(1)` (/ `FUN_401328c4`), then writes DSPI `0xFC05C0xx`. The helper touches `0xEC094010`, `0x1C`, `0x28`, `0x4E`, `0x51`: argument 1 writes `0xEC09401C = 0x10` and sets `0xEC09404E` bits; argument 0 writes `0xEC094028 = 0xEF`. This is the same shape as the Syntakt's NOR chip select `FUN_40114288` above (the pin number is not read here **[O]**). References to the block range `0xEC09400C-0xEC094070` in 40+ functions on each device (boot, frame link, UI helpers). | chip select of the NOR flash (one pin), the other pins are the firmware's |
| DSPI2 `0xEC038000` (page) | Two functions: `FUN_400cd0d8` (/ `FUN_400cf4b0`, 8 references from `+0x00` to `+0x34`: MCR `0x80010c00`, CTAR `0xFA010000` and `0xBA010000`, PUSHR) and `FUN_400cd420` (/ `FUN_400cf922`, 7 references). `FUN_400cd0d8` also sets GPIO and the FlexBus bytes below. | SHARC program load at boot and the frame link ([04](04-coldfire-dsp-link.md) "FlexBus `0x8c000000`..." section) |
| DSPI1 `0xFC03C000` (page) | Four functions on each device: `FUN_400cc7e8`, `FUN_400cc81c` (MCR `\|= 0xC00`, PUSHR `0x28000000`), `FUN_400cc9c8`, `FUN_400cd5ee` (/ `FUN_400cec1c`, `FUN_400cec50`, `FUN_400cedfc`, `FUN_400cf7f8`), offsets `+0x00` to `+0x34`. What the peer on its chip select is **[O]**. | a DSPI controller that only the firmware drives; role open |
| FlexBus `0x8C000000-0x8C00000F` | `FUN_400ccf04`, `FUN_400ccf90`, `FUN_400cd0d8`, `FUN_40001a10` (/ `FUN_400cf2dc`, `FUN_400cf368`, `FUN_400cf4b0`, `FUN_40001a10`) reference offsets `0x0`, `0x2`, `0x4`, `0x6`, `0xA` only (request/ack handshake bytes and a reset strobe, [04](04-coldfire-dsp-link.md)). No reference to `0x8C000010-0x8CFFFFFF` on either device. | the SHARC side device behind FlexBus |

- The extent of the FlexBus device is **not** evidenced beyond the 16 bytes above: `refscan.py` finds 0 references to the FlexBus registers
  `0xFC008000-0xFC008FFF` (CSAR/CSMR/CSCR) in MAIN on both devices, and cfdb has none in `0xFC000000-0xFC00FFFF`. The chip select is set before MAIN
  runs (the Syntakt sets it in MAIN, `FUN_400824ea` above). So the DT2 and DN2 windows cover `0x8C000000-0x8C00000F` only **[O]**: a wider
  window needs the chip select setup of the boot loader.
- Neither device has the string "RBOZ" or a `0x300000` record writer; no analog calibration.
- Other NOR users on DT2: `FUN_400ca63a` family (the 0x200-byte records at NOR `0x380000` and `0x3c0000`), `console_task_entry` `FUN_400caff0`, `FUN_40107682`
  (calls the OS write).

## What the guard uses

`[protect] windows` in `devices/*.toml` lists the windows above that are **[D]**, each device with its own evidence (Syntakt: the tables above; DT2 and DN2:
the last section). A window below 0x10000 is checked against absolute-symbol relocations only. See `sdk/README.md` "Modules never touch hardware".
