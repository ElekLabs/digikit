# core/usb-bulk

A vendor bulk interface (interface 2, EP2 OUT and IN, 512-byte packets) added to the firmware's USB MIDI mode, for run-time modules: `usb_bulk_enable`,
`usb_bulk_take`, `usb_bulk_release`, `usb_bulk_send`, `usb_bulk_state`, `usb_bulk_takeover` and `usb_bulk_giveback` (`sdk/sdk.toml`, `sdk/include/dk/platform/usb_bulk.h`). Plan: `docs/plans/usb-fast.md`
section 4; facts: `docs/findings/42-usb-transfer-routes.md` section 4. EP2 IN runs with 64-byte packets and no zero-length termination, like the firmware's EP1 IN, because the controller's TX buffer holds 64 bytes per endpoint and two back-to-back 512-byte IN packets wedged it on the DN2.

- **No firmware patch.** The USB engine is the same code at the same addresses on DT2 1.17 and DN2 1.12. Its port table is RAM: `usb_bulk_enable` checks
  the stock entry 2 (callbacks, length, the 101 descriptor bytes), then writes four words (connect, disconnect, descriptor pointer and length) and
  re-enumerates with the firmware's own stop and restart pair. Not called, nothing is written and USB is stock. Outside USB MIDI mode (USB CONFIG) enable
  prepares the entry only (the next switch to USB MIDI enumerates with interface 2: one enumeration for a take-over) and returns E_STATE until the device is in USB MIDI mode and no
  switch is settling; bit 2 of `usb_bulk_state` shows the wrong mode live. While enabled, the adapter wakes the module's job four times a second (a timer for the module).
- **Take-over** (`docs/plans/usb-fast-takeover.md`): `usb_bulk_takeover` records the original port (USB AUDIO/MIDI, port 5) and a request; `usb_bulk_tick` (UI task, on the runtime's
  `dkrt_poll_add` list from the first take-over call on, also while bulk is not enabled) calls the firmware's mode switch (`cf.usb.fn.mode_switch`) with port 2 at least 50 ms
  later. `usb_bulk_giveback` and the automatic give-back (the disconnect callback, a bus suspend of 0.5 s, 30 s without a transfer or renew) switch back the same way. After each of these switches the adapter holds the firmware's restart for 2 s (counter 60 instead of 30), releases the PHY's line while the controller is stopped (ULPI function control through ULPI_VIEWPORT, written back before the restart; `UB_ULPI_DETACH 0` turns it off), waits for a reply on EP0 or EP1 IN before it switches and 3.5 s between switches, and forces the firmware's restart when USBCMD.RS is still clear 4 s after a switch with VBUS present (`usb_bulk_forced_restarts`). Facts and reasons: `docs/findings/123-usb-takeover.md`. Refused in
  Overbridge. A port change by another route drops the state without a switch. The USB CONFIG setting byte is never written (test: `cf.usb.value.config_mode`). Bit 3 of the state word.
- **Anchors** (`devices/fw/usb.toml`): the port table, the active port, the USB CONFIG setting byte, the firmware's restart delay counter, connect, disconnect, queue, cancel, dTD allocation, stop, restart and the mode switch.
- **Receive**: eight 16 KiB buffers (128 KiB: two 64 KiB frames of the usb-fast protocol, so the next frame arrives while the CPU writes the last one; four 4 KiB pages each: every dTD names all five page pointers) in the uncached alias of DDR, one dTD each on EP2 OUT.
  The completion callback marks a buffer full, queues a free one and wakes the module's I/O job (`dkrt_io_wake`). No free buffer: nothing queued, the host
  gets NAK. **Send**: the bytes are copied into one of two slots and queued on EP2 IN.
- **Reset**: the connect and disconnect callbacks cancel both EP2 queues with a bounded flush (the stock disconnect polls without a bound), drop buffers
  that were full and not taken, and change the reset count of `usb_bulk_state`.
- **Emulator**: the controller is not modelled, except that ULPI_VIEWPORT completes at once and `usb.phy` (`docs/CONTROL.md`) lists the PHY accesses and USBCMD.RS changes. `usb.bulk.in` and `usb.bulk.out` (`docs/CONTROL.md`) complete the dTDs through the guest's own callbacks.
  Test: `tests/usb-bulk.device.test.ts` with the fixture `test/usb-echo`.
