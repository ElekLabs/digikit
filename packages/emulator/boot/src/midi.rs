//! MIDI DIN port of the main firmware (UART9; finding 25).
//!
//! Input: the firmware's driver (DT2 1.16 `FUN_40002844`, the same code at the same address on DN2 1.11) receives
//! through eDMA channel 36 (UART9 data register to a 32 KiB modulo ring, one byte per major loop, an interrupt per
//! byte on INTC1 source 28, vector 156); the interrupt routine drains the ring up to the channel's DADDR into the
//! MIDI parser. The emulator has no UART9 model: it does what the channel would do (store the byte into the
//! ring, advance DADDR) and offers the vector, one byte per wire time. Output: the driver stores bytes straight
//! into the data register (its status register reads back the value the driver wrote, 0xdd: always ready), which
//! `Board` records. DN2 1.11 also sends through a 4 KiB ring that eDMA channel 37 drains into the data register
//! (the ring write `FUN_40002612`, the completion handler `0x40002102` on vector 157). `Board` runs the channel's
//! major loop at its SERQ write and records the bytes. The runtime offers vector 157 (INTC1 source 29) once the
//! bytes' wire time has passed, so the driver can start its next transfer.

use std::collections::VecDeque;

use serde::Serialize;

use crate::common::{data_mask, unique};

/// eDMA channel 36 transfer control descriptor.
pub(crate) const TCD36_BASE: u32 = 0xfc04_5480;
pub(crate) const TCD36_DADDR: u32 = TCD36_BASE + 0x10;
/// INTC1 source 28 (eDMA channel 36 major loop done).
pub(crate) const RX_SOURCE: u32 = 28;
pub(crate) const RX_VECTOR: u8 = 128 + RX_SOURCE as u8;
/// One MIDI byte on the wire: 10 bits at 31250 baud = 320 us = 42,240 instructions at 132 M instructions per second.
pub const WIRE_PERIOD: u64 = 42_240;
/// Longest input queue.
pub const QUEUE_MAX: usize = 1 << 20;

/// A USB MIDI byte stream handed to the firmware's USB receive callback (finding 25). The USB device is not modelled:
/// the bytes enter where the USB endpoint handler (`FUN_40004504` on DT2 1.16) would pass the de-multiplexed
/// cable-0 bytes, and replies are captured where the firmware hands bytes to the USB transmit function
/// (`FUN_40004734`).
#[derive(Default)]
pub(crate) struct UsbPort {
    pub enabled: bool,
    /// `DIGI_USB_TRACE`: log every hand-over to the callback and every call of the transmit function (instruction count, length, hash, caller) to stderr.
    pub trace: bool,
    /// `DIGI_USB_TRACE_PCS`: more addresses whose execution the trace logs (instruction count, SP, the stack top, d0, a0).
    pub trace_pcs: Vec<u32>,
    pub rx_cb: u32,
    pub tx_fn: u32,
    /// An `rte` the callback returns to (resolved from the MIDI receive ISR).
    pub ret_pc: u32,
    /// The two .bss slots the registration function stores its callbacks in (`resolve_usb_slots`). `None`: the image has another registration function, delivery is not gated.
    pub slots: Option<(u32, u32)>,
    /// The last `service_usb` check found the callbacks not registered: no call is due, so the fast loop does not single-step (false until checked).
    pub unregistered: bool,
    pub queue: VecDeque<UsbItem>,
    pub next_ic: u64,
    pub period: u64,
    pub active: Option<UsbCall>,
    pub out: Vec<u8>,
    pub in_bytes: u64,
    pub calls: u64,
    /// The vendor bulk interface of `core/usb-bulk` (EP2): the firmware's dTD queue function, watched for transfers queued on EP2 IN. 0: not armed.
    pub bulk_queue_fn: u32,
    /// What the guest queued on EP2 IN, in order, not yet taken.
    pub bulk_out: Vec<u8>,
    pub bulk_in_bytes: u64,
    pub bulk_out_bytes: u64,
    /// Times the host found no dTD queued on EP2 OUT and tried again a period later (the controller's NAK).
    pub bulk_naks: u64,
    /// Zero-length packets the controller sent after a transfer of a multiple of 512 bytes on EP2 IN (zero-length termination on).
    pub bulk_out_zlps: u64,
    /// The controller's system error: set when a dTD made the DMA fault; no USB traffic after it, as on hardware, where only a module reset recovers.
    pub sys_error: Option<String>,
    /// `usb.ctrl`: the guest's EP0 send function, watched for the answer to a control request. 0: not armed.
    pub ctrl_send: u32,
    /// The (length, bytes) of each entry to `ctrl_send` since the last `usb.ctrl`.
    pub ctrl_sent: Vec<(u32, Vec<u8>)>,
}

/// FNV-1a over `bytes`: the payload id in the USB trace lines (`DIGI_USB_TRACE`).
pub(crate) fn trace_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// One delivery to the guest at interrupt level, in order (`midi.usb.in` and `usb.bulk.in` share the queue, the period and the interrupt).
pub(crate) enum UsbItem {
    /// MIDI bytes for the callback of the stock USB MIDI port.
    Midi(Vec<u8>),
    /// The rest of a host message on EP2 OUT: it fills the dTD at the head of the endpoint's queue (as many bytes as the dTD takes), and what is
    /// left stays queued for the next dTD. The flag: the message is a multiple of 512 bytes, so the host ends it with a zero-length packet. An empty message is that packet.
    Bulk(Vec<u8>, bool),
    /// A transfer the guest queued on EP2 IN (this dTD) is complete: the controller would have sent it.
    BulkTxDone(u32),
    /// A call of a guest function with arguments (`usb.bulk.in connect`: the bus reset the host causes, a callback's second half).
    Call(u32, Vec<u32>),
}

impl UsbItem {
    pub fn len(&self) -> usize {
        match self {
            UsbItem::Midi(b) | UsbItem::Bulk(b, _) => b.len(),
            _ => 0,
        }
    }
}

/// The firmware's USB queue function (queue index, dTD; `FUN_40005a86` on DT2 1.17), found by its prologue.
const QUEUE_TD_SIG: [u8; 8] = [0x4f, 0xef, 0xff, 0xec, 0x20, 0x2f, 0x00, 0x18];
/// The queue heads sit in the controller's SRAM at this base: the head of EP2 OUT's list of dTDs is a pointer at +0x30 of its queue head, its tail at +0x34.
pub(crate) const QH_BASE: u32 = 0x8000_0000;
pub(crate) const QH_EP2_OUT: u32 = QH_BASE + 0x100;
pub(crate) const QH_EP2_IN: u32 = QH_BASE + 0x140;

/// The address of the firmware's USB queue function in the image, when its prologue is unique.
pub(crate) fn resolve_queue_td(main: &[u8]) -> Option<u32> {
    unique_at(main, &QUEUE_TD_SIG)
}

/// The address of the only place in `main` where `sig` matches, with absolute addresses inside MAIN masked out.
fn unique_at(main: &[u8], sig: &[u8]) -> Option<u32> {
    unique(main, sig, &data_mask(sig, None)).ok()
}

pub(crate) struct UsbCall {
    /// A7 right after the callback's `rts`.
    pub sp_after_rts: u32,
    /// A7 the interrupted code gets back at the `rte`.
    pub frame_sp: u32,
    /// d0, d1, a0, a1 of the interrupted code.
    pub saved: [u32; 4],
}

/// The registration function of the USB callbacks (`resolve_usb_reg`: `FUN_4000434a` on DT2 and DN2, `FUN_40004636` on Syntakt 1.42) stores its two
/// arguments in two .bss globals: `lea (4,sp),a0; move.l (a0),slot0.l; lea (8,sp),a0; move.l (a0),slot1.l; ...`. The slot addresses differ per image.
const USB_REG_STORE0: [u8; 6] = [0x41, 0xef, 0x00, 0x04, 0x23, 0xd0];
const USB_REG_STORE1: [u8; 6] = [0x41, 0xef, 0x00, 0x08, 0x23, 0xd0];

/// The addresses of the registration function's two slots, read from its code; `None` when the function is not found or its code is not the known shape.
/// The entry is found per image by its signature (`resolve_usb_reg`), like every other boot address of this crate.
pub(crate) fn resolve_usb_slots(main: &[u8], load: u32) -> Option<(u32, u32)> {
    let entry = resolve_usb_reg(main)?;
    let at = |addr: u32, n: usize| main.get(addr.checked_sub(load)? as usize..)?.get(..n);
    if at(entry, 6)? != USB_REG_STORE0 || at(entry + 10, 6)? != USB_REG_STORE1 {
        return None;
    }
    let slot0 = u32::from_be_bytes(at(entry + 6, 4)?.try_into().ok()?);
    let slot1 = u32::from_be_bytes(at(entry + 16, 4)?.try_into().ok()?);
    Some((slot0, slot1))
}

/// The guest registered the callbacks in this boot: slot0 holds the receive callback and slot1 is set (it is stored after slot0, so the stores are done).
/// The slots are zero after the boot's bss clear.
pub(crate) fn usb_registered(slot0: u32, slot1: u32, rx_cb: u32) -> bool {
    rx_cb != 0 && slot0 == rx_cb && slot1 != 0
}

impl UsbPort {
    /// The guest restarted: forget what this boot learned (the cached return address, a call in progress).
    pub fn reset_boot(&mut self) {
        self.ret_pc = 0;
        self.active = None;
        self.unregistered = false;
    }
}

/// Longest chunk per callback call (a USB bulk packet).
pub const USB_CHUNK: usize = 64;
/// One USB full-speed frame: 1 ms.
pub const USB_PERIOD: u64 = 132_000;

/// The USB registration function: prologue and the first operands it uses, unique on every MAIN image of the registry.
const REG_SIG: [u8; 32] = [
    0x41, 0xef, 0x00, 0x04, 0x23, 0xd0, 0x40, 0x96, 0x59, 0xd0, 0x41, 0xef, 0x00, 0x08, 0x23, 0xd0,
    0x40, 0x96, 0x59, 0xd4, 0x20, 0x3c, 0x47, 0xdb, 0x40, 0x6c, 0x2f, 0x40, 0x00, 0x04, 0x4e, 0xf9,
];
/// The USB transmit function's prologue (unique on the same images).
const TX_SIG: [u8; 16] = [
    0x4f, 0xef, 0xff, 0xe4, 0x72, 0x01, 0x48, 0xd7, 0x0c, 0x7c, 0x20, 0x2f, 0x00, 0x20, 0x22, 0x6f,
];

/// The address of the USB transmit function.
pub(crate) fn resolve_usb_tx(main: &[u8]) -> Option<u32> {
    unique_at(main, &TX_SIG)
}

/// The address of the USB registration function.
pub(crate) fn resolve_usb_reg(main: &[u8]) -> Option<u32> {
    unique_at(main, &REG_SIG)
}

/// The USB receive callback and transmit function of a MAIN image: the callback is the first argument of the call to
/// the registration function, the transmit function is found by its prologue. Both are found by unique signatures.
pub(crate) fn resolve_usb(main: &[u8]) -> Option<(u32, u32)> {
    let tx = resolve_usb_tx(main)?;
    let reg = resolve_usb_reg(main)?;
    let mut call = vec![0x4e, 0xb9];
    call.extend_from_slice(&reg.to_be_bytes());
    let call = main.windows(call.len()).position(|w| w == call)?;
    // pea.l cb2; pea.l cb1; jsr reg
    let pre = main.get(call.checked_sub(12)?..call)?;
    if pre[0..2] != [0x48, 0x79] || pre[6..8] != [0x48, 0x79] {
        return None;
    }
    let cb1 = u32::from_be_bytes(pre[8..12].try_into().ok()?);
    Some((cb1, tx))
}

#[derive(Default)]
pub(crate) struct MidiPort {
    pub usb: UsbPort,
    pub queue: VecDeque<u8>,
    /// A byte is in the ring and the interrupt has not been taken yet.
    pub pending: bool,
    pub next_ic: u64,
    pub period: u64,
    pub rx_bytes: u64,
    pub rx_irqs: u64,
    pub tx_bytes: u64,
    /// An eDMA channel-37 transfer is on the wire: its completion interrupt is due at this count.
    pub tx_due: Option<u64>,
    /// Channel-37 transfers started, and their completion interrupts taken.
    pub tx_dma: u64,
    pub tx_irqs: u64,
}

impl MidiPort {
    #[inline]
    pub fn busy(&self) -> bool {
        self.pending
            || self.tx_due.is_some()
            || !self.queue.is_empty()
            || self.usb.active.is_some()
            || !self.usb.queue.is_empty()
    }

    /// Bytes on the DIN input still to hand over (every instruction boundary is a delivery chance for them, so the fast path stays off).
    #[inline]
    pub fn din_busy(&self) -> bool {
        self.pending || !self.queue.is_empty()
    }

    /// A DIN output transfer is on the wire: its completion interrupt is due in the returned number of instructions
    /// (`Some(0)`: due now). The fast paths run up to that count, as for the USB input.
    #[inline]
    pub fn tx_due_in(&self, icount: u64) -> Option<u64> {
        self.tx_due.map(|due| due.saturating_sub(icount))
    }

    /// The USB input is idle until the guest's instruction count reaches the returned value: the next callback call is not due (a call
    /// in progress, nothing queued, or the guest not registered yet, needs no deadline: `None`). `Some(0)` when a call is due now: step it.
    #[inline]
    pub fn usb_due_in(&self, icount: u64) -> Option<u64> {
        (self.usb.active.is_none() && !self.usb.queue.is_empty() && !self.usb.unregistered)
            .then(|| self.usb.next_ic.saturating_sub(icount))
    }
}

/// `midi.in` / `midi.out` counters.
#[derive(Clone, Debug, Serialize)]
pub struct MidiStatus {
    /// Bytes still queued for the guest.
    pub queued: usize,
    /// Bytes put into the guest's receive ring so far.
    pub rx_bytes: u64,
    /// Receive interrupts the guest took.
    pub rx_irqs: u64,
    /// Bytes the guest sent so far (taken with `take_midi_out`, or still pending).
    pub tx_bytes: u64,
    /// eDMA channel-37 transfers started, and their completion interrupts the guest took.
    pub tx_dma: u64,
    pub tx_irqs: u64,
    /// USB bytes still queued, handed over so far, callback calls, and bytes the firmware sent to USB (not yet taken).
    pub usb_queued: usize,
    pub usb_in_bytes: u64,
    pub usb_calls: u64,
    pub usb_out_bytes: usize,
    /// The vendor bulk interface: bytes the host sent (taken by the guest or pending), bytes the guest queued on EP2 IN, and the part not yet taken.
    pub bulk_in_bytes: u64,
    pub bulk_out_bytes: u64,
    pub bulk_out_pending: usize,
    /// Times EP2 OUT answered NAK: no dTD primed, or the endpoint's receive side is off.
    pub bulk_naks: u64,
    pub bulk_out_zlps: u64,
    /// The controller's system error (see `UsbPort::sys_error`): the reason, or none.
    pub usb_sys_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOAD: u32 = 0x4000_0400;

    fn image() -> Vec<u8> {
        let mut main = vec![0u8; 0x8000];
        // transmit function prologue at 0x40004734
        let tx = (0x4000_4734 - LOAD) as usize;
        main[tx..tx + 16].copy_from_slice(&TX_SIG);
        // the registration function at 0x4000434a
        let reg = (0x4000_434a - LOAD) as usize;
        main[reg..reg + REG_SIG.len()].copy_from_slice(&REG_SIG);
        // pea cb2; pea cb1; jsr 0x4000434a
        let call = [
            0x48, 0x79, 0x40, 0x11, 0xd9, 0xa0, 0x48, 0x79, 0x40, 0x11, 0xd7, 0x6c, 0x4e, 0xb9,
            0x40, 0x00, 0x43, 0x4a,
        ];
        main[0x100..0x100 + call.len()].copy_from_slice(&call);
        main
    }

    #[test]
    fn usb_callback_is_the_first_argument_of_the_registration_call() {
        assert_eq!(resolve_usb(&image()), Some((0x4011_d76c, 0x4000_4734)));
    }

    #[test]
    fn usb_input_waits_for_its_deadline_without_making_the_din_port_busy() {
        let mut port = MidiPort::default();
        assert_eq!(port.usb_due_in(100), None, "nothing queued");
        assert!(!port.busy() && !port.din_busy());
        port.usb.queue.push_back(UsbItem::Midi(vec![0xf0]));
        port.usb.next_ic = 1_000;
        assert_eq!(port.usb_due_in(100), Some(900));
        assert_eq!(port.usb_due_in(1_000), Some(0), "due now");
        assert_eq!(port.usb_due_in(5_000), Some(0), "late: due, never negative");
        assert!(
            port.busy() && !port.din_busy(),
            "the DIN fast-path gate is not the USB queue"
        );
        port.usb.active = Some(UsbCall {
            sp_after_rts: 0,
            frame_sp: 0,
            saved: [0; 4],
        });
        assert_eq!(
            port.usb_due_in(100),
            None,
            "a call in progress has no deadline"
        );
        port.usb.active = None;
        port.pending = true;
        assert!(port.din_busy());
    }

    #[test]
    fn usb_delivery_waits_for_both_slots() {
        let cb = 0x4011_d76c;
        assert!(!usb_registered(0, 0, cb), "bss clear: nothing registered");
        assert!(!usb_registered(cb, 0, cb), "slot1 is stored after slot0");
        assert!(
            !usb_registered(0x4011_0000, 0x4011_0004, cb),
            "another callback"
        );
        assert!(usb_registered(cb, 0x4011_d9a0, cb));
        assert!(!usb_registered(0, 5, 0), "no callback resolved");
    }

    #[test]
    fn usb_slots_are_read_from_the_registration_code() {
        let mut main = image();
        let at = (0x4000_434a - LOAD) as usize;
        assert_eq!(
            resolve_usb_slots(&main, LOAD),
            Some((0x4096_59d0, 0x4096_59d4))
        );
        main[at + 10] = 0;
        assert_eq!(resolve_usb_slots(&main, LOAD), None);
        assert_eq!(resolve_usb_slots(&[0; 16], LOAD), None);
    }

    #[test]
    fn usb_boot_state_is_cleared_on_a_guest_reset() {
        let mut usb = UsbPort {
            ret_pc: 0x4000_2000,
            ..Default::default()
        };
        usb.active = Some(UsbCall {
            sp_after_rts: 0,
            frame_sp: 0,
            saved: [0; 4],
        });
        usb.reset_boot();
        assert_eq!(usb.ret_pc, 0);
        assert!(usb.active.is_none());
    }

    #[test]
    fn usb_resolution_refuses_an_unknown_image() {
        let mut main = image();
        main[(0x4000_4734 - LOAD) as usize] = 0;
        assert_eq!(resolve_usb(&main), None);
        let mut main = image();
        main[0x100] = 0;
        assert_eq!(resolve_usb(&main), None);
        assert_eq!(resolve_usb(&[0; 16]), None);
    }

    #[test]
    fn port_is_busy_while_input_is_queued_or_a_call_is_open() {
        let mut port = MidiPort::default();
        assert!(!port.busy());
        port.queue.push_back(0xf0);
        assert!(port.busy());
        port.queue.clear();
        port.usb.queue.push_back(UsbItem::Midi(vec![1]));
        assert!(port.busy());
    }

    #[test]
    fn wire_period_is_320_microseconds_at_132_mips() {
        assert_eq!(WIRE_PERIOD, 320 * 132);
    }
}
