use crate::common::*;
use coldfire::{Bus, Cpu, InterruptPolicy};
use periph::peer::PeerIrq;

/// EPORT interrupt enable register (EPIER); IRQ4 is bit 4.
pub(crate) const EPORT_EPIER: u32 = 0xfc09_0003;

pub(crate) const PEER_IRQ4_VECTOR: u16 = 68;

pub(crate) const PEER_EDMA29_VECTOR: u16 = 149;

/// The mailbox peer's due interrupt (`periph::peer`), delivered like a hardware source: ICR level above the CPU IPL, source unmasked in
/// IMR, IRQ4 enabled in EPIER, handler installed. The hello block goes into the window just before. -> whether it was delivered; when
/// it is not deliverable yet it stays queued.
pub(crate) fn service_peer<const TRACE: bool>(
    bus: &mut LoggingBus<TRACE>,
    cpu: &mut Cpu,
    now: u64,
) -> Result<bool, String> {
    let due: Vec<PeerIrq> = bus
        .board
        .peer()
        .map(|peer| peer.due_list(now).collect())
        .unwrap_or_default();
    for irq in due {
        if offer_peer_irq(bus, cpu, irq)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn offer_peer_irq<const TRACE: bool>(
    bus: &mut LoggingBus<TRACE>,
    cpu: &mut Cpu,
    irq: PeerIrq,
) -> Result<bool, String> {
    let vector = match irq {
        PeerIrq::Eport4 => PEER_IRQ4_VECTOR,
        PeerIrq::Edma29 => PEER_EDMA29_VECTOR,
        PeerIrq::Frame => bus.board.peer().map_or(174, |peer| peer.frame_vector),
    };
    let Some(level) = bus
        .board
        .time()
        .and_then(|time| time.interrupt_level(vector))
    else {
        return Ok(false);
    };
    if ((cpu.sr >> 8) & 7) >= u16::from(level) {
        return Ok(false);
    }
    if irq == PeerIrq::Eport4
        && bus
            .board
            .read8(EPORT_EPIER)
            .map_err(|_| "PeerEportUnavailable".to_string())?
            & 0x10
            == 0
    {
        return Ok(false);
    }
    let handler = bus
        .board
        .read32(cpu.ctrl.vbr.wrapping_add(4 * u32::from(vector)))
        .map_err(|_| "PeerVectorUnavailable".to_string())?;
    if handler == 0 || handler >= 0x4800_0000 {
        return Ok(false);
    }
    let stack = if cpu.sr & 0x2000 == 0 && cpu.ctrl.cacr & 0x20 != 0 {
        cpu.other_a7
    } else {
        cpu.a[7]
    };
    let frame = (stack & !3).wrapping_sub(8);
    if !bus.board.can_write_ram_range(frame, 4)
        || !bus.board.can_write_ram_range(frame.wrapping_add(4), 4)
    {
        return Err(format!("PeerInterruptFrameUnavailable({frame:#010x})"));
    }
    if let Some(hello) = bus.board.peer().and_then(|peer| peer.window_before(irq)) {
        bus.board.peer_window_store(0, &hello);
    }
    match cpu.take_interrupt(
        &mut bus.board,
        vector as u8,
        Some(level),
        InterruptPolicy::Oracle,
    ) {
        Ok(true) => {
            if let Some(peer) = bus.board.peer_mut() {
                peer.delivered(irq);
            }
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(stop) => Err(format!("PeerDeliveryPoisoned({stop:?})")),
    }
}
