//! Host tests of the shape graph (`graph.rs`): the tiles built from 32 samples, and publishing, looking up and withdrawing a graph. The statics of the crate are shared, so each test takes `LOCK`.
use dkrt_svc_lfo::graph::{
    build, digest, lookup, publish, retract, tiles_of, Tile, MASK_WORD, TILE_W,
};
use dkrt_svc_lfo::records::{MAX_SHAPES, SAMPLES};
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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

/// What the stock image shows of the TRI tiles, without a picture of them: the geometry of the build (the comparison with the image is `packages/patchkit/tests/lfo_graph.rs`). A sampled triangle is one pixel per column on a diagonal, with the peak
/// (row 0 upright) in column 7; the mirrored tile has every row `14 - row`.
#[test]
fn a_sampled_triangle_is_one_pixel_per_column_on_its_diagonal() {
    let t = build(&triangle());
    let row = |w: u32| 31 - w.trailing_zeros();
    for c in 0..TILE_W {
        assert_eq!(t[2][c].count_ones(), 1, "upright column {c}: one pixel");
        assert_eq!(t[0][c].count_ones(), 1, "mirrored column {c}: one pixel");
        assert_eq!(
            row(t[0][c]),
            14 - row(t[2][c]),
            "column {c}: mirrored row is 14 - row"
        );
    }
    // column 0 is the zero crossing (row 7); the peak (row 0) is column 7; the trough (row 14) is column 21; the slopes are one row per column
    assert_eq!(row(t[2][7]), 0, "the peak is in column 7");
    assert_eq!(row(t[2][21]), 14);
    assert_eq!(row(t[2][0]), 7);
    for c in 0..7 {
        assert_eq!(row(t[2][c + 1]) + 1, row(t[2][c]), "rising, column {c}");
    }
    for c in 7..21 {
        assert_eq!(row(t[2][c]) + 1, row(t[2][c + 1]), "falling, column {c}");
    }
    // the elements: 1 and 2 are the upright tile (the stock TRI is odd), 0 and 3 the mirrored one
    assert_eq!(t[1], t[2]);
    assert_eq!(t[0], t[3]);
}

#[test]
fn the_mirrored_tile_is_the_upright_one_turned_over_the_middle_row() {
    // an asymmetric shape: a ramp 0 up to full scale over the first half, then a step down to -0.5 and flat
    let mut s = [0i16; SAMPLES];
    for i in 0..SAMPLES {
        s[i] = if i < 16 {
            (i as i32 * 2047) as i16
        } else {
            -16000
        };
    }
    let t = build(&s);
    for c in 0..TILE_W {
        // row r (bit 31 - r) of the upright tile is row 14 - r of the mirrored one
        let flipped = (t[2][c] >> 17).reverse_bits() >> (32 - 15) << 17;
        assert_eq!(t[0][c], flipped, "column {c}");
        let flipped = (t[3][c] >> 17).reverse_bits() >> (32 - 15) << 17;
        assert_eq!(t[1][c], flipped, "column {c} of the time-reversed tile");
    }
    assert_ne!(
        t[2], t[3],
        "an asymmetric shape has a time-reversed picture of its own"
    );
    for tile in &t {
        assert!(tile.iter().all(|w| w & 0x1ffff == 0), "only rows 0 to 14");
    }
}

#[test]
fn a_unipolar_shape_stays_in_the_rows_of_its_sign() {
    // 0 up to full scale: rows 0 to 7 upright (zero is row 7), rows 7 to 14 mirrored
    let mut s = [0i16; SAMPLES];
    for i in 0..SAMPLES {
        s[i] = (i as i32 * 1000) as i16;
    }
    let t = build(&s);
    let rows = |tile: &Tile| tile.iter().fold(0u32, |a, w| a | w);
    assert_eq!(
        rows(&t[2]) & 0x00ff_ffff,
        0,
        "upright: rows 8 to 14 are empty"
    );
    assert_eq!(
        rows(&t[0]) & 0xfe00_0000,
        0,
        "mirrored: rows 0 to 6 are empty"
    );
    assert_ne!(rows(&t[2]) & 0xff00_0000, 0);
}

#[test]
fn a_graph_is_published_looked_up_and_withdrawn() {
    let _g = lock();
    let flag = |f: &mut u8| f as *mut u8;
    let mut f = 9u8;
    assert!(
        unsafe { lookup(7, 0x4021_cff8, flag(&mut f)) }.is_null(),
        "nothing published"
    );
    assert_eq!(digest(), 0);
    unsafe { publish(0, &triangle(), false) };
    assert_eq!(tiles_of(7).unwrap(), build(&triangle()));
    assert!(
        unsafe { lookup(7, 0, flag(&mut f)) }.is_null(),
        "no vtable, no graph"
    );
    let p = unsafe { lookup(7, 0x4021_cff8, flag(&mut f)) };
    assert!(!p.is_null());
    assert_eq!(f, 0, "bipolar");
    // the vector header: begin, end, end of storage over four 28-byte Bitmaps
    let w = |o: usize| unsafe { (p.add(o) as *const u32).read() };
    assert_eq!(w(4).wrapping_sub(w(0)), 4 * 28);
    assert_eq!(w(0), p as usize as u32 + 20, "begin is the first Bitmap");
    assert_eq!(w(8), w(4));
    let e = |k: usize, o: usize| unsafe { ((p.add(20 + 28 * k + o)) as *const u32).read() };
    for k in 0..4 {
        assert_eq!(
            (e(k, 0), e(k, 4), e(k, 8), e(k, 12), e(k, 24)),
            (0x4021_cff8, 28, 15, 1, 0)
        );
    }
    // the 32-bit fields hold the low half of a host pointer here: put the upper half of the vector's address back (the target has 32-bit pointers)
    let high = p as usize & !0xffff_ffff;
    let ptr = |v: u32| {
        let a = high | v as usize;
        debug_assert_eq!(a as u32, v);
        a as *const u32
    };
    let tile = unsafe { std::slice::from_raw_parts(ptr(e(2, 16)), TILE_W) };
    assert_eq!(tile, &build(&triangle())[2]);
    let mask = unsafe { std::slice::from_raw_parts(ptr(e(2, 20)), TILE_W) };
    assert!(mask.iter().all(|w| *w == MASK_WORD));
    assert!(
        unsafe { lookup(8, 0x4021_cff8, flag(&mut f)) }.is_null(),
        "another number"
    );
    assert!(
        unsafe { lookup(7 + MAX_SHAPES as u32, 0x4021_cff8, flag(&mut f)) }.is_null(),
        "out of range"
    );
    assert!(
        unsafe { lookup(6, 0x4021_cff8, flag(&mut f)) }.is_null(),
        "RAND stays the firmware's"
    );
    assert_ne!(digest(), 0);
    unsafe { retract(0) };
    assert!(unsafe { lookup(7, 0x4021_cff8, flag(&mut f)) }.is_null());
    assert!(tiles_of(7).is_none());
    assert_eq!(digest(), 0);
}

#[test]
fn a_gap_number_gets_the_stock_tri_marker() {
    use dkrt_abi::provider::{Consumer, ConsumerId, Provider, Records};
    use dkrt_abi::service::SettleCtx;
    use dkrt_sdk::records::KIND_LFO_SHAPE;
    use dkrt_svc_lfo::graph::STOCK_TRI;
    use dkrt_svc_lfo::lfo::Lfo;
    use dkrt_svc_lfo::records::NAME_LEN;
    use dkrt_test_support::FakeCore;
    let _g = lock();
    let core = FakeCore::new();
    let mut lfo = *dkrt_test_support::start_boxed::<Lfo>(&core).unwrap();
    let (key_a, key_b) = (0x00a1_0100u32, 0x00b2_0200u32);
    let mut sel = SettleCtx::new();
    sel.count = 1;
    core.set_keys(0, KIND_LFO_SHAPE, &[key_a, key_b]);
    Provider::settle(&mut lfo, &core, &sel);
    core.boot_index.set(0);
    // only the second key is applied: number 8 is kept and published, number 7 is a gap
    let mut body = key_b.to_be_bytes().to_vec();
    body.extend_from_slice(&[b'T'; NAME_LEN]);
    for s in 0..SAMPLES as i16 {
        body.extend_from_slice(&(100 * s).to_be_bytes());
    }
    let records = [(KIND_LFO_SHAPE, &body[..])];
    let r = Records::apply(
        &mut lfo,
        &core,
        KIND_LFO_SHAPE,
        &body,
        &Consumer::new(ConsumerId::new(1), &records),
    );
    assert_eq!(r, Ok(()));
    let mut f = 0u8;
    let vt = 0x4021_cff8;
    assert_eq!(
        unsafe { lookup(7, vt, &mut f) },
        STOCK_TRI,
        "a gap below the highest"
    );
    assert!(!unsafe { lookup(8, vt, &mut f) }.is_null());
    assert_ne!(unsafe { lookup(8, vt, &mut f) }, STOCK_TRI);
    assert!(
        unsafe { lookup(9, vt, &mut f) }.is_null(),
        "above the highest"
    );
    assert!(
        unsafe { lookup(6, vt, &mut f) }.is_null(),
        "RAND stays the firmware's"
    );
    Provider::detach(&mut lfo, &core, ConsumerId::new(1));
    assert!(
        unsafe { lookup(7, vt, &mut f) }.is_null(),
        "nothing kept, no gap"
    );
}

#[test]
fn a_unipolar_shape_sets_the_flag_byte() {
    let _g = lock();
    let mut s = [0i16; SAMPLES];
    for i in 0..SAMPLES {
        s[i] = (i as i32 * 1000) as i16;
    }
    unsafe { publish(1, &s, true) };
    let mut f = 0u8;
    assert!(!unsafe { lookup(8, 0x4021_cff8, &mut f) }.is_null());
    assert_eq!(f, 1);
    unsafe { retract(1) };
}
