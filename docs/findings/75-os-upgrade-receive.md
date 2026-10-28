# 75. The MAIN OS upgrade receive path (DT2 1.17, DN2 1.12)

How MAIN OS takes a firmware update over MIDI: the route from SETTINGS > SYSTEM > OS UPGRADE, the validator, the flash writer, what is acknowledged and what a cut transfer leaves. Read statically with `tools/coldfire/fw.py` (one agent), and run in the emulator with `tools/emulator/os_upgrade_rig.py`. Marks: **[D]** read or measured once here (not yet checked by a second agent), **[O]** open. Addresses are DT2 1.17 / DN2 1.12; DN2 pairs are from `fw dossier --other` (same strict hash unless noted).

## Two entry routes, one receive state

| Part | DT2 1.17 | DN2 1.12 | Note |
| --- | --- | --- | --- |
| Menu entry (shows "Waiting for SysEx", opens the raw gate) | `0x4010732a` (`OsUpgradeMenuView` slot, string `0x402464e7`) | `0x40108e1e` | calls the gate setter |
| Gate setter `FUN_4012141a` -> `FUN_40120b08(1)` | `0x4012141a` -> `0x40120b08` | `0x40122dc2` -> `0x401224b0` | writes `0x44e08974` (DT2); cleared by `FUN_401209b2` at init |
| MIDI task, sysex dispatch | `0x4011f08e` -> `0x4011ecd0` (case 0) | `0x40120a02` -> `0x40120644` | queues for ports 0 and 1 both dispatch `F0` messages (DIN, USB); port 2 ignored |
| Sysex handler table: `F0 00 20 3C 14 00 <cmd>`, entry `0x4021e5a8`, table `0x402b4c88` | cmd `0x7e` (data) and `0x7f` (header, footer) -> `0x4011f4dc` (slots `0x402b4e80`, `0x402b4e84`) | handler `0x40120e50` | **[D]** |
| Raw receiver | `0x4011f4dc` (returns at once when the gate is 0) | `0x40120e50` | calls the parser |
| Stream parser `FUN_40120bd4` | `0x40120bd4` | `0x4012257c` | no hash pair; found as callee of `0x40120e50` |
| Receive state object (`FUN_40120bcc` returns it) | `0x403a0ed0` | `0x4038bec0` (`0x40122574`) | state byte, `+8` announced count, `+0xc` taken, `+0x10` next sequence, payload from `+0x20` |
| MidiRpc dispatcher with OsUpgrade Start/Write/End | `0x401246a6` (caller `0x40125464`) | `0x40125f9e` | **[D]** |
| RPC Start / Write(feed) / End-confirm | `0x4012b55e` / `0x4012b83e` / `0x4012b1d0` | `0x40129c56` / `0x40129f36` / `0x401298c8` | |
| Worker run after the last message (shows "ERASING FLASH...", validates, writes) | `0x40107682` (started by `0x40107a26`) | `0x40109144` (`0x401094e8`, no hash pair) | BgWorker "OS Upgrade" |
| Validator | `0x400d9fb0` | `0x400dbdb0` | below |
| Flash writer (erase, program, reboot) | `0x4013ac54` | `0x40139104` | only caller is the worker |
| Erase sector / program page | `0x401373e6` / `0x40137cbe` | `0x401357ce` / `0x401360a6` | |

1. **Menu route (the .syx stream).** The raw receiver accepts the plain stream only while the menu has set the gate (`0x44e08974` on DT2). At the main screen the stream is ignored: emulated, 400 messages sent with no menu, state stayed 0, nothing replied **[D]**. Both MIDI queues (DIN and USB) reach it; USB was run in the emulator, DIN not run **[D][O]**.
2. **RPC route (run in the emulator, section "The RPC route" below).** `OsUpgradeStart (0x50)`, `OsUpgradeWrite (0x51)`, `OsUpgradeEnd (0x52)` are handled in the MidiRpc dispatcher without the menu or the gate. Start opens the OS UPGRADE screen by itself. Write carries bytes of the same `.syx` stream (`FUN_4012b83e` collects them into a buffer, cuts at `F7` and calls the same parser `FUN_40120bd4` on the same state object `0x403a0ed0`). **[D]**
3. **Which route Transfer uses [D][O].** Em's rungs A and B (finding 47) were flashed with Transfer while the device showed "OS UPGRADE WAITING FOR SYSEX". The RPC route shows that same screen after Start, so that observation fits both routes. Em remembers no menu step in Transfer: this fits the RPC route, where the host's End decides, not a question on the device. No capture of Transfer exists.

## Stream format (parser, both devices) **[D]**

Messages are exactly 128 bytes `F0 00 20 3C 14 00 <cmd> ... F7`. Header `cmd 0x7f`, byte 7 = 1, byte 8 = stream id (`0x0f` DT2, `0x10` DN2; any other value returns 5, the stream is refused), bytes 9..11 start counter, bytes 12.. announced message count. Data `cmd 0x7e`: sequence number from bytes 7..9 must equal the next expected one, checksum `(key + sum((b ^ (key + i)))) & 0x7f` over 0x77 bytes must equal byte 0x7e (`key` = stream id), then 0x65 bytes at `+10` are copied to staging `state + 0x20 + taken * 0x65`. Footer byte 7 = 2: count, id, start counter and the taken count must match, then state 3. A wrong sequence number resets the state to 0 and returns 6; a bad checksum resets to 0 and returns 4; later data messages are then ignored without a reply. A new header restarts the transfer at any time. There is no bound check of the announced count against the staging size: DT2 1.17 announces 14,703 messages, DN2 1.12 18,674 (about 1.49 MB and 1.89 MB of staged payload); the staging size itself was not found **[O]**.

Each accepted message posts an event (progress `0x15` with taken and total; complete `0x14`); `OsUpgradeMenuView` handler `0x40107e28` turns them into the "Receiving" bar.

## Acknowledgement, buffering, pacing **[D]**

- **No SysEx reply on the menu route.** `0x4011f4dc` sends nothing. Emulated, DT2 and DN2: 14,703 and 18,674 messages sent, `midi.usb.out` returned 0 bytes through the whole receive and the flash write. Only the screen shows progress.
- **Buffering: RAM staging, flash only at the end.** The whole container is staged in RAM at `0x403a0ed0 + 0x20`; the flash is untouched until the worker runs. The MIDI queues hold 0x100 entries each (`FUN_40001834(queue, 0, buffer, 0x100)` in `0x401209b2`); what happens when a producer overruns a queue was not traced **[O]**.
- **Consumption, emulated DT2 1.17 (ColdFire at the 132 M instructions per second oracle clock):** with 64-byte USB chunks per 1 ms frame (default) the stream of 14,703 messages is taken without a gap in 3.9 G instructions (= 264 k per message, the wire). Fed faster (`--chunk 384 --period 4000` and `1000`) the receiver finished in 320 M instructions: about 21.8 k instructions (0.165 ms at the oracle clock) per message is the ceiling here, on a UI that is idle. 128 bytes every 16.5 k, 30 k instructions and 384 bytes every 40 k, 66 k instructions also completed. One setting, `--chunk 384 --period 16500`, stalled after 473 messages with the receiver waiting for message 474 and no further input reaching it; not explained, probably the emulator's USB injection (the daemon hands bytes in an interrupt the guest allows), not a firmware property **[O]**. DN2 1.12 at 128 bytes per 30 k instructions: 570 M instructions for 18,674 messages **[D]**. Real timing is not known: the emulator does not model the USB host or the real clock rate.
- **Slowest step: the flash, after the receive.** `0x4013ac54` erases every sector from flash `0x80000` for the container length (one erase per sector, sector size from the flash probe, default `0x10000`), then programs in pages (default 0x100 bytes) from `0x80000`; "Writing Flash", "Success! Rebooting", a 3 s wait, then the soft reset (`0x80` to `0xec090000`). Erase and program return codes are ignored: the screen says Success whatever the chips did **[D]**. Emulated: about 1.0 to 1.5 G instructions from the last message to the reset; the emulated flash has no real erase time, so this is not a hardware number.
- **Safe send scheme (menu route):** no acknowledgement exists, so pace by time. 128-byte messages, one every 2 ms (about 29 s DT2, 37 s DN2) is 12 times slower than the emulated ceiling and below the USB 1 ms frame limit; the receiver needs no pause for the flash because that starts only after the last message. Check the count and the footer on the host (14,703 / 18,674 messages, the last one `cmd 0x7f`, byte 7 = 2). An ack-driven scheme needs the RPC route, which answers every Write (section "The RPC route"; run on a real DT2 since 2026-10-29).

## Validation **[D]**

`FUN_400d9fb0` (DT2 1.17), run on the staged container `state + 0x20`:

1. content checksum `FUN_40120d50` (sum of `(index ^ word)` over the container words, compared with the word at `+4`): fails -> 3
2. build floor: `word at +0x10 < 0x30303630` ("0060") -> 6 "Unsupported downgrade" (same floor as the 1.16 `006/` compare)
3. HMAC-SHA256 trailer `FUN_400d06ec` (key derived from the string "Master Overdrive"): fails -> 4
4. else 1.

DN2 1.12 `0x400dbdb0`: checksum `0x401226f8`, HMAC `0x400d2bc4` (key from "Multiplier"), **no build floor**. The product is separated by the HMAC key and by the stream id byte (`0x0f`, `0x10`), not by a product-byte compare in the validator. A DT2 image on a DN2 therefore fails the parser at the header.

When it runs: the worker `0x40107682` shows "ERASING FLASH..." and validates first (`FUN_400d9fb0`), then `0x4013ac54` validates a second time before the first erase. The RPC feed `0x4012b83e` validates once more when the last message arrives (before "Waiting for confirm"). On failure the view shows "Missing data!" (no container), "Checksum error!" (3 and 4), "Downgrade not possible" (6), "No power adapter" (5) (`0x40107dc8`; the RPC status strings are the table at `0x4021fbb4`). Nothing is erased on a failure.

**YES.** On the menu route the confirm dialog "UPGRADE OS NOW?" exists (`0x40107e28`, event `0x14`) but is shown only when the delegate's slot 3 returns true; the delegate (`OsUpgradeDelegate`, `0x401cf6b2`) returns 0 and slot 5 returns 1, so no YES is asked. Emulated DT2 and DN2: after the last message the screen goes to "ERASING FLASH...", then "SUCCESS! REBOOTING", then the reset, with no key pressed (the YES the rig sends arrives during the erase and changes nothing) **[D]**. NO while receiving asks "CANCEL OS UPGRADE?". Hardware may differ if the device's delegate is not the base class (no derived class exists in the RTTI **[D]**).

## A cut transfer **[D]**

- Stream stops before the footer: the state stays 1 or 2, the screen stays on "Receiving", nothing is validated, erased or written. Emulated with 300 messages: state 1, taken 299 of 14,703, `Receiving`; a new header message restarts cleanly. The only flash writer is `0x4013ac54` (one caller, the worker), reached only after state 3.
- Cable pulled, or power lost, in the flash phase: the data is already in RAM, the cable no longer matters; power loss between the first erase and the last program leaves the container area `0x80000..` partly erased. The bootstrap (`0x40000`) is never written by this path, so the FUNC boot menu and the DIN OS UPGRADE stay usable (finding 01, "Recovery", emulated with a faulting MAIN OS). What the bootstrap does with a fully erased container table at boot without FUNC was not run **[O]**.

## Emulator run **[D]**

`uv run python -m tools.emulator.os_upgrade_rig dt2|dn2 --binary BIN --ready READYDIR --out DIR [--chunk N --period N --messages N --yes --no-open --limit N]` boots the stock image from the ready snapshot (zero DSP), taps SETTINGS > SYSTEM > OS UPGRADE, streams the stock `.syx` through `midi.usb.in` (the firmware's SysEx assembler), samples the state words, `midi.usb.out` and the OLED. Used with an immutable copy of the prod daemon (`digi-emud-74a9fcf6ff41`). Results: DT2 1.17 and DN2 1.12 both reach complete (state 3), erase, write, "SUCCESS! REBOOTING", reset. Replies cannot exist (the firmware sends none).

### Flash model and the shared sender (2026-10-28) **[D]**

The emulator did not model the flash writes: erase and program went to an unmodelled QSPI controller and the page was lost. `elektron-native-boot` now takes over MAIN OS's sector erase and page program (found by their first bytes, unique in DT2 1.16/1.17 and DN2 1.11/1.12; DT2 1.17 `0x401373e6` and `0x40137cbe`, DN2 1.12 `0x401357ce` and `0x401360a6`) when the address lies in the container area `0x80000..0x380000`; any other write (the settings records, the bootstrap) runs the firmware's own code as before. Erase sets a 0x10000 unit (the writer's default when the flash probe, not modelled, tells nothing) to 0xFF; program copies the page. The daemon has `flash.status` (erases, programs, bytes) and `flash.read {offset, length}`. At the soft reset the controller boots what the guest wrote: the flash container becomes the `.syx` again (`patchkit::flash::image_from_flash`, the same framing, the HMAC checked) and cold boots.

`patchkit flash STOCK.syx --emulator SOCKET` sends the checked image with the same `Sender` the USB path uses (the clock is the guest: 132,000 instructions per millisecond, one message per 2 ms, each through `midi.usb.in`), runs until the programming stops, compares the flash with the image's container, waits for the restart (the `firmware.loaded` event) and asks INFO over the emulated USB. `os_upgrade_rig.py ... --patchkit BIN [--restore-after]` does the panel steps first. Wall times on the host, release daemon:

| Run | Messages | Send (guest 2 ms each) | Programming (wall) | Flash equals image | Restart to INFO | Whole run |
| --- | --- | --- | --- | --- | --- | --- |
| DT2 1.17, patched (`device` set) | 15,162 | 10.4 s wall, 30.3 s guest | 24 erases, 5,982 programs, 5.3 s | 1,531,152 bytes equal | boots the patched runtime 0.5.2; INFO image id `281e6b9b1a0a9b83` = the id `prepare` computed | 27.4 s |
| DT2 1.17, `--restore` (from the patched image) | 14,705 | 18.2 s wall, 29.4 s guest | 23 erases, 5,801 programs, 4.3 s | 1,484,992 bytes equal (the stock container) | boots stock again (main screen) | 29.8 s |
| DN2 1.12, patched | 19,290 | 12.0 s wall, 38.6 s guest | 30 erases, 7,610 programs, 6.3 s | 1,947,984 bytes equal | runtime 0.5.2; image id `c5d8ccb27e139cbc` | 29.6 s |

The receiver took every message at the 2 ms pace (no stall at this pace, as in the earlier runs); after the last message the flash writer needed about 5 s wall. The tests of the checks themselves (tampered images, a wrong product byte, a DT2 build word below the floor, an oversize container, a message list with one message dropped, reordered or altered) are in `packages/patchkit/src/flash.rs` and `packages/patchkit/tests/flash.rs`.

### The Firmware page's flash in the emulator (2026-10-28) **[D]**

The page's own flash logic (`apps/web/src/firmware/firmware-flow.ts`: the Rust sender through `patchkit.wasm`, the pacing on the guest clock, the wait for the port to go away and
come back, then SoftwareVersion and INFO through `DkSession`) was run headless against the emulated devices at OS UPGRADE (`apps/web/tests/firmware-emulator.test.ts`, started by `os_upgrade_rig.py --exec`). The emulated flash equals the image's container in every run, as with the command line. The
image id INFO reports equals the id `prepare` made, and after the restore the stock firmware answers SoftwareVersion with its OS and INFO does not answer.

| Run | Messages | Flash writes | Container equals the image | Verdict of the page |
| --- | --- | --- | --- | --- |
| DT2 1.17, patch | 15,162 | 24 erases, 5,982 programs | 1,531,152 bytes | runtime 0.5.2, image id `281e6b9b1a0a9b83` matches |
| DT2 1.17, restore | 14,705 | 23 erases, 5,801 programs | 1,484,992 bytes | stock OS 1.17, no runtime |
| DN2 1.12, patch | 19,290 | 30 erases, 7,610 programs | 1,947,984 bytes | runtime 0.5.2, image id `c5d8ccb27e139cbc` matches |
| DN2 1.12, restore | 18,676 | 29 erases, 7,368 programs | 1,886,016 bytes | stock OS 1.12, no runtime |

Not modelled: real USB framing, real flash timing, the flash probe (sector and page size are the writer's defaults), erase failures. The programming model copies the page; a real chip ANDs it with the erased bytes, which is the same after an erase.

## The RPC route (2026-10-29) **[D]**

Run with `uv run python -m tools.emulator.os_upgrade_rig dt2|dn2 ... --rpc [--syx FILE] [--rpc-chunk N] [--rpc-auto] [--rpc-no-end] [--rpc-confirm yes|no]`: the stock image boots to the main screen (no menu), the rig sends the requests over `midi.usb.in`, reads the replies from `midi.usb.out`, follows the screen and compares the flash (`flash.read`) with the stream's container. DT2 1.17 and DN2 1.12, the stock file and the patched image (`patchkit flash --dry-run --save`) all went through: **no menu, no key on the panel**. The flash container equalled the stream every time (DT2 patched 1,531,152 bytes, 24 erases, 5,982 programs; DN2 patched 1,947,984 bytes, 30 erases, 7,610 programs; DN2 stock 1,886,016 bytes), then the soft reset.

**Framing.** The same envelope as every MidiRpc request (`F0 00 20 3C 10 00`, 8-in-7 packed `seq16 0000 type payload`, `F7`, no checksum; `DkSession.rpc` builds it). All numbers are u32 big endian, strings end with a zero byte. Replies have `type | 0x80`, the request's `seq`.

| Request | Payload | Reply payload |
| --- | --- | --- |
| Start `0x50` | stream length (bytes of the `.syx` file, the sum of all Writes), file type `sysex\0`, flag (0 = wait for End, 1 = go on after the last byte) | status byte (0 ok, 1 refused), message string. Refusals: "Unsupported file type", "OS Upgrade is already in progress" (only after the first Write), "Could not initiate OS upgrade" |
| Write `0x51` | CRC-32 of the data (init `0xffffffff`, i.e. zlib `crc32(data, 0xffffffff)`), data length, stream offset, data | bytes taken so far (u32), status, message. Status 0 ok, 1 complete (the last Write; validated), 2 CRC mismatch, 3 error ("Data not expected" before Start, or a validator message) |
| End `0x52` | commit byte (1 = flash) | status 0 ok (commit, and the length taken equals Start's), 2 refused (commit 0 or a short stream: this also cancels, the screen returns to the main screen after about 0.8 s and a new Start works), 1 error string; with nothing started: 0, no effect |

**What the firmware does with them [D].** Dispatcher `0x401246a6` (DN2 `0x40125f9e`); request readers `0x401e1098` Start, `0x401e1238` Write, `0x401dd57c` End (DN2 `0x401c87ae`, `0x401c894e`; End not paired by hash); session object fields: state `+4`, bytes taken `+0x14`, announced length `+0x18`, flag `+0x20`. Write feeds `FUN_4012b83e`, which appends the bytes to its buffer and calls the parser per `F7`; the dispatcher checks the CRC first (`0x4013db34`, table `0x4022437c`). A Write whose offset is below the bytes taken is acknowledged and ignored; an offset above it is **not** refused: the data is appended as if contiguous (the host must send in order). The last Write runs the validator (`0x400d9fb0`, about 243 M guest instructions = 1.8 s on DT2, 306 M on DN2) and answers status 1. Flag 1 then proceeds at once; flag 0 waits for End with commit. Both post the same event to the main task as the menu route (`0x4012b1d0`, DN2 `0x401298c8`).

**Screens (DT2 and DN2 alike).** Start: "OS UPGRADE WAITING FOR SYSEX" (the menu route's screen, opened by Start). First Write: "RECEIVING" with the bar. After the last Write the bar is full and the screen stays so until End. After End with commit: "ERASING FLASH...", "SUCCESS! REBOOTING", reset. **No "UPGRADE OS NOW?" confirm and no YES on this route**: the delegate is the same `OsUpgradeDelegate` (slot 3 returns 0). YES at the full bar with no End did nothing (no erase); NO showed "CANCEL OS UPGRADE? CONFIRM Y/N" as in the menu route. With the Start flag 0 "Waiting for confirm" means "waiting for the host's End". The question the device asks in Em's memory is therefore not in this firmware's code path; a host app (Transfer) asks it before it sends End, **[O]** unless a real device differs.

**A cut transfer.** After 5 Writes and no End: flash untouched (0 erases, 0 programs), screen "RECEIVING". A new Start is refused ("already in progress") once a Write was taken; End with commit 0 clears it, and the next Start works.

**Timing (guest instructions, 132 M per second; the emulator's USB takes 384 bytes per 16,500 instructions).** Start to reply 14.4 M. A Write of 2048 data bytes 0.3 to 0.4 M (about 3 ms) with its USB hand-over; 16,384 bytes 3.0 M; 65,536 bytes 17.5 M; the emulator took all sizes, the largest size a real device takes is **[O]** (the stock file writes use 2048). One flash by DT2 patched: 948 Writes of 2048 bytes, 6.3 s wall, then 24 erases and 5,982 programs. End reply 0.1 M; erase and program follow after End (not before: the data stays in RAM until End).

**Open.** Real device: the largest Write, the 3 s after "SUCCESS", what Transfer sends (Start flag, chunk size); the host decides before it sends End; the device asks nothing.

### The host side of the RPC route (2026-10-29) **[D]**

One state machine, `patchkit::flash_rpc::RpcFlash` (sans-io: `next`, `on_reply`, `on_timeout`, `confirm`, `cancel`), is used by the command line (`patchkit flash`, default `--route rpc`, `--chunk N`), the Firmware page on the site and in the workbench (the same machine through `patchkit.wasm`; the workbench sends the bytes with midir through its Tauri MIDI commands) and the tests. The question "Flash ... now?" is asked once, before Start: the CLI prompts (`--yes` skips it), the page has one Flash button whose click is the yes. At AskConfirm (the device's own check passed) both answer yes with no second question. The page only moves bytes and the clock (`apps/web/src/firmware/firmware-flow.ts`, `runRpcFlash`). The menu route (`Sender`, 2 ms pacing) stays as the fallback.

What the machine does:

| Step | Request | Timeout | Reply handling |
| --- | --- | --- | --- |
| Start | `0x50`, stream length, `sysex\0`, flag 0 | 5 s | status 0 goes on; else fail with the device's text, then End 0 (clears "already in progress") |
| Write, each chunk of 2048 bytes (`--chunk`) | `0x51`, CRC, length, offset, data, in order, one in flight | 2 s; the last Write 15 s (the validation takes 1.8 to 2.3 s here) | status 0 or 1 and `taken` = offset + length; status 2: send the same chunk again, at most 3 times, then fail; status 3: fail with the device's text; any other status or `taken`: fail |
| After the last Write | - | - | only status 1 means the device validated the image (the machine reports AskConfirm); status 0 fails ("did not report the image complete") |
| Commit | the host answers AskConfirm | - | the CLI and the page asked before Start (the Flash click, `Flash ... now? [y/N]`), so both answer yes at once: End 1. Cancel before AskConfirm sends End 0, "Nothing was written" |
| End | `0x52`, commit 1 or 0 | 5 s | commit 1 status 0: the device flashes and restarts; End 0 status 2 is the normal answer |

Every failure and every cancel before End 1 sends End 0, so the device returns to the main screen and a new Start works. **A timeout is a failure; a Write is not sent again.** The device acknowledges a Write whose offset is below the bytes taken and ignores it (section above), so a resend after a lost reply would be safe in the emulator. It was not chosen: the real device may behave otherwise, the last Write has a 2 s validation whose duplicate answer was not tried, and a failed run costs only a new Start (nothing is written before End 1). With End 1 sent nothing can be cancelled, and an End 1 without a reply is reported as "look at the screen".

**Reply layout [C].** A reply is `counter16 seq16 (type | 0x80) payload` after the 8-in-7 unpacking: the request's seq is echoed in bytes 2 and 3; bytes 0 and 1 are a counter of the device's own (2, 3, ... after a boot). A first version of the machine read the seq from bytes 0 and 1: it worked on a freshly booted emulator (counter and seq both start at 1) and lost every reply after the patched image had answered other requests. Found by the second flash in one emulator session (the page's restore after its patch). `parse_reply` reads bytes 2 and 3 and a unit test pins it.

**Proofs, emulator (DT2 1.17 and DN2 1.12), through the page's code path** (`apps/web/tests/firmware-emulator.test.ts`, started by `os_upgrade_rig ... --rpc --exec`; the device stays on its main screen): patch and restore on both devices: the flash equals the container (DT2 patched 1,531,152 bytes, 24 erases, 5,982 programs; DT2 restore 1,484,992 bytes, 23 and 5,801; DN2 patched 1,947,984 bytes, 30 and 7,610; DN2 restore 1,886,016 bytes, 29 and 7,368), INFO after the restart shows runtime 0.5.2 with the image id the patch made (`281e6b9b1a0a9b83`, `c5d8ccb27e139cbc`), and after a restore the stock OS answers and no runtime. An earlier page version asked after the check; Cancel there (DN2 patched) gave End 0, 0 erases and 0 programs (the same End 0 as a Cancel during the send). `patchkit flash STOCK --emulator SOCKET --yes` ran DT2 patch and restore the same way. Wall times in the debug build: the Send 6 to 9 s, the flash phase 4 to 7 s.

**Real device (Em's DT2, OS 1.17, USB, 2026-10-29) [D].** The RPC route with 2048-byte Writes flashed the patched image from the workbench (midir) and restored stock, then flashed the patch again, from the site in Chrome (WebMIDI). Every Write was acknowledged, the device checked the image and showed "RECEIVING" with a full bar until End, and End 1 erased, wrote and restarted as in the emulator. A content module (freqshift) installed afterwards through the site's Transfer page and loaded. While the page waited for End the device looked stuck on "RECEIVING"; that led to the one Flash button (commit right after the check).

**Open on a real device [O].** The largest Write the device takes (the emulator took 65,536), the real validation and flash times against the 15 s timeout, and DN2 on hardware.

## Open

- RPC route on a real device: the largest Write, the Start flag Transfer uses, DN2 (DT2 flashed with 2048-byte Writes, see above); no on-device confirm exists on either route in the emulator; the host commits after the device's own check.
- Staging buffer size and the MIDI queue overrun behaviour; the `--chunk 384 --period 16500` stall.
- Bootstrap behaviour with an erased container table and no FUNC.
- A real device: the menu route's 2 ms pace (not run on hardware; the RPC route has), how long the flash phase takes.
