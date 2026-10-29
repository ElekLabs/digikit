// SPDX-License-Identifier: GPL-2.0-or-later
//! A patched build in digikit's emulator with the DSP side answering zeros, so the frame interrupt (vector 191) runs:
//! the daemon refuses a build it does not know at start, and loads a patched one ColdFire-only, where no frame runs.
//! Build it as an example of digikit's `digi-coupled` crate, with `plusdrive-format` as its dev-dependency
//! (tests/emu/README.md):
//!
//!     zero_patched STOCK.syx BUILD.syx CF_STATE [CARD_SECTORS]
//!
//! CF_STATE is the build's ColdFire state at the main screen; when the file does not exist the runner makes it first (a
//! ColdFire-only cold boot with the SSI diagnostic on, as zero-DSP mode needs). Commands, one per line on stdin, each
//! answered by one line on stdout:
//!   step N | down CODE | up CODE | turn ENCODER DETENTS | peek ADDR (a long, hex) | poke ADDR HEXBYTES | leds | frame PATH
//!   | card-save PATH (every sector the guest wrote: a big-endian u64 sector and its 512 bytes each) | quit
//! CARD_SECTORS is such a file: the card starts with those sectors, as after a restart of the unit.
use std::io::{BufRead, Write};

use digi_coupled::{AudioMode, AudioOptions, CoupledRuntime};
use emmc_card::{Card, CardIdentity};
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 && args.len() != 5 {
        eprintln!("usage: zero_patched STOCK.syx BUILD.syx CF_STATE [CARD_SECTORS]");
        std::process::exit(2);
    }
    let stock = std::fs::read(&args[1]).expect("stock .syx");
    let build = std::fs::read(&args[2]).expect("build .syx");
    let base = format!("{:x}", Sha256::digest(&stock));
    let card = || -> Option<Card> {
        let path = args.get(4)?;
        let bytes = std::fs::read(path).expect("CARD_SECTORS");
        // the card the emulator boots with when it is given none (an empty +Drive), with the sectors written over it
        let image = plusdrive_format::build_sample_image(Vec::new()).expect("empty +Drive").image;
        let mut card = Card::with_backing(CardIdentity::from_env().expect("identity"), Some(Box::new(image))).expect("card");
        for record in bytes.chunks(8 + 512) {
            let sector = u64::from_be_bytes(record[..8].try_into().expect("sector")) as u32;
            card.write_data(25, sector, &record[8..]).expect("card write");
        }
        Some(card)
    };
    if !std::path::Path::new(&args[3]).exists() {
        let mut cold = CoupledRuntime::new_patched(&build, card(), &base).expect("cold runtime");
        cold.enable_ssi_diagnostic(96_000).expect("SSI diagnostic");
        let mut steps = 0u32;
        while !cold.step_chunk(1_000_000).status.main_ui_reached {
            steps += 1;
            assert!(steps < 4000, "no main screen after 4G instructions");
        }
        cold.step_chunk(200_000_000);
        std::fs::write(&args[3], cold.save_state().expect("save")).expect("CF_STATE");
        eprintln!("zero_patched: cold boot to the main screen in {steps}M instructions, saved");
    }
    let options = AudioOptions {
        image: Default::default(),
        state: Default::default(),
        snapshot: args[3].clone().into(),
        buffer_seconds: 0.0,
        playback: false,
        automatic: false,
        expected_syx: None,
        link_timing: false,
        audio_mode: AudioMode::Rebuffer,
        dsp_patch: None,
        zero: true,
    };
    let mut rt = CoupledRuntime::new_patched_audio(&build, card(), &base, &options).expect("runtime");
    let mut leds = String::new();
    let mut frame: Vec<u8> = Vec::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.expect("stdin");
        let w: Vec<&str> = line.split_whitespace().collect();
        let reply = match w.as_slice() {
            ["step", n] => {
                let mut left: u64 = n.parse().expect("count");
                while left > 0 {
                    let chunk = left.min(1_000_000) as u32;
                    let snap = rt.step_chunk(chunk);
                    if let Some(f) = snap.frame {
                        frame = f;
                    }
                    if let Some(l) = snap.leds {
                        leds = l;
                    }
                    left -= u64::from(chunk);
                }
                let st = rt.snapshot();
                if let Some(f) = st.frame {
                    frame = f;
                }
                if let Some(l) = st.leds {
                    leds = l;
                }
                format!("ok {}", rt.elapsed_instructions())
            }
            ["down", c] => rt.button(c.parse().expect("code"), true).map(|_| "ok".into()).unwrap_or_else(|e| e),
            ["up", c] => rt.button(c.parse().expect("code"), false).map(|_| "ok".into()).unwrap_or_else(|e| e),
            ["turn", e, d] => rt
                .turn(e.parse().expect("encoder"), d.parse().expect("detents"))
                .map(|_| "ok".into())
                .unwrap_or_else(|e| e),
            ["peek", a] => {
                let addr = u32::from_str_radix(a.trim_start_matches("0x"), 16).expect("address");
                match rt.peek32(addr) {
                    Some(v) => format!("{v:08x}"),
                    None => "none".into(),
                }
            }
            ["poke", a, hex] => {
                let addr = u32::from_str_radix(a.trim_start_matches("0x"), 16).expect("address");
                let bytes: Vec<u8> = (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex bytes"))
                    .collect();
                rt.write_ram(addr, &bytes).map(|_| "ok".into()).unwrap_or_else(|e| e)
            }
            ["dump", a, n, path] => {
                let addr = u32::from_str_radix(a.trim_start_matches("0x"), 16).expect("address");
                let len = u32::from_str_radix(n.trim_start_matches("0x"), 16).expect("length");
                let mut out = Vec::with_capacity(len as usize);
                for k in (0..len).step_by(4) {
                    out.extend_from_slice(&rt.peek32(addr + k).unwrap_or(0).to_be_bytes());
                }
                match std::fs::write(path, &out) {
                    Ok(()) => format!("ok {}", out.len()),
                    Err(e) => e.to_string(),
                }
            }
            ["leds"] => leds.clone(),
            ["frame", path] => match std::fs::write(path, &frame) {
                Ok(()) => format!("ok {}", frame.len()),
                Err(e) => e.to_string(),
            },
            ["card-save", path] => {
                let mut out = Vec::new();
                let sectors = rt.card_overlay();
                for s in &sectors {
                    out.extend_from_slice(&s.sector.to_be_bytes());
                    out.extend_from_slice(&s.data);
                }
                match std::fs::write(path, out) {
                    Ok(()) => format!("ok {}", sectors.len()),
                    Err(e) => e.to_string(),
                }
            }
            ["quit"] => break,
            _ => format!("unknown: {line}"),
        };
        writeln!(out, "{reply}").expect("stdout");
        out.flush().expect("stdout");
    }
}
