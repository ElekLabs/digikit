# USB take-over: switching USB AUDIO/MIDI to USB MIDI in RAM

Plan: [usb-fast-takeover.md](../plans/usb-fast-takeover.md). Code: `modules/core/adapters/usb-bulk` (`usb_bulk_takeover`, `usb_bulk_giveback`), `modules/tools/usb-fast` (TAKEOVER 0x1e, RELEASE 0x1f). Static facts from DT2 1.17 and DN2 1.12, plus emulator runs. Marks: **[D]** read or measured once, **[O]** open. Nothing here is **[V]**: no second agent has checked it against the image bytes. The only hardware run so far is the one of 2026-11-12 below.

## Why a take-over

The USB controller has 4 endpoints (RM DCCPARAMS DEN = 4). USB AUDIO/MIDI uses EP1 (MIDI), EP2 IN (audio clock feedback) and EP3 (audio streams), so the bulk pair (EP2 OUT and IN) cannot exist beside live USB audio. The workbench switches the device to the USB MIDI configuration in RAM for a transfer and switches back. USB audio pauses meanwhile **[D]**.

## The mode switch

- `FUN_40006c16(port)` has the same address and bytes on both images (hash `6ca45a25378d0a0f22d18114089f4984`, anchor `cf.usb.fn.mode_switch`). It compares `port` with the active port index (`cf.usb.value.active_port`: DT2 0x40965a60, DN2 0x4058f2a8) and returns when equal. Otherwise it stops USB (`FUN_40006bac`: running flag 0x40965a44 clear, `FUN_40005dcc` = USBCMD.RS clear, USBINTR 0 and the old port's disconnect callback, restart type 0x40965a34 clear, counter 30), writes the active port and the pointer to its table entry (the word below the port), and restarts the device (`FUN_40006bc6`) after a delay. The USB CONFIG menu calls the same function with no extra delay. **[D]**
- The delay is a counter set to 30 by the stop and counted down by the USB tick `FUN_4000713e`; the restart runs when it reaches 0. In the emulator one tick is about 33 ms, so the restart comes about 1 s after the switch on both images. The counter is the anchor `cf.usb.value.restart_delay` (rule `cf_literal` on the tick, offset 0xa: DT2 0x40965a38, DN2 0x4058f280). The adapter used a fixed offset (`active_port - 0x28`) before; the anchor replaces it. **[D]**
- The restart `FUN_40006bc6` sets the running flag, returns when USBCMD.RS is set already, and starts the controller (`FUN_40005cde`) only when OTGSC.BSV (bit 11, VBUS valid) is set. The start needs the running flag and a non-zero active port, writes USBMODE 0x1e, the queue head list, USBINTR 0x157 and sets RS. The VBUS interrupt (`vector` 0x40005e14, OTGSC.BSVIS) calls the same start when BSV is set and the stop when it is clear. OTGSC.BSVIE is set once at init (OTGSC = 0x08000000, address 0x40006d8a); nothing re-arms it and nothing needs to. **[D]**
- The switch never writes the USB CONFIG setting byte (anchor `cf.usb.value.config_mode`, the operand of its getter: DT2 0x4099d5c4, DN2 0x405ce898; values 0 USB MIDI, 1 Overbridge, 2 USB AUDIO/MIDI). Its writers are the three menu actions, the factory defaults and the settings load. The settings record holds the byte, not the active port, so a save or a power-off during a take-over keeps the stored mode, and a boot always returns to it. The emulator test reads the byte before and after every switch: unchanged. **[D]**
- There is no lock. The stop runs in the caller's task and cuts a reply in flight on EP1. A command that triggers the switch must reply first: the adapter records the request and the UI-pass tick switches at least 50 ms later, and waits up to 300 ms more while EP0 or EP1 IN has data primed (ENDPTSTAT or ENDPTPRIME bits 16 and 17) or an EP2 IN send of its own is queued. A second switch inside the restart window is not safe, so the adapter keeps a guard (the restart counter, and a floor of 3.5 s on its own clock: the 2 s hold below plus the host's re-enumeration) and runs one switch at a time. **[D]**
- Only one firmware path re-applies the stored mode by itself: an Overbridge (port 6) disconnect (`FUN_400f2ce0`). A change of the USB CONFIG menu during a take-over stores the new value and switches: the user's choice wins, the adapter drops the take-over without switching back. **[D]**
- Unplug (VBUS) and bus suspend call the active port's disconnect callback and leave the active port as it is. The adapter gives back after a host disconnect, a suspend of 0.5 s, or 30 s without host activity. **[D]**

## One enumeration

The bulk interface is part of port entry 2's configuration descriptor, which the adapter repoints (`usb_bulk_enable`). Done in USB MIDI mode, that needs a stop and restart, so the host sees the device leave and return. A take-over from USB AUDIO/MIDI would then cost two enumerations: the switch, then enable's own stop and restart.

Now `usb_bulk_enable` prepares entry 2 in any mode (it writes only the port table entry and the adapter's own state, with interrupts masked), and re-enumerates only when the device is in USB MIDI mode and no switch is settling. The usb-fast job calls it at its start, also in USB AUDIO/MIDI, so the take-over switch enumerates with interface 2 already present. The emulator test counts the switches of a take-over (`usb_bulk_switches`): one. **[D]** The first enable in USB AUDIO/MIDI followed by a take-over has no emulator test: the emulated boot is in USB MIDI mode and the module's first enable runs there. **[O]**

## Host side

TAKEOVER and RELEASE are SysEx commands of the usb-fast module (`usb_fast_takeover` range 0x1e to 0x1f). The host (`BulkTakeOver` in `packages/dk-session/src/bulk-select.ts`) asks `usbFastWrongMode` (the wrong-mode bit of usb-fast's STATUS) and takes over only then, waits for the device to return on its MIDI port, runs the nonce handshake, transfers, and releases after a linger (10 s in the desktop app; at once in `wb.mjs`). The MIDI port name and the CoreMIDI connection across the configuration change, and the macOS re-enumeration time each way (expected 1 to 2 s), are open **[O]**.

**[D]** After about 15 take-overs in quick succession, the macOS CoreMIDI connection to the DN2 went stale. The port stayed listed, but neither our runtime nor the firmware's universal identity request got replies. A device power cycle did not help; a USB cable replug did. The device itself was fine: no crash record, and the stored USB CONFIG unchanged.

## The failed take-over of 2026-11-12 (hardware)

`wb.mjs takeover-cycle 20 --gap 2000` on a DT2 in USB AUDIO/MIDI, take-over and release in turn. **[D]**

- Cycles 1 to 4: after TAKEOVER and after RELEASE macOS saw the device leave within 0.2 to 0.3 s and return after 1.3 to 1.5 s with new interface object ids; the runtime INFO answered.
- Cycle 5: after TAKEOVER macOS saw no disconnect (same ids). The device no longer answered at its USB address (libusb reset: "No such device"; the firmware identity request and the runtime INFO: no reply), and macOS kept the old AUDIO/MIDI interface list. A host-side re-enumeration is not possible on macOS (nusb reset: exclusive access refused, 0xe00002c5, the class drivers hold the device).
- A cable replug after the failure did not bring the DT2 back at all (not on USB, no MIDI port, after more than 5 s). Only a power cycle did (then 5 interfaces, the runtime INFO answered). Earlier, a first failure was cured by a replug.
- macOS kept the same ids across a device power cycle (the device stayed electrically attached while off).

Two failure modes follow: (1) the host misses the detach and keeps the old configuration; (2) the device stays off the bus after a failed switch, also through a replug, until a power cycle.

## What the firmware does with the ULPI PHY

Addresses in this section are DT2 1.17. The USB engine is the same code on DN2 1.12 (same bytes, other data addresses: the anchors). **[D]**

- The controller has an external ULPI PHY (PORTSC.PTS = 10: init writes bit 31 and clears bit 30, `FUN_40006c52` at 0x40006d1c to 0x40006d2c). The RM says the controller itself performs the PHY register accesses and that writes through ULPI_VIEWPORT (0xFC0B0170, 24.3.4.12) "can substantially harm standard USB operations". **[D]**
- The firmware writes no PHY register. The only viewport accesses are in one periodic function (0x40006af8, counter 0x40965a4c, state 0x40965a50): every 300 calls a read of register 0 (viewport 0x48000000, stores at 0x40006b1c and 0x40006b32); at the next check, a read that has not finished (RUN still set) makes it reset the controller (USBCMD.RS = 0, RST = 1, wait for RST to clear), clear the running flag, stop and set restart type 2 with counter 30. The tick then calls 0x40007104: RST again, the full init (`FUN_40006c52`, which sets the running flag again) and the start when BSV is set. So the firmware does set USBCMD.RST, and it treats a hung viewport as a dead PHY. It never writes the PHY's reset bit: a PHY that is stuck is not reset by this path. **[D]**
- The firmware does not set PORTSC.PHCD. **[D]**
- What the controller writes to the PHY when RS changes (function control: XcvrSelect, TermSelect, OpMode) is not in the RM. RS = 0 "causes a detach event" (RM 24.3.4.1, USBCMD.RS), and in high speed the D+ pull-up is already off, so the host sees the detach only if the PHY removes the high-speed termination. How the controller does that, and whether it still does it with the bus suspended (the PHY clock may be off, the viewport shows ULPI_SS clear and needs the wake-up operation first), is **[O]**.

## Causes, most likely first (static; none proven)

1. The detach never reached the line: RS was cleared while the bus or the PHY was suspended or busy, or the controller left the termination on. The host then sees no disconnect, keeps its configuration, and the restart re-attaches a device at address 0 that the host does not address. Fits failure mode 1 and the intermittent 1 in 5 pattern. **[O]**
2. The PHY is left in a state that the restart does not undo (a stop during traffic, or the viewport hang above that resets only the controller). The firmware then loops stop and restart or never gets VBUS or sync back, and a replug does not help. Only a power cycle resets the PHY. Fits failure mode 2. **[O]**
3. The restart is lost: the counter reached 0 while BSV was clear, or an interrupt arrived while the running flag was clear, and nothing starts the controller until the next BSV edge. A replug gives that edge, so this alone does not explain mode 2. **[O]**
4. A second switch or a reply cut by the stop during the 50 ms after the reply. The adapter already serialises its own switches. **[O]**

## The fix in core/usb-bulk

Around the take-over and give-back switches only; the firmware's menu path is not touched, and nothing runs while no take-over is used. **[D]** (emulator only; the effect on hardware is not proven)

- **Hold.** Right after the firmware's stop the adapter raises the restart counter from 30 to 60 ticks: the device stays off the bus for 2 s. The tick is 33.3 ms; macOS noticed the detach within 0.3 s when it noticed it, so 2 s is about six times that, and covers the hub's 100 ms debounce and the OS teardown. A longer hold only slows every switch.
- **Release the line.** While the controller is stopped the adapter reads the PHY's function control (register 0x04) and writes 0x49 (XcvrSelect full speed, TermSelect 0, OpMode non-driving, SuspendM 1). 3 ticks before the restart it writes the saved value back. If the controller was started first (an early restart, or a UI stall past the last 100 ms) the function control is read and the saved value is written only when it still holds 0x49, so the device does not keep running without termination (`usb_bulk_phy_late`). A failed access is retried every UI pass, 50 times at most (`usb_bulk_phy_refused`, log "ub.phy.restore.no"). Each viewport access runs with interrupts masked, starts only when ULPI_SS is set (a wake-up first if not) and RUN is clear, and never abandons a run: a run that does not finish is waited for on the next pass before anything new starts. The spin bound is 1000 reads for each of three waits, so a dead PHY clock stalls interrupts for about 3 ms at most. The firmware's watchdog read (0x40006af8) is a callback on the USB timer (FUN_40006888 sets USBINTR.TIE0 and queues it), so it runs in interrupt context, and the stop clears USBINTR: while the controller is stopped it does not run, and the mask keeps it out of our accesses otherwise. **[D]** The ULPI 1.1 register layout is from the standard, which is not in the repository **[O]**. Whether the controller rewrites the function control itself on RS = 1 is not in the RM **[O]**. `UB_ULPI_DETACH 0` turns the PHY writes off and keeps the rest.
- **Settle.** The next switch waits 3.5 s from the previous one (hold 2 s plus the re-enumeration of 1.3 to 1.5 s) and for the restart counter to be 0. A switch also waits up to 300 ms for a reply on EP0 or EP1 IN or an EP2 IN send of its own.
- **Recovery.** When one of the adapter's switches has been made, the controller is still stopped (RS = 0) and the active port is still the switch's target, a window of 4 s runs while VBUS is present (OTGSC.BSV); it restarts at every moment without VBUS, so a plug-in starts a new window and the firmware's own BSV restart goes first. At the end of the window, with the firmware's restart counter at 0 and the hold over, the adapter resets the PHY (function control bit 5), calls the firmware's restart and logs "ub.forced.rstart" (`usb_bulk_forced_restarts`). It does this at most 3 times for one switch, then gives up with the log "ub.rstart.giveup". It disarms when the controller runs or the active port changes to something else (menu, Overbridge, safe mode). So a restart lost while VBUS is present is repeated a few times; with no VBUS nothing is forced, and a PHY that stays dead after three resets is not recovered by this code. **[D]** (emulator only)

Emulator: ULPI_VIEWPORT completes a run at once (RUN clears, a read returns the register in ULPI_DATRD) and shows ULPI_SS set; the PHY registers are plain storage; `usb.phy` (`docs/CONTROL.md`) returns the order of viewport accesses and RS changes. No PHY behaviour is modelled. The device test `take-over hold` checks the detach write, the hold of about 2 s, the write-back before any RS = 1, one switch per take-over, and a forced restart after the counter and the controller state are made to look lost. **[D]**

## Why a late fix was needed: byte reads

m68k GCC folds `USB_REG(x) & small_mask` into a `btst` on one byte, so the code read ULPI_VIEWPORT, USBCMD and OTGSC with byte accesses at the byte's address. The emulator modelled those registers only for long reads, so a byte read returned the stored byte (no VBUS, no ULPI_SS) and the recovery never fired. Two fixes: every register read in usb_bulk.c is now one `move.l` (`reg_rd`, an `asm volatile` that cannot be folded), and the emulator answers byte, word and long accesses of every USB register from the 32-bit value (big-endian lanes), with sub-word writes to USBCMD and the viewport merged into the long word. The hardware accepts byte reads of these registers; the point is that the code and the model agree on the width. **[D]**

## Hardware test

`wb.mjs takeover-cycle 50 --gap 2000`, then `takeover-cycle 50 --gap 0`, on the DT2 (and the DN2) in USB AUDIO/MIDI. Pass: in every cycle macOS reports a disconnect after TAKEOVER and after RELEASE (new interface object ids each time), the runtime INFO answers after each return, and the device is back within 4 s of each command. Fail on the first unchanged id or missing answer; then read `usb_bulk_phy_*` and `usb_bulk_forced_restarts` and the log. **[O]**

## Hardware result with the hold and PHY park, 2026-11-13

`wb.mjs takeover-cycle` on a DT2 in USB AUDIO/MIDI with the fix above. **[D]**

- Cycles 1 to 10: clean. macOS saw the detach and the return (2.4 s) after every TAKEOVER and every RELEASE.
- Cycle 11: failed on TAKEOVER the same way as cycle 5 before the fix (no disconnect seen, device unreachable). No self-recovery within 30 s. A cable replug did not bring the DT2 back; a power cycle did.
- Both failures (cycle 5 without the fix, cycle 11 with it) were on TAKEOVER, never on RELEASE.
- With USB CONFIG = USB MIDI (no switching) fast USB was fully reliable.

The hold and the PHY park make the failure rarer, not absent. The take-over is therefore off by default: the desktop app has a "take USB over" switch in Samples mode (off, kept per app profile), `wb.mjs` takes over only with `--takeover` (`takeover-cycle` always does). Without it a device in USB AUDIO/MIDI stays on SysEx and the Target pill says `SYSEX`, with the hint "Set USB CONFIG to USB MIDI on the device for Fast USB". The device side is unchanged and does nothing until a host sends TAKEOVER. **[O]** Track 2 before the take-over can become the default again: a device-side enumeration watchdog, a persistent failure record, and runs with audio idle against audio playing.
