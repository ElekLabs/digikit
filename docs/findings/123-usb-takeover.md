# USB take-over: why it is not reliable, and the bulk OUT endpoint that replaces it

Plan: [usb-fast-audio-bulk.md](../plans/usb-fast-audio-bulk.md). Earlier plan: [usb-fast-takeover.md](../plans/usb-fast-takeover.md). Code: `modules/core/adapters/usb-bulk` (`usb_bulk_takeover`, `usb_bulk_giveback`, card-log events `USB_SW`), `modules/tools/usb-fast`. Static facts from DT2 1.17 and DN2 1.12; hardware facts from one DT2 unit, 2026-11-12 and 2026-11-13. Marks: **[D]** measured on one unit or read once by one agent, **[O]** open. Nothing here is **[V]**: no second agent has checked it against the image bytes.

## Conclusion

A take-over (switching USB AUDIO/MIDI to USB MIDI in RAM, to get a bulk pair) cannot be made reliable. The mode switch stops the USB controller. On some switches the ULPI PHY then stops answering, and only a power cycle brings the device back. Nothing we tried in software revives it. **[D]**

The replacement adds a bulk OUT endpoint (interface 5, EP 0x02) to the USB AUDIO/MIDI configuration at boot. No mode switch, no controller stop. Replies come back over SysEx. A probe (commit 8585548d) sent 200 MB at 28 MB/s while audio played; macOS kept audio and MIDI and asks once to allow the accessory. **[D]**

## Endpoint budget

The USB controller has 4 endpoints (RM DCCPARAMS DEN = 4). USB AUDIO/MIDI uses EP1 (MIDI bulk pair), EP2 IN (audio clock feedback) and EP3 (audio streams). EP2 OUT is unused by the stock configuration. A bulk pair (EP2 OUT and IN) cannot exist beside live audio, but a single bulk OUT on EP2 can. **[D]**

## The firmware mode switch

- `FUN_40006c16(port)` has the same address and bytes on both images (hash `6ca45a25378d0a0f22d18114089f4984`, anchor `cf.usb.fn.mode_switch`). It returns when `port` equals the active port (`cf.usb.value.active_port`: DT2 0x40965a60, DN2 0x4058f2a8). Otherwise it stops USB (`FUN_40006bac`: running flag clear, `FUN_40005dcc` = USBCMD.RS clear, USBINTR 0, the old port's disconnect callback, restart type 0, counter 30), writes the active port and its table entry, and restarts after the counter reaches 0. **[D]**
- The USB tick `FUN_4000713e` counts the delay down. One tick is about 33 ms, so the restart comes about 1 s after the switch (anchor `cf.usb.value.restart_delay`: DT2 0x40965a38, DN2 0x4058f280). **[D]**
- The restart `FUN_40006bc6` starts the controller (`FUN_40005cde`) only when OTGSC.BSV (VBUS valid) is set. The VBUS interrupt calls the same start when BSV is set and the stop when it is clear. **[D]**
- The switch never writes the USB CONFIG setting byte (`cf.usb.value.config_mode`: DT2 0x4099d5c4, DN2 0x405ce898; 0 USB MIDI, 1 Overbridge, 2 USB AUDIO/MIDI). A boot always returns to the stored mode. **[D]**
- There is no lock. The stop runs in the caller's task and cuts a reply in flight on EP1. **[D]**
- The firmware's controller reset: `FUN_40006ada` (anchor `cf.usb.fn.reset_stop`, same address 0x40006ada on DT2 1.17 and DN2 1.12) is a stop with restart type 2. After 30 ticks the tick calls `FUN_40007104`: USBCMD.RST with an unbounded spin, the full init `FUN_40006c52`, and the start when BSV is set. The firmware's own viewport watchdog uses this path. Each reset leaks one timer slot of the 16 shared slots. **[D]**

## The ULPI PHY

- The controller has an external ULPI PHY (PORTSC.PTS = 10). The firmware writes no PHY register. Its only viewport access is a periodic read of register 0 (`0x40006af8`); a read that has not finished at the next check makes it reset the controller. It never sets PORTSC.PHCD and has no PHY reset GPIO. **[D]**
- The USB clock comes from the PHY. Firmware init clears MISCCR.USBSRC (0xEC09000E bit 0 = 0): the USB_CLKIN pin, which is the PHY's 60 MHz output in ULPI mode, drives the USB serial interface clocks (RM 10.3.4). A stopped PHY clock therefore freezes the controller's USB side. **[D]**
- ULPI_VIEWPORT (0xFC0B0170) writes can harm normal operation (RM 24.3.4.12). A wake-up (WU set, RUN clear) is the documented way to restart the PHY clock. WU and RUN must never be set together. **[D]**

## Hardware facts: the failure (DT2, card-log timelines)

Source: the card log (`USB_SW` events) of `wb.mjs takeover-cycle` runs. **[D]**

- A failed switch looks the same every time. After RS is set again, PORTSC shows line state SE0 (`0xa0000005`; good switches show J, `0x...805` or `0x...885`). No SOF arrives. The host never addresses the device (no ADDRESSED). The ULPI function control stays `0x41` and never reaches `0x49` in the failing park case. ULPI_VIEWPORT runs hang with SS set. After a controller reset BSV reads 0 and the controller never restarts.
- The device keeps running and its firmware is healthy; only the USB side is dead. Audio was idle in all recorded failures.
- Failures happened on take-over and on release.
- A cable replug never helps. A power cycle always does. macOS keeps the old interface ids until then, or sees the device leave and not return.
- The PHY park (writing function control 0x49 during the hold, to drop the high-speed termination) made failures about 10 times more frequent: about 1 in 15 switches with it. Without it: build A failed 1 in 76 switches, build B 1 in 200 (about 1 in 140 over both). Hold time and park together were no fix.
- Recovery by a plain stop, a 2 s hold and a restart left the line at SE0.
- The firmware's controller reset (`cf.usb.fn.reset_stop`) does not revive it: BSV stays 0 after the reset.
- A PHY wake-up after the reset was built as experiment C (GPT0 counter as the clock indicator, then a viewport wake-up), but never run on hardware: the work moved to the bulk OUT endpoint in USB AUDIO/MIDI first. Whether a wake-up revives the PHY is open. **[O]**

The PHY link dies at the controller stop. The hung viewport reads seen with the park are a symptom, not the cause. Why the stop kills the PHY on some switches is **[O]**.

## What the host sees on macOS

- On a good switch macOS sees the device leave within 0.2 to 0.3 s and return after 1.3 to 2.4 s with new interface object ids. **[D]**
- A host-side re-enumeration is not possible while the class drivers hold the device (nusb reset: exclusive access refused, 0xe00002c5). **[D]**
- After about 15 take-overs in quick succession, the CoreMIDI connection to a DN2 went stale: the port stayed listed and no replies came until a cable replug. **[D]**
- macOS keeps the same ids across a device power cycle when the device stays electrically attached while off. **[D]**

## The replacement: bulk OUT in USB AUDIO/MIDI

Static facts for port entry 5 (USB AUDIO/MIDI), DT2 1.17; DN2 1.12 has identical code and other data addresses. **[D]**

- The port table (anchor `cf.usb.data.port_table`) has 0x28-byte entries. Entry 5 is at table + 0xC8 (DT2 0x4029f048): +0 connect `0x400040a6`, +4 disconnect `0x40004054`, +8 device descriptor, +0xC configuration descriptor pointer, +0x10 length 0x148, +0x24 high-speed flag.
- The configuration has 5 interfaces: IAD over interfaces 0 to 2 (UAC2), interface 3 MIDI control, interface 4 MIDIStreaming with the EP1 bulk pair. EP2 IN is the iso feedback; EP2 OUT is unused.
- GET_DESCRIPTOR(configuration) serves `min(wLength, e[4])`; nothing assumes length 0x148 or bNumInterfaces. SET_INTERFACE acts only on interfaces 1 and 2 and ACKs the others.
- Port 5 connect runs its disconnect first (clears ENDPTCTRL2 and 3 whole words), then sets ENDPTCTRL2 = `0x00c40000` (TX iso only), EP3, and MIDI. It never touches the EP2 OUT queue head (`0x80000100`). The audio stop routines never flush EP2 OUT.
- A bus reset (`FUN_40006dba`) clears ENDPTCTRL1 to 3 and all queue head lists and re-inits the dTD pool without callbacks. SET_CONFIGURATION then calls the connect callback again: this is the point to prime EP2 OUT again.
- The shared dTD pool has 31 entries; audio iso does not use it.

The probe appends interface 5 (class 0xFF, one bulk OUT endpoint 0x02, 512 bytes) to a copy of the configuration at boot, with a hook at the USB init function `FUN_40006c52` (`ua_prepare`), and wraps the port 5 connect and disconnect callbacks to enable the EP2 OUT receive half. It never stops or restarts the controller, so none of the failures above can occur. **[D]** (one hardware run; 200 MB at 28 MB/s with audio playing, macOS keeps audio and MIDI)

Open: DN2 hardware run, a long run for stability, the behaviour after a host sleep and after a bus reset during a transfer. **[O]**
