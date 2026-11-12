# core/usb-bulk

A vendor bulk interface (interface 2, EP2 OUT and IN, 512-byte packets) added to the firmware's USB MIDI mode, for run-time modules: `usb_bulk_enable`,
`usb_bulk_take`, `usb_bulk_release`, `usb_bulk_send`, `usb_bulk_state`, `usb_bulk_takeover`, `usb_bulk_giveback` and `usb_bulk_ring` (`sdk/sdk.toml`, `sdk/include/dk/platform/usb_bulk.h`). Plan: `docs/plans/usb-fast.md`
section 4; facts: `docs/findings/42-usb-transfer-routes.md` section 4. EP2 IN runs with 64-byte packets and no zero-length termination, like the firmware's EP1 IN, because the controller's TX buffer holds 64 bytes per endpoint and two back-to-back 512-byte IN packets wedged it on the DN2.

- **No firmware patch.** The USB engine is the same code at the same addresses on DT2 1.17 and DN2 1.12. Its port table is RAM: `usb_bulk_enable` checks
  the stock entry 2 (callbacks, length, the 101 descriptor bytes), then writes four words (connect, disconnect, descriptor pointer and length) and
  re-enumerates with the firmware's own stop and restart pair. Not called, nothing is written and USB is stock. Outside USB MIDI mode (USB CONFIG) enable
  prepares the entry only (the next switch to USB MIDI enumerates with interface 2: one enumeration for a take-over) and returns E_STATE until the device is in USB MIDI mode and no
  switch is settling; bit 2 of `usb_bulk_state` shows the wrong mode live. While enabled, the adapter wakes the module's job four times a second (a timer for the module).
- **Take-over** (`docs/plans/usb-fast-takeover.md`): `usb_bulk_takeover` records the original port (USB AUDIO/MIDI, port 5) and a request; `usb_bulk_tick` (UI task, on the runtime's
  `dkrt_poll_add` list from the first take-over call on, also while bulk is not enabled) calls the firmware's mode switch (`cf.usb.fn.mode_switch`) with port 2 at least 50 ms
  later. `usb_bulk_giveback` and the automatic give-back (the disconnect callback, a bus suspend of 0.5 s, 30 s without a transfer or renew) switch back the same way. Refused in
  Overbridge. A port change by another route drops the state without a switch. The USB CONFIG setting byte is never written (test: `cf.usb.value.config_mode`). Bit 3 of the state word.
- **Anchors** (`devices/fw/usb.toml`): the port table, the active port, the USB CONFIG setting byte, the firmware's restart delay counter, connect, disconnect, queue, cancel, dTD allocation, stop, restart and the mode switch.
- **Receive ring**: up to 60 slots of 16 KiB (960 KiB; four 4 KiB pages each: every dTD names all five page pointers) in the uncached alias of DDR, allocated per session with
  the send slots as one block (a 1 MiB block from the firmware's buddy allocator; the tiers are tried 60, 28, 8 slots, so a heap that cannot give 1 MiB gives 512 KiB, then the old 256 KiB block with 8 slots;
  all tiers refused: E_NOMEM, ST_NOMEM, nothing enabled). At most 12 dTDs are queued on EP2 OUT at once (the firmware's pool has 31, 2 are ours for EP2 IN); the completion callback, in the interrupt,
  marks the slot full, queues the next free slot round the ring and wakes the module's I/O job (`dkrt_io_wake`), so the controller keeps receiving while the job writes the card. The job takes the full slots in
  arrival order. No free slot: nothing queued, the host gets NAK. A 64 KiB frame of usb-fast spans five slots, so the ring takes 12 frames (`usb_bulk_ring` reports it). The run-time switch
  (`usb_bulk_ring`, bit 1 of the speed word, default on; `usb_bulk_ring_off` in the emulator test) off: 8 slots, all queued, the old geometry; it is read when the buffers are allocated and at every queueing. Switching it off while buffers are live keeps the block and only stops queueing beyond 8 slots; a session that starts with the switch off allocates the 8-slot block, which is the clean A/B. `usb_bulk_ring` returns bit 0 the switch, bit 1 always set (the SDK stub returns 0 for an adapter without it), bits 8..15 the window, bits 16..23 the slots of the live block.
  **Send**: the bytes are copied into one of two slots and queued on EP2 IN.
- **Reset**: the connect and disconnect callbacks cancel both EP2 queues with a bounded flush (the stock disconnect polls without a bound), drop buffers
  that were full and not taken, and change the reset count of `usb_bulk_state`.
- **Emulator**: the controller is not modelled. `usb.bulk.in` and `usb.bulk.out` (`docs/CONTROL.md`) complete the dTDs through the guest's own callbacks.
  Test: `tests/usb-bulk.device.test.ts` with the fixture `test/usb-echo`.
