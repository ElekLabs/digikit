//! The tiles that the lfo service builds for a sampled triangle equal the stock TRI tiles of the LFO page, read from the firmware image at test time (nothing of the image is
//! kept in the repository; skipped when the image is not here). The addresses are in docs/findings/149-lfo-waveforms.md "Graph".
use dkrt_svc_lfo::graph::{TILE_W, Tile, build};
use dkrt_svc_lfo::records::SAMPLES;
use patchkit::firmware::Firmware;
use std::path::PathBuf;

struct Stock {
    name: &'static str,
    image: &'static str,
    /// element 2 (upright: the peak is row 0 in column 7) and element 0 (mirrored): the first pixel word of each tile
    upright: u32,
    mirrored: u32,
}

const STOCK: [Stock; 2] = [
    Stock {
        name: "dt2",
        image: "Digitakt_II_OS1.17.syx",
        upright: 0x402c_e918,
        mirrored: 0x402c_e838,
    },
    Stock {
        name: "dn2",
        image: "Digitone_II_OS1.12.syx",
        upright: 0x402b_9420,
        mirrored: 0x402b_9340,
    },
];

/// 32 samples of a full-scale triangle that is 0 at phase 0 and rises first (the stock TRI function).
fn triangle() -> [i16; SAMPLES] {
    let mut s = [0i16; SAMPLES];
    for (i, v) in s.iter_mut().enumerate() {
        let p = i as f64 / SAMPLES as f64;
        let t = if p < 0.25 {
            4.0 * p
        } else if p < 0.75 {
            2.0 - 4.0 * p
        } else {
            4.0 * p - 4.0
        };
        *v = (32767.0 * t).round() as i16;
    }
    s
}

/// The 28 words of a tile at image address `addr`.
fn tile_at(fw: &Firmware, addr: u32) -> Tile {
    let s = fw
        .sections
        .iter()
        .find(|s| s.dest <= addr && ((addr - s.dest) as usize) + 4 * TILE_W <= s.data.len())
        .unwrap_or_else(|| panic!("no section holds {addr:#x}"));
    let at = (addr - s.dest) as usize;
    let mut t = [0u32; TILE_W];
    for (c, w) in t.iter_mut().enumerate() {
        *w = u32::from_be_bytes(s.data[at + 4 * c..at + 4 * c + 4].try_into().unwrap());
    }
    t
}

#[test]
fn a_sampled_triangle_gives_the_stock_triangle_tiles_of_the_image() {
    let t = build(&triangle());
    for d in STOCK {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../firmware")
            .join(d.image);
        if !path.exists() {
            eprintln!("SKIP {}: {} is not here", d.name, path.display());
            continue;
        }
        let fw = Firmware::parse(&std::fs::read(&path).unwrap()).unwrap();
        let up = tile_at(&fw, d.upright);
        let mi = tile_at(&fw, d.mirrored);
        assert_eq!(t[2], up, "{}: upright", d.name);
        assert_eq!(t[1], up, "{}: element 1 is the upright tile", d.name);
        assert_eq!(t[0], mi, "{}: mirrored", d.name);
        assert_eq!(t[3], mi, "{}: element 3 is the mirrored tile", d.name);
    }
}
