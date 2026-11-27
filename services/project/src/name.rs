//! The store names of the project files (docs/design/digikit-project.md section 5): `projects/<number>-<name>.dkp`, the number `001` to `128` (the slot the stock menus show: firmware slot + 1), and `projects/archive/<name>-<stamp>.dkp`.
//!
//! The name is the stock project name as the slot listing reads it: 16 bytes, zero padded, bytes of 0x80 and up stored as 0 by the firmware. A store path is 1 to 159 bytes of
//! 0x20 to 0x7e, so the bytes of the stock table that are not (`c5 c4 d6 dc df c6 d8 c7 d1`, stored as 0 or kept) and anything else outside that range become `_`, and so does `/`.
//! Trailing spaces and zeros are padding and go; a name that is nothing but padding gives an empty name part (`013-.dkp`).

/// The folder of the project files and the folder of the archive.
pub const DIR: &[u8] = b"projects";
pub const ARCHIVE_DIR: &[u8] = b"projects/archive";
/// The extension of a project file.
pub const EXT: &[u8] = b".dkp";
/// The most bytes of a path of this service: `projects/archive/` + 16 + `-` + 8 + `-N` + `.dkp`.
pub const PATH_MAX: usize = 48;
/// The most archive files of one name and stamp: the first has no suffix, the others `-2` to `-9` (one digit keeps the longest path within [`PATH_MAX`]).
pub const ARCHIVE_COPIES_MAX: u32 = 9;
/// The stock name length.
pub const NAME_MAX: usize = 16;

/// A path of this service (bytes of 0x20 to 0x7e, at most [`PATH_MAX`]).
#[derive(Clone, Copy)]
pub struct Path {
    pub b: [u8; PATH_MAX],
    pub n: usize,
}

impl Path {
    pub const EMPTY: Path = Path {
        b: [0; PATH_MAX],
        n: 0,
    };
    pub fn as_bytes(&self) -> &[u8] {
        &self.b[..self.n]
    }
    fn push(&mut self, s: &[u8]) {
        let mut i = 0;
        while i < s.len() {
            self.b[self.n + i] = s[i];
            i += 1;
        }
        self.n += s.len();
    }
    /// A path as the store holds it: bytes of 0x20 to 0x7e only (a carried path is checked with this).
    pub fn from_store_bytes(s: &[u8]) -> Option<Path> {
        if !s.iter().all(|b| (0x20..=0x7e).contains(b)) {
            return None;
        }
        Path::from_bytes(s)
    }
    pub fn from_bytes(s: &[u8]) -> Option<Path> {
        if s.len() > PATH_MAX {
            return None;
        }
        let mut p = Path::EMPTY;
        p.push(s);
        Some(p)
    }
}

/// The length of `name` without its trailing spaces and zeros.
pub fn trimmed_len(name: &[u8]) -> usize {
    let mut n = name.len().min(NAME_MAX);
    while n > 0 && (name[n - 1] == 0 || name[n - 1] == b' ') {
        n -= 1;
    }
    n
}

/// The length of `name` without its trailing zeros only (what the header's name length is: the name as read, spaces kept).
pub fn trimmed_len_raw(name: &[u8]) -> usize {
    let mut n = name.len().min(NAME_MAX);
    while n > 0 && name[n - 1] == 0 {
        n -= 1;
    }
    n
}

/// The name part of a file name: the trimmed stock name with each byte that a store path cannot hold (or `/`) as `_`.
pub fn clean(name: &[u8], out: &mut [u8; NAME_MAX]) -> usize {
    let n = trimmed_len(name);
    let mut i = 0;
    while i < n {
        let c = name[i];
        out[i] = if !(0x20..=0x7e).contains(&c) || c == b'/' {
            b'_'
        } else {
            c
        };
        i += 1;
    }
    n
}

/// The most firmware slots: 0 to 127.
pub const SLOTS: u32 = 128;

/// The file number of a firmware slot (0 to 127): the number the stock menus show, 1 to 128. The only place that maps between the two.
pub const fn file_number(slot: u32) -> u32 {
    slot + 1
}

/// The firmware slot of a file number (1 to 128); `None` for 0 and above 128.
pub const fn slot_of_number(number: u32) -> Option<u32> {
    if number >= 1 && number <= SLOTS {
        Some(number - 1)
    } else {
        None
    }
}

fn digits3(number: u32, out: &mut [u8; 3]) {
    out[0] = b'0' + (number / 100 % 10) as u8;
    out[1] = b'0' + (number / 10 % 10) as u8;
    out[2] = b'0' + (number % 10) as u8;
}

/// `projects/<number>-<name>.dkp` for the firmware `slot` 0 to 127 (number 001 to 128).
pub fn file_path(slot: u32, name: &[u8]) -> Path {
    let mut p = Path::EMPTY;
    p.push(DIR);
    p.push(b"/");
    let mut d = [0u8; 3];
    digits3(file_number(slot), &mut d);
    p.push(&d);
    p.push(b"-");
    let mut c = [0u8; NAME_MAX];
    let n = clean(name, &mut c);
    p.push(&c[..n]);
    p.push(EXT);
    p
}

fn hex8(v: u32, out: &mut [u8; 8]) {
    let mut i = 0;
    while i < 8 {
        let d = ((v >> (28 - 4 * i)) & 15) as u8;
        out[i] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        i += 1;
    }
}

/// `projects/archive/<name>-<stamp>.dkp`, the stamp as 8 lower case hex digits.
pub fn archive_path(name: &[u8], stamp: u32) -> Path {
    archive_path_copy(name, stamp, 1)
}

/// The `copy`-th archive path of a name and stamp (`copy` 1 to [`ARCHIVE_COPIES_MAX`]): copy 1 is [`archive_path`], copy `n` adds `-n` before the extension.
pub fn archive_path_copy(name: &[u8], stamp: u32, copy: u32) -> Path {
    let mut p = Path::EMPTY;
    p.push(ARCHIVE_DIR);
    p.push(b"/");
    let mut c = [0u8; NAME_MAX];
    let n = clean(name, &mut c);
    p.push(&c[..n]);
    p.push(b"-");
    let mut h = [0u8; 8];
    hex8(stamp, &mut h);
    p.push(&h);
    if copy > 1 && copy <= ARCHIVE_COPIES_MAX {
        p.push(&[b'-', b'0' + copy as u8]);
    }
    p.push(EXT);
    p
}

/// The firmware slot of a listing row `projects/<number>-<name>.dkp` (the row's whole path): `None` for any other row (the archive folder, a file of another shape).
pub fn slot_of_row(row: &[u8]) -> Option<u32> {
    let pre = DIR.len() + 1;
    if row.len() < pre + 4 + EXT.len() || &row[..DIR.len()] != DIR || row[DIR.len()] != b'/' {
        return None;
    }
    let f = &row[pre..];
    if f[3] != b'-' || !f.ends_with(EXT) {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0;
    while i < 3 {
        if !f[i].is_ascii_digit() {
            return None;
        }
        v = v * 10 + (f[i] - b'0') as u32;
        i += 1;
    }
    slot_of_number(v)
}

/// The name part of a project file's path (between `<number>-` and `.dkp`), for a path that `slot_of_row` accepts.
pub fn name_part(row: &[u8]) -> &[u8] {
    let pre = DIR.len() + 1 + 4;
    &row[pre..row.len() - EXT.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(s: &[u8]) -> [u8; 16] {
        let mut n = [0u8; 16];
        n[..s.len()].copy_from_slice(s);
        n
    }

    #[test]
    fn a_name_keeps_the_stock_table_and_pads_go() {
        assert_eq!(
            file_path(12, &pad(b"MY SONG")).as_bytes(),
            b"projects/013-MY SONG.dkp"
        );
        assert_eq!(
            file_path(0, &pad(b"A-Z 09~!@#$%^&)")).as_bytes(),
            b"projects/001-A-Z 09~!@#$%^&).dkp"
        );
        // spaces and zeros at the end are padding
        let mut n = pad(b"X ");
        n[2] = 0;
        assert_eq!(file_path(127, &n).as_bytes(), b"projects/128-X.dkp");
    }

    #[test]
    fn bytes_a_path_cannot_hold_become_underscores_and_an_empty_name_is_valid() {
        // the stock letters Ä Ö Ü are bytes of 0xc4 and up; a zero inside the name; a slash
        let mut n = pad(b"A");
        n[1] = 0xc4;
        n[2] = 0;
        n[3] = b'/';
        n[4] = b'B';
        assert_eq!(file_path(5, &n).as_bytes(), b"projects/006-A___B.dkp");
        assert_eq!(file_path(12, &[0u8; 16]).as_bytes(), b"projects/013-.dkp");
        assert_eq!(file_path(12, &[b' '; 16]).as_bytes(), b"projects/013-.dkp");
    }

    #[test]
    fn the_longest_path_fits_a_store_path() {
        let full = file_path(127, &[b'W'; 16]);
        assert_eq!(full.n, 9 + 3 + 1 + 16 + 4);
        let a = archive_path(&[b'W'; 16], 0xdead_beef);
        assert_eq!(
            a.as_bytes(),
            b"projects/archive/WWWWWWWWWWWWWWWW-deadbeef.dkp"
        );
        assert!(a.n <= PATH_MAX && a.n <= 159);
        assert!(full
            .as_bytes()
            .iter()
            .chain(a.as_bytes())
            .all(|&c| (0x20..=0x7e).contains(&c)));
    }

    #[test]
    fn a_numbered_archive_copy_adds_a_suffix_and_the_longest_still_fits() {
        let name = [b'W'; 16];
        assert_eq!(
            archive_path_copy(&name, 0xdead_beef, 1).as_bytes(),
            archive_path(&name, 0xdead_beef).as_bytes()
        );
        assert_eq!(
            archive_path_copy(&name, 0xdead_beef, 2).as_bytes(),
            b"projects/archive/WWWWWWWWWWWWWWWW-deadbeef-2.dkp"
        );
        assert_eq!(archive_path_copy(&name, 1, ARCHIVE_COPIES_MAX).n, PATH_MAX);
    }

    #[test]
    fn the_file_number_is_the_menu_number_of_the_slot() {
        assert_eq!(file_path(0, &pad(b"X")).as_bytes(), b"projects/001-X.dkp");
        assert_eq!(file_path(127, &pad(b"X")).as_bytes(), b"projects/128-X.dkp");
        assert_eq!(slot_of_row(b"projects/001-X.dkp"), Some(0));
        assert_eq!(slot_of_row(b"projects/128-X.dkp"), Some(127));
        assert_eq!(slot_of_row(b"projects/000-x.dkp"), None);
        assert_eq!(slot_of_row(b"projects/129-x.dkp"), None);
    }

    #[test]
    fn a_listing_row_names_its_slot_only_when_it_has_the_shape() {
        assert_eq!(slot_of_row(b"projects/013-MY SONG.dkp"), Some(12));
        assert_eq!(slot_of_row(b"projects/128-.dkp"), Some(127));
        assert_eq!(name_part(b"projects/013-MY SONG.dkp"), b"MY SONG");
        for bad in [
            &b"projects/archive"[..],
            b"projects/000-X.dkp",
            b"projects/129-X.dkp",
            b"projects/12-X.dkp",
            b"projects/012_X.dkp",
            b"projects/012-X.dk",
            b"modules/012-X.dkp",
            b"projects/0a2-X.dkp",
        ] {
            assert_eq!(slot_of_row(bad), None, "{:?}", core::str::from_utf8(bad));
        }
    }
}
