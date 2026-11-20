# core/usb-bulk-audio

A vendor bulk OUT interface (interface 5, EP2 OUT, 512-byte packets) in the firmware's USB AUDIO/MIDI configuration (port entry 5), for the usb-fast upload path. No mode switch and no controller
stop: macOS keeps audio and MIDI, and the host sees one more interface. Plan: `docs/plans/usb-fast-audio-bulk.md`; hardware probe result and facts: `docs/findings/123-usb-takeover.md`,
`docs/findings/42-usb-transfer-routes.md` section 4.

- **Hook.** The module hooks the entry of the firmware's USB init (`cf.usb.hook.init`, claim `cf.usb.site.init`; same code at the same address on DT2 1.17 and DN2 1.12). The init runs at boot
  and again from the firmware tick (interrupt context) after a controller reset, so `ua_prepare` is ISR-safe, idempotent and makes no runtime calls. It never stops or restarts the controller.
- **`ua_prepare`.** It checks entry 5 (USB AUDIO/MIDI: length 0x148, a configuration header of 9 bytes, type 2, 5 interfaces; wTotalLength is not checked, the firmware fills it in after the
  hook) and that its static buffer lies in the cached DDR window. If not, `usb_bulk_audio_state` is 2 and nothing is written (USB stays stock). Otherwise it copies the 0x148 descriptor bytes to
  a static buffer, appends interface 5 (vendor class, one endpoint: EP2 OUT, bulk, 512 bytes; EP2 OUT is unused by the stock configuration, EP2 IN stays the audio feedback), sets wTotalLength
  0x0158 and bNumInterfaces 6, and repoints the entry's connect, disconnect, descriptor pointer and length. `usb_bulk_audio_state`: 0 not prepared, 1 prepared, 2 refused.
- **Wrappers.** `ua_connect` (the host's bus reset or SET_CONFIGURATION) runs the stock connect, then `usb_bulk_port5_connect`. `ua_disconnect` calls `usb_bulk_port5_disconnect`, then the stock
  disconnect. Both run in the USB interrupt.
- **Receive side: core/usb-bulk.** `ua_prepare` calls `usb_bulk_port5_attach()` once the entry is ours. In USB AUDIO/MIDI with it, core/usb-bulk treats port 5 as bulk capable, OUT only: the queue
  head caps of EP2 OUT and the receive half of ENDPTCTRL2 (mask 0x0000ffff; the transmit half is the firmware's feedback endpoint) are set at connect, the receive ring runs as in USB MIDI mode, and
  bit `DK_USB_BULK_OUT_ONLY` (0x40) of `usb_bulk_state` is set. `usb_bulk_send` returns E_STATE there. A bus reset drops the dTDs without callbacks: every connect rebuilds the ring. Details:
  the README of core/usb-bulk.
- **Exports** (symbols of this module, called by patchkit's link and by tests): `void ua_prepare(void)`, `void ua_connect(void)`, `void ua_disconnect(void)`, `volatile u32 usb_bulk_audio_state`.
  Used from core/usb-bulk (symbols of that module): `usb_bulk_port5_attach`, `usb_bulk_port5_connect`, `usb_bulk_port5_disconnect`. The DDR window and the uncached alias come from the runtime's device object `dkrt_dev`.
- **Windows (MS OS 2.0).** So that Windows binds WinUSB to interface 5, a second hook (a `before` hook, `cf.usb.hook.setup_dispatch`, claim `cf.usb.fn.setup_dispatch`) sits on the entry of the EP0
  setup dispatcher: the stub `ua_setup_hook` calls `ua_setup_handle`, and returns to the USB interrupt if it answered, else runs the stock dispatcher from its first instruction. Only when the active port is 5 and `usb_bulk_audio_state` is 1 (attached), it answers three requests through the firmware's EP0 send: GET_DESCRIPTOR
  device (port 5's descriptor with bcdUSB 0x0201 and bcdDevice 0x0101), GET_DESCRIPTOR BOS (33 bytes), and the vendor request 0xC0, bRequest 0x44 (the vendor code), wIndex 7 (the 178-byte
  descriptor set: compatible ID WINUSB and interface GUID `{7F45739F-0563-45C1-823A-43A14AD7F745}` for interface 5). Each answer is cut to wLength. Every other request, and every request in
  another port or when not attached, goes to the stock dispatcher unchanged. No runtime calls. Design and bytes: `docs/plans/usb-winusb.md`; read them back from a device with `dkbulk descriptors`.
- **Patch sets.** It is in the `device` and `device-introblank` sets (`devices/digitakt-ii.toml`, `devices/digitone-ii.toml`).
