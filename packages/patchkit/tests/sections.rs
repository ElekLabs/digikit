//! Section roles and the seal come from the device file (`[container]`, `[capabilities] seal`): a device without a seal builds a
//! container without a trailer, and a section of `[container] readonly` is refused by name of the role, not of the device.
//! The images are small and synthetic: no firmware file is read.

use patchkit::firmware::{Firmware, Sec};
use patchkit::syx::{self, Device};

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
    let st = Device::from_id(0x16).unwrap();
    assert!(!st.sealed());
    assert_eq!(st.dsp_section(), None);
    let fw = image(st, &[2, 3]);
    let entries: Vec<(u32, u32, &[u8])> = fw
        .sections
        .iter()
        .map(|s| (s.id, s.dest, s.stored.as_slice()))
        .collect();
    let c = syx::build_container(&fw.header, &entries, fw.first_offset, st).unwrap();
    // the stock container ends at the last section's end rounded up to 16 (finding 70): the sealed layout of the same
    // sections is 32 bytes longer
    let dt2 = Device::from_id(0x14).unwrap();
    let with_slot = syx::build_container(&fw.header, &entries, fw.first_offset, dt2).unwrap();
    assert_eq!(with_slot.len(), c.len() + 32);
    assert_eq!(c.len() % 16, 0);
    assert_eq!(syx::trailer_len(st), 0);
    let sealed = syx::seal(c.clone(), st);
    assert_eq!(sealed, c, "no seal: the container is unchanged");
    assert!(syx::trailer(&sealed, st).is_none());
    assert!(syx::trailer_matches(&sealed, st));
}

#[test]
fn a_sealed_device_still_gets_its_hmac() {
    let dt2 = Device::from_id(0x14).unwrap();
    assert!(dt2.sealed());
    let fw = image(dt2, &[2, 3, 7]);
    let entries: Vec<(u32, u32, &[u8])> = fw
        .sections
        .iter()
        .map(|s| (s.id, s.dest, s.stored.as_slice()))
        .collect();
    let c = syx::build_container(&fw.header, &entries, fw.first_offset, dt2).unwrap();
    let sealed = syx::seal(c.clone(), dt2);
    assert_ne!(sealed[sealed.len() - 32..], [0u8; 32]);
    assert!(syx::trailer_matches(&sealed, dt2));
    assert!(
        !syx::trailer_matches(&c, dt2),
        "a zero trailer is not the HMAC"
    );
}

#[test]
fn a_read_only_section_is_refused_and_main_is_not() {
    let st = Device::from_id(0x16).unwrap();
    assert_eq!(st.readonly_sections(), [1, 2, 4, 6, 7, 8]);
    let mut fw = image(st, &[1, 2, 3, 4, 6, 7, 8]);
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
    let dt2 = Device::from_id(0x14).unwrap();
    assert_eq!(dt2.readonly_sections(), [2, 4, 8]);
    let mut fw = image(dt2, &[2, 3, 4, 7, 8]);
    for id in [2u32, 4, 8] {
        assert!(fw.replace(id, vec![0; 16]).is_err(), "section {id}");
    }
    assert_eq!(fw.dsp().unwrap().id, 7);
    fw.replace(7, vec![0; 16]).unwrap();
    fw.replace(3, vec![0; 16]).unwrap();
}
