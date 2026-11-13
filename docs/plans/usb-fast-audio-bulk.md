# Fast USB in USB AUDIO/MIDI without a take-over

Status: **agreed 2026-11-13; PR A in progress.** Facts: [finding 123](../findings/123-usb-takeover.md). Replaces [usb-fast-takeover.md](usb-fast-takeover.md). Follows [usb-fast.md](usb-fast.md).

## Why

A take-over (mode switch in RAM) fails on about 1 in 15 to 1 in 200 switches and needs a power cycle (finding 123). The probe of 2026-11-13 (commit 8585548d) showed another way: add a bulk OUT endpoint (interface 5, EP 0x02, 512 bytes) to the USB AUDIO/MIDI configuration at boot. macOS keeps audio and MIDI; 200 MB went at 28 MB/s while audio played. The controller is never stopped.

## Design

- New core module `core/usb-bulk-audio` (`modules/core/adapters/usb-bulk-audio/`): the boot hook on the USB init function (`cf.usb.hook.init`, `ua_tramp.s`), `ua_prepare`, and the port 5 connect and disconnect wrappers. New patch set `device-audio-bulk` = the `device` set plus this module, in `devices/digitakt-ii.toml` and `devices/digitone-ii.toml`.
- `ua_prepare` checks the stock configuration (length 0x148, header `09 02 xx xx 05`), copies it, appends interface 5, repoints port entry 5, and calls `usb_bulk_port5_attach()`. It is idempotent and ISR-safe. If a check fails it refuses and touches nothing.
- `core/usb-bulk` exports `usb_bulk_port5_attach`, `usb_bulk_port5_connect` and `usb_bulk_port5_disconnect`. The receive ring gate becomes "bulk-capable port": port 2 (pair) or port 5 attached (OUT only). Every EP2 control write in port 5 touches only the RX half (mask `0x0000ffff`). EP2 IN is audio feedback and is never queued.
- `usb_bulk_state()` gets the bit `DK_USB_BULK_OUT_ONLY`. `usb_bulk_send` returns E_STATE in that mode.
- Replies go over SysEx. usb-fast keeps replies in its queue (16) and the host drains them with `0x37 'R'`: reply body = complete frames (`DKB1`, cmd|0x40, seq, status, len, body), oldest first, at most 879 bytes, never split. INFO `flags` bit 0 = replies over SysEx. The parse throttle (no parsing with 14 or more queued replies) stays, so an unpolled host stalls by NAK.
- Host: `SysExReplyLink` (bulk link whose send() writes OUT and whose receive() polls `0x37 'R'`), `EmuOutOnlyLink` for emulator tests, `chooseBulk` tries OUT-only candidates after the pair candidates, `dkbulk open --out-only`. The desktop and web pill reads "FAST USB" in both modes. The "Set USB CONFIG to USB MIDI" hint stays only for firmware without `core/usb-bulk-audio`.

## Split

| PR | Content |
|---|---|
| A | The design above, in USB AUDIO/MIDI and USB MIDI. Take-over code stays in place; hosts stop using it (call site and checkbox wiring removed). The `0x37 'S'` speed switch, the card log and the USB MIDI bulk pair stay. |
| B | Delete the take-over: `usb_bulk_takeover`, `usb_bulk_giveback`, TAKEOVER and RELEASE (0x1e, 0x1f), the PHY park and the recovery code, `BulkTakeOver`, `takeover-cycle`, the anchors only they use. |

## Gating

- The emulator probe test becomes a test of the upload path in USB AUDIO/MIDI (emulator `usb.bulk.in` targets EP2 OUT; replies through `'R'`).
- Hardware: DT2 and DN2 in USB AUDIO/MIDI, upload while audio plays, replug, host sleep, a bus reset during a transfer. The DN2 has not run the probe yet.
- The SDK change (new state bit, `'R'`) selects a wide merge gate: plan a targeted test list.

## Open [O]

- DN2 hardware run.
- Long-run stability and bus reset during a transfer.
- macOS asks once to allow the accessory: the workbench should say so.
