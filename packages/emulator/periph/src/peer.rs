//! The second CPU's mailbox, answered without a second CPU.
//!
//! A two-CPU board (finding 136): MAIN streams the peer's boot images through DSPI2 (slave, eDMA channel 29) and talks to it through a
//! 128 KB FlexBus window, a GPIO doorbell (port A bit 4) and EPORT IRQ4. `MailboxPeer` is the peer's side as data transport only: it
//! consumes the image bytes, counts and keeps them, and answers every doorbell edge with one IRQ4 after a fixed delay. It runs no peer
//! code and models no FPGA logic. The reply sequence is MAIN's own state machine, read from the firmware (finding 70 "Peer link"):
//!
//! 1. The first transfer on channel 29 is the stage-1 loader, raw after a run of 0xFF bytes, with an interrupt on its major loop: eDMA
//!    interrupt, vector 149.
//! 2. The second transfer is `0xAA | length | payload` (the sound-engine program, which the stage-1 loader reads): when its frame is
//!    complete the peer has booted, IRQ4 #1.
//! 3. Each doorbell edge gets one IRQ4. Before IRQ4 #2 and #3 the window holds the hello block (`HO`, `HA`, 0xA5A5).
//!
//! The model is pure: the board feeds it events with the instruction count and takes due interrupts from it.

use std::collections::VecDeque;

use crate::snap::{Reader, Result as SnapResult, Writer};

/// MAIN's FlexBus window onto the peer (CS0): base and size.
pub const WINDOW_BASE: u32 = 0x1000_0000;
pub const WINDOW_BYTES: usize = 0x2_0000;
/// The eDMA channel that carries the images (DSPI2 slave transmit).
pub const STREAM_CHAN: usize = 29;
/// GPIO port A pin-data/set and clear registers, and the doorbell pin.
pub const PPDSDR_A: u32 = 0xEC09_4018;
pub const PCLRR_A: u32 = 0xEC09_4024;
const DOORBELL_BIT: u8 = 0x10;
/// Instructions between an event and the interrupt that answers it.
pub const REPLY_DELAY: u64 = 50_000;
/// The hello block MAIN checks before IRQ4 #2 and samples before IRQ4 #3: +0 `HO`, +2 `HA`, +4 0xA5A5.
pub const HELLO: [u8; 6] = [0x48, 0x4f, 0x48, 0x41, 0xa5, 0xa5];
/// IRQ4 numbers (1-based) that need the hello block in the window.
const HELLO_FROM: u64 = 2;
const HELLO_TO: u64 = 3;
/// 0xFF bytes in front of the stage-1 loader in MAIN's stream buffer.
const LEADER: usize = 8;
/// A frame longer than this is not a boot image (MAIN's own limit for the sound engine is 0x5fffb).
const MAX_IMAGE: usize = 0x6_0000;

/// An interrupt the peer raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerIrq {
    /// eDMA channel 29 major loop done: INTC1 source 21, vector 149.
    Edma29,
    /// EPORT IRQ4: INTC0 source 4, vector 68.
    Eport4,
    /// The audio frame: eDMA channel 54 major loop of MAIN's audio DMA, INTC1 source 46, vector 174 (peer CPU mode; finding 136).
    Frame,
}

/// MAIN ticks between audio frames at the base rate: 32 samples at 48 kHz is 1500 frames a second, at 132 M ticks a second. A device with
/// another instruction rate sets its own period (`MailboxPeer::set_frame_ticks`, `clock::frame_ticks`).
pub const FRAME_TICKS: u64 = 132_000_000 / 1500;

/// Reads `0xAA | length (4 bytes, big endian) | payload` out of the byte stream (what the stage-1 loader of the peer does).
#[derive(Default)]
struct FrameParser {
    header: Vec<u8>,
    payload: Vec<u8>,
    want: Option<usize>,
}

impl FrameParser {
    /// Feeds one byte; returns the payload when this byte completes a frame.
    fn feed(&mut self, byte: u8) -> Option<Vec<u8>> {
        match self.want {
            None => {
                if self.header.is_empty() {
                    if byte == 0xaa {
                        self.header.push(byte);
                    }
                    return None;
                }
                self.header.push(byte);
                if self.header.len() == 5 {
                    let len = u32::from_be_bytes(self.header[1..5].try_into().expect("4 bytes"));
                    self.header.clear();
                    let len = len as usize;
                    if len == 0 || len > MAX_IMAGE {
                        return None; // not a frame: look for the next marker
                    }
                    self.want = Some(len);
                    self.payload = Vec::with_capacity(len);
                }
                None
            }
            Some(len) => {
                self.payload.push(byte);
                if self.payload.len() == len {
                    self.want = None;
                    return Some(std::mem::take(&mut self.payload));
                }
                None
            }
        }
    }
}

#[derive(Default)]
pub struct MailboxPeer {
    parser: FrameParser,
    /// The images the peer was sent, in order: the stage-1 loader (raw bytes after the 0xFF run), then the framed payloads. Not part of a
    /// snapshot.
    pub images: Vec<Vec<u8>>,
    /// Transfers started on channel 29 (the first is raw, the later ones framed).
    transfers: u32,
    raw: Vec<u8>,
    image_lens: Vec<u32>,
    /// Bytes consumed from channel 29.
    pub stream_bytes: u64,
    pub doorbell_edges: u64,
    /// IRQ4 delivered so far (the next one is `irq4_delivered + 1`).
    pub irq4_delivered: u64,
    /// Audio frame interrupts delivered (peer CPU mode).
    pub frames_delivered: u64,
    pin_high: bool,
    queue: VecDeque<(u64, PeerIrq)>,
    /// `true` when a peer CPU answers MAIN (`machine::peer_cpu`): the responder keeps the image stream and the stage-1 "alive" interrupt, but
    /// the hello block and the replies to doorbell edges come from the peer's own code. A doorbell edge is then an input for the peer.
    pub cpu_mode: bool,
    /// Doorbell edges MAIN made that the peer CPU has not taken yet.
    edges_unread: u32,
    /// Peer CPU mode: the next audio frame is due at this MAIN tick; `frame_pending` while its interrupt waits for MAIN to take it.
    next_frame: u64,
    frame_pending: bool,
    /// Times the frame deadline lagged by a period or more and was moved to `now + frame_ticks` (a restore, or MAIN masked the frame
    /// interrupt for a long stretch) instead of firing the missed frames back to back.
    pub frame_resyncs: u64,
    /// The vector of the audio frame interrupt (INTC1: 120 + the eDMA channel), 174 unless the device names its audio channel.
    pub frame_vector: u16,
    /// MAIN ticks between audio frames ([`FRAME_TICKS`] unless the device sets another).
    frame_ticks: u64,
}

impl MailboxPeer {
    pub fn new() -> Self {
        Self {
            frame_vector: 174,
            frame_ticks: FRAME_TICKS,
            ..Self::default()
        }
    }

    /// Sets the audio frame period in MAIN ticks (the converter's frame rate at this machine's instruction rate).
    pub fn set_frame_ticks(&mut self, ticks: u64) {
        self.frame_ticks = ticks.max(1);
    }

    /// MAIN ticks between audio frames.
    pub fn frame_ticks(&self) -> u64 {
        self.frame_ticks
    }

    /// A new transfer starts on channel 29 (the stage-1 loader of the peer begins at a frame marker again).
    pub fn begin_transfer(&mut self) {
        self.parser = FrameParser::default();
        self.transfers += 1;
        self.raw.clear();
    }

    /// A major loop of channel 29 finished. The raw stage-1 transfer becomes an image here; with INT_MAJOR set the peer raises the channel
    /// interrupt.
    pub fn end_major_loop(&mut self, now: u64, int_major: bool) {
        if self.transfers == 1 && !self.raw.is_empty() {
            // MAIN puts eight 0xFF bytes in front of the loader (the loader itself may start with 0xFF).
            let image = self.raw.split_off(LEADER.min(self.raw.len()));
            self.raw.clear();
            self.image_lens.push(image.len() as u32);
            self.images.push(image);
        }
        if int_major {
            self.queue.push_back((now + REPLY_DELAY, PeerIrq::Edma29));
        }
    }

    /// Image bytes taken from channel 29. Returns true when the sound-engine image (the second one) is complete: the peer has booted.
    pub fn consume(&mut self, bytes: &[u8], now: u64) -> bool {
        self.stream_bytes += bytes.len() as u64;
        if self.transfers <= 1 {
            self.raw.extend_from_slice(bytes);
            return false;
        }
        let mut booted = false;
        for &byte in bytes {
            if let Some(image) = self.parser.feed(byte) {
                self.image_lens.push(image.len() as u32);
                self.images.push(image);
                if self.images.len() == 2 {
                    booted = true;
                }
            }
        }
        if booted {
            self.queue.push_back((now + REPLY_DELAY, PeerIrq::Eport4));
        }
        booted
    }

    /// Images complete so far.
    pub fn image_count(&self) -> usize {
        self.image_lens.len()
    }

    /// A one-byte store to GPIO port A. A change of the doorbell pin is one edge, and every edge is answered with IRQ4.
    pub fn gpio_write(&mut self, addr: u32, value: u8, now: u64) {
        let pin = match addr {
            PPDSDR_A if value & DOORBELL_BIT != 0 => true,
            PCLRR_A if value & DOORBELL_BIT == 0 => false,
            _ => return,
        };
        if pin != self.pin_high {
            self.pin_high = pin;
            self.doorbell_edges += 1;
            if self.cpu_mode {
                self.edges_unread += 1;
            } else {
                self.queue.push_back((now + REPLY_DELAY, PeerIrq::Eport4));
            }
        }
    }

    /// Whether doorbell edges wait for the peer CPU.
    pub fn has_edges(&self) -> bool {
        self.edges_unread != 0
    }

    /// Doorbell edges since the last call (peer CPU mode).
    pub fn take_edges(&mut self) -> u32 {
        std::mem::take(&mut self.edges_unread)
    }

    /// The peer CPU changed its output pin: MAIN gets one IRQ4 after the reply delay (EPORT IRQ4 sees the pin through the bridge).
    pub fn peer_pin_edge(&mut self, now: u64) {
        self.queue.push_back((now + REPLY_DELAY, PeerIrq::Eport4));
    }

    /// The sound-engine program the peer was sent (the second image: `0xAA | length | payload`), once complete.
    pub fn program(&self) -> Option<&[u8]> {
        self.images.get(1).map(Vec::as_slice)
    }

    /// The next interrupt that is due at `now`, without taking it.
    pub fn due(&self, now: u64) -> Option<PeerIrq> {
        self.queue
            .front()
            .filter(|(at, _)| *at <= now)
            .map(|(_, irq)| *irq)
    }

    /// Every interrupt that is due at `now` and may be offered to MAIN, in the order to try: the queue's first, then the audio frame
    /// (a frame that MAIN cannot take yet does not hold the queue back, nor the other way round).
    pub fn due_list(&self, now: u64) -> impl Iterator<Item = PeerIrq> {
        self.due(now)
            .into_iter()
            .chain(self.frame_pending.then_some(PeerIrq::Frame))
    }

    /// Peer CPU mode: the audio frame source. The frame interrupt becomes pending at exact multiples of `frame_ticks` of MAIN time (at
    /// most one waits). The next deadline moves by one period from the last one, not from `now`: the frame rate does not depend on how
    /// late the caller looked. The run loop stops at `next_deadline` so it never looks late. When the deadline is still due after the
    /// step (the caller looked a period or more late: a snapshot restored at a later tick, a long masked stretch), the missed frames are
    /// dropped: the deadline moves to `now + frame_ticks` and `frame_resyncs` counts it.
    pub fn frame_source(&mut self, now: u64) {
        if self.cpu_mode && now >= self.next_frame {
            self.frame_pending = true;
            self.next_frame += self.frame_ticks;
            if now >= self.next_frame {
                self.next_frame = now + self.frame_ticks;
                self.frame_resyncs += 1;
            }
        }
    }

    /// Peer CPU mode: the MAIN tick after `now` of the next event the run loop must stop at, the frame or the next queued reply.
    /// A deadline that has passed is not returned: its interrupt waits for MAIN, and stopping again would not move it.
    pub fn next_deadline(&self, now: u64) -> Option<u64> {
        if !self.cpu_mode {
            return None;
        }
        [Some(self.next_frame), self.queue.front().map(|(at, _)| *at)]
            .into_iter()
            .flatten()
            .filter(|at| *at > now)
            .min()
    }

    /// The window bytes to store at +0 before the due interrupt is delivered (the hello block for IRQ4 #2 and #3).
    pub fn window_before(&self, irq: PeerIrq) -> Option<[u8; 6]> {
        let number = self.irq4_delivered + 1;
        (!self.cpu_mode && irq == PeerIrq::Eport4 && (HELLO_FROM..=HELLO_TO).contains(&number))
            .then_some(HELLO)
    }

    /// The interrupt `irq` (from `due_list`) was delivered to the CPU.
    pub fn delivered(&mut self, irq: PeerIrq) {
        if irq == PeerIrq::Frame {
            self.frame_pending = false;
            self.frames_delivered += 1;
            return;
        }
        if let Some((_, queued)) = self.queue.pop_front()
            && queued == PeerIrq::Eport4
        {
            self.irq4_delivered += 1;
        }
    }

    /// Whether any interrupt is waiting (due or not).
    pub fn pending(&self) -> bool {
        !self.queue.is_empty() || self.frame_pending
    }

    /// Run state for a snapshot (section `PEER2`): the counters, the pin, the frame deadline, the queue and the image lengths (not the images).
    pub fn snap_save(&self, w: &mut Writer) {
        w.u64(self.stream_bytes);
        w.u64(self.doorbell_edges);
        w.u64(self.irq4_delivered);
        w.bool(self.pin_high);
        w.u64(self.next_frame);
        w.u64(self.queue.len() as u64);
        for (at, irq) in &self.queue {
            w.u64(*at);
            w.u8(match irq {
                PeerIrq::Edma29 => 0,
                PeerIrq::Eport4 => 1,
                PeerIrq::Frame => 2,
            });
        }
        w.u64(self.image_lens.len() as u64);
        for len in &self.image_lens {
            w.u32(*len);
        }
    }

    /// Loads the run state. `with_frame` is false for a snapshot of the older `PEER` section, which has no `next_frame`: the deadline
    /// restarts at 0 and the first `frame_source` call resyncs it.
    pub fn snap_load(&mut self, r: &mut Reader, with_frame: bool) -> SnapResult<()> {
        self.stream_bytes = r.u64()?;
        self.doorbell_edges = r.u64()?;
        self.irq4_delivered = r.u64()?;
        self.pin_high = r.bool()?;
        self.next_frame = if with_frame { r.u64()? } else { 0 };
        self.frame_pending = false;
        let n = r.len(1 << 12)?;
        self.queue = (0..n)
            .map(|_| {
                let at = r.u64()?;
                let irq = match r.u8()? {
                    0 => PeerIrq::Edma29,
                    1 => PeerIrq::Eport4,
                    _ => PeerIrq::Frame,
                };
                Ok((at, irq))
            })
            .collect::<SnapResult<_>>()?;
        let n = r.len(1 << 8)?;
        self.image_lens = (0..n).map(|_| r.u32()).collect::<SnapResult<_>>()?;
        self.images.clear();
        self.parser = FrameParser::default();
        self.transfers = self.image_lens.len().min(2) as u32;
        self.raw.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xff; 8];
        out.push(0xaa);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn stage1_is_raw_the_second_transfer_is_framed_and_boots() {
        let mut p = MailboxPeer::new();
        p.begin_transfer();
        assert!(!p.consume(&[0xff; 8], 0));
        assert!(!p.consume(&[0xff, 8, 9], 0));
        p.end_major_loop(5, true);
        assert_eq!(p.images, vec![vec![0xff, 8, 9]]);
        assert_eq!(p.due(5 + REPLY_DELAY), Some(PeerIrq::Edma29));
        p.delivered(PeerIrq::Edma29);
        p.begin_transfer();
        let two = framed(&[9; 20]);
        let (a, b) = two.split_at(11);
        assert!(!p.consume(a, 10));
        assert!(p.consume(b, 20));
        assert_eq!(p.images, vec![vec![0xff, 8, 9], vec![9; 20]]);
        assert_eq!(p.due(20), None);
        assert_eq!(p.due(20 + REPLY_DELAY), Some(PeerIrq::Eport4));
    }

    #[test]
    fn doorbell_edges_each_get_one_irq4_and_hello_comes_before_two_and_three() {
        let mut p = MailboxPeer::new();
        // A clear while the pin is low is no edge (the thread's own initial clear).
        p.gpio_write(PCLRR_A, 0xef, 0);
        assert_eq!(p.doorbell_edges, 0);
        let mut seen = vec![];
        for (i, (addr, value)) in [
            (PPDSDR_A, 0x10),
            (PCLRR_A, 0xef),
            (PPDSDR_A, 0x10),
            (PCLRR_A, 0xef),
        ]
        .into_iter()
        .enumerate()
        {
            let now = 1000 * i as u64;
            p.gpio_write(addr, value, now);
            let at = now + REPLY_DELAY;
            let irq = p.due(at).expect("due");
            seen.push(p.window_before(irq).is_some());
            p.delivered(irq);
        }
        // numbers 1..=4: hello before 2 and 3 only
        assert_eq!(seen, [false, true, true, false]);
        assert!(!p.pending());
        // A set of another pin is ignored.
        p.gpio_write(PPDSDR_A, 0x20, 0);
        assert!(!p.pending());
    }

    #[test]
    fn in_cpu_mode_a_doorbell_edge_is_an_input_for_the_peer_and_the_peer_pin_is_the_irq4_source() {
        let mut p = MailboxPeer::new();
        p.cpu_mode = true;
        p.gpio_write(PPDSDR_A, 0x10, 0);
        p.gpio_write(PCLRR_A, 0xef, 0);
        assert_eq!(p.doorbell_edges, 2);
        assert!(!p.pending(), "no reply without peer code");
        assert!(p.has_edges());
        assert_eq!(p.take_edges(), 2);
        assert!(!p.has_edges());
        p.peer_pin_edge(100);
        assert_eq!(p.due(100), None);
        let irq = p.due(100 + REPLY_DELAY).unwrap();
        assert_eq!(
            p.window_before(irq),
            None,
            "the hello block comes from the peer's own writes"
        );
    }

    #[test]
    fn the_audio_frame_is_pending_once_per_period_and_does_not_hold_back_the_queue() {
        let mut p = MailboxPeer::new();
        p.frame_source(0);
        assert!(!p.pending(), "only in peer CPU mode");
        p.cpu_mode = true;
        p.peer_pin_edge(0);
        p.frame_source(0);
        p.frame_source(FRAME_TICKS - 1);
        let due: Vec<_> = p.due_list(REPLY_DELAY).collect();
        assert_eq!(due, [PeerIrq::Eport4, PeerIrq::Frame]);
        p.delivered(PeerIrq::Frame);
        assert_eq!(p.frames_delivered, 1);
        assert_eq!(p.due_list(0).count(), 0);
        p.frame_source(FRAME_TICKS);
        assert_eq!(p.due_list(0).collect::<Vec<_>>(), [PeerIrq::Frame]);
        assert_eq!(FRAME_TICKS, 88_000);
    }

    #[test]
    fn frames_fall_on_exact_ticks_however_late_the_source_is_looked_at() {
        let mut p = MailboxPeer::new();
        p.cpu_mode = true;
        p.frame_source(0);
        p.delivered(PeerIrq::Frame);
        assert_eq!(p.next_deadline(0), Some(FRAME_TICKS));
        // looked at 100 ticks late: the next deadline is still the next multiple
        p.frame_source(FRAME_TICKS + 100);
        p.delivered(PeerIrq::Frame);
        assert_eq!(p.next_deadline(0), Some(2 * FRAME_TICKS));
        // a gap of more than a period: one frame, then the deadline restarts from now
        p.frame_source(5 * FRAME_TICKS);
        p.delivered(PeerIrq::Frame);
        assert_eq!(p.next_deadline(0), Some(6 * FRAME_TICKS));
        assert_eq!(p.frame_resyncs, 1);
        assert!(
            MailboxPeer::new().next_deadline(0).is_none(),
            "no peer CPU, no deadline"
        );
    }

    #[test]
    fn snapshot_keeps_counters_and_queue() {
        let mut p = MailboxPeer::new();
        p.gpio_write(PPDSDR_A, 0x10, 5);
        p.begin_transfer();
        p.end_major_loop(6, true);
        let mut w = Writer::new();
        p.snap_save(&mut w);
        let mut q = MailboxPeer::new();
        q.snap_load(&mut Reader::new(&w.buf), true).unwrap();
        assert_eq!(q.doorbell_edges, 1);
        assert_eq!(q.due(5 + REPLY_DELAY), Some(PeerIrq::Eport4));
        q.delivered(PeerIrq::Eport4);
        assert_eq!(q.due(6 + REPLY_DELAY), Some(PeerIrq::Edma29));
    }

    #[test]
    fn a_restore_at_a_large_tick_raises_one_frame_not_a_burst() {
        let mut p = MailboxPeer::new();
        p.cpu_mode = true;
        p.frame_source(10 * FRAME_TICKS);
        p.delivered(PeerIrq::Frame);
        let mut w = Writer::new();
        p.snap_save(&mut w);
        let mut q = MailboxPeer::new();
        q.cpu_mode = true;
        q.snap_load(&mut Reader::new(&w.buf), true).unwrap();
        let now = 1_000_000 * FRAME_TICKS;
        let mut taken = 0;
        for i in 0..100 {
            q.frame_source(now + i);
            if q.due_list(0).next().is_some() {
                q.delivered(PeerIrq::Frame);
                taken += 1;
            }
        }
        assert_eq!(taken, 1);
        assert_eq!(q.frame_resyncs, 1);
        // a snapshot of the older section has no deadline: same result
        let mut old = MailboxPeer::new();
        old.cpu_mode = true;
        old.snap_load(&mut Reader::new(&legacy(&p)), false).unwrap();
        old.frame_source(now);
        old.delivered(PeerIrq::Frame);
        old.frame_source(now + 1);
        assert_eq!(old.due_list(0).count(), 0);
        assert_eq!(old.frame_resyncs, 1);
    }

    /// The `PEER` section as older snapshots wrote it (no frame deadline).
    fn legacy(p: &MailboxPeer) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(p.stream_bytes);
        w.u64(p.doorbell_edges);
        w.u64(p.irq4_delivered);
        w.bool(p.pin_high);
        w.u64(0);
        w.u64(0);
        w.buf.clone()
    }

    #[test]
    fn a_long_mask_then_unmask_raises_one_frame() {
        let mut p = MailboxPeer::new();
        p.cpu_mode = true;
        p.frame_source(0); // pending, MAIN has the frame interrupt masked and does not take it
        p.frame_source(50 * FRAME_TICKS); // one look after a long stretch
        assert_eq!(p.frame_resyncs, 1, "the first look at tick 0 is on time");
        p.delivered(PeerIrq::Frame); // unmasked
        p.frame_source(50 * FRAME_TICKS);
        p.frame_source(51 * FRAME_TICKS - 1);
        assert_eq!(p.due_list(0).count(), 0);
        p.frame_source(51 * FRAME_TICKS);
        assert_eq!(p.due_list(0).count(), 1);
    }
}
