/* <dk/platform/usb_bulk.h> (needs core/usb-bulk): a vendor bulk interface to the host, beside the stock USB MIDI.
 *
 * `dk_usb_bulk_open(ctx, &ub)` in `init` gives the handle, or DK_E_MISSING when core/usb-bulk is not in the image (the module then loads and decides what to
 * do; a module that cannot run without it returns the code from `init`, and the load fails with E_IMPORT and shows core/usb-bulk as the module's need).
 * dk_usb_bulk_enable, _disable, _take, _release, _send, _state, _takeover, _giveback and _ring take the handle as the first argument.
 *
 *   dk_usb_bulk_open(ctx, &ub);          in init
 *   id = dk_io_post(step, done, 0);      in init
 *   dk_usb_bulk_enable(ub, id);          in the first step: adds interface 2 (vendor class 0xFF, EP2 OUT and EP2 IN, bulk, 512-byte packets) to the
 *                                        USB MIDI configuration and, in USB MIDI mode, re-enumerates once (the host sees the device leave and return)
 *   while (dk_usb_bulk_take(ub, &buf, &len)) { ...use buf[0..len)...; dk_usb_bulk_release(ub, buf); }
 *   dk_usb_bulk_send(ub, reply, n);
 *   return DK_IO_WAIT;                   the adapter wakes the job (dk_io_wake) when a buffer is full or a send slot is free
 *
 * Transfers. The host's OUT data arrives in buffers of up to 16 KiB (four 4 KiB pages, the same address space as the DMA: the pointer is in the uncached
 * alias of DDR). A buffer ends with the USB transfer: a full buffer, or a packet shorter than 512 bytes. A host frame whose length is a multiple of 512 bytes
 * must therefore be followed by a zero-length packet (the usual USB rule; without it the buffer never completes); the adapter drops the empty buffer. There are four buffers. When all are full or taken
 * the adapter queues nothing, the controller answers NAK, and the host waits: that is the flow control, no window protocol is needed. A message may span
 * buffers: the module's protocol (lengths in its header) puts the stream together.
 *
 * dk_usb_bulk_send copies up to 16 KiB into one of two send slots and returns at once (E_BUSY when both are in flight: return DK_IO_WAIT and try again
 * when woken; DK_USB_BULK_E_AGAIN when nothing is in flight and the engine refused: the adapter wakes the job within a few milliseconds).
 *
 * dk_usb_bulk_enable prepares the interface in any USB mode, and re-enumerates once when the device is in USB MIDI mode. It returns E_STATE (-53) while the device is
 * not in USB MIDI mode (USB AUDIO/MIDI uses EP2 IN for the audio clock feedback) or a mode switch is settling (the firmware restarts the device about a second after
 * it): the job calls it again later (a timed wake). Bit 2 of the state word shows the mode live.
 *
 * dk_usb_bulk_takeover and dk_usb_bulk_giveback are removed: both always return E_STATE (the import stays so that the table does not change). In USB AUDIO/MIDI with
 * core/usb-bulk-audio the bulk OUT interface works without a take-over (bit 6 of the state word).
 *
 * The adapter also wakes the job about four times a second while the interface is enabled (a timer for a module that has none: the retry of dk_usb_bulk_enable,
 * a check for an idle session).
 *
 * Memory. The adapter allocates its 160 KiB of buffers (a 256 KiB block of the firmware's allocator) when the interface can go live (dk_usb_bulk_enable)
 * and frees them only in dk_usb_bulk_disable (once EP2 is stopped). A host disconnect, suspend or USB CONFIG change does not free them: the module disables on its own rules.
 * dk_usb_bulk_take gives no buffer (returns 0) after dk_usb_bulk_disable.
 * After that DK_USB_BULK_LIVE is clear: call dk_usb_bulk_enable again (it allocates again; EP2 stays enabled meanwhile, so the host sees NAK, and nothing re-enumerates).
 *
 * One job owns the interface (dk_usb_bulk_enable from another job is E_BUSY); call dk_usb_bulk_disable(ub, id) in `fini`. The job is also woken when the bus is reset.
 *
 * A USB bus reset or a disconnect cancels every queued transfer and drops the buffers that were full and not taken; the reset count in the state word
 * changes. A buffer that is taken stays yours. The module's protocol must treat a changed reset count as the end of any transfer in progress.
 *
 * State word of dk_usb_bulk_state: */
#ifndef DK_USB_BULK_H
#define DK_USB_BULK_H

#include <dk/core/module.h>
#include <dk/core/status.h>

#define DK_USB_BULK_ENABLED 0x1u   /* dk_usb_bulk_enable was called */
#define DK_USB_BULK_CONNECTED 0x2u /* the host configured the device and EP2 is on */
#define DK_USB_BULK_WRONG_MODE                                                                     \
    0x4u /* the active USB port has no bulk route: not USB MIDI mode, and not USB AUDIO/MIDI with core/usb-bulk-audio; live, also after enable */
#define DK_USB_BULK_TAKEN 0x8u /* never set (take-over removed) */
#define DK_USB_BULK_LIVE                                                                           \
    0x10u /* the adapter's buffers are allocated; dk_usb_bulk_disable frees them and dk_usb_bulk_enable allocates them again */
#define DK_USB_BULK_NOMEM                                                                          \
    0x20u /* the last allocation of the buffers failed (dk_usb_bulk_enable returned E_NOMEM) */
#define DK_USB_BULK_OUT_ONLY                                                                       \
    0x40u /* USB AUDIO/MIDI with core/usb-bulk-audio is the active port: bulk OUT only (interface 5, EP2 OUT), dk_usb_bulk_send returns E_STATE; live */
#define DK_USB_BULK_FULL(st) (((st) >> 8) & 0xffu)     /* receive buffers full and not taken */
#define DK_USB_BULK_TX_BUSY(st) (((st) >> 16) & 0xffu) /* send slots in flight */
#define DK_USB_BULK_RESETS(st)                                                                     \
    (((st) >> 24) & 0xffu) /* changes with every bus reset or disconnect */

#define DK_USB_BULK_E_STATE                                                                        \
    DK_E_STATE /* dk_usb_bulk_enable: the device is not in USB MIDI mode (USB CONFIG); nothing was changed */
#define DK_USB_BULK_E_NOMEM                                                                        \
    DK_E_NOMEM /* dk_usb_bulk_enable: no memory for the buffers (also: no slot on the runtime's per-pass list) */
#define DK_USB_BULK_E_AGAIN                                                                        \
    DK_E_AGAIN /* dk_usb_bulk_send: nothing in flight, the engine refused; wait for the wake and call again */

/* dk_usb_bulk_ring: the receive ring (up to 60 buffers of 16 KiB, so the controller keeps receiving while the job writes the card). */
#define DK_USB_BULK_RING_ON(r) ((r) & 1) /* the run-time switch */
#define DK_USB_BULK_RING_HAVE(r)                                                                   \
    (((r) >> 1) & 1) /* 0: the adapter has no ring (the stub returned 0): the old behaviour */
#define DK_USB_BULK_RING_WINDOW(r)                                                                 \
    (((r) >> 8) & 0xff) /* frames of 64 KiB the live buffers take; 0: the old window of 2 */

#define DK_USB_BULK_RX_BYTES 16384u /* the largest buffer dk_usb_bulk_take gives */
#define DK_USB_BULK_TX_BYTES 16384u /* the most one dk_usb_bulk_send takes */

#endif
