use super::*;

const BASE: u32 = L2_BYTE_BASE + 0x2000;

fn entry(at: u32, kind: u32, class: u32, imp: u32) -> [u8; 8] {
    let mut e = [0u8; 8];
    e[..4].copy_from_slice(&at.to_be_bytes());
    e[4..].copy_from_slice(&(kind << 28 | class << 24 | imp).to_be_bytes());
    e
}

/// A 48-bit instruction: opcode bytes, then the 24-bit field in bytes 2 (high) and 4..6 (low, little endian).
fn insn24(v: u32) -> [u8; 6] {
    [0x11, 0x22, (v >> 16) as u8, 0x33, v as u8, (v >> 8) as u8]
}

fn field24(b: &[u8]) -> u32 {
    (b[2] as u32) << 16 | b[4] as u32 | (b[5] as u32) << 8
}

fn insn32(v: u32) -> [u8; 6] {
    [
        0x44,
        0x55,
        (v >> 16) as u8,
        (v >> 24) as u8,
        v as u8,
        (v >> 8) as u8,
    ]
}

fn field32(b: &[u8]) -> u32 {
    (b[2] as u32 | (b[3] as u32) << 8) << 16 | b[4] as u32 | (b[5] as u32) << 8
}

#[test]
fn a_pc_field_moves_by_half_the_byte_shift_and_a_data_field_by_the_byte_shift() {
    let mut image = vec![0u8; 24];
    image[0..6].copy_from_slice(&insn24(0x100));
    image[6..12].copy_from_slice(&insn32(0x2000_0040));
    let table = [entry(0, 0, 0, 0), entry(6, 1, 1, 0)].concat();
    let mut out = vec![0u8; 24];
    Relocation::at(BASE, &[])
        .place(&image, &mut out, &table)
        .unwrap();
    assert_eq!(field24(&out[0..6]), 0x100 + 0x1000);
    assert_eq!(field32(&out[6..12]), 0x2000_0040 + 0x2000);
    assert_eq!(&out[0..2], &image[0..2], "the opcode bytes are untouched");
    assert_eq!(&out[12..], &image[12..]);
}

#[test]
fn an_import_class_adds_the_value_of_the_import() {
    let mut image = vec![0u8; 12];
    image[0..6].copy_from_slice(&insn24(0x10));
    image[6..12].copy_from_slice(&insn32(5));
    let table = [entry(0, 0, 2, 1), entry(6, 1, 2, 0)].concat();
    let mut out = vec![0u8; 12];
    Relocation::at(BASE, &[0x1000, 0x20])
        .place(&image, &mut out, &table)
        .unwrap();
    assert_eq!(field24(&out[0..6]), 0x30);
    assert_eq!(field32(&out[6..12]), 5 + 0x1000);
}

#[test]
fn a_table_that_names_a_place_or_an_import_that_is_not_there_is_malformed() {
    let image = vec![0u8; 12];
    let mut out = vec![0u8; 12];
    let r = Relocation::at(BASE, &[1]);
    assert_eq!(
        r.place(&image, &mut out, &entry(8, 0, 0, 0)),
        Err(Malformed),
        "six bytes do not fit at 8 of 12"
    );
    assert_eq!(
        r.place(&image, &mut out, &entry(0, 0, 2, 1)),
        Err(Malformed),
        "import 1 of 1"
    );
    assert_eq!(
        r.place(&image, &mut vec![0u8; 8], &[]),
        Err(Malformed),
        "the output is as long as the image"
    );
}

#[test]
fn a_pc_field_that_leaves_24_bits_is_malformed() {
    let mut image = vec![0u8; 6];
    image.copy_from_slice(&insn24(0x00ff_ffff));
    let mut out = vec![0u8; 6];
    assert_eq!(
        Relocation::at(BASE, &[]).place(&image, &mut out, &entry(0, 0, 0, 0)),
        Err(Malformed)
    );
}
