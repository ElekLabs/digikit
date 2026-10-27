use coldfire::{Bus, Cpu};
use emmc_card::{Card, CardIdentity};
use machine::{Board, CompletionPolicy, SemaphoreAddresses, Time, TimerPolicy};
use periph::intc::BASES;

#[test]
fn guest_simr_cimr_writes_reach_native_interrupt_masks() {
    let mut board = Board::new(
        Card::new(CardIdentity::Mlc4G).unwrap(),
        SemaphoreAddresses::default(),
        CompletionPolicy::Oracle,
    );
    board.attach_time(Time::new(TimerPolicy::Oracle, vec![], 132_000_000.));
    const CODE: u32 = 0x4000_0400;
    board.map_ram_page(0x4000_0000).unwrap();
    let mut cpu = Cpu::new();
    cpu.sr = 0x2300; // The receive ISR lowers IPL below its source's level 4.
    for base in BASES {
        board.write32(base + 0x08, 0).unwrap();
        board.write32(base + 0x0c, 0).unwrap();
        for source in [28u8, 32, 63] {
            board.write8(base + 0x40 + u32::from(source), 4).unwrap();
            let offset = if source < 32 { 0x0c } else { 0x08 };
            for (command, expected) in [(0x1c, 1u32 << (source & 31)), (0x1d, 0)] {
                // MOVE.B D1,absolute-long through the actual ColdFire/Board bus.
                board.write16(CODE, 0x13c1).unwrap();
                board.write32(CODE + 2, base + command).unwrap();
                cpu.d[1] = u32::from(source);
                cpu.pc = CODE;
                cpu.invalidate_external_write(CODE, 6);
                cpu.step(&mut board).unwrap();
                assert_eq!(cpu.pc, CODE + 6);
                assert_eq!(board.read32(base + offset), Ok(expected));
                assert_eq!(board.read8(base + command), Ok(0));
                assert_eq!(board.read8(base + 0x40 + u32::from(source)), Ok(4));
                assert_eq!(board.read32(base + (offset ^ 4)), Ok(0));
            }
        }
    }
}

#[test]
fn native_all_mask_commands_override_source_and_leave_neighbor_registers() {
    let mut time = Time::new(TimerPolicy::Oracle, vec![], 132_000_000.);
    for base in BASES {
        time.write(base + 0x5c, 1, 4); // The MIDI RX source's ICR is unaffected.
        time.write(base + 0x1c, 1, 0x7f);
        assert_eq!(time.read(base + 0x08, 4), Some(u32::MAX));
        assert_eq!(time.read(base + 0x0c, 4), Some(u32::MAX));
        time.write(base + 0x1d, 1, 0x7f);
        assert_eq!(time.read(base + 0x08, 4), Some(0));
        assert_eq!(time.read(base + 0x0c, 4), Some(0));
        assert_eq!(time.read(base + 0x1c, 1), Some(0));
        assert_eq!(time.read(base + 0x1d, 1), Some(0));
        assert_eq!(time.read(base + 0x5c, 1), Some(4));
    }
}
