//! DMA timer 0's counter as a free-running bus clock (`Board::enable_dtcn0`).

use coldfire::Bus;
use emmc_card::{Card, CardIdentity};
use machine::{Board, CompletionPolicy, DTCN0, SemaphoreAddresses, Time, TimerPolicy};

fn board() -> Board {
    let mut b = Board::new(
        Card::new(CardIdentity::Mlc4G).unwrap(),
        SemaphoreAddresses::default(),
        CompletionPolicy::Oracle,
    );
    b.attach_time(Time::with_dtims(
        TimerPolicy::Oracle,
        vec![3, 2, 0],
        vec![3, 1],
        periph::clock::BASE_IPS as f64,
    ));
    b
}

#[test]
fn dtcn0_counts_the_bus_clock_from_the_instruction_count() {
    let mut b = board();
    b.enable_dtcn0();
    b.set_clock(0);
    assert_eq!(b.read32(DTCN0), Ok(0));
    let ips = periph::clock::BASE_IPS;
    b.set_clock(ips / 1000); // 1 ms of emulated time
    assert_eq!(b.read32(DTCN0), Ok(132_000));
    b.set_clock(ips * 40); // past 2^32 ticks: the counter wraps
    assert_eq!(
        b.read32(DTCN0),
        Ok((132_000_000u64 * 40 % (1 << 32)) as u32)
    );
}

#[test]
fn without_the_model_dtcn0_is_a_plain_register() {
    let mut b = board();
    b.set_clock(periph::clock::BASE_IPS);
    assert_eq!(b.read32(DTCN0), Ok(0));
}

#[test]
fn enabled_channels_count_the_bus_clock_and_others_stay_plain() {
    let mut b = board();
    b.enable_dtcn_channels(0b0100); // DTCN2 only
    b.set_clock(periph::clock::BASE_IPS / 1000);
    assert_eq!(b.read32(0xfc07_800c), Ok(132_000));
    assert_eq!(b.read32(DTCN0), Ok(0));
}
