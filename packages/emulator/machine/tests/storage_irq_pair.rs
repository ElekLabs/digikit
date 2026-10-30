//! Firmware-free, bounded proof of the explicit INTC2 storage pair.
use coldfire::{Bus, Cpu};
use emmc_card::{Card, CardIdentity};
use machine::runner::StorageStepError;
use machine::{
    Board, CompletionEvent, CompletionPolicy, Machine, SemaphoreAddresses, Time, TimerPolicy,
};
use periph::{edma, esdhc, intc};

const RAM: u32 = 0x4000_0000;
const CODE: u32 = RAM + 0x1000;
const SD_HANDLER: u32 = RAM + 0x2000;
const DMA_HANDLER: u32 = RAM + 0x2100;
const DEST: u32 = RAM + 0x3000;
const INTC: u32 = intc::BASES[2];
const DMA59: u32 = 1 << 27;

fn setup(policy: CompletionPolicy, timer: Option<TimerPolicy>) -> Machine {
    let mut board = Board::new(
        Card::with_backing(CardIdentity::Mlc4G, None).unwrap(),
        SemaphoreAddresses {
            dma_sem: Some(RAM + 0x4000),
            data_sem: Some(RAM + 0x4100),
            cmd_sem: Some(RAM + 0x4200),
            status: Some(RAM + 0x4300),
        },
        policy,
    );
    board.map_ram_page(RAM).unwrap();
    for off in (0..32).step_by(2) {
        board.write16(CODE + off, 0x4e71).unwrap();
    }
    for (vector, handler) in [(223, SD_HANDLER), (192, DMA_HANDLER)] {
        board.write32(RAM + vector * 4, handler).unwrap();
        board.write16(handler, 0x4e71).unwrap(); // NOP; RTE on next boundary
        board.write16(handler + 2, 0x4e73).unwrap();
    }
    if let Some(timer) = timer {
        board.attach_time(Time::new(timer, vec![], 1_000_000.0));
        board.write32(INTC + 0x0c, u32::MAX).unwrap();
        for source in [0, 31] {
            board.write8(INTC + 0x40 + source, 3).unwrap();
            board.write8(INTC + 0x1d, source as u8).unwrap();
        }
    }
    board.write32(esdhc::BASE + esdhc::IRQSTATEN, 3).unwrap();
    board.write32(esdhc::BASE + esdhc::IRQSIGEN, 3).unwrap();
    let mut cpu = Cpu::new();
    cpu.pc = CODE;
    cpu.sr = 0x2000;
    cpu.a[7] = RAM + 0x8000;
    cpu.ctrl.vbr = RAM;
    Machine::new(cpu, board)
}

fn device() -> Machine {
    setup(CompletionPolicy::Device, Some(TimerPolicy::Device))
}

fn read_dma(m: &mut Machine, interrupt: bool) {
    let tcd = edma::TCD_BASE + 59 * 0x20;
    m.board.write32(tcd + edma::NBYTES as u32, 512).unwrap();
    m.board.write32(tcd + edma::DADDR as u32, DEST).unwrap();
    m.board.write16(tcd + edma::CITER as u32, 1).unwrap();
    m.board.write16(tcd + edma::BITER as u32, 1).unwrap();
    m.board
        .write16(
            tcd + edma::CSR as u32,
            if interrupt { edma::CSR_INT_MAJOR } else { 0 },
        )
        .unwrap();
    m.board.write32(DEST, 0xdead_beef).unwrap();
    m.board.write8(edma::SERQ, 59).unwrap();
    m.board
        .write32(
            esdhc::BASE + esdhc::XFERTYP,
            (18 << 24) | (1 << 21) | (1 << 4),
        )
        .unwrap();
}

fn drain(m: &mut Machine) {
    m.board.take_completion_events();
    m.board.take_dma_written_ranges();
}

fn pending(m: &mut Machine) -> u32 {
    m.board.read32(edma::INTH).unwrap()
}

#[test]
fn actual_copy_latches_tc_and_dma_then_equal_level_sd_wins() {
    let mut m = device();
    read_dma(&mut m, true);
    assert_eq!(m.board.read32(DEST).unwrap(), 0); // unbacked card reads zero
    assert_eq!(pending(&mut m), DMA59);
    assert_eq!(m.board.read32(esdhc::BASE + esdhc::IRQSTAT).unwrap() & 3, 3);
    let events = m.step_storage().unwrap(); // accepts queued board completion
    assert!(events.iter().any(|e| matches!(e, CompletionEvent::Dma59 { completion, .. } if completion.done && completion.major_interrupt)));
    assert_eq!(m.cpu.pc, SD_HANDLER + 2);
    m.board.write32(esdhc::BASE + esdhc::IRQSTAT, 3).unwrap();
    m.step_storage().unwrap(); // RTE
    m.step_storage().unwrap();
    assert_eq!(m.cpu.pc, DMA_HANDLER + 2);
    assert_eq!(m.irqs_delivered, 2);
    assert_eq!(pending(&mut m), DMA59); // entry does not acknowledge
    for addr in [RAM + 0x4000, RAM + 0x4100, RAM + 0x4200, RAM + 0x4300] {
        assert_eq!(m.board.read32(addr).unwrap(), 0);
    }
}

#[test]
fn higher_dma_level_wins_and_unacknowledged_request_repeats_after_rte() {
    let mut m = device();
    read_dma(&mut m, true);
    m.board.write8(INTC + 0x40, 4).unwrap();
    m.step_storage().unwrap();
    assert_eq!(m.cpu.pc, DMA_HANDLER + 2);
    m.step_storage().unwrap(); // RTE
    m.step_storage().unwrap();
    assert_eq!(m.cpu.pc, DMA_HANDLER + 2);
    assert_eq!(m.irqs_delivered, 2);
    m.board.write8(edma::CINT, 59).unwrap();
    m.step_storage().unwrap();
    m.step_storage().unwrap();
    assert_eq!(m.cpu.pc, SD_HANDLER + 2);
}

#[test]
fn mask_icr_and_cpu_ipl_defer_without_losing_dma_latch() {
    let mut m = device();
    read_dma(&mut m, true);
    m.board.write32(esdhc::BASE + esdhc::IRQSIGEN, 0).unwrap();
    m.board.write8(INTC + 0x1c, 0).unwrap();
    m.step_storage().unwrap();
    assert_eq!(m.irqs_delivered, 0);
    m.board.write8(INTC + 0x1d, 0).unwrap();
    m.board.write8(INTC + 0x40, 0).unwrap();
    m.step_storage().unwrap();
    assert_eq!(m.irqs_delivered, 0);
    m.board.write8(INTC + 0x40, 3).unwrap();
    m.cpu.sr = 0x2300;
    m.step_storage().unwrap();
    assert_eq!(m.irqs_delivered, 0);
    assert_eq!(pending(&mut m), DMA59);
    m.cpu.sr = 0x2000;
    m.step_storage().unwrap();
    assert_eq!(m.cpu.pc, DMA_HANDLER + 2);
}

#[test]
fn tc_requires_completed_dma_and_major_irq_requires_int_major() {
    let mut m = device();
    m.board
        .write32(
            esdhc::BASE + esdhc::XFERTYP,
            (18 << 24) | (1 << 21) | (1 << 4),
        )
        .unwrap();
    assert_eq!(m.board.read32(esdhc::BASE + esdhc::IRQSTAT).unwrap() & 2, 0);
    read_dma(&mut m, false);
    assert_eq!(m.board.read32(esdhc::BASE + esdhc::IRQSTAT).unwrap() & 2, 2);
    assert_eq!(pending(&mut m), 0);
    // CITER zero does not complete another data transfer.
    m.board.write32(esdhc::BASE + esdhc::IRQSTAT, 3).unwrap();
    m.board
        .write16(edma::TCD_BASE + 59 * 0x20 + edma::CITER as u32, 0)
        .unwrap();
    m.board
        .write32(
            esdhc::BASE + esdhc::XFERTYP,
            (18 << 24) | (1 << 21) | (1 << 4),
        )
        .unwrap();
    assert_eq!(m.board.read32(esdhc::BASE + esdhc::IRQSTAT).unwrap() & 2, 0);
}

#[test]
fn cint_and_w1c_acknowledge_only_selected_latches() {
    let mut m = device();
    // Seed unrelated register latches through the shared backing bank, as
    // another channel model would; guest writes themselves are W1C.
    m.board
        .dma
        .edma_regs
        .write(edma::INTH, 4, DMA59 | (1 << 28) | 1);
    m.board.dma.edma_regs.write(edma::INTL, 4, 3);
    m.board.write8(edma::CINT, 0x80 | 59).unwrap();
    assert_eq!(pending(&mut m), DMA59 | (1 << 28) | 1);
    m.board.write8(edma::CINT, 58).unwrap();
    assert_eq!(pending(&mut m), DMA59 | (1 << 28) | 1);
    m.board.write8(edma::CINT, 59).unwrap();
    assert_eq!(pending(&mut m), (1 << 28) | 1);
    assert_eq!(m.board.read32(edma::INTL).unwrap(), 3);
    m.board.write8(edma::INTH, 0x10).unwrap(); // BE high byte clears channel60
    assert_eq!(pending(&mut m), 1);
    m.board.write16(edma::INTL + 2, 2).unwrap();
    assert_eq!(m.board.read32(edma::INTL).unwrap(), 1);
    let before = pending(&mut m);
    assert!(m.board.write_guest(edma::INTH + 3, 2, 0xffff).is_err());
    assert_eq!(pending(&mut m), before);
    assert_eq!(m.board.read32(edma::INTL).unwrap(), 1);
    m.board.write8(edma::CINT, 0xc0).unwrap(); // NOP takes precedence over CAIR
    assert_eq!(pending(&mut m), 1);
    m.board.write8(edma::CINT, 0x40).unwrap();
    assert_eq!(pending(&mut m), 0);
    assert_eq!(m.board.read32(edma::INTL).unwrap(), 0);
}

#[test]
fn register_pending_survives_existing_board_snapshot() {
    let mut m = device();
    read_dma(&mut m, true);
    drain(&mut m);
    let mut writer = periph::snap::Writer::new();
    m.board.snap_save(&mut writer).unwrap();
    let mut restored = device();
    restored
        .board
        .snap_load(&mut periph::snap::Reader::new(&writer.buf))
        .unwrap();
    assert_eq!(pending(&mut restored), DMA59);
    restored.step_storage().unwrap();
    assert_eq!(restored.cpu.pc, SD_HANDLER + 2);
}

fn reject_unchanged(m: &mut Machine, error: StorageStepError) {
    let regs = (
        m.cpu.pc,
        m.cpu.sr,
        m.cpu.a,
        m.cpu.d,
        m.clock,
        m.irqs_delivered,
    );
    let events = m.board.completion_events().to_vec();
    let inth = pending(m);
    let status = m.board.read32(esdhc::BASE + esdhc::IRQSTAT).unwrap();
    assert_eq!(m.step_storage(), Err(error));
    assert_eq!(
        regs,
        (
            m.cpu.pc,
            m.cpu.sr,
            m.cpu.a,
            m.cpu.d,
            m.clock,
            m.irqs_delivered
        )
    );
    assert_eq!(m.board.completion_events(), events);
    assert_eq!(pending(m), inth);
    assert_eq!(
        m.board.read32(esdhc::BASE + esdhc::IRQSTAT).unwrap(),
        status
    );
}

#[test]
fn route_preflight_rejects_before_draining_queued_effects() {
    reject_unchanged(
        &mut setup(CompletionPolicy::Oracle, Some(TimerPolicy::Device)),
        StorageStepError::DeviceCompletionRequired,
    );
    reject_unchanged(
        &mut setup(CompletionPolicy::Device, None),
        StorageStepError::TimerNotAttached,
    );
    reject_unchanged(
        &mut setup(CompletionPolicy::Device, Some(TimerPolicy::Oracle)),
        StorageStepError::DeviceTimerRequired,
    );
    let mut m = device();
    read_dma(&mut m, true);
    m.configure_dma_irq(Some((210, 3))).unwrap();
    reject_unchanged(&mut m, StorageStepError::LegacyDmaIrqConfigured);
    m.configure_dma_irq(None).unwrap();
    // Rejection retained both event and external-write queues.
    assert!(!m.board.take_dma_written_ranges().is_empty());
    m.step().unwrap(); // ordinary API creates its legacy pending bool
    reject_unchanged(&mut m, StorageStepError::LegacyDmaIrqPending);
    let mut m = device();
    read_dma(&mut m, true);
    m.board
        .dma
        .edma_regs
        .write(edma::INTH, 4, DMA59 | (1 << 24));
    reject_unchanged(
        &mut m,
        StorageStepError::UnsupportedDmaGroupSources { pending: 1 << 24 },
    );
    assert!(!m.board.take_dma_written_ranges().is_empty());
}

#[test]
fn oracle_interrupt_register_writes_keep_passthrough_behavior() {
    let mut m = setup(CompletionPolicy::Oracle, Some(TimerPolicy::Oracle));
    m.board.write32(edma::INTH, DMA59).unwrap();
    m.board.write8(edma::CINT, 59).unwrap();
    assert_eq!(pending(&mut m), DMA59);
    m.board.write32(edma::INTH, 1).unwrap();
    assert_eq!(pending(&mut m), 1);
    assert_eq!(m.board.read32(INTC + 0x0c).unwrap(), u32::MAX);
}

#[test]
fn queued_dma_invalidates_decoded_code_before_handler_or_normal_instruction() {
    for deliver in [false, true] {
        let mut m = device();
        let mut payload = [0u8; 512];
        payload[..2].copy_from_slice(&0x7002u16.to_be_bytes()); // MOVEQ #2,D0
        m.board
            .esdhc
            .card_mut()
            .write_data(25, 0, &payload)
            .unwrap();
        m.board.write16(DEST, 0x7001).unwrap(); // MOVEQ #1,D0
        m.cpu.pc = DEST;
        m.cpu.step(&mut m.board).unwrap(); // warm CPU decode cache
        assert_eq!(m.cpu.d[0], 1);
        m.cpu.pc = if deliver { CODE } else { DEST };
        read_dma(&mut m, true);
        m.board.write32(esdhc::BASE + esdhc::IRQSIGEN, 0).unwrap();
        if deliver {
            m.board.write32(RAM + 192 * 4, DEST).unwrap();
        } else {
            m.board.write8(INTC + 0x1c, 0).unwrap();
        }
        m.step_storage().unwrap();
        assert_eq!(m.cpu.d[0], 2);
        assert_eq!(m.irqs_delivered, u64::from(deliver));
    }
}

#[test]
fn guest_dma_completion_does_not_feed_legacy_latch_and_esdhc_rejects_live_dma() {
    let mut m = device();
    read_dma(&mut m, true);
    m.board.write32(esdhc::BASE + esdhc::IRQSIGEN, 0).unwrap();
    m.board.write8(INTC + 0x1c, 0).unwrap();
    m.step_storage().unwrap(); // drains queued events without a legacy latch
    let pc = m.cpu.pc;
    let clock = m.clock;
    assert_eq!(
        m.step_esdhc(),
        Err(machine::runner::EsdhcStepError::LegacyDmaIrqPending)
    );
    assert_eq!((m.cpu.pc, m.clock), (pc, clock));
    // Generate the next DMA completion from a guest instruction through the
    // new route, rather than from a host Board write.
    m.board.write8(edma::CINT, 59).unwrap();
    m.board.write16(CODE + 2, 0x23c0).unwrap(); // MOVE.L D0,(XFERTYP).L
    m.board
        .write32(CODE + 4, esdhc::BASE + esdhc::XFERTYP)
        .unwrap();
    m.cpu.d[0] = (18 << 24) | (1 << 21) | (1 << 4);
    m.step_storage().unwrap();
    assert_eq!(pending(&mut m), DMA59);
    m.board.write8(edma::CINT, 59).unwrap();
    m.step_storage().unwrap(); // must not reject a legacy pending bool
    assert_eq!(m.irqs_delivered, 0);
    m.board.dma.edma_regs.write(edma::INTH, 4, 1 << 24);
    assert_eq!(
        m.step_esdhc(),
        Err(machine::runner::EsdhcStepError::LegacyDmaIrqPending)
    );
}
