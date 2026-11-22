//! The hook installer (docs/plans/runtime-and-services.md, R12-3): writes a `jmp.l` to the entry of a service over each hook site and writes the stock bytes
//! back on restore. The sites come from the port file (`SITES`): address, displaced length and the check value of the stock bytes. Generic over `Platform`,
//! so the host tests run it on `FakePlatform`, whose call log is the fake of the installer.
//!
//! All-or-nothing per provider: pass 1 checks every site and writes nothing; pass 2 reads the live stock bytes (kept for the restore), writes under the same
//! interrupt mask, flushes the covered lines and fills the slot words.

use dkrt_abi::link::Sequence;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_sdk::*;

/// What one installer backend installs for one record and takes down again. The ColdFire backend ([`Installer`]) writes `jmp.l` over hook sites and fills slot words in
/// the image; the DSP backend ([`DspInstaller`]) claims words of core/dsp-link's slot block. Both belong to the one lifecycle of the Loader: a file that fails after its
/// install, a module whose DSP part fails and an unload (hot reload) all end in `restore`.
pub trait Backend {
    /// The items of one install.
    type Items<'a>;
    /// Install `items` for record `rec`, all or nothing.
    unsafe fn install<P: PackagePlatform>(&mut self, p: &P, rec: u8, items: &Self::Items<'_>) -> Result<(), i32>;
    /// Take everything of `rec` down again (hook sites back to stock, slots cleared or released). Nothing of `rec` is left behind.
    unsafe fn restore<P: PackagePlatform>(&mut self, p: &P, rec: u8);
    /// Items installed for all records.
    fn count(&self) -> usize;
}

/// One hook site a service takes, from the port file's `SITES` item of the hook and the service's `HOOKS` item: where the site is, how many stock bytes it
/// displaces, the check value of those bytes (the first 8 bytes of their SHA-256) and the image offset of the service's entry that the site jumps to.
#[derive(Clone, Copy)]
pub struct HookItem {
    pub addr: u32,
    pub len: u32,
    pub check: [u8; 8],
    pub entry: u32,
}

/// The ColdFire items of one record: its hook sites (`hooks`, with the entries in the image at `image_base`) and the slot words it fills (address, value).
pub struct CfItems<'a> {
    pub hooks: &'a [HookItem],
    pub image_base: u32,
    pub image_size: u32,
    pub slots: &'a [(u32, u32)],
}

/// Installed sites at most (`[limits.format] install_sites` of sdk/sdk.toml): 48 today, over all providers.
pub const SITES_MAX: usize = dkneeds::INSTALL_SITES as usize;

/// `jmp.l` and `nop`, as big-endian words.
const JMP_L: u16 = 0x4ef9;
const NOP: u16 = 0x4e71;

/// One installed site, owned by record `rec`.
#[derive(Clone, Copy)]
pub struct Site {
    pub rec: u8,
    pub addr: u32,
    pub len: u8,
    pub stock: [u8; 64],
}

const EMPTY_SITE: Site = Site { rec: 0, addr: 0, len: 0, stock: [0; 64] };

/// The slot words the ColdFire backend filled for each record, kept so that `restore` needs only the record.
const SLOT_WORDS_MAX: usize = dkneeds::SLOTS_MAX as usize * dkrt_sdk::PROVIDER_MAX;

pub struct Installer {
    sites: [Site; SITES_MAX],
    n: usize,
    slot_words: [(u8, u32); SLOT_WORDS_MAX],
    ns: usize,
    /// True in safe mode: `install` does nothing. (Stored inverted: the zero fill of the static keeps the whole loader out of `.data`.)
    disarmed: bool,
}

impl Installer {
    pub const fn new() -> Installer {
        Installer { sites: [EMPTY_SITE; SITES_MAX], n: 0, slot_words: [(0, 0); SLOT_WORDS_MAX], ns: 0, disarmed: false }
    }

    pub fn set_armed(&mut self, armed: bool) {
        self.disarmed = !armed;
    }

    /// Installed sites of all records.
    pub fn count(&self) -> usize {
        self.n
    }

    /// Pass 1: check every item, write nothing. Errors: `E_RANGE` (length, entry), `E_PLAN_FIT` (no room for the sites), `E_PLAN_OWNER` (an installed site or an
    /// earlier item overlaps), `E_HOOK_STOCK` (the live bytes are not the stock bytes: their check value differs from the item's).
    #[inline(never)]
    pub unsafe fn check<P: PackagePlatform>(&self, p: &P, hooks: &[HookItem], image_size: u32) -> Result<(), i32> {
        if self.n + hooks.len() > SITES_MAX {
            return Err(E_PLAN_FIT);
        }
        let mut i = 0;
        while i < hooks.len() {
            let b = &hooks[i];
            let len = b.len as usize;
            if !(6..=64).contains(&len) || len % 2 != 0 || b.entry >= image_size {
                return Err(E_RANGE);
            }
            let end = b.addr.wrapping_add(b.len);
            let mut j = 0;
            while j < self.n {
                let s = &self.sites[j];
                if b.addr < s.addr + s.len as u32 && s.addr < end {
                    return Err(E_PLAN_OWNER);
                }
                j += 1;
            }
            let mut j = 0;
            while j < i {
                let o = &hooks[j];
                if b.addr < o.addr + o.len && o.addr < end {
                    return Err(E_PLAN_OWNER);
                }
                j += 1;
            }
            if super::port::site_check(p, b.addr, len) != b.check {
                return Err(E_HOOK_STOCK);
            }
            i += 1;
        }
        Ok(())
    }

    /// Install the hooks of record `rec` and fill `slots` (slot address, value). Safe mode (not armed): `Ok` with no platform call. One mask:
    /// pass 1 again, then the stock bytes are read, then the writes, then the flush of the covered lines, then the slot words, then the restore. A refusal in pass 1
    /// writes nothing.
    pub unsafe fn install<P: PackagePlatform>(
        &mut self,
        p: &P,
        rec: u8,
        hooks: &[HookItem],
        image_base: u32,
        image_size: u32,
        slots: &[(u32, u32)],
    ) -> Result<(), i32> {
        if self.disarmed {
            return Ok(());
        }
        // The check hashes every site (SHA-256 of at most 64 bytes: two blocks) with interrupts masked, once per provider and only at boot: a service is installed by
        // the boot's provider stage, and a run-time load (E5) takes content modules only, never a service, so no site is hashed while the device plays. At most `SITES_MAX`
        // sites are hashed in all (18 and 22 today). The mask time is not measured: a host run says nothing about the ColdFire, and the hash code is the one of the file check.
        let sr = p.irq_mask();
        if let Err(e) = self.check(p, hooks, image_size) {
            p.irq_restore(sr);
            return Err(e);
        }
        let first = self.n;
        let mut i = 0;
        while i < hooks.len() {
            let b = &hooks[i];
            let len = b.len as usize;
            let mut bytes = [0u8; 64];
            let mut k = 0;
            while k < 64 {
                bytes[k..k + 2].copy_from_slice(&NOP.to_be_bytes());
                k += 2;
            }
            bytes[0..2].copy_from_slice(&JMP_L.to_be_bytes());
            bytes[2..6].copy_from_slice(&image_base.wrapping_add(b.entry).to_be_bytes());
            // the stock bytes are the live ones (the check just compared them), kept for the restore
            let mut stock = [0u8; 64];
            p.read_code(b.addr, &mut stock[..len]);
            p.write_code(b.addr, &bytes[..len]);
            self.sites[self.n] = Site { rec, addr: b.addr, len: b.len as u8, stock };
            self.n += 1;
            i += 1;
        }
        let mut i = first;
        while i < self.n {
            flush_site(p, self.sites[i].addr, self.sites[i].len);
            i += 1;
        }
        let mut i = 0;
        while i < slots.len() {
            p.write_word(slots[i].0, slots[i].1);
            p.flush(slots[i].0 & !15, 16);
            if self.ns < SLOT_WORDS_MAX {
                self.slot_words[self.ns] = (rec, slots[i].0);
                self.ns += 1;
            }
            i += 1;
        }
        p.irq_restore(sr);
        Ok(())
    }

    /// Write the stock bytes back for every site of `rec`, clear the slot words it filled and drop the sites. One mask.
    pub unsafe fn restore<P: PackagePlatform>(&mut self, p: &P, rec: u8) {
        // a record that installed nothing (a file that failed before its install) costs no platform call
        if !self.sites[..self.n].iter().any(|s| s.rec == rec) && !self.slot_words[..self.ns].iter().any(|s| s.0 == rec) {
            return;
        }
        let sr = p.irq_mask();
        let mut i = 0;
        while i < self.n {
            if self.sites[i].rec == rec {
                let s = self.sites[i];
                p.write_code(s.addr, &s.stock[..s.len as usize]);
                flush_site(p, s.addr, s.len);
                self.n -= 1;
                self.sites[i] = self.sites[self.n];
                self.sites[self.n] = EMPTY_SITE;
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.ns {
            if self.slot_words[i].0 == rec {
                let at = self.slot_words[i].1;
                p.write_word(at, 0);
                p.flush(at & !15, 16);
                self.ns -= 1;
                self.slot_words[i] = self.slot_words[self.ns];
                self.slot_words[self.ns] = (0, 0);
            } else {
                i += 1;
            }
        }
        p.irq_restore(sr);
    }
}

impl Backend for Installer {
    type Items<'a> = CfItems<'a>;
    unsafe fn install<P: PackagePlatform>(&mut self, p: &P, rec: u8, items: &CfItems<'_>) -> Result<(), i32> {
        Installer::install(self, p, rec, items.hooks, items.image_base, items.image_size, items.slots)
    }
    unsafe fn restore<P: PackagePlatform>(&mut self, p: &P, rec: u8) {
        Installer::restore(self, p, rec)
    }
    fn count(&self) -> usize {
        self.n
    }
}

/// The words of core/dsp-link's slot block (window 0 words 0 to 15; `[dsp.slots]` of sdk/sdk.toml): a hook stub of the image reads one, zero takes the stock path.
pub const DSP_SLOT_WORDS: u32 = dkneeds::DSP_SLOT_WORDS;
/// Bytes of the slot block: it lies directly below the area in the byte addresses of window 0 (`dl_send` takes them).
const DSP_SLOT_BYTES: u32 = 4 * DSP_SLOT_WORDS;

/// The slots one record claimed: (slot index, value) and whether a write of them was sent (so that teardown must clear them).
#[derive(Clone, Copy)]
struct DspClaim {
    rec: u8,
    used: bool,
    n: usize,
    slots: [(u32, u32); dkneeds::SLOTS_MAX as usize],
    sent: bool,
}

const NO_CLAIM: DspClaim = DspClaim { rec: 0, used: false, n: 0, slots: [(0, 0); dkneeds::SLOTS_MAX as usize], sent: false };

/// The DSP items of one record: the slots it fills, (index in the slot block, the pc of its dispatch code).
pub struct DspItems<'a> {
    pub slots: &'a [(u32, u32)],
}

/// The DSP backend: the slot writes of a service file whose DSP code is in the area. `install` claims the slots (a slot has one owner); the write itself is one
/// `Sequence` through window 0 that the DSP loader sends after the code is acknowledged (`sequence`), and the clear is the same sequence of zeros (`clear_sequence`).
/// `restore` drops the claim. The slot words are DSP memory: the stub reads them once per call, so a slot is zero before the code behind it is freed.
pub struct DspInstaller {
    claims: [DspClaim; dkrt_sdk::PROVIDER_MAX],
}

impl DspInstaller {
    pub const fn new() -> DspInstaller {
        DspInstaller { claims: [NO_CLAIM; dkrt_sdk::PROVIDER_MAX] }
    }

    fn of(&self, rec: u8) -> Option<&DspClaim> {
        self.claims.iter().find(|c| c.used && c.rec == rec)
    }

    /// Whether `rec` holds a claim.
    pub fn claimed(&self, rec: u8) -> bool {
        self.of(rec).is_some()
    }

    /// The write of the slots of `rec`: one sequence of window 0 steps (the byte address of each slot word in the area's addressing: the block lies below
    /// `area_base`), or `None` when `rec` claims no slot. Marks the claim as sent: the slots may be live from here on.
    pub fn sequence(&mut self, rec: u8, area_base: u32) -> Option<Sequence> {
        let c = self.claims.iter_mut().find(|c| c.used && c.rec == rec)?;
        let mut s = Sequence::new();
        let mut i = 0;
        while i < c.n {
            s.push(0, area_base - DSP_SLOT_BYTES + 4 * c.slots[i].0, c.slots[i].1);
            i += 1;
        }
        c.sent = true;
        (s.n != 0).then_some(s)
    }

    /// The sequence of zeros that clears the slots of `rec`, or `None` when no write of them was ever sent.
    pub fn clear_sequence(&self, rec: u8, area_base: u32) -> Option<Sequence> {
        let c = self.of(rec)?;
        if !c.sent {
            return None;
        }
        let mut s = Sequence::new();
        let mut i = 0;
        while i < c.n {
            s.push(0, area_base - DSP_SLOT_BYTES + 4 * c.slots[i].0, 0);
            i += 1;
        }
        (s.n != 0).then_some(s)
    }
}

impl Backend for DspInstaller {
    type Items<'a> = DspItems<'a>;
    unsafe fn install<P: PackagePlatform>(&mut self, _p: &P, rec: u8, items: &DspItems<'_>) -> Result<(), i32> {
        if items.slots.is_empty() {
            return Ok(());
        }
        if items.slots.len() > dkneeds::SLOTS_MAX as usize {
            return Err(E_FORMAT);
        }
        let mut i = 0;
        while i < items.slots.len() {
            let (idx, _) = items.slots[i];
            if idx >= DSP_SLOT_WORDS {
                return Err(E_RANGE);
            }
            // a slot has one owner, also within one item list
            if self.claims.iter().any(|c| c.used && c.slots[..c.n].iter().any(|s| s.0 == idx)) || items.slots[..i].iter().any(|s| s.0 == idx) {
                return Err(E_PLAN_OWNER);
            }
            i += 1;
        }
        let at = match self.claims.iter().position(|c| !c.used) {
            Some(a) => a,
            None => return Err(E_PLAN_FIT),
        };
        let mut c = NO_CLAIM;
        c.rec = rec;
        c.used = true;
        c.n = items.slots.len();
        c.slots[..c.n].copy_from_slice(items.slots);
        self.claims[at] = c;
        Ok(())
    }
    unsafe fn restore<P: PackagePlatform>(&mut self, _p: &P, rec: u8) {
        if let Some(c) = self.claims.iter_mut().find(|c| c.used && c.rec == rec) {
            *c = NO_CLAIM;
        }
    }
    fn count(&self) -> usize {
        self.claims.iter().filter(|c| c.used).map(|c| c.n).sum()
    }
}

/// Flush the 16-byte lines that cover `len` bytes at `addr`.
fn flush_site<P: PackagePlatform>(p: &P, addr: u32, len: u8) {
    let start = addr & !15;
    let end = (addr + len as u32 + 15) & !15;
    p.flush(start, end - start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::{Call, FakePlatform};
    use dkrt_abi::profiles;
    use std::vec::Vec;

    const BASE: u32 = 0x4100_0000;
    const SIZE: u32 = 0x1000;

    /// A hook site of `len` stock bytes (the first `len` of `stock`) at `addr`, with the entry 0x40 in the image.
    #[derive(Clone, Copy)]
    struct Bd<'a> {
        addr: u32,
        len: u32,
        stock: &'a [u8],
    }
    fn bind(addr: u32, len: u32, stock: &[u8]) -> Bd<'_> {
        Bd { addr, len, stock }
    }
    /// The installer's items of `bs`: the check value is the one of the stock bytes.
    fn items(bs: &[Bd]) -> Vec<HookItem> {
        bs.iter()
            .map(|b| {
                let n = (b.len as usize).min(b.stock.len());
                let mut check = [0u8; 8];
                if n != 0 {
                    let mut sha = dkrt_abi::sha256::Sha::new();
                    let mut out = [0u8; 32];
                    unsafe {
                        sha.update(b.stock.as_ptr(), n as u32);
                        sha.finish(out.as_mut_ptr());
                    }
                    check.copy_from_slice(&out[..8]);
                }
                HookItem { addr: b.addr, len: b.len, check, entry: 0x40 }
            })
            .collect()
    }
    fn fake(binds: &[Bd]) -> FakePlatform {
        let f = FakePlatform::new(profiles::DN2);
        for b in binds {
            f.put_code(b.addr, &b.stock[..b.len as usize]);
        }
        f
    }
    fn code(f: &FakePlatform, addr: u32, n: usize) -> Vec<u8> {
        let mut out = std::vec![0u8; n];
        unsafe { f.read_code(addr, &mut out) };
        out
    }

    #[test]
    fn one_provider_is_mask_writes_flush_slots_restore() {
        let (sa, sb) = ([0xaa; 6], [0xbb; 8]);
        let bs = [bind(0x1000_0000, 6, &sa), bind(0x1000_0100, 8, &sb)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        unsafe { ins.install(&f, 3, &items(&bs), BASE, SIZE, &[(0x2000_0000, 0x4100_0080), (0x2000_0004, 0x4100_0090)]).unwrap() };
        assert_eq!(
            *f.calls.borrow(),
            [
                Call::Mask,
                Call::Write { addr: 0x1000_0000, len: 6 },
                Call::Write { addr: 0x1000_0100, len: 8 },
                Call::Flush { addr: 0x1000_0000, len: 16 },
                Call::Flush { addr: 0x1000_0100, len: 16 },
                Call::Word { addr: 0x2000_0000, value: 0x4100_0080 },
                Call::Flush { addr: 0x2000_0000, len: 16 },
                Call::Word { addr: 0x2000_0004, value: 0x4100_0090 },
                Call::Flush { addr: 0x2000_0000, len: 16 },
                Call::Restore,
            ]
        );
        assert_eq!(code(&f, 0x1000_0000, 6), [0x4e, 0xf9, 0x41, 0x00, 0x00, 0x40]);
        assert_eq!(code(&f, 0x1000_0100, 8), [0x4e, 0xf9, 0x41, 0x00, 0x00, 0x40, 0x4e, 0x71]);
        assert_eq!((ins.count(), f.depth.get()), (2, 0));
    }

    /// A hook whose site carries the port file's check value of the stock bytes: a firmware that differs from those bytes is refused before any write, and the stock
    /// one installs `jmp entry`.
    #[test]
    fn a_hook_is_refused_on_other_stock_and_installed_on_its_own() {
        let stock = [0x70, 0x06, 0xb2, 0x80, 0x67, 0x00, 0x00, 0x20];
        let bs = [Bd { addr: 0x1000_0000, len: 8, stock: &stock }];
        let f = fake(&bs);
        let mut other = stock;
        other[1] = 0x07;
        f.put_code(0x1000_0000, &other);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[]) }, Err(E_HOOK_STOCK));
        assert_eq!(*f.calls.borrow(), [Call::Mask, Call::Restore]);
        assert_eq!(code(&f, 0x1000_0000, 8), other, "nothing was written");
        f.put_code(0x1000_0000, &stock);
        unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[]).unwrap() };
        assert_eq!(code(&f, 0x1000_0000, 6), [0x4e, 0xf9, 0x41, 0x00, 0x00, 0x40]);
    }

    #[test]
    fn a_stock_mismatch_writes_nothing() {
        let (sa, sb) = ([0xaa; 6], [0xbb; 6]);
        let bs = [bind(0x1000_0000, 6, &sa), bind(0x1000_0100, 6, &sb)];
        let f = fake(&bs);
        f.put_code(0x1000_0100, &[0; 6]);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[(0x2000_0000, 1)]) }, Err(E_HOOK_STOCK));
        assert_eq!(*f.calls.borrow(), [Call::Mask, Call::Restore]);
        assert_eq!(ins.count(), 0);
        assert_eq!(code(&f, 0x1000_0000, 6), sa, "the first site is still stock");
    }

    #[test]
    fn check_makes_no_call() {
        let sa = [0xaa; 6];
        let bs = [bind(0x1000_0000, 6, &sa)];
        let f = fake(&bs);
        let ins = Installer::new();
        assert_eq!(unsafe { ins.check(&f, &items(&bs), SIZE) }, Ok(()));
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn a_second_claimant_of_an_installed_site_is_refused() {
        let (sa, sc) = ([0xaa; 6], [0xcc; 6]);
        let bs = [bind(0x1000_0000, 6, &sa)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[]).unwrap() };
        f.calls.borrow_mut().clear();
        // overlapping range, stock that matches nothing is irrelevant: the owner check comes first
        let again = [bind(0x1000_0004, 6, &sc)];
        f.put_code(0x1000_0004, &sc);
        assert_eq!(unsafe { ins.install(&f, 1, &items(&again), BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        assert_eq!(*f.calls.borrow(), [Call::Mask, Call::Restore]);
        assert_eq!(ins.count(), 1);
    }

    #[test]
    fn overlap_inside_one_batch_is_refused() {
        let s = [0xaa; 8];
        let bs = [bind(0x1000_0000, 8, &s), bind(0x1000_0006, 6, &s)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        assert!(!f.calls.borrow().iter().any(|c| matches!(c, Call::Write { .. })));
        assert_eq!(ins.count(), 0);
    }

    #[test]
    fn bad_length_or_entry_is_refused() {
        let s = [0xaa; 66];
        let f = FakePlatform::new(profiles::DN2);
        let ins = Installer::new();
        for len in [4u32, 7, 66] {
            assert_eq!(unsafe { ins.check(&f, &items(&[bind(0x1000_0000, len, &s)]), SIZE) }, Err(E_RANGE), "len {len}");
        }
        let mut it = items(&[bind(0x1000_0000, 6, &s)]);
        it[0].entry = SIZE;
        assert_eq!(unsafe { ins.check(&f, &it, SIZE) }, Err(E_RANGE));
    }

    #[test]
    fn safe_mode_makes_no_call() {
        let sa = [0xaa; 6];
        let bs = [bind(0x1000_0000, 6, &sa)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        ins.set_armed(false);
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[(0x2000_0000, 1)]) }, Ok(()));
        assert!(f.calls.borrow().is_empty());
        assert_eq!(ins.count(), 0);
        assert_eq!(code(&f, 0x1000_0000, 6), sa);
    }

    #[test]
    fn restore_writes_the_stock_back_and_clears_the_slot() {
        let (sa, sb) = ([0xaa; 6], [0xbb; 6]);
        let bs = [bind(0x1000_0000, 6, &sa), bind(0x1000_0100, 6, &sb)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        unsafe { ins.install(&f, 4, &items(&bs), BASE, SIZE, &[(0x2000_0000, 0x4100_0080)]).unwrap() };
        f.calls.borrow_mut().clear();
        unsafe { ins.restore(&f, 4) };
        assert_eq!(code(&f, 0x1000_0000, 6), sa);
        assert_eq!(code(&f, 0x1000_0100, 6), sb);
        assert_eq!(code(&f, 0x2000_0000, 4), [0, 0, 0, 0]);
        let c = f.calls.borrow();
        assert_eq!((c[0], *c.last().unwrap()), (Call::Mask, Call::Restore));
        let w = c.iter().position(|x| *x == Call::Word { addr: 0x2000_0000, value: 0 }).unwrap();
        assert_eq!(c[w + 1], Call::Flush { addr: 0x2000_0000, len: 16 });
        assert_eq!((ins.count(), f.depth.get()), (0, 0));
    }

    #[test]
    fn restore_keeps_the_sites_of_other_records() {
        let (sa, sb) = ([0xaa; 6], [0xbb; 6]);
        let a = [bind(0x1000_0000, 6, &sa)];
        let b = [bind(0x1000_0100, 6, &sb)];
        let f = fake(&a);
        f.put_code(0x1000_0100, &sb);
        let mut ins = Installer::new();
        unsafe {
            ins.install(&f, 1, &items(&a), BASE, SIZE, &[]).unwrap();
            ins.install(&f, 2, &items(&b), BASE, SIZE, &[]).unwrap();
            ins.restore(&f, 1);
        }
        assert_eq!(ins.count(), 1);
        assert_eq!(code(&f, 0x1000_0000, 6), sa);
        assert_eq!(code(&f, 0x1000_0100, 2), [0x4e, 0xf9]);
    }

    #[test]
    fn the_flush_covers_a_site_that_straddles_a_line() {
        let (sa, sb) = ([0xaa; 6], [0xbb; 6]);
        let bs = [bind(0x1000_000c, 6, &sa), bind(0x1000_0100, 6, &sb)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[]).unwrap() };
        let c = f.calls.borrow();
        assert!(c.contains(&Call::Flush { addr: 0x1000_0000, len: 32 }));
        assert!(c.contains(&Call::Flush { addr: 0x1000_0100, len: 16 }));
    }

    #[test]
    fn more_sites_than_the_table_holds_are_refused() {
        let s = [0xaa; 6];
        let bs: Vec<Bd> = (0..SITES_MAX as u32 + 1).map(|i| bind(0x1000_0000 + i * 16, 6, &s)).collect();
        let f = fake(&bs);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), BASE, SIZE, &[]) }, Err(E_PLAN_FIT));
        assert_eq!(ins.count(), 0);
        // exactly the capacity fits, and the next provider then finds no room
        unsafe { ins.install(&f, 0, &items(&bs[..SITES_MAX]), BASE, SIZE, &[]).unwrap() };
        assert_eq!(unsafe { ins.check(&f, &items(&bs[SITES_MAX..]), SIZE) }, Err(E_PLAN_FIT));
    }

    #[test]
    fn the_dsp_backend_claims_slots_once_writes_one_sequence_and_clears_only_what_was_sent() {
        let f = FakePlatform::new(profiles::DT2);
        let mut d = DspInstaller::new();
        let area = 0x2000_0000;
        unsafe { d.install(&f, 7, &DspItems { slots: &[(0, 0xb80100), (1, 0xb80140)] }).unwrap() };
        // a slot has one owner, in another record and twice in one list; an index outside the block is out of range
        assert_eq!(unsafe { d.install(&f, 8, &DspItems { slots: &[(1, 5)] }) }, Err(E_PLAN_OWNER));
        assert_eq!(unsafe { d.install(&f, 8, &DspItems { slots: &[(2, 5), (2, 6)] }) }, Err(E_PLAN_OWNER));
        assert_eq!(unsafe { d.install(&f, 8, &DspItems { slots: &[(DSP_SLOT_WORDS, 5)] }) }, Err(E_RANGE));
        assert_eq!((d.count(), d.claimed(8)), (2, false), "a refused install claims nothing");
        // nothing was sent: nothing to clear
        assert!(d.clear_sequence(7, area).is_none());
        // one sequence of window 0 steps; the slot block lies directly below the area
        let s = d.sequence(7, area).unwrap();
        assert_eq!(s.n, 2);
        assert_eq!((s.steps[0].win, s.steps[0].at, s.steps[0].value), (0, area - 64, 0xb80100));
        assert_eq!((s.steps[1].win, s.steps[1].at, s.steps[1].value), (0, area - 64 + 4, 0xb80140));
        // sent: the clear is the same words with zeros
        let z = d.clear_sequence(7, area).unwrap();
        assert_eq!((z.n, z.steps[0].at, z.steps[0].value, z.steps[1].value), (2, area - 64, 0, 0));
        unsafe { d.restore(&f, 7) };
        assert_eq!((d.count(), d.claimed(7)), (0, false));
        assert!(d.sequence(7, area).is_none());
        // the platform saw nothing: the DSP backend writes through the link, not through the ColdFire
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn restore_of_a_record_that_installed_nothing_costs_no_platform_call() {
        let f = FakePlatform::new(profiles::DN2);
        let mut ins = Installer::new();
        unsafe { ins.restore(&f, 3) };
        assert!(f.calls.borrow().is_empty());
    }
}
