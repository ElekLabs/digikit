//! The `.dkp` codec: the shared vectors, the edits and the limits of the project scope, the owner prefix.
use super::json::{Parser, Value};
use super::*;
use dkrt_sdk::proto::{E_SET_FULL, E_SET_KEY, E_SET_NOKEY, E_SET_VALUE};
use dkrt_sdk::settings::{entry_at, get32};
use std::boxed::Box;
use std::string::String;
use std::vec::Vec;

const DEV: u16 = 0x1234;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn vectors() -> Value {
    Parser {
        b: include_str!("../../../../runtime/core/vectors/dkp-vectors.json").as_bytes(),
        at: 0,
    }
    .value()
}

fn image(name: &[u8; 16]) -> Box<Image> {
    let mut i = Box::new(Image::new());
    reset(&mut i.0, DEV, 12, name, 0x1234_5678);
    i
}

fn name16(s: &[u8]) -> [u8; 16] {
    let mut n = [0u8; 16];
    n[..s.len()].copy_from_slice(s);
    n
}

fn entries(b: &[u8]) -> Vec<(String, String, Vec<u8>)> {
    // every data section: (owner, key, value) in file order
    let used = get32(b, FMT.o_used()) as usize;
    let mut out = Vec::new();
    let mut at = HEADER;
    while at < used {
        let s = sec_at(b, used, at).unwrap();
        if let Some((o, n)) = owner_of(b, &s) {
            let owner = String::from_utf8(b[o..o + n].to_vec()).unwrap();
            let mut p = o + n;
            while p < s.end {
                let e = entry_at(b, s.end, p).unwrap();
                out.push((
                    owner.clone(),
                    String::from_utf8(b[e.key.0..e.key.0 + e.key.1].to_vec()).unwrap(),
                    b[e.value.0..e.value.0 + e.value.1].to_vec(),
                ));
                p = e.end;
            }
        }
        at = s.end;
    }
    out
}

#[test]
fn valid_vectors_decode_and_encode_back() {
    let v = vectors();
    assert_eq!(v["device"].as_u64().unwrap(), DEV as u64);
    for c in v["valid"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let bytes = hex(c["hex"].as_str().unwrap());
        let h = check(&bytes, DEV).unwrap_or_else(|e| panic!("{name}: refused {e:?}"));
        assert_eq!(h.slot as u64, c["slot"].as_u64().unwrap(), "{name}");
        assert_eq!(h.counter as u64, c["counter"].as_u64().unwrap(), "{name}");
        assert_eq!(h.stamp as u64, c["stamp"].as_u64().unwrap(), "{name}");
        assert_eq!(h.used as u64, c["used"].as_u64().unwrap(), "{name}");
        // as a file of the store: the tail sector is padded with zeros
        let mut padded = bytes.clone();
        padded.resize(AREA, 0);
        assert!(check(&padded, DEV).is_ok(), "{name}: padded");
        let want: Vec<(String, String, Vec<u8>)> = c["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e[0].as_str().unwrap().into(),
                    e[1].as_str().unwrap().into(),
                    hex(e[2].as_str().unwrap()),
                )
            })
            .collect();
        assert_eq!(entries(&bytes), want, "{name}");
        let mods: Vec<(String, u32)> = c["modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| (m[0].as_str().unwrap().into(), m[1].as_u64().unwrap() as u32))
            .collect();
        let got: Vec<(String, u32)> = modules(&bytes)
            .map(|(i, h)| (String::from_utf8(i.to_vec()).unwrap(), h))
            .collect();
        assert_eq!(got, mods, "{name}");
        assert_eq!(
            has_data(&bytes),
            !want.is_empty() || !mods.is_empty(),
            "{name}"
        );
        if c["reencode"].as_bool().unwrap() {
            // the encoder gives the file's own bytes
            let mut i = Box::new(Image::new());
            reset(&mut i.0, h.device, h.slot, &h.name, h.stamp);
            let owned: Vec<(Vec<u8>, u32)> = mods
                .iter()
                .map(|(a, b)| (a.as_bytes().to_vec(), *b))
                .collect();
            let refs: Vec<(&[u8], u32)> = owned.iter().map(|(a, b)| (a.as_slice(), *b)).collect();
            set_modules(&mut i.0, &refs).unwrap();
            for (o, k, val) in &want {
                let bare = &k[o.len() + 1..];
                assert_eq!(
                    put(&mut i.0, o.as_bytes(), bare.as_bytes(), val),
                    Ok(true),
                    "{name}"
                );
            }
            seal(&mut i.0, h.counter);
            assert_eq!(
                &i.0[..h.used as usize],
                &bytes[..h.used as usize],
                "{name}: encoded"
            );
        }
    }
}

#[test]
fn invalid_vectors_are_refused_for_their_reason() {
    let v = vectors();
    for c in v["invalid"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let bytes = hex(c["hex"].as_str().unwrap());
        let want = match c["why"].as_str().unwrap() {
            "format" => Ignore::Format,
            "device" => Ignore::Device,
            "version" => Ignore::Version,
            w => panic!("{w}"),
        };
        assert_eq!(check(&bytes, DEV).err(), Some(want), "{name}");
    }
}

#[test]
fn a_file_for_another_device_is_refused_with_that_reason() {
    let mut i = image(&name16(b"X"));
    seal(&mut i.0, 1);
    assert!(check(&i.0, DEV).is_ok());
    assert_eq!(check(&i.0, DEV + 1).err(), Some(Ignore::Device));
}

#[test]
fn put_get_remove_keep_the_owner_prefix_and_the_settings_limits() {
    let mut i = image(&name16(b"X"));
    let b = &mut i.0;
    assert_eq!(put(b, b"test/owner", b"pool", &[1, 2, 3]), Ok(true));
    let mut out = [0u8; 8];
    assert_eq!(get(b, b"test/owner", b"pool", &mut out), 3);
    assert_eq!(&out[..3], &[1, 2, 3]);
    // stored as <owner>/<key>
    assert_eq!(
        entries(b),
        [(
            "test/owner".into(),
            "test/owner/pool".into(),
            std::vec![1, 2, 3]
        )]
    );
    // another owner cannot reach it: the key is the owner's own
    assert_eq!(get(b, b"digikit/other", b"pool", &mut out), E_SET_NOKEY);
    // a bare key with a slash, `..`, a space or nothing is refused; the whole key is 47 bytes at most
    for bad in [&b"a/b"[..], b"..", b"x..y", b"a b", b""] {
        assert_eq!(put(b, b"test/owner", bad, b"1"), Err(E_SET_KEY), "{bad:?}");
        assert_eq!(get(b, b"test/owner", bad, &mut out), E_SET_KEY);
    }
    let long = [b'k'; 47 - 11];
    assert_eq!(
        put(b, b"test/owner", &long, b"1"),
        Ok(true),
        "47 bytes with the owner"
    );
    assert_eq!(
        put(b, b"test/owner", &[b'k'; 47 - 10], b"1"),
        Err(E_SET_KEY),
        "48 bytes"
    );
    // the value: 1024 bytes at most; a small `out` is refused and writes nothing
    assert_eq!(put(b, b"test/owner", b"v", &[7u8; 1024]), Ok(true));
    assert_eq!(put(b, b"test/owner", b"v", &[7u8; 1025]), Err(E_SET_VALUE));
    let mut small = [0u8; 4];
    assert_eq!(get(b, b"test/owner", b"v", &mut small), E_SET_VALUE);
    assert_eq!(small, [0; 4]);
    assert_eq!(remove(b, b"test/owner", b"v"), Ok(true));
    assert_eq!(remove(b, b"test/owner", b"v"), Ok(false));
    assert_eq!(get(b, b"test/owner", b"v", &mut out), E_SET_NOKEY);
}

#[test]
fn a_put_of_the_stored_value_changes_nothing() {
    let mut i = image(&name16(b"X"));
    let b = &mut i.0;
    assert_eq!(put(b, b"a/b", b"k", b"one"), Ok(true));
    let before: Vec<u8> = b[..get32(b, FMT.o_used()) as usize].to_vec();
    assert_eq!(put(b, b"a/b", b"k", b"one"), Ok(false));
    assert_eq!(&b[..before.len()], &before[..]);
    assert_eq!(get32(b, FMT.o_used()) as usize, before.len());
}

#[test]
fn edits_in_the_middle_keep_every_section_whole() {
    let mut i = image(&name16(b"X"));
    let b = &mut i.0;
    for (o, k, v) in [
        (&b"a/b"[..], &b"one"[..], &b"1"[..]),
        (b"c/d", b"two", b"22"),
        (b"a/b", b"three", b"333"),
        (b"e/f", b"x", b""),
    ] {
        assert_eq!(put(b, o, k, v), Ok(true));
    }
    // a longer and a shorter value in the first section, with sections after it
    assert_eq!(put(b, b"a/b", b"one", b"1111111111"), Ok(true));
    assert_eq!(put(b, b"a/b", b"three", b"3"), Ok(true));
    seal(b, 1);
    assert!(check(b, DEV).is_ok());
    let mut out = [0u8; 16];
    assert_eq!(get(b, b"a/b", b"one", &mut out), 10);
    assert_eq!(get(b, b"c/d", b"two", &mut out), 2);
    assert_eq!(get(b, b"e/f", b"x", &mut out), 0);
    // removing the only entry of an owner takes its section; the others stay
    assert_eq!(remove(b, b"c/d", b"two"), Ok(true));
    seal(b, 2);
    assert!(check(b, DEV).is_ok());
    assert_eq!(
        entries(b).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
        ["a/b", "a/b", "e/f"]
    );
    assert_eq!(remove(b, b"a/b", b"one"), Ok(true));
    assert_eq!(remove(b, b"a/b", b"three"), Ok(true));
    assert_eq!(remove(b, b"e/f", b"x"), Ok(true));
    assert_eq!(get32(b, FMT.o_used()) as usize, HEADER);
    assert!(!has_data(b));
}

#[test]
fn a_put_over_16_kib_is_full_and_leaves_the_image_as_it_was() {
    let mut i = image(&name16(b"X"));
    let b = &mut i.0;
    let mut n = 0;
    loop {
        let key = std::format!("k{n:03}");
        match put(b, b"a/b", key.as_bytes(), &[9u8; 1000]) {
            Ok(_) => n += 1,
            Err(e) => {
                assert_eq!(e, E_SET_FULL);
                break;
            }
        }
    }
    assert!(
        (15..=16).contains(&n),
        "{n} values of 1000 bytes fit in 16 KiB"
    );
    // then small ones until nothing fits
    let mut m = 0;
    while put(b, b"a/b", std::format!("s{m:03}").as_bytes(), &[3u8; 20]) == Ok(true) {
        m += 1;
    }
    let used = get32(b, FMT.o_used()) as usize;
    assert!(used <= AREA);
    let snap: Vec<u8> = b[..used].to_vec();
    // a new entry, a longer value and another owner all fail the same way
    assert_eq!(put(b, b"a/b", b"zzz", &[1u8; 1000]), Err(E_SET_FULL));
    assert_eq!(put(b, b"a/b", b"k000", &[1u8; 1024]), Err(E_SET_FULL));
    assert_eq!(put(b, b"c/d", b"k", &[1u8; 1000]), Err(E_SET_FULL));
    assert_eq!(
        put(b, b"c/d", b"k", b""),
        Err(E_SET_FULL),
        "a new section needs room too"
    );
    assert_eq!(&b[..used], &snap[..]);
    assert_eq!(get32(b, FMT.o_used()) as usize, used);
    // a shorter value still fits, and the file stays whole
    assert_eq!(put(b, b"a/b", b"k000", &[2u8; 10]), Ok(true));
    seal(b, 1);
    assert!(check(b, DEV).is_ok());
}

#[test]
fn the_module_list_goes_first_and_survives_data_edits() {
    let mut i = image(&name16(b"X"));
    let b = &mut i.0;
    assert_eq!(put(b, b"a/b", b"k", b"v"), Ok(true));
    set_modules(b, &[(b"test/owner", 0xdead_beef), (b"test/x", 7)]).unwrap();
    assert_eq!(
        modules(b).collect::<Vec<_>>(),
        [(&b"test/owner"[..], 0xdead_beef), (&b"test/x"[..], 7)]
    );
    assert_eq!(b[HEADER], TAG_MODULES);
    assert_eq!(put(b, b"a/b", b"j", b"w"), Ok(true));
    assert_eq!(remove(b, b"a/b", b"k"), Ok(true));
    assert_eq!(modules(b).count(), 2);
    seal(b, 1);
    assert!(check(b, DEV).is_ok());
    // a list replaces the old one; an empty list takes the section away; modules alone are data
    set_modules(b, &[(b"only/one", 1)]).unwrap();
    assert_eq!(modules(b).collect::<Vec<_>>(), [(&b"only/one"[..], 1)]);
    assert_eq!(remove(b, b"a/b", b"j"), Ok(true));
    assert!(has_data(b));
    set_modules(b, &[]).unwrap();
    assert!(!has_data(b));
    assert_eq!(get32(b, FMT.o_used()) as usize, HEADER);
    assert_eq!(set_modules(b, &[(b"", 1)]), Err(E_SET_KEY));
}

#[test]
fn a_module_list_over_the_limit_is_refused_whole() {
    let mut i = image(&name16(b"X"));
    let b = &mut i.0;
    assert_eq!(put(b, b"a/b", b"k", &[1u8; 1000]), Ok(true));
    let used = get32(b, FMT.o_used());
    let id = [b'm'; 200];
    let many: Vec<(&[u8], u32)> = (0..100).map(|n| (&id[..], n)).collect();
    assert_eq!(set_modules(b, &many), Err(E_SET_FULL));
    assert_eq!(get32(b, FMT.o_used()), used);
}

#[test]
fn a_header_edit_reaches_the_file_and_the_crc_follows() {
    let mut i = image(&name16(b"OLD NAME"));
    let b = &mut i.0;
    put(b, b"a/b", b"k", b"v").unwrap();
    seal(b, 3);
    let mut h = check(b, DEV).unwrap();
    assert_eq!((h.slot, h.counter, h.stamp), (12, 3, 0x1234_5678));
    assert_eq!(&h.name[..h.name_len as usize], b"OLD NAME");
    h.slot = 99;
    h.stamp = 5;
    h.name = name16(b"NEW");
    h.name_len = 3;
    set_head(b, &h);
    assert_eq!(
        check(b, DEV).err(),
        Some(Ignore::Format),
        "the crc32 covers the header"
    );
    seal(b, 4);
    let g = check(b, DEV).unwrap();
    assert_eq!((g.slot, g.counter, g.stamp, g.name_len), (99, 4, 5, 3));
}
