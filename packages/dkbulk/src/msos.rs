//! The USB descriptors Windows reads to bind WinUSB to the Fast USB interface: BOS with the MS OS 2.0 platform capability, and the MS OS 2.0 descriptor set.
//! Pure functions over byte slices; `dkbulk descriptors` reads the bytes from a device and prints the checks.

/// UUID of the MS OS 2.0 platform capability (D8DD60DF-4589-4CC7-9CD2-659D9E648A9F, as bytes).
pub const MSOS_UUID: [u8; 16] = [
    0xDF, 0x60, 0xDD, 0xD8, 0x89, 0x45, 0xC7, 0x4C, 0x9C, 0xD2, 0x65, 0x9D, 0x9E, 0x64, 0x8A, 0x9F,
];
/// Windows 8.1, the first version that reads MS OS 2.0 descriptors.
pub const WINDOWS_8_1: u32 = 0x0603_0000;

/// One line of the PASS/FAIL list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub ok: bool,
}

fn chk(v: &mut Vec<Check>, name: impl Into<String>, ok: bool) {
    v.push(Check {
        name: name.into(),
        ok,
    });
}

/// Little-endian u16 at `at`; 0 past the end (the length checks fail first in that case).
fn u16at(b: &[u8], at: usize) -> u16 {
    match b.get(at..at + 2) {
        Some(s) => u16::from_le_bytes([s[0], s[1]]),
        None => 0,
    }
}

fn u32at(b: &[u8], at: usize) -> u32 {
    match b.get(at..at + 4) {
        Some(s) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        None => 0,
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// bcdUSB of a device descriptor.
pub fn bcd_usb(device: &[u8]) -> Option<u16> {
    (device.len() >= 18 && device[1] == 1).then(|| u16at(device, 2))
}

/// wTotalLength of a configuration descriptor (or its first 9 bytes).
pub fn config_total_length(cfg: &[u8]) -> Option<usize> {
    (cfg.len() >= 9 && cfg[1] == 2).then(|| u16at(cfg, 2) as usize)
}

/// wTotalLength of a BOS descriptor (or its first 5 bytes).
pub fn bos_total_length(bos: &[u8]) -> Option<usize> {
    (bos.len() >= 5 && bos[1] == 15).then(|| u16at(bos, 2) as usize)
}

/// The number of the first interface of class 0xFF in a whole configuration descriptor.
pub fn vendor_interface(cfg: &[u8]) -> Option<u8> {
    let mut i = 0;
    while i + 2 <= cfg.len() {
        let l = cfg[i] as usize;
        if l < 2 || i + l > cfg.len() {
            return None;
        }
        if cfg[i + 1] == 4 && l >= 9 && cfg[i + 5] == 0xFF {
            return Some(cfg[i + 2]);
        }
        i += l;
    }
    None
}

/// What the BOS says about the MS OS 2.0 set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bos {
    pub windows_version: u32,
    /// wMSOSDescriptorSetTotalLength
    pub set_len: u16,
    /// bMS_VendorCode: the bRequest that reads the set
    pub vendor_code: u8,
}

/// Check a whole BOS descriptor; the second value is the platform capability when the UUID matches.
pub fn check_bos(bos: &[u8]) -> (Vec<Check>, Option<Bos>) {
    let mut v = Vec::new();
    chk(
        &mut v,
        "BOS header: type 15, wTotalLength equals the bytes received",
        bos.len() >= 5 && bos[0] == 5 && bos[1] == 15 && u16at(bos, 2) as usize == bos.len(),
    );
    let (mut cap, mut i) = (None, bos.first().map_or(0, |&l| l as usize));
    while i + 3 <= bos.len() {
        let l = bos[i] as usize;
        if l < 3 || i + l > bos.len() {
            break;
        }
        if bos[i + 1] == 0x10 && bos[i + 2] == 5 && cap.is_none() {
            cap = Some(&bos[i..i + l]);
        }
        i += l;
    }
    chk(
        &mut v,
        "BOS has a platform capability (bLength 0x1C, type 0x10, cap 0x05)",
        cap.is_some_and(|c| c.len() == 0x1C),
    );
    let c = cap.filter(|c| c.len() == 0x1C);
    let uuid_ok = c.is_some_and(|c| c[4..20] == MSOS_UUID);
    chk(&mut v, "platform capability UUID is MS OS 2.0", uuid_ok);
    let version = c.map_or(0, |c| u32at(c, 20));
    chk(
        &mut v,
        format!("dwWindowsVersion {version:#010x} >= {WINDOWS_8_1:#010x}"),
        c.is_some() && version >= WINDOWS_8_1,
    );
    let info = c.filter(|_| uuid_ok).map(|c| Bos {
        windows_version: version,
        set_len: u16at(c, 24),
        vendor_code: c[26],
    });
    (v, info)
}

/// Split a run of MS OS 2.0 descriptors (wLength u16, wDescriptorType u16, body) into (type, whole descriptor); None when the lengths do not add up.
fn walk(mut b: &[u8]) -> Option<Vec<(u16, &[u8])>> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let l = u16at(b, 0) as usize;
        if l < 4 || l > b.len() {
            return None;
        }
        out.push((u16at(b, 2), &b[..l]));
        b = &b[l..];
    }
    Some(out)
}

/// Check the MS OS 2.0 descriptor set against the length the BOS announced and the vendor interface of the configuration.
pub fn check_set(set: &[u8], bos_set_len: u16, interface: Option<u8>) -> Vec<Check> {
    let mut v = Vec::new();
    let total = u16at(set, 8) as usize;
    chk(
        &mut v,
        "set header: wLength 10, type 0 (SET_HEADER_DESCRIPTOR)",
        set.len() >= 10 && u16at(set, 0) == 10 && u16at(set, 2) == 0,
    );
    chk(
        &mut v,
        format!(
            "set header wTotalLength {total} == BOS length {bos_set_len} == bytes received {}",
            set.len()
        ),
        total == bos_set_len as usize && total == set.len(),
    );
    chk(
        &mut v,
        format!(
            "set header dwWindowsVersion {:#010x} >= {WINDOWS_8_1:#010x}",
            u32at(set, 4)
        ),
        u32at(set, 4) >= WINDOWS_8_1,
    );
    let body = set.get(10..).unwrap_or(&[]);
    // Subset headers carry wLength 8; their wTotalLength / wSubsetLength cover the nested descriptors.
    let cfg = body.get(..8).filter(|c| u16at(c, 2) == 1);
    let fun_run = cfg.and_then(|_| body.get(8..)).unwrap_or(&[]);
    let fun = fun_run.get(..8).filter(|f| u16at(f, 2) == 2);
    let items = fun.and_then(|_| walk(&fun_run[8..]));
    chk(
        &mut v,
        "descriptor lengths add up to the set",
        items.is_some(),
    );
    chk(
        &mut v,
        "one configuration subset (type 1, wLength 8, bConfigurationValue 0, wTotalLength = rest of the set)",
        cfg.is_some_and(|c| u16at(c, 0) == 8 && c[4] == 0 && u16at(c, 6) as usize == body.len()),
    );
    chk(
        &mut v,
        "one function subset (type 2, wLength 8, wSubsetLength = rest of the configuration subset)",
        fun.is_some_and(|f| u16at(f, 0) == 8 && u16at(f, 6) as usize == fun_run.len()),
    );
    let first = fun.and_then(|f| f.get(4).copied());
    chk(
        &mut v,
        format!(
            "function bFirstInterface {} == vendor-class interface {}",
            first.map_or("?".to_string(), |n| n.to_string()),
            interface.map_or("?".to_string(), |n| n.to_string())
        ),
        first.is_some() && first == interface,
    );
    let find = |ty: u16| {
        items
            .as_ref()
            .and_then(|t| t.iter().find(|(k, _)| *k == ty))
            .map(|(_, d)| *d)
    };
    chk(
        &mut v,
        "compatible ID (type 3, wLength 20) is \"WINUSB\"",
        find(3).is_some_and(|d| d.len() == 20 && d[4..12] == *b"WINUSB\0\0"),
    );
    let name: Vec<u8> = "DeviceInterfaceGUIDs\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let reg = find(4).is_some_and(|d| {
        let nl = u16at(d, 6) as usize;
        let dl = u16at(d, 8 + nl) as usize;
        let data = d.get(10 + nl..).unwrap_or(&[]);
        d.len() >= 10 + nl
            && u16at(d, 4) == 7
            && d.get(8..8 + nl) == Some(&name[..])
            && data.len() == dl
            && guid_multi_sz(data)
    });
    chk(
        &mut v,
        "registry property (type 4, data type 7): \"DeviceInterfaceGUIDs\" = one {GUID}, NUL, NUL (UTF-16LE)",
        reg,
    );
    chk(
        &mut v,
        "set holds only the compatible ID and the registry property",
        items.as_ref().is_some_and(|t| t.len() == 2),
    );
    v
}

/// REG_MULTI_SZ of exactly one `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}` string.
fn guid_multi_sz(data: &[u8]) -> bool {
    if data.len() != 40 * 2 {
        return false;
    }
    let u: Vec<u16> = data
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s: Option<String> = u[..38].iter().map(|&c| char::from_u32(c as u32)).collect();
    let Some(s) = s else { return false };
    let b = s.as_bytes();
    u[38] == 0
        && u[39] == 0
        && b[0] == b'{'
        && b[37] == b'}'
        && [9, 14, 19, 24].iter().all(|&i| b[i] == b'-')
        && b.iter()
            .enumerate()
            .all(|(i, &c)| matches!(i, 0 | 9 | 14 | 19 | 24 | 37) || c.is_ascii_hexdigit())
}

/// Print-ready decoded lines of the set (one per descriptor found), for the report.
pub fn describe_set(set: &[u8]) -> Vec<String> {
    let mut out = vec![format!(
        "set header: wLength {}, type {}, dwWindowsVersion {:#010x}, wTotalLength {}",
        u16at(set, 0),
        u16at(set, 2),
        u32at(set, 4),
        u16at(set, 8)
    )];
    // Subset headers (types 1 and 2) are 8 bytes long and contain the descriptors after them.
    let mut rest = set.get(10..).unwrap_or(&[]);
    while !rest.is_empty() {
        let l = u16at(rest, 0) as usize;
        if l < 4 || l > rest.len() {
            out.push("  (descriptor lengths do not add up)".into());
            break;
        }
        let (ty, d) = (u16at(rest, 2), &rest[..l]);
        rest = &rest[l..];
        {
            let pad = "  ".repeat(match ty {
                1 => 1,
                2 => 2,
                _ => 3,
            });
            match ty {
                1 => out.push(format!(
                    "{pad}configuration subset: bConfigurationValue {}, wTotalLength {}",
                    d.get(4).copied().unwrap_or(0),
                    u16at(d, 6)
                )),
                2 => out.push(format!(
                    "{pad}function subset: bFirstInterface {}, wSubsetLength {}",
                    d.get(4).copied().unwrap_or(0),
                    u16at(d, 6)
                )),
                3 => out.push(format!(
                    "{pad}compatible ID: {:?}",
                    String::from_utf8_lossy(d.get(4..d.len().min(12)).unwrap_or(&[]))
                        .trim_end_matches('\0')
                )),
                4 => out.push(format!(
                    "{pad}registry property: data type {}, {} bytes",
                    u16at(d, 4),
                    d.len()
                )),
                t => out.push(format!("{pad}descriptor type {t}, {} bytes", d.len())),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUID: &str = "{6f4a1b2c-3d4e-4f50-8a9b-0c1d2e3f4a5b}";

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A valid set: header 10 + configuration subset 8 + function subset 8 + compatible ID 20 + registry property 132 = 178 bytes.
    fn valid_set(interface: u8) -> Vec<u8> {
        let name = utf16("DeviceInterfaceGUIDs\0");
        let data = utf16(&format!("{GUID}\0\0"));
        let mut reg = Vec::new();
        reg.extend(((8 + name.len() + 2 + data.len()) as u16).to_le_bytes());
        reg.extend(4u16.to_le_bytes());
        reg.extend(7u16.to_le_bytes());
        reg.extend((name.len() as u16).to_le_bytes());
        reg.extend(&name);
        reg.extend((data.len() as u16).to_le_bytes());
        reg.extend(&data);
        let mut compat = Vec::new();
        compat.extend(20u16.to_le_bytes());
        compat.extend(3u16.to_le_bytes());
        compat.extend(b"WINUSB\0\0");
        compat.extend([0u8; 8]);
        let fun_len = 8 + compat.len() + reg.len();
        let mut fun = Vec::new();
        fun.extend(8u16.to_le_bytes());
        fun.extend(2u16.to_le_bytes());
        fun.extend([interface, 0]);
        fun.extend((fun_len as u16).to_le_bytes());
        fun.extend(&compat);
        fun.extend(&reg);
        let cfg_len = 8 + fun.len();
        let mut cfg = Vec::new();
        cfg.extend(8u16.to_le_bytes());
        cfg.extend(1u16.to_le_bytes());
        cfg.extend([0, 0]);
        cfg.extend((cfg_len as u16).to_le_bytes());
        cfg.extend(&fun);
        let mut set = Vec::new();
        set.extend(10u16.to_le_bytes());
        set.extend(0u16.to_le_bytes());
        set.extend(WINDOWS_8_1.to_le_bytes());
        set.extend(((10 + cfg.len()) as u16).to_le_bytes());
        set.extend(&cfg);
        set
    }

    fn valid_bos(set_len: u16) -> Vec<u8> {
        let mut b = vec![5, 15, 33, 0, 1, 28, 0x10, 5, 0];
        b.extend(MSOS_UUID);
        b.extend(WINDOWS_8_1.to_le_bytes());
        b.extend(set_len.to_le_bytes());
        b.extend([0x21, 0]);
        b
    }

    fn failed(v: &[Check]) -> Vec<&str> {
        v.iter()
            .filter(|c| !c.ok)
            .map(|c| c.name.as_str())
            .collect()
    }

    #[test]
    fn valid_set_has_the_expected_size_and_passes() {
        let set = valid_set(2);
        assert_eq!(set.len(), 178);
        assert_eq!(failed(&check_set(&set, 178, Some(2))), Vec::<&str>::new());
        assert!(describe_set(&set).len() >= 5);
    }

    #[test]
    fn valid_bos_passes_and_names_the_vendor_code() {
        let bos = valid_bos(178);
        assert_eq!(bos.len(), 33);
        let (checks, info) = check_bos(&bos);
        assert_eq!(failed(&checks), Vec::<&str>::new());
        assert_eq!(
            info,
            Some(Bos {
                windows_version: WINDOWS_8_1,
                set_len: 178,
                vendor_code: 0x21
            })
        );
    }

    #[test]
    fn bos_with_another_uuid_fails() {
        let mut bos = valid_bos(178);
        bos[10] ^= 1;
        let (checks, info) = check_bos(&bos);
        assert_eq!(info, None);
        assert!(!failed(&checks).is_empty());
    }

    #[test]
    fn bos_with_an_old_windows_version_fails() {
        let mut bos = valid_bos(178);
        bos[28] = 0x05; // dwWindowsVersion 0x05030000
        assert!(!failed(&check_bos(&bos).0).is_empty());
    }

    #[test]
    fn set_with_a_wrong_total_length_fails() {
        let mut set = valid_set(2);
        set[8] = 177;
        assert!(!failed(&check_set(&set, 178, Some(2))).is_empty());
        assert!(!failed(&check_set(&valid_set(2), 177, Some(2))).is_empty());
        let short = &valid_set(2)[..170];
        assert!(!failed(&check_set(short, 178, Some(2))).is_empty());
    }

    #[test]
    fn set_for_another_interface_fails() {
        let set = valid_set(3);
        let checks = check_set(&set, 178, Some(2));
        let f = failed(&checks);
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("bFirstInterface"));
    }

    #[test]
    fn set_without_winusb_fails() {
        let mut set = valid_set(2);
        let at = set.windows(6).position(|w| w == b"WINUSB").unwrap();
        set[at..at + 6].copy_from_slice(b"LIBUSB");
        let checks = check_set(&set, 178, Some(2));
        let f = failed(&checks);
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("WINUSB"));
    }

    #[test]
    fn set_with_a_wrong_property_name_fails() {
        let mut set = valid_set(2);
        let at = set.windows(2).position(|w| w == [b'D', 0]).unwrap();
        set[at] = b'd';
        assert!(
            failed(&check_set(&set, 178, Some(2)))
                .iter()
                .any(|n| n.contains("registry"))
        );
    }

    #[test]
    fn vendor_interface_is_found_in_a_configuration() {
        let mut cfg = vec![9, 2, 0, 0, 3, 1, 0, 0x80, 50];
        cfg.extend([9, 4, 0, 0, 0, 1, 1, 0, 0]); // audio
        cfg.extend([9, 4, 1, 0, 2, 0xFF, 0, 0, 0]); // vendor
        let n = cfg.len() as u16;
        cfg[2..4].copy_from_slice(&n.to_le_bytes());
        assert_eq!(config_total_length(&cfg[..9]), Some(cfg.len()));
        assert_eq!(vendor_interface(&cfg), Some(1));
        assert_eq!(vendor_interface(&cfg[..18]), None);
    }
}
