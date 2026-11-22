//! The port file at run time (docs/plans/port-file.md): what the loader reads of the table of the loaded port file (role 2).
//!
//! A service file names the firmware through imports of the forms `dk_fw_<hash>`, `dk_exit_<hash>_<n>`, `dk_tail_<hash>`, `dk_stock_<hash>` and `dk_ret_<hash>`
//! (`<hash>`: 8 lower-case hex digits of the `name_hash` of an anchor or a hook). The loader resolves them here, in `check_file` of a service file (a module file and a
//! reload never ask), from the table copy the port file keeps and its image:
//!
//! - a name or an exit: the address of its `FWNAMES` item;
//! - a tail or a stock path: the copy of the displaced instructions of the site, followed by `jmp.l` behind the site, in the bss of the port file (`tail_off`);
//! - a `dk_ret_`: the `rts` at the start of the port file's image.
//!
//! A name that the firmware of the port lacks (flag `absent`, or a site of that flag) resolves to 0 and is accepted: the code that uses it never runs on this firmware.
//! A name that the port file does not list is `Lookup::Unlisted`, which refuses the service file with `E_IMPORT`.
use dkneeds::keys::{Keys, SiteRec, FLAG_ABSENT, NAME_EXIT, NAME_FW, SITE_RAW, SITE_STOCK};

/// The longest displaced run of a site (the longest stock run a bound file held).
const SITE_LEN_MAX: u16 = 64;
/// The shortest: `jmp.l` is 6 bytes.
const SITE_LEN_MIN: u16 = 6;
/// Bytes after a copied run: `jmp.l <address>`.
const JMP_BYTES: u32 = 6;
/// `rts`: the first word of the port file's image, the target of every `dk_ret_`.
pub const RTS: u16 = 0x4e75;

/// A firmware reference of an import name: its kind (`R_*`), the name hash and, for an exit, the exit number. A struct and plain tests, not an enum and a `match`: a
/// match on four or more dense arms compiles to a jump table, which is not ColdFire code (`patchkit build` refuses it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ref {
    pub kind: u8,
    pub hash: u32,
    pub n: u16,
}

/// `dk_fw_`: the address of a firmware name.
pub const R_NAME: u8 = 0;
/// `dk_exit_<hash>_<n>`: the end of a hook's site (`n` 0) or an exit of the hook.
pub const R_EXIT: u8 = 1;
/// `dk_tail_`: the copy of the displaced instructions of a hook.
pub const R_TAIL: u8 = 2;
/// `dk_stock_`: the same code, called from inside the hook.
pub const R_STOCK: u8 = 3;
/// `dk_ret_`: one `rts`.
pub const R_RET: u8 = 4;

impl Ref {
    pub const fn new(kind: u8, hash: u32, n: u16) -> Ref {
        Ref { kind, hash, n }
    }
}

/// What the port file says about a reference.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lookup {
    /// The import is no firmware reference, or no port file is loaded for it to ask.
    NotPort,
    /// The address.
    Addr(u32),
    /// The firmware has no such name or site: the import is 0.
    Absent,
    /// The port file does not list the name (or the site has no copy to refer to): the hash that is missing.
    Unlisted(u32),
}

fn hex8(b: &[u8]) -> Option<u32> {
    if b.len() != 8 {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0;
    while i < 8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => c - b'0',
            c @ b'a'..=b'f' => c - b'a' + 10,
            _ => return None,
        };
        v = v << 4 | d as u32;
        i += 1;
    }
    Some(v)
}

fn strip<'a>(name: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if name.len() >= prefix.len() && &name[..prefix.len()] == prefix {
        Some(&name[prefix.len()..])
    } else {
        None
    }
}

/// The reference an import name stands for; `None` for every other name (a function of the runtime, an export of a service).
pub fn parse_import(name: &[u8]) -> Option<Ref> {
    const FORMS: [(&[u8], u8); 4] = [(b"dk_fw_", R_NAME), (b"dk_tail_", R_TAIL), (b"dk_stock_", R_STOCK), (b"dk_ret_", R_RET)];
    let mut f = 0;
    while f < FORMS.len() {
        if let Some(h) = strip(name, FORMS[f].0) {
            return hex8(h).map(|h| Ref::new(FORMS[f].1, h, 0));
        }
        f += 1;
    }
    let rest = strip(name, b"dk_exit_")?;
    if rest.len() < 10 || rest.len() > 11 || rest[8] != b'_' {
        return None;
    }
    let digits = &rest[9..];
    let mut n = 0u16;
    let mut i = 0;
    while i < digits.len() {
        if !digits[i].is_ascii_digit() {
            return None;
        }
        n = n * 10 + (digits[i] - b'0') as u16;
        i += 1;
    }
    Some(Ref::new(R_EXIT, hex8(&rest[..8])?, n))
}

/// Does the site carry a copy of its displaced bytes (a tail or a stock path)?
pub fn copies(s: &SiteRec) -> bool {
    s.kind != SITE_RAW
}

/// A site the runtime can use: a length of whole instructions that `jmp.l` fits in, a known kind and no fixups (the encoder is not there).
pub fn site_ok(s: &SiteRec) -> bool {
    s.len >= SITE_LEN_MIN && s.len <= SITE_LEN_MAX && s.len % 2 == 0 && s.kind <= SITE_STOCK && s.flags >> 4 == 0
}

/// The bytes of one copied site in the bss of the port file: the displaced run and the jump behind it.
pub fn tail_bytes(s: &SiteRec) -> u32 {
    s.len as u32 + JMP_BYTES
}

/// The bytes of bss the copies of all sites need.
pub fn tails_total(keys: &Keys) -> u32 {
    let mut total = 0u32;
    let mut i = 0;
    while let Some(s) = keys.site(i) {
        i += 1;
        if s.flags & FLAG_ABSENT == 0 && copies(&s) {
            total = total.saturating_add(tail_bytes(&s));
        }
    }
    total
}

/// The offset from the port file's base of the copy of site `hash`: the copies follow the image in SITES order, each `tail_bytes` long. `None` for a site with no copy
/// (not listed, absent, raw).
pub fn tail_off(keys: &Keys, image_size: u32, hash: u32) -> Option<u32> {
    let mut off = image_size;
    let mut i = 0;
    while let Some(s) = keys.site(i) {
        i += 1;
        if s.flags & FLAG_ABSENT != 0 || !copies(&s) {
            continue;
        }
        if s.hash == hash {
            return Some(off);
        }
        off = off.saturating_add(tail_bytes(&s));
    }
    None
}

/// Resolve `r` against the table `keys` of the port file loaded at `base` with an image of `image_size` bytes.
pub fn lookup(keys: &Keys, base: u32, image_size: u32, r: Ref) -> Lookup {
    if r.kind == R_NAME || r.kind == R_EXIT {
        return name(keys, if r.kind == R_NAME { NAME_FW } else { NAME_EXIT }, r.hash, r.n);
    }
    let h = r.hash;
    let Some(s) = keys.find_site(h) else {
        return Lookup::Unlisted(h);
    };
    if s.flags & FLAG_ABSENT != 0 {
        return Lookup::Absent;
    }
    if r.kind == R_RET {
        return Lookup::Addr(base);
    }
    // a tail or a stock path: the copy (a raw site has none)
    if !copies(&s) {
        return Lookup::Unlisted(h);
    }
    match tail_off(keys, image_size, h) {
        Some(off) => Lookup::Addr(base.wrapping_add(off)),
        None => Lookup::Unlisted(h),
    }
}

fn name(keys: &Keys, kind: u8, hash: u32, n: u16) -> Lookup {
    match keys.find_name(kind, hash, n) {
        None => Lookup::Unlisted(hash),
        Some(x) if x.flags & FLAG_ABSENT != 0 => Lookup::Absent,
        Some(x) => Lookup::Addr(x.addr),
    }
}

/// The check value of the `len` live bytes at `addr`: the first 8 bytes of their SHA-256.
pub unsafe fn site_check<P: dkrt_abi::package_platform::PackagePlatform>(p: &P, addr: u32, len: usize) -> [u8; 8] {
    let mut cur = [0u8; SITE_LEN_MAX as usize];
    p.read_code(addr, &mut cur[..len]);
    let mut sha = crate::sha256::Sha::new();
    sha.update(cur.as_ptr(), len as u32);
    let mut out = [0u8; 32];
    sha.finish(out.as_mut_ptr());
    let mut v = [0u8; 8];
    v.copy_from_slice(&out[..8]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use dkneeds::keys::{write_table, NameRec, Pair, Table, SITE_TAIL};
    use std::vec;

    fn site(hash: u32, addr: u32, len: u16, kind: u8, flags: u8) -> SiteRec {
        SiteRec { hash, addr, len, kind, flags, check: [0; 8] }
    }

    fn table() -> std::vec::Vec<u8> {
        let names = [
            NameRec { hash: 5, addr: 0x100, n: 0, kind: NAME_FW, flags: 0 },
            NameRec { hash: 6, addr: 0, n: 0, kind: NAME_FW, flags: FLAG_ABSENT },
            NameRec { hash: 9, addr: 0x240, n: 1, kind: NAME_EXIT, flags: 0 },
        ];
        let sites = [
            site(7, 0x1000, 8, SITE_TAIL, 0),
            site(8, 0x2000, 6, SITE_RAW, 0),
            site(10, 0x3000, 10, SITE_STOCK, 0),
            site(11, 0, 0, SITE_RAW, FLAG_ABSENT),
        ];
        let t = Table { names: &names, sites: &sites, fw: &[[1; 8]], provides: &[Pair { a: 1, b: 2 }], ..Table::default() };
        let mut b = vec![0u8; dkneeds::keys::table_size(&t)];
        write_table(&t, &mut b).unwrap();
        b
    }

    #[test]
    fn the_import_names_of_the_firmware_references_are_parsed() {
        assert_eq!(parse_import(b"dk_fw_0000beef"), Some(Ref::new(R_NAME, 0xbeef, 0)));
        assert_eq!(parse_import(b"dk_tail_deadbeef"), Some(Ref::new(R_TAIL, 0xdead_beef, 0)));
        assert_eq!(parse_import(b"dk_stock_00000001"), Some(Ref::new(R_STOCK, 1, 0)));
        assert_eq!(parse_import(b"dk_ret_00000002"), Some(Ref::new(R_RET, 2, 0)));
        assert_eq!(parse_import(b"dk_exit_00000003_0"), Some(Ref::new(R_EXIT, 3, 0)));
        assert_eq!(parse_import(b"dk_exit_00000003_12"), Some(Ref::new(R_EXIT, 3, 12)));
        // another name, upper-case digits, a short hash or a bad exit number is no reference
        for bad in [&b"dk_alloc"[..], b"dk_fw_0000BEEF", b"dk_fw_beef", b"dk_exit_00000003_", b"dk_exit_00000003_x", b"dk_exit_00000003_123", b"dk_exit_00000003", b"dk_tail_"] {
            assert_eq!(parse_import(bad), None, "{:?}", std::str::from_utf8(bad));
        }
    }

    #[test]
    fn a_name_resolves_to_its_address_to_zero_when_absent_and_is_refused_when_not_listed() {
        let b = table();
        let k = Keys::parse(&b).unwrap();
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_NAME, 5, 0)), Lookup::Addr(0x100));
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_NAME, 6, 0)), Lookup::Absent);
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_NAME, 99, 0)), Lookup::Unlisted(99));
        // a name is not an exit and an exit is not a name
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_EXIT, 5, 0)), Lookup::Unlisted(5));
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_EXIT, 9, 1)), Lookup::Addr(0x240));
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_EXIT, 9, 2)), Lookup::Unlisted(9));
    }

    #[test]
    fn the_copies_follow_the_image_in_sites_order_and_raw_and_absent_sites_have_none() {
        let b = table();
        let k = Keys::parse(&b).unwrap();
        // site 7: 8 + 6 bytes at the image's end; site 10 behind it
        assert_eq!(tail_off(&k, 16, 7), Some(16));
        assert_eq!(tail_off(&k, 16, 10), Some(16 + 14));
        assert_eq!(tail_off(&k, 16, 8), None);
        assert_eq!(tail_off(&k, 16, 11), None);
        assert_eq!(tails_total(&k), 14 + 16);
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_TAIL, 7, 0)), Lookup::Addr(0x4010));
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_STOCK, 10, 0)), Lookup::Addr(0x4000 + 30));
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_TAIL, 8, 0)), Lookup::Unlisted(8), "a raw site has no copy");
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_TAIL, 11, 0)), Lookup::Absent);
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_TAIL, 77, 0)), Lookup::Unlisted(77));
        // the return stub is the first word of the image, for every listed site
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_RET, 8, 0)), Lookup::Addr(0x4000));
        assert_eq!(lookup(&k, 0x4000, 16, Ref::new(R_RET, 11, 0)), Lookup::Absent);
    }

    #[test]
    fn a_site_the_runtime_can_use_has_whole_instructions_a_known_kind_and_no_fixups() {
        assert!(site_ok(&site(1, 0, 6, SITE_RAW, 0)));
        assert!(site_ok(&site(1, 0, 64, SITE_TAIL, 0)));
        for bad in [site(1, 0, 4, 0, 0), site(1, 0, 7, 0, 0), site(1, 0, 66, 0, 0), site(1, 0, 6, 3, 0), site(1, 0, 6, 0, 0x10)] {
            assert!(!site_ok(&bad), "{bad:?}");
        }
    }
}
