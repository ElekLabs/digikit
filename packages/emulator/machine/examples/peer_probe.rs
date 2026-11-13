//! Runs the peer program (section 7) alone against a plain 128 KB window and prints what it touched: bounded by an
//! instruction count. Usage: peer_probe SECTION7.bin [INSNS] [FRAME_EVERY]
use machine::peer_cpu::{PeerCpu, Window};

fn main() {
    let mut args = std::env::args().skip(1);
    let blob = std::fs::read(args.next().expect("section 7 file")).expect("read");
    let insns: u64 = args.next().map_or(5_000_000, |s| s.parse().expect("insns"));
    let frame_every: u64 = args.next().map_or(0, |s| s.parse().expect("frame period"));
    let mut window = vec![0u8; 0x2_0000];
    let mut peer = PeerCpu::new();
    peer.mem.log_limit = 400;
    peer.start(&blob).expect("peer image");
    let mut done = 0;
    let mut slice_end = 0;
    let mut hist = std::collections::BTreeMap::<u32, u64>::new();
    while done < insns {
        let step = if frame_every == 0 {
            (insns - done).min(10_000)
        } else {
            frame_every.min(insns - done)
        };
        *hist.entry(peer.cpu.pc & !0x3f).or_default() += 1;
        match peer.run(&mut window, step) {
            Ok(()) => done += step,
            Err(e) => {
                println!(
                    "stop {e:?} after {} insns pc={:#x}",
                    peer.cpu.icount, peer.cpu.pc
                );
                break;
            }
        }
        if frame_every != 0 {
            peer.frame_tick(&mut window);
        }
        let e = peer.mem.take_pin_edges();
        if e != 0 {
            println!(
                "pin edges {e} at insn {} pc {:#x}",
                peer.cpu.icount, peer.cpu.pc
            );
        }
        slice_end += 1;
    }
    println!(
        "insns {} pc {:#010x} sr {:#06x} frames raised {} taken {} slices {slice_end}",
        peer.cpu.icount, peer.cpu.pc, peer.cpu.sr, peer.frames_raised, peer.frames_taken
    );
    for a in &peer.mem.log {
        println!(
            "{} pc={:08x} {:08x}/{} {:08x}",
            if a.write { "W" } else { "R" },
            a.pc,
            a.addr,
            a.size,
            a.value
        );
    }
    for a in &peer.mem.faults {
        println!(
            "FAULT {} pc={:08x} {:08x}/{} {:08x}",
            if a.write { "W" } else { "R" },
            a.pc,
            a.addr,
            a.size,
            a.value
        );
    }
    let w: Vec<String> = (0..0x20)
        .map(|o| format!("{:02x}", window.read(o, 1).unwrap()))
        .collect();
    let mut h: Vec<_> = hist.into_iter().collect();
    h.sort_by_key(|x| std::cmp::Reverse(x.1));
    for (pc, n) in h.iter().take(8) {
        println!("hot {pc:08x} {n}");
    }
    println!("window[0..0x20] {}", w.join(""));
}
