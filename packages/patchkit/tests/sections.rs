//! Section roles and the seal come from the device file (`[container]`, `[capabilities] seal`): a device without a seal builds a
//! container without a trailer, and a section of `[container] readonly` is refused by name of the role, not of the device.
//! The images are small and synthetic: no firmware file is read.

use patchkit::devdata;
use patchkit::firmware::{Firmware, Sec};
use patchkit::syx::{self, Device};

/// A patchable device by what it has (`[capabilities]`), not by name: `sealed` and `dsp` as in `[capabilities]`.
fn device(seal: &str, dsp: &str) -> Device {
    let d = devdata::all()
        .iter()
        .find(|d| d.identity.capabilities.seal == seal && d.identity.capabilities.dsp == dsp)
        .unwrap_or_else(|| panic!("no patchable device with seal {seal} and dsp {dsp}"));
    Device::from_id(d.syx_id).unwrap()
}
/// The device without a seal and without a DSP, and one with both.
fn bare() -> Device {
    device("none", "none")
}
fn sealed() -> Device {
    device("hmac", "sharc")
}

fn sec(id: u32, dest: u32, data: &[u8]) -> Sec {
    Sec {
        id,
        dest,
        // sections 1 and 6 have no storage rule: they are never written
        stored: syx::store_for_section(id, data).unwrap_or_else(|_| data.to_vec()),
        data: data.to_vec(),
        changed: false,
    }
}

/// A container-shaped image of `device` with the given section ids, never parsed from a `.syx`.
fn image(device: Device, ids: &[u32]) -> Firmware {
    let framing = |n: u8| {
        let mut m = vec![0xf0, 0, 0x20, 0x3c, device.syx_id(), 0, 0x7f, 1, 0x10 + n];
        m.resize(15, 0);
        m.push(0xf7);
        m
    };
    Firmware {
        device,
        device_id: device.syx_id(),
        framing_start: framing(0),
        framing_end: framing(0),
        header: vec![0; syx::COUNT_OFF],
        first_offset: 0x90,
        sections: ids
            .iter()
            .map(|&id| sec(id, 0x4000_0400, &[id as u8; 64]))
            .collect(),
        syx_sha256: String::new(),
    }
}

#[test]
fn a_device_without_a_seal_is_accepted_and_its_container_has_no_trailer_slot() {
    let bare_dev = bare();
    assert!(!bare_dev.sealed());
    assert_eq!(bare_dev.dsp_section(), None);
    let fw = image(bare_dev, &[2, 3]);
    let entries: Vec<(u32, u32, &[u8])> = fw
        .sections
        .iter()
        .map(|s| (s.id, s.dest, s.stored.as_slice()))
        .collect();
    let c = syx::build_container(&fw.header, &entries, fw.first_offset, bare_dev).unwrap();
    // the stock container ends at the last section's end rounded up to 16 (finding 70): the sealed layout of the same
    // sections is 32 bytes longer
    let with_seal = sealed();
    let with_slot = syx::build_container(&fw.header, &entries, fw.first_offset, with_seal).unwrap();
    assert_eq!(with_slot.len(), c.len() + 32);
    assert_eq!(c.len() % 16, 0);
    assert_eq!(syx::trailer_len(bare_dev), 0);
    let sealed = syx::seal(c.clone(), bare_dev);
    assert_eq!(sealed, c, "no seal: the container is unchanged");
    assert!(syx::trailer(&sealed, bare_dev).is_none());
    assert!(syx::trailer_matches(&sealed, bare_dev));
}

#[test]
fn a_sealed_device_still_gets_its_hmac() {
    let with_seal = sealed();
    assert!(with_seal.sealed());
    let fw = image(with_seal, &[2, 3, 7]);
    let entries: Vec<(u32, u32, &[u8])> = fw
        .sections
        .iter()
        .map(|s| (s.id, s.dest, s.stored.as_slice()))
        .collect();
    let c = syx::build_container(&fw.header, &entries, fw.first_offset, with_seal).unwrap();
    let sealed = syx::seal(c.clone(), with_seal);
    assert_ne!(sealed[sealed.len() - 32..], [0u8; 32]);
    assert!(syx::trailer_matches(&sealed, with_seal));
    assert!(
        !syx::trailer_matches(&c, with_seal),
        "a zero trailer is not the HMAC"
    );
}

#[test]
fn a_read_only_section_is_refused_and_main_is_not() {
    let bare_dev = bare();
    // safety pin: the device without a seal (FPGA bitstream in 1, the second CPU in 6 and 7) may only change MAIN
    assert_eq!(bare_dev.readonly_sections(), [1, 2, 4, 6, 7, 8]);
    let mut fw = image(bare_dev, &[1, 2, 3, 4, 6, 7, 8]);
    for id in [1u32, 2, 4, 6, 7, 8] {
        let e = fw.replace(id, vec![0; 16]).unwrap_err().to_string();
        assert!(e.contains("read-only"), "section {id}: {e}");
        assert!(!fw.section(id).unwrap().changed, "section {id}");
    }
    // the refusal is by the role list: the second CPU's section 7 is not a DSP stream here
    assert!(fw.dsp().is_none());
    fw.replace(3, vec![0; 16]).unwrap();
    assert!(fw.section(3).unwrap().changed);
    assert_eq!(fw.main().unwrap().id, 3);
}

#[test]
fn a_sealed_device_with_a_dsp_changes_main_and_the_dsp_only() {
    let with_seal = sealed();
    assert_eq!(with_seal.readonly_sections(), [2, 4, 8]);
    let mut fw = image(with_seal, &[2, 3, 4, 7, 8]);
    for id in [2u32, 4, 8] {
        assert!(fw.replace(id, vec![0; 16]).is_err(), "section {id}");
    }
    assert_eq!(fw.dsp().unwrap().id, 7);
    fw.replace(7, vec![0; 16]).unwrap();
    fw.replace(3, vec![0; 16]).unwrap();
}

/// Section content that packs well and unpacks to itself.
fn compressible() -> Vec<u8> {
    (0..4000u32).map(|i| (i % 13) as u8).collect()
}

#[test]
fn a_repackable_section_is_stored_packed_and_decodes_to_the_same_bytes() {
    let dev = bare();
    // the device file lists the second CPU's section: a role, read-only in content, that a module may store packed
    assert!(dev.is_repackable(7));
    assert!(
        !dev.is_repackable(1),
        "the FPGA bitstream is never repacked"
    );
    assert!(!dev.is_repackable(2));
    let mut fw = image(dev, &[1, 3, 7]);
    let data = compressible();
    {
        let s = fw.sections.iter_mut().find(|s| s.id == 7).unwrap();
        // stock form: stored raw
        s.data = data.clone();
        s.stored = data.clone();
    }
    let stored = fw.repack(7).unwrap();
    let s = fw.section(7).unwrap();
    assert!(s.changed);
    assert_eq!(s.data, data, "the decoded bytes do not change");
    assert!(stored < data.len() && s.stored.len() == stored);
    let back = dt2_firmware_loader::depack_section(&s.stored, 0, 1 << 20).unwrap();
    assert_eq!(back, data, "the stored stream unpacks to the stock bytes");
    // a section that is not listed, and the read-only rule itself, stay as they are
    for id in [1u32, 2] {
        let e = fw.repack(id).unwrap_err().to_string();
        assert!(e.contains("cannot be stored packed"), "section {id}: {e}");
        assert!(fw.section(id).is_none_or(|s| !s.changed));
    }
    assert!(
        fw.replace(7, data)
            .unwrap_err()
            .to_string()
            .contains("read-only")
    );
}

#[test]
fn a_device_without_repack_roles_stores_nothing_packed() {
    let dev = sealed();
    assert!(dev.repack_roles().is_empty());
    let mut fw = image(dev, &[2, 3, 4, 7, 8]);
    for id in [2u32, 4, 7, 8] {
        assert!(fw.repack(id).is_err(), "section {id}");
    }
}

/// MAIN data that packs to almost nothing and is longer than the bootstrap's stage lies above its load address: 3 MiB of zeros against the stage 2 MiB up.
fn big_main() -> Vec<u8> {
    vec![0u8; 3 << 20]
}

#[test]
fn a_main_stream_whose_unpack_overtakes_the_staged_stream_is_refused_and_a_literal_tail_repairs_it()
{
    let dev = bare();
    let (dest, stage, floor) = dev.main_unpack();
    assert!(
        stage - dest < 3 << 20,
        "the test data reaches past the stage"
    );
    let data = big_main();
    // the plain stream is a few bytes: the output passes the stage with nothing read ahead
    let mut fw = image(dev, &[3]);
    let plain = patchkit::aplib::pack_section(&data);
    fw.replace_main_stored(data.clone(), plain.clone()).unwrap();
    let e = patchkit::verify::main_budget(&fw, 0, 1 << 22)
        .unwrap_err()
        .0;
    assert!(
        e.contains("in-place margin") && e.contains("hang before MAIN starts"),
        "{e}"
    );
    // the same bytes packed to meet the margin floor
    let made = patchkit::aplib::pack_section_in_place(&data, dest, stage, floor as i64).unwrap();
    assert!(made.literal_bytes > 0 && made.stream.len() > plain.len());
    assert!(made.margin.unwrap() >= floor as i64);
    // shortest: one token less of literals would not do (the tail is minimal up to one token)
    assert_eq!(
        patchkit::aplib::inplace_margin(&made.stream, dest, stage).unwrap(),
        made.margin,
        "the margin is that of the final stream"
    );
    let back = dt2_firmware_loader::depack_section(&made.stream, 0, 1 << 24).unwrap();
    assert!(
        back == data,
        "the stream with the tail decodes to the same bytes"
    );
    fw.replace_main_stored(data, made.stream).unwrap();
    let line = patchkit::verify::main_budget(&fw, 1 << 20, 1 << 21)
        .unwrap()
        .to_string();
    assert!(
        line.starts_with("bootstrap margin ") && line.contains("KB free"),
        "{line}"
    );
}

#[test]
fn a_main_stream_with_room_is_packed_exactly_as_before() {
    let dev = bare();
    let (dest, stage, floor) = dev.main_unpack();
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let made = patchkit::aplib::pack_section_in_place(&data, dest, stage, floor as i64).unwrap();
    assert_eq!(made.literal_bytes, 0);
    assert_eq!(
        made.stream,
        patchkit::aplib::pack_section(&data),
        "no tail: the bytes of the plain packer"
    );
    assert_eq!(made.margin, None, "the output stays below the stage");
}

/// A MAIN stream of `n` zeros with the last `tail` bytes as literals: the cut falls inside the long zero match.
fn zeros_with_tail(n: usize, tail: usize) -> (Vec<u8>, Vec<u8>) {
    let data = vec![0u8; n];
    let toks = patchkit::aplib::plan_with(&data, 24, 96);
    let body = patchkit::aplib::emit(&data, &toks, n - tail);
    (data, patchkit::aplib::section_of(body))
}

#[test]
fn a_margin_of_exactly_the_floor_passes_and_one_byte_less_is_refused() {
    let dev = bare();
    let (dest, stage, floor) = dev.main_unpack();
    let room = (stage - dest) as usize;
    let margin = |n: usize, tail: usize| {
        let (_, s) = zeros_with_tail(n, tail);
        patchkit::aplib::inplace_margin(&s, dest, stage).unwrap()
    };
    // output 100,000 bytes past the stage: the tail of literals buys the margin, 9/8 of a byte per tail byte
    let mut n = room + 100_000;
    let (mut lo, mut hi) = (1usize, 200_000usize);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if margin(n, mid).unwrap() >= floor as i64 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let tail = hi;
    // the margin falls by one for each more byte of output: bring it to exactly the floor
    while margin(n, tail).unwrap() > floor as i64 {
        n += 1;
    }
    assert_eq!(margin(n, tail), Some(floor as i64));
    let check = |n: usize| {
        let (data, s) = zeros_with_tail(n, tail);
        let mut fw = image(dev, &[3]);
        fw.replace_main_stored(data, s).unwrap();
        patchkit::verify::main_budget(&fw, 0, 1 << 22)
    };
    assert!(check(n).is_ok(), "margin == floor passes");
    assert_eq!(margin(n + 1, tail), Some(floor as i64 - 1));
    let e = check(n + 1).unwrap_err().0;
    assert!(e.contains("in-place margin"), "{e}");
}

#[test]
fn the_literal_tail_is_cut_inside_a_long_match_and_is_nearly_minimal() {
    let dev = bare();
    let (dest, stage, floor) = dev.main_unpack();
    let n = (stage - dest) as usize + 100_000;
    let data = vec![0u8; n];
    let made = patchkit::aplib::pack_section_in_place(&data, dest, stage, floor as i64).unwrap();
    // the output crosses the stage inside the tail: the margin there is 9/8 of the tail bytes before the crossing, so the tail is 100,000 + floor * 8 / 9
    // bytes, not the whole token of up to a MiB that a cut at token boundaries would literalize
    let want = 100_000 + floor as usize * 8 / 9;
    assert!(
        made.literal_bytes + 64 > want && made.literal_bytes < want + 64,
        "tail {}",
        made.literal_bytes
    );
    assert!(
        made.margin.unwrap() >= floor as i64 && made.margin.unwrap() < floor as i64 + 16,
        "{:?}",
        made.margin
    );
    let back = dt2_firmware_loader::depack_section(&made.stream, 0, 1 << 24).unwrap();
    assert!(back == data);
}
