//! ColdFire instruction rate of the emulated machine.
//!
//! The core retires one instruction per bus tick by default, so a guest second is
//! [`BASE_IPS`] instructions (the 132 MHz bus; the real core runs at 264 MHz).
//! `DIGI_CF_IPS` changes the number of instructions per emulated second to test whether
//! firmware behaviour depends on the rate. Timers, SSI pacing and scenario instruction
//! counts follow it; the bus clock (`pit::F_BUS`) does not.
//!
//! A device may set its own default (`[emulator] instructions_per_second`: core clock over the busy CPI of its main program).
//! [`resolve_ips`] picks the rate once per emulator (`DIGI_CF_IPS`, the device default, [`BASE_IPS`]); the emulator keeps it and hands it
//! to the timers, the wire times, the frame periods and the controller, so two emulators in one process keep their own rates. A rate other than [`BASE_IPS`] is not the rate
//! of the ready snapshots: they are rescaled while they load (`Emulator::load_state`).

/// Instructions per emulated second when `DIGI_CF_IPS` is unset (and the rate every
/// ready snapshot was taken at).
pub const BASE_IPS: u64 = 132_000_000;

/// Parse a `DIGI_CF_IPS` value: a decimal integer from 1 000 000 to 4 000 000 000.
pub fn parse_ips(text: &str) -> Result<u64, String> {
    let value: u64 = text
        .trim()
        .parse()
        .map_err(|_| format!("DIGI_CF_IPS must be an integer, got {text:?}"))?;
    if !(1_000_000..=4_000_000_000).contains(&value) {
        return Err(format!(
            "DIGI_CF_IPS must be between 1000000 and 4000000000, got {value}"
        ));
    }
    Ok(value)
}

/// The `DIGI_CF_IPS` override, read once (native builds only). Panics on an invalid value so a typo cannot silently run the default
/// rate.
fn env_ips() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static IPS: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
        *IPS.get_or_init(|| {
            std::env::var("DIGI_CF_IPS")
                .ok()
                .map(|text| parse_ips(&text).unwrap_or_else(|e| panic!("{e}")))
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// The instruction rate of one emulator: `DIGI_CF_IPS`, else the device's default (`[emulator] instructions_per_second`), else
/// [`BASE_IPS`]. Resolved once when the emulator is created and stored in it; nothing reads a process-wide rate.
pub fn resolve_ips(device: Option<u64>) -> u64 {
    pick_ips(env_ips(), device.unwrap_or(0))
}

fn pick_ips(env: Option<u64>, device: u64) -> u64 {
    env.unwrap_or(if device == 0 { BASE_IPS } else { device })
}

/// MAIN ticks per audio frame of `frame_samples` samples at `sample_rate` Hz, at `ips` instructions a second (rounded to the nearest
/// tick: under 3 ppm of error at 190 M, and the deadline keeps the period, not the error).
pub fn frame_ticks(ips: u64, sample_rate: u32, frame_samples: u32) -> u64 {
    let rate = u128::from(sample_rate.max(1));
    ((u128::from(ips) * u128::from(frame_samples) + rate / 2) / rate).max(1) as u64
}

/// `n` instructions authored at [`BASE_IPS`] (emulated time) expressed at `ips`.
pub fn scale_instr(n: u64, ips: u64) -> u64 {
    if ips == BASE_IPS {
        return n;
    }
    ((n as u128 * ips as u128 + BASE_IPS as u128 / 2) / BASE_IPS as u128) as u64
}

/// Inverse of [`scale_instr`]: `n` instructions counted at `ips` expressed at [`BASE_IPS`].
pub fn unscale_instr(n: u64, ips: u64) -> u64 {
    if ips == BASE_IPS {
        return n;
    }
    ((n as u128 * BASE_IPS as u128 + ips as u128 / 2) / ips as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_validates() {
        assert_eq!(parse_ips("66000000"), Ok(66_000_000));
        assert!(parse_ips("abc").is_err());
        assert!(parse_ips("0").is_err());
        assert!(parse_ips("9999999999").is_err());
    }

    #[test]
    fn env_beats_device_beats_base() {
        assert_eq!(pick_ips(None, 0), BASE_IPS);
        assert_eq!(pick_ips(None, 189_928_000), 189_928_000);
        assert_eq!(pick_ips(Some(66_000_000), 189_928_000), 66_000_000);
    }

    #[test]
    fn resolve_keeps_device_rates_apart() {
        // no process-wide state: the same call with two devices gives each its own rate
        let (a, b) = (resolve_ips(Some(189_928_000)), resolve_ips(None));
        if env_ips().is_none() {
            assert_eq!((a, b), (189_928_000, BASE_IPS));
        }
    }

    #[test]
    fn frame_period_follows_rate() {
        assert_eq!(frame_ticks(BASE_IPS, 48_000, 32), 88_000);
        assert_eq!(frame_ticks(264_000_000, 48_000, 32), 176_000);
        // 189,928,000 * 32 / 48,000 = 126,618.67
        assert_eq!(frame_ticks(189_928_000, 48_000, 32), 126_619);
    }

    #[test]
    fn scale_keeps_emulated_time() {
        assert_eq!(scale_instr(1_234_567, BASE_IPS), 1_234_567);
        assert_eq!(scale_instr(132_000_000, 264_000_000), 264_000_000);
        assert_eq!(scale_instr(132_000_000, 33_000_000), 33_000_000);
        assert_eq!(scale_instr(3, 66_000_000), 2); // 1.5 rounds up
        assert_eq!(
            unscale_instr(scale_instr(1_000_000, 198_000_000), 198_000_000),
            1_000_000
        );
    }
}
