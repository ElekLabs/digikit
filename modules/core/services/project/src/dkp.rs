//! The project file `.dkp` (docs/design/digikit-project.md section 4) over the record codec of the settings record (`dkrt_sdk::settings::Fmt`): the header, the sections, the
//! RAM image and the three calls of the project scope (`get`, `put`, `remove`).
//!
//! ```text
//! 0   4   magic "DKPJ"
//! 4   2   u16 version = 1
//! 6   2   u16 device id (`[digikit] id`)
//! 8   1   slot (0 to 127)
//! 9   1   name length (0 to 16)
//! 10  16  name, zero padded, as the slot listing read it
//! 26  4   stock save stamp (the listing record's word at +0x18)
//! 30  4   write counter: 1 for a new file, +1 for each write (the `seq` of the codec)
//! 34  4   `used`, 42 to 16384
//! 38  4   crc32 (zlib) over 0..38 and 42..used
//! 42  ..  sections: u8 tag | u16 length | body; a tag this version does not know is skipped
//!           tag 1, the module list: u8 id length | id | u32 hash, repeated
//!           tag 2, the data of one owner: u8 owner length | owner | entries (u8 key length | key | u16 value length | value), each key `<owner>/<key>`
//! ```
//! All integers are big-endian. The RAM image is the file's own bytes: a `put` splices into the owner's section and keeps `used` current; `seal` writes the counter and the crc32.
use dkrt_sdk::proto::{E_SET_FULL, E_SET_KEY, E_SET_NOKEY, E_SET_VALUE};
use dkrt_sdk::settings::{
    entry_at, find_from, get16, get32, module_key, put16, put32, shift, Fmt, KEY_MAX, VALUE_MAX,
};

pub const MAGIC: u32 = 0x444b_504a; // "DKPJ"
pub const VERSION: u16 = 1;
pub const HEADER: usize = 42;
/// The most bytes of a file and of a RAM image.
pub const AREA: usize = 16384;
pub const FMT: Fmt = Fmt {
    magic: MAGIC,
    version: VERSION,
    header: HEADER,
    area: AREA,
};

const O_DEVICE: usize = 6;
const O_SLOT: usize = 8;
const O_NAME_LEN: usize = 9;
const O_NAME: usize = 10;
const O_STAMP: usize = 26;

pub const TAG_MODULES: u8 = 1;
pub const TAG_DATA: u8 = 2;

/// A RAM image: the bytes of a file, 16-byte aligned (`dk_files_write` needs it).
#[repr(C, align(16))]
pub struct Image(pub [u8; AREA]);

impl Image {
    pub const fn new() -> Image {
        Image([0; AREA])
    }
}

impl Default for Image {
    fn default() -> Self {
        Self::new()
    }
}

/// The fields of the header that name the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    pub device: u16,
    pub slot: u8,
    pub name_len: u8,
    pub name: [u8; 16],
    pub stamp: u32,
    pub counter: u32,
    pub used: u32,
}

/// Why a file is not used (the reason word of `PROJECT_IGNORED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ignore {
    /// A bad crc32, magic, bounds or section.
    Format = 1,
    /// A file of another device.
    Device = 2,
    /// A version this service does not read.
    Version = 3,
    /// The file's save stamp is not the slot's.
    Stamp = 4,
    /// Not a file's reason: the store refused the listing of the slot's files, so the project could not be bound.
    Store = 5,
    /// Not a file's reason: a stock save into the slot whose load failed was skipped (it has no image to write, and its files stay as they are).
    SaveSkipped = 6,
    /// Not a file's reason: a stock save was dropped because the firmware's listing had no record of the slot within `REC_WAIT_MS`.
    SaveNoRecord = 7,
}

/// The header of `b` as it stands (no checks; `b` holds at least `HEADER` bytes).
pub fn head_of(b: &[u8]) -> Head {
    let mut name = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        name[i] = b[O_NAME + i];
        i += 1;
    }
    Head {
        device: get16(b, O_DEVICE) as u16,
        slot: b[O_SLOT],
        name_len: b[O_NAME_LEN],
        name,
        stamp: get32(b, O_STAMP),
        counter: get32(b, FMT.o_seq()),
        used: get32(b, FMT.o_used()),
    }
}

/// Write the fields of `h` that name the project (not `used`, and not the crc32: `seal` does).
pub fn set_head(b: &mut [u8], h: &Head) {
    put16(b, O_DEVICE, h.device as u32);
    b[O_SLOT] = h.slot;
    b[O_NAME_LEN] = h.name_len;
    let mut i = 0;
    while i < 16 {
        b[O_NAME + i] = h.name[i];
        i += 1;
    }
    put32(b, O_STAMP, h.stamp);
}

/// Make `b` an empty image for this project: the header and no section.
pub fn reset(b: &mut [u8], device: u16, slot: u8, name: &[u8; 16], stamp: u32) {
    FMT.init(b, 0);
    let mut n = 16;
    while n > 0 && (name[n - 1] == 0) {
        n -= 1;
    }
    set_head(
        b,
        &Head {
            device,
            slot,
            name_len: n as u8,
            name: *name,
            stamp,
            counter: 0,
            used: HEADER as u32,
        },
    );
}

/// The write counter and the crc32 of a file's header: two files that agree on both hold the same bytes (the crc32 covers the counter). `b` holds a whole file.
pub fn identity(b: &[u8]) -> (u32, u32) {
    (head_of(b).counter, get32(b, FMT.o_crc()))
}

/// Set the write counter and the crc32 of an image whose bytes are final.
pub fn seal(b: &mut [u8], counter: u32) {
    FMT.seal(b, counter);
}

/// A section: where it starts, its tag and where its body lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sec {
    at: usize,
    tag: u8,
    body: usize,
    end: usize,
}

fn sec_at(b: &[u8], used: usize, at: usize) -> Option<Sec> {
    if at + 3 > used || used > b.len() {
        return None;
    }
    let len = get16(b, at + 1) as usize;
    if at + 3 + len > used {
        return None;
    }
    Some(Sec {
        at,
        tag: b[at],
        body: at + 3,
        end: at + 3 + len,
    })
}

fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn owner_of(b: &[u8], s: &Sec) -> Option<(usize, usize)> {
    if s.tag != TAG_DATA || s.body >= s.end {
        return None;
    }
    let n = b[s.body] as usize;
    if n == 0 || s.body + 1 + n > s.end {
        return None;
    }
    Some((s.body + 1, n))
}

fn find_section(b: &[u8], used: usize, owner: &[u8]) -> Option<Sec> {
    let mut at = HEADER;
    while at < used {
        let s = sec_at(b, used, at)?;
        if let Some((o, n)) = owner_of(b, &s) {
            if same(&b[o..o + n], owner) {
                return Some(s);
            }
        }
        at = s.end;
    }
    None
}

/// Does the key start with `<owner>/`?
fn has_prefix(key: &[u8], owner: &[u8]) -> bool {
    key.len() > owner.len() + 1 && same(&key[..owner.len()], owner) && key[owner.len()] == b'/'
}

/// Check a file as read from the store: the codec (magic, version, bounds, crc32), the device, the header's own bounds and every section. `device` is this device's id.
pub fn check(b: &[u8], device: u16) -> Result<Head, Ignore> {
    if b.len() < HEADER || dkrt_sdk::settings::get32(b, 0) != MAGIC {
        return Err(Ignore::Format);
    }
    if get16(b, 4) != VERSION as u32 {
        return Err(Ignore::Version);
    }
    let (_, used) = FMT.peek(b).ok_or(Ignore::Format)?;
    if !FMT.crc_ok(b, used) {
        return Err(Ignore::Format);
    }
    let h = head_of(b);
    if h.device != device {
        return Err(Ignore::Device);
    }
    if h.slot > 127 || h.name_len > 16 {
        return Err(Ignore::Format);
    }
    sections_ok(b, used)?;
    Ok(h)
}

fn sections_ok(b: &[u8], used: usize) -> Result<(), Ignore> {
    let mut at = HEADER;
    let mut modules = false;
    while at < used {
        let s = sec_at(b, used, at).ok_or(Ignore::Format)?;
        if s.tag == TAG_MODULES {
            if modules {
                return Err(Ignore::Format);
            }
            modules = true;
            let mut p = s.body;
            while p < s.end {
                let n = b[p] as usize;
                if n == 0 || p + 1 + n + 4 > s.end {
                    return Err(Ignore::Format);
                }
                p += 1 + n + 4;
            }
        } else if s.tag == TAG_DATA {
            let (o, n) = owner_of(b, &s).ok_or(Ignore::Format)?;
            // one section of an owner
            if let Some(f) = find_section(b, used, &b[o..o + n]) {
                if f.at != s.at {
                    return Err(Ignore::Format);
                }
            }
            let mut p = o + n;
            while p < s.end {
                let e = entry_at(b, s.end, p).ok_or(Ignore::Format)?;
                let key = &b[e.key.0..e.key.0 + e.key.1];
                if !has_prefix(key, &b[o..o + n]) {
                    return Err(Ignore::Format);
                }
                // a key twice: sections are small
                let mut q = o + n;
                while q < p {
                    let f = entry_at(b, s.end, q).ok_or(Ignore::Format)?;
                    if same(&b[f.key.0..f.key.0 + f.key.1], key) {
                        return Err(Ignore::Format);
                    }
                    q = f.end;
                }
                p = e.end;
            }
        }
        at = s.end;
    }
    Ok(())
}

/// Does the image hold digikit data: a module list with a module, or a section of an owner? (A file is written only then.)
pub fn has_data(b: &[u8]) -> bool {
    let used = get32(b, FMT.o_used()) as usize;
    let mut at = HEADER;
    while at < used {
        let Some(s) = sec_at(b, used, at) else {
            return false;
        };
        if s.tag == TAG_DATA || (s.tag == TAG_MODULES && s.end > s.body) {
            return true;
        }
        at = s.end;
    }
    false
}

/// The modules of the module list, in order: (id, store hash).
pub struct Modules<'a> {
    b: &'a [u8],
    at: usize,
    end: usize,
}

pub fn modules(b: &[u8]) -> Modules<'_> {
    let used = get32(b, FMT.o_used()) as usize;
    let mut at = HEADER;
    while at < used {
        let Some(s) = sec_at(b, used, at) else { break };
        if s.tag == TAG_MODULES {
            return Modules {
                b,
                at: s.body,
                end: s.end,
            };
        }
        at = s.end;
    }
    Modules { b, at: 0, end: 0 }
}

impl<'a> Iterator for Modules<'a> {
    type Item = (&'a [u8], u32);
    fn next(&mut self) -> Option<Self::Item> {
        if self.at >= self.end {
            return None;
        }
        let n = self.b[self.at] as usize;
        if self.at + 1 + n + 4 > self.end {
            return None;
        }
        let id = &self.b[self.at + 1..self.at + 1 + n];
        let hash = get32(self.b, self.at + 1 + n);
        self.at += 1 + n + 4;
        Some((id, hash))
    }
}

/// Every entry of every data section, in file order: (owner, key as stored, value).
pub struct Entries<'a> {
    b: &'a [u8],
    used: usize,
    /// The next section to look at, and inside a data section the next entry and the section's end and owner.
    sec: usize,
    at: usize,
    end: usize,
    owner: (usize, usize),
}

pub fn entries(b: &[u8]) -> Entries<'_> {
    let used = (get32(b, FMT.o_used()) as usize).min(b.len());
    Entries {
        b,
        used,
        sec: HEADER,
        at: 0,
        end: 0,
        owner: (0, 0),
    }
}

impl<'a> Iterator for Entries<'a> {
    type Item = (&'a [u8], &'a [u8], &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.at < self.end {
                let e = entry_at(self.b, self.end, self.at)?;
                self.at = e.end;
                return Some((
                    &self.b[self.owner.0..self.owner.0 + self.owner.1],
                    &self.b[e.key.0..e.key.0 + e.key.1],
                    &self.b[e.value.0..e.value.0 + e.value.1],
                ));
            }
            if self.sec >= self.used {
                return None;
            }
            let s = sec_at(self.b, self.used, self.sec)?;
            self.sec = s.end;
            if let Some((o, n)) = owner_of(self.b, &s) {
                self.owner = (o, n);
                self.at = o + n;
                self.end = s.end;
            }
        }
    }
}

/// Replace the module list. An empty list removes the section. `E_SET_FULL` when the image would pass `AREA` (nothing changes), `E_SET_KEY` for an id of 0 or over 255 bytes.
pub fn set_modules(b: &mut [u8], list: &[(&[u8], u32)]) -> Result<(), i32> {
    let used = get32(b, FMT.o_used()) as usize;
    let mut body = 0usize;
    for (id, _) in list {
        if id.is_empty() || id.len() > 255 {
            return Err(E_SET_KEY);
        }
        body += 1 + id.len() + 4;
    }
    let mut at = HEADER;
    let mut old: Option<Sec> = None;
    while at < used {
        let s = sec_at(b, used, at).ok_or(E_SET_KEY)?;
        if s.tag == TAG_MODULES {
            old = Some(s);
            break;
        }
        at = s.end;
    }
    let old_len = old.map(|s| s.end - s.at).unwrap_or(0);
    let new_len = if list.is_empty() { 0 } else { 3 + body };
    if body > 0xffff || used - old_len + new_len > AREA {
        return Err(E_SET_FULL);
    }
    // the module list goes first: remove the old one, make room at HEADER, write
    if old_len > 0 {
        let s = old.unwrap_or(Sec {
            at: 0,
            tag: 0,
            body: 0,
            end: 0,
        });
        shift(b, s.end, used, s.at);
    }
    let used = used - old_len;
    if new_len > 0 {
        shift(b, HEADER, used, HEADER + new_len);
        b[HEADER] = TAG_MODULES;
        put16(b, HEADER + 1, body as u32);
        let mut p = HEADER + 3;
        for (id, hash) in list {
            b[p] = id.len() as u8;
            p += 1;
            let mut i = 0;
            while i < id.len() {
                b[p + i] = id[i];
                i += 1;
            }
            p += id.len();
            put32(b, p, *hash);
            p += 4;
        }
    }
    put32(b, FMT.o_used(), (used + new_len) as u32);
    Ok(())
}

/// `dk_settings_get` on the project scope: the value of `<owner>/<bare>` into `out`; its length, `E_SET_NOKEY`, `E_SET_KEY` (a key the rules refuse) or `E_SET_VALUE` (`out` too small).
pub fn get(b: &[u8], owner: &[u8], bare: &[u8], out: &mut [u8]) -> i32 {
    let mut full = [0u8; KEY_MAX];
    let Some(kn) = module_key(owner, bare, &mut full) else {
        return E_SET_KEY;
    };
    let used = get32(b, FMT.o_used()) as usize;
    let Some(s) = find_section(b, used, owner) else {
        return E_SET_NOKEY;
    };
    let Some((o, n)) = owner_of(b, &s) else {
        return E_SET_NOKEY;
    };
    match find_from(b, o + n, s.end, &full[..kn]) {
        None => E_SET_NOKEY,
        Some(e) => {
            if e.value.1 > out.len() {
                return E_SET_VALUE;
            }
            let mut i = 0;
            while i < e.value.1 {
                out[i] = b[e.value.0 + i];
                i += 1;
            }
            e.value.1 as i32
        }
    }
}

/// `dk_settings_put` on the project scope: set `<owner>/<bare>`. `Ok(true)` when the image changed, `Ok(false)` when the key already had this value, `E_SET_KEY`, `E_SET_VALUE`
/// (over 1024 bytes) or `E_SET_FULL` (the image would pass 16 KiB: it is left as it was).
pub fn put(b: &mut [u8], owner: &[u8], bare: &[u8], value: &[u8]) -> Result<bool, i32> {
    let mut full = [0u8; KEY_MAX];
    let kn = module_key(owner, bare, &mut full).ok_or(E_SET_KEY)?;
    let key = &full[..kn];
    if value.len() > VALUE_MAX {
        return Err(E_SET_VALUE);
    }
    let used = get32(b, FMT.o_used()) as usize;
    let entry_len = 1 + kn + 2 + value.len();
    match find_section(b, used, owner) {
        None => {
            let body = 1 + owner.len() + entry_len;
            if used + 3 + body > AREA {
                return Err(E_SET_FULL);
            }
            let mut p = used;
            b[p] = TAG_DATA;
            put16(b, p + 1, body as u32);
            b[p + 3] = owner.len() as u8;
            p += 4;
            let mut i = 0;
            while i < owner.len() {
                b[p + i] = owner[i];
                i += 1;
            }
            p += owner.len();
            write_entry(b, p, key, value);
            put32(b, FMT.o_used(), (used + 3 + body) as u32);
            Ok(true)
        }
        Some(s) => {
            let (o, n) = owner_of(b, &s).ok_or(E_SET_KEY)?;
            match find_from(b, o + n, s.end, key) {
                Some(e) => {
                    let old = e.value.1;
                    if same(&b[e.value.0..e.value.0 + old], value) {
                        return Ok(false);
                    }
                    let new_used = used - old + value.len();
                    if new_used > AREA {
                        return Err(E_SET_FULL);
                    }
                    shift(b, e.end, used, e.value.0 + value.len());
                    put16(b, e.value.0 - 2, value.len() as u32);
                    let mut i = 0;
                    while i < value.len() {
                        b[e.value.0 + i] = value[i];
                        i += 1;
                    }
                    let len = get16(b, s.at + 1) as usize - old + value.len();
                    put16(b, s.at + 1, len as u32);
                    put32(b, FMT.o_used(), new_used as u32);
                    Ok(true)
                }
                None => {
                    if used + entry_len > AREA {
                        return Err(E_SET_FULL);
                    }
                    shift(b, s.end, used, s.end + entry_len);
                    write_entry(b, s.end, key, value);
                    let len = get16(b, s.at + 1) as usize + entry_len;
                    put16(b, s.at + 1, len as u32);
                    put32(b, FMT.o_used(), (used + entry_len) as u32);
                    Ok(true)
                }
            }
        }
    }
}

fn write_entry(b: &mut [u8], at: usize, key: &[u8], value: &[u8]) {
    b[at] = key.len() as u8;
    let mut p = at + 1;
    let mut i = 0;
    while i < key.len() {
        b[p + i] = key[i];
        i += 1;
    }
    p += key.len();
    put16(b, p, value.len() as u32);
    p += 2;
    i = 0;
    while i < value.len() {
        b[p + i] = value[i];
        i += 1;
    }
}

/// `dk_settings_remove` on the project scope: `Ok(true)` when the key was there, `Ok(false)` when not, `E_SET_KEY`. The last entry of an owner takes its section with it.
pub fn remove(b: &mut [u8], owner: &[u8], bare: &[u8]) -> Result<bool, i32> {
    let mut full = [0u8; KEY_MAX];
    let kn = module_key(owner, bare, &mut full).ok_or(E_SET_KEY)?;
    let used = get32(b, FMT.o_used()) as usize;
    let Some(s) = find_section(b, used, owner) else {
        return Ok(false);
    };
    let Some((o, n)) = owner_of(b, &s) else {
        return Ok(false);
    };
    let Some(e) = find_from(b, o + n, s.end, &full[..kn]) else {
        return Ok(false);
    };
    let cut = e.end - e.at;
    let len = get16(b, s.at + 1) as usize - cut;
    if len == 1 + n {
        // the section's only entry: the section goes
        let sec_len = s.end - s.at;
        shift(b, s.end, used, s.at);
        put32(b, FMT.o_used(), (used - sec_len) as u32);
        return Ok(true);
    }
    shift(b, e.end, used, e.at);
    put16(b, s.at + 1, len as u32);
    put32(b, FMT.o_used(), (used - cut) as u32);
    Ok(true)
}

#[cfg(test)]
mod json;
#[cfg(test)]
mod tests;
