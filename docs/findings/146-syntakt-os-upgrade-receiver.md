# Syntakt OS 1.42: the OS-upgrade receiver, what it checks, where it writes, and the calibration record

Marks: **[V]** checked against image bytes by a second agent; **[D]** read once, static, in the decompiled MAIN of `st-1.42` (cfdb) and the bootstrap strings; **[O]** open. No emulator, no device. Compared with DT2 1.17
(`dt2-1.17` cfdb) where stated. Context: [01](01-container-and-patching.md) "Recovery", "NOR flash map", [70](70-syntakt-1-42.md),
[141](141-syntakt-analog-and-nor-paths.md), [142](142-syntakt-sample-storage.md) "Dispatch", `docs/MIDI-SYSEX-RPC.md` section 7, finding 75 (the RPC route on DT2/DN2).

## Result

- MAIN 1.42 has the same OS-upgrade receiver as DT2 1.17: one state machine (`OsUpgradeState`, parser `FUN_4010cf4a`, DT2 `FUN_4012b83e`), fed by two doors, the
  MidiRpc `OsUpgradeStart/Write/End` (types `0x50/0x51/0x52`) and the raw SysEx stream of the OS UPGRADE menu. Same code shape, same framing, same checksum. **[D]**
- It checks only checksums: the per-message checksum, the end counters and one sum over the whole image. **There is no signature and no HMAC.** An image
  rebuilt by patchkit (container without trailer slot, finding 70) passes **[V]**.
- It writes NOR from `0x80000` upward and nowhere else. The length of that write comes from the image's own length word, and nothing on the device bounds it
  against the calibration record at `0x300000`. Two bounds exist only by construction: the RAM stage of 2 MiB and the host (patchkit). A rule is needed (below).
- The same workbench code can flash the Syntakt with device data only. What differs is data, not code (section 5). The RPC route was never run against a real
  Syntakt **[O]**.
- Recovery: the bootstrap (NOR `0x40000`) is not written by this path and contains the STARTUP MENU, "OS UPGRADE" and "READY TO RECEIVE". The key that enters it
  on a Syntakt is not read **[O]** (DT2: hold FUNC at power-on).
- The stop rule did not trigger: the receiver is mapped. Two links are inferred from structure, not traced instruction by instruction (section 3, marked).

## 1. The two doors

| Door | Entry | Evidence |
| --- | --- | --- |
| MidiRpc | Request classes `MidiRpcOsUpgradeStart/Write/EndRequest` are built by the message factory `FUN_4011ac50` for type bytes `0x50`, `0x51`, `0x52` (`case 'O'`, `'P'`, `'Q'` of `type - 1`) and handled in the dispatcher `FUN_40108fbe` (the only code with the typeinfo references, finding 142) | **[D]** |
| Raw SysEx | `FUN_401042ec` has no caller. It is a handler in a SysEx descriptor table: the table at `0x402d5800` (referenced by the record at `0x4024237c`, the same record kind as the `0x10` RPC record at `0x40242368`) holds `FUN_401042ec` for the command bytes `0x7e` and `0x7f` (data words at `0x402d59f8` and `0x402d59fc`) and the default `FUN_40103a26` for all others. It returns at once while `_DAT_444852d4` is 0, otherwise hands the message to `FUN_40105870` and posts the result. The OS UPGRADE screen is the class `OsUpgradeMenuView` (typeinfo `0x4023c858`); its message handler is the function named `ConfirmWindow::ctor_dtor` at `0x400e220c` (a mis-named vfunc) | **[D]** for the table words; that the record's device byte is `0x16` is **[O]** (the first word of the record reads `0xffff0000`, not decoded) |

- Write (RPC): the request carries a CRC-32 (`FUN_4011d9b4`: init `0xffffffff`, the DT2 CRC), a length and a stream offset. The handler compares the CRC with
  the data, then calls `FUN_4010cf4a` with the bytes. A wrong CRC gives status 2. This is the DT2 layout in `flash_rpc.rs`. **[D]** for the CRC and the call;
  the status codes are the DT2 ones **[O]**.
- Start (RPC): `FUN_4010cc6a` posts the state change to the UI thread ("Brain") and returns the "open" flag; the request's flag byte is stored in the state at `+0x20`
  by `FUN_4010c8b2` (1 = no confirm needed). End: `OsUpgradeState::vfunc_4` stores the commit byte at `+0x21`; `vfunc_5` reads it. **[D]**
- Both doors feed `FUN_4010cf4a(state, buffers, size)`, which looks for `F0 ... F7` frames in the bytes and calls `FUN_40105870(stage, frame, length)` per frame. So
  a chunk of the RPC Write is a slice of the same SysEx stream that the stock `.syx` file holds. **[D]**

## 2. Framing and checks (`FUN_40105870`) **[D]**

The stage object is at `0x403d10b0` (`FUN_40105868` returns it; the anchor `cf.midi.site.os_upgrade_state_lit` points into that function's literal). Fields:
`+0` state byte (0 idle, 1 receiving, 2 all messages in, 3 image ready), `+1` stream byte, `+4` total word, `+8` message count, `+0xc` messages taken, `+0x10`
sequence, data from `+0x20`.

- A frame must start with `F0` and end with `F7`, else status 2 or 3. The device byte of the envelope is not compared here.
- Start message (byte 7 = 1): byte 8 must be `0x11` (the OS stream byte, the value in `devices/syntakt.toml`), else status 5. It stores the total (bytes 9..) and the
  message count (bytes 0xc..) and sets state 1.
- Data message (byte 6 = `0x7e`): state must be 1; the sequence word must equal `+0x10`; the checksum of 0x77 bytes `sum((byte ^ (K + i)) )` with K the stream
  byte `0x11`, masked `& 0x7f`, must equal byte `0x7e` (finding 70: K = 0x11 verifies on all 20,094 data messages). Then 0x65 = 101 bytes are copied to
  `stage + 0x20 + taken * 0x65`. **The message count from the start message is not compared with the stage size**: a host that sends more than 20,763 messages
  writes beyond `0x405d10d0` **[V]** (see section 4).
- End message (byte 7 = 2): state must be 2, the stream byte, the total word and the message count must match the start message; then state 3 and status 0.
- Whole-image check: `FUN_400a5c20(stage + 0x20)` (called when status 0): `FUN_401059ec` computes the sum over `length / 4` big-endian words, `sum((i ^ word_i))`
  with i counting from 1 (finding 01 "content checksum"), compared with the word at +4; then a flash-chip rule (next paragraph). Return 1 means "accepted" (the table
  of texts at `0x40242fb8` starts with "No error" at index 1; anything else shows "Validation failed" or a table text).
- The flash-chip rule: read the JEDEC id (`FUN_40117360`); for manufacturer `0x9d` type `0x6017` and for manufacturer `0xc2` type `0x2017` the container's build number
  word (container offset 8, the ASCII "0097" of 1.42) must be greater than `0x30303430` ("0040"); any other chip is accepted. A rebuilt image keeps the header, so it
  passes. **[V]**
- There is no vendor signature, no HMAC and no version-greater rule on this path. DT2 1.17 has the same functions. **[V]**

## 3. When the flash starts, and the switch-over

- Image ready (state 3): `FUN_4010cf4a` posts a message to the OS UPGRADE view; with the flag at `+0x20` clear it shows "Waiting for confirm", otherwise it acknowledges
  at once (`FUN_4010c8dc`). The view's handler (`0x400e220c`, case `0x14`, "image ready") flashes without a question when the commit byte is set, and otherwise opens
  the confirm window "UPGRADE OS NOW?" whose callback `FUN_400e1ff2` re-reads the commit byte. This is the DT2 behaviour of finding 75 (the host's End with commit 1
  flashes, no on-device question). **[D]** for the functions and the calls; that End with commit 1 reaches `FUN_400e1e1c` on 1.42 without a key press is inferred from
  that structure **[O]** (one emulator observation settles it, plan step "one emulator pass").
- `FUN_400e1e1c` starts a background job "OS Upgrade" whose body is `FUN_400e1a78` (data references at `0x400e1ef8` and `0x400e1fa0`; the "no known caller" of
  [141](141-syntakt-analog-and-nor-paths.md) was the missing data reference). It shows "ERASING FLASH...", re-validates with `FUN_400a5c20`, then calls the writer.
- Writer `FUN_4011aad4(stage + 0x20, log)`: with `L` the length word and `S`, `P` the NOR sector and page sizes (`FUN_40117bd6`, `FUN_40117bc2`, from the chip
  id): erase `ceil(L / S)` sectors from `0x80000` with the raw erase `FUN_4011719a`; program `ceil(L / P)` pages from stage `+0x28` to `0x80000` with `FUN_40117a72`;
  print "Success, Rebooting", wait 3 s (`FUN_401162f8(3000000)`), drain the console, `clr.l 0x48000000`, write `0x80` to `RCM.RCR` (soft reset). **[V]**
- The bootstrap (NOR `0x40000`, never written here) then loads MAIN from the container at `0x80000`. On DT2 the bootstrap checks only the container magic and the
  header word (`ELE3`, `0x2b`; DN2 `0x34`), no checksum or HMAC of MAIN (finding 01). The Syntakt bootstrap was not read for this check; the container header word
  is `0x4a` and the bootstrap's compare value is **[O]** (unchanged by patchkit, so it passes whatever it is). The bootstrap keeps its own version word (`0x0400` at
  the start of section 2, DT2 `0x0200`).

## 4. Where it can write: the stage and the calibration record

NOR map used here (finding 01, 141; Syntakt flash map itself **[O]**): bootstrap `0x40000`, OS container `0x80000..`, calibration record `0x300000` (RBOZ), its backup
`0x340000`, records `0x380000` and `0x3c0000`.

- The stage data area is `0x403d10d0..0x405d10d0`, exactly **2 MiB** (`0x200000`): the next global symbol of the cfdb is `DAT_405d10d0`. DT2 1.17: stage
  `0x403a0ed0`, data `0x403a0ef0`, next symbol `0x405a0f64`: the same 2 MiB **[V]**.
- Stock container is 2,029,486 bytes decoded; `out/syntakt/patched/Syntakt_OS1.42-expansion.syx` (the dry run of finding 137, core/runtime and core/expansion) decodes
  to a content of 2,000,880 bytes (finding 137). A patched image of the full emulator set must fit the stage in whole messages of 101 bytes, and stay below `0x280000` for
  the NOR. The tighter bound is the stage: the stage is `0x200000` = 2,097,152 bytes, and 2,097,152 / 101 = 20763 whole messages. 20763 messages of 101 bytes = **2,097,063 bytes** fit the 2 MiB stage; rounded down to a `0x200` page: `0x1FFE00`. Headroom today is about 96 KB.
- **Nothing on the device compares the length word with the received bytes or with 2 MiB.** The writer erases `ceil(L / S)` sectors from `0x80000`: a length of
  `0x280001` or more reaches `0x300000` and erases the calibration record. The sum check reads `L / 4` words from RAM beyond the stage if `L` is larger than what was
  received, so only a forged image whose header, length and sum agree gets through; patchkit never makes one. The OS write path therefore cannot touch
  `0x300000..0x37ffff` for an honest image, and can for a forged length word **[V]**.
- `container_max = 0x1FFE00` in all three device files and the port files: 20763 messages of 101 bytes = 2,097,063 bytes fit the 2 MiB stage; rounded down to a `0x200` page: `0x1FFE00` (below the NOR bound `0x280000`). DT2 1.17 and DN2 1.12 stage verified the same
  (DT2 `0x403a0ef0`, DN2 `0x4038bee0`, next symbols 2 MiB on); they have the same unbounded writer, with their own NOR map.
- Other NOR users of MAIN, each with its address arguments **[V]** (callers of the wrappers `FUN_40117a72` program, `FUN_40117aee` and `FUN_40117b58` erase):

| Caller | Addresses | Purpose |
| --- | --- | --- |
| `FUN_4006eb7e(addr)` | argument `0x300000` (from `FUN_4006ebec`, `FUN_4006ec6e`) or `0x340000` (from `FUN_4006ec48`, `moveq 0x34; swap`); programs `0x22d40` bytes after erasing one `0x40000` block | the calibration record and its backup |
| `FUN_4007fcfc`, `FUN_4007fdd8`, `FUN_4007feb4`, `FUN_4007ff90` | `0x380000` | the 0x200-byte tagged record; they also (un)protect `0x380000` and `0x3c0000` |
| `FUN_4008006c` | `0x3c0000`, `0x380000` | the serial number record |
| `FUN_40080646` (self-test / updater screen) | `0x40000` and `0x80000` loops over a length from RAM, `0x3c0000` | the factory test and the bootstrap or OS write from the self-test; reached only from that screen **[O]** |
| `FUN_400bd608` | NOR offsets of the table at `0x401c1490`: `0x400000`, `0x800000`, `0xc00000` | the COKi arena copy to the card, then erase of that NOR copy |
| `FUN_4011aad4` | `0x80000` and up | the OS write above |

  The raw primitives have no other callers: `FUN_40116ea2` (page program) only through `FUN_40117a72`, `FUN_4011719a` and `FUN_4011721c` (sector erase) only through the
  wrappers and `FUN_4011aad4`. The protection commands `FUN_40117672`, `FUN_40117794`, `FUN_4011794c` are called from the record writers and the self-test only.
  So `0x300000..0x37ffff` is written only by `FUN_4006eb7e` (the calibration menu path, [141](141-syntakt-analog-and-nor-paths.md)) and, with a forged length, by the OS write.
  **[O]** Indirect or table calls to the NOR wrappers are not excluded (refscan not run).

### The patch set modules cannot reach the NOR

- The port file `devices/ports/st-1.42.toml` anchors no NOR function: its names for flash are `cf.flash_cave` (RAM cave) and `cf.midi.site.os_upgrade_state_lit`
  (read only, by `os_upgrade_active`, used to stop the runtime's resets during an OS upgrade). `cf.storage.fn.block.read/write` are the eMMC (`FUN_4010f612`, `FUN_4010f7f8`).
  A module reaches firmware code only through named anchors, so `core/runtime`, `core/ui`, `core/digikit-menu`, `core/health` and `core/input` have no symbol for `FUN_40117a72`
  and the others.
- A search of `modules/core` for DSPI `0xFC05C000`, `RBOZ`, `0x300000`, `0x340000`, NOR or flash names finds nothing. The MMIO literals in `modules/core` are the eSDHC base
  (profile data), the RCM reset register (`tramp.s`) and the USB controller of `core/usb-bulk` (not in the Syntakt set). **[D]** (text search; a module could still build an
  address at run time, which the review of each module's source for this device covers).
- The runtime's MIDI hook (`cf.midi.hook.rx`, mode `before`, at `0x40103f5e` inside `FUN_40103e0a`) sees raw MIDI before the SysEx descriptor dispatch **[O]** (the
  caller chain of `FUN_40103e0a` was not traced) and must pass OS-upgrade frames through untouched. The runtime handles only its own SysEx id (finding 138); a frame of the
  OS stream (byte 6 `0x7e`) is not its id **[D]** for the id rule, **[O]** for the byte-level hand-off on 1.42 (the emulator pass flashes through it).

### What `[protect]` covers

`devices/syntakt.toml [protect]` lists MMIO windows (FlexBus, DSPI `0xFC05C000` for the NOR, `0xFC03C000`, `0xEC038000`, the GPIO block). `packages/patchkit/src/protect.rs`
is called from `pack.rs:278` for content modules only ("not `core`: patched in, ours", the test at `pack.rs:397`). So:

- A content module is refused when it holds an address in the NOR controller window. That covers direct register access.
- A `core/*` module is **not** checked. For the five modules above this rests on the review in the previous section, not on a gate.
- A content module normally holds no address of a firmware NOR wrapper (`0x40117a72` and so on): it links the runtime table only. With the seven code windows in `[protect]` of `devices/syntakt.toml`, `patchkit pack` refuses a content module whose code operand, relocation or data word points into a NOR writer. Before them it was not refused. Each window is a
  whole-function range checked against the cfdb: the NOR driver `[0x401163f4, 0x40117be8)` (`FUN_401163f4` to `FUN_40117bd6`), the chip select `[0x40114288, 0x40114368)`,
  the calibration writer `[0x4006eb7e, 0x4006ecb8)`, the OS writer `[0x4011aad4, 0x4011ac1c)`, the record and serial writers `[0x4007fcfc, 0x40080180)`, the arena
  NOR erase `[0x400bd608, 0x400bd74e)` and the self-test and updater screen `[0x40080646, 0x400819f6)`. A literal address is refused; an address computed at run time is not.

## 5. The workbench route and what differs

`packages/patchkit/src/flash_rpc.rs` (the sans-io state machine; the app, the browser and the CLI drive it) sends the DT2 route: Start `0x50 {u32 length, "sysex\0", flag}`,
Write `0x51 {u32 CRC, u32 length, u32 offset, data}`, End `0x52 {commit}`, in the envelope `F0 00 20 3C 10 00`. Everything it needs is in the MAIN above:

| Item | Syntakt | Needs |
| --- | --- | --- |
| Envelope and framing | the same record `0x10` (finding 142: the Data API on `0x10` was observed on a real 1.42 unit) | nothing |
| Request types | `0x50..0x52` in the factory and dispatcher | nothing |
| CRC | CRC-32 init `0xffffffff` | nothing |
| Stream byte | `0x11` in the start message, K = 0x11 in the checksum | `[patch] stream_id = 0x11` is set; the file's own bytes carry it |
| Image check | sums only | `seal = "none"` is set |
| Chunk size | 2048 (stock file); the largest accepted is not known; the Data API gather limit on 1.42 is **[O]** | none: default stays |
| Size cap | stage 2 MiB | `container_max = 0x1FFE00` (done) |
| Hardware patch set | none: `[patch_sets]` has only `emulator` (the page says so, commit `ea0039933`) | a `device` set |

No device name is in the flash code path; the differences are the data above. What is untested: the RPC route on a real Syntakt. A first test with the stock image (restore stock)
exercises the whole path with an image that cannot be bad, and is the recommended first hardware step.

## 6. Recovery

- The bootstrap section holds "STARTUP MENU", "3 ... FACTORY RESET", "4 ... OS UPGRADE", "READY TO RECEIVE", "UPGRADE ABORTED", "BOOTSTRAP UPGRADE" (strings of the
  Syntakt section 2, the same set as DT2). **[D]** (strings only). It lies at NOR `0x40000`; `FUN_4011aad4` starts at `0x80000`, so a failed or cut OS write leaves it intact.
- Entering it: DT2 holds FUNC at power-on (the key test precedes any use of MAIN, finding 01). The Syntakt bootstrap has a key-name table with "FUNCTION"; the tested key
  and the order of the test in this bootstrap are not read **[O]**. The cheap check is on the unit itself: power on holding FUNCTION and look for the STARTUP MENU with the
  stock firmware (no write involved). Do this before flashing.
- The bootstrap's receive path is the legacy MIDI DIN upgrade. On DT2 it validates the content checksum and the HMAC; the Syntakt image has no HMAC trailer slot, so its
  bootstrap's rule is not the DT2 rule **[O]**. The stock `Syntakt_OS1.42.syx` is the file to send. A MIDI DIN interface and the file must be at hand.
- Power loss between the first erase and the last program leaves the container partly erased; the bootstrap still starts (finding 75, [O] for a fully erased table without the key).

## Open items

- The Syntakt bootstrap: the startup key, the container check (`0x4a`), what it does with an erased container.
- End with commit 1 reaching the flash start on 1.42 (one emulator observation), and the status codes of the Write reply.
- The largest accepted Write chunk on 1.42.
- Which Syntakt NOR chip is fitted (the flash-chip rule of section 2 applies only to two chips; both pass for a rebuilt image).
- Which screens reach `FUN_40080646`'s NOR writes.
