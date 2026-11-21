/* usb_bulk_port5.h: the USB AUDIO/MIDI (port 5) entry points of core/usb-bulk for core/usb-bulk-audio only. Not part of the module SDK.
 * All three make no runtime calls and are safe in the USB interrupt and in the boot hook (README.md "USB AUDIO/MIDI, OUT only"). */
#ifndef USB_BULK_PORT5_H
#define USB_BULK_PORT5_H

/* entry 5 now carries the OUT-only interface: marks port 5 bulk capable (set once, after the entry is repointed) */
void usb_bulk_port5_attach(void);
/* after the stock port 5 connect: EP2 OUT queue head and the receive half of ENDPTCTRL2 on, the ring queued when its buffers exist */
void usb_bulk_port5_connect(void);
/* before the stock port 5 disconnect: the receive half of ENDPTCTRL2 off, EP2 OUT flushed and cancelled, the ring idle */
void usb_bulk_port5_disconnect(void);

#endif
