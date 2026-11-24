//! The hook installer (docs/plans/runtime-and-services.md, R12-3): writes a `jmp.l` to the entry of a service over each hook site and writes the stock bytes
//! back on restore. The sites come from the port file (`SITES`): address, displaced length and the check value of the stock bytes. Generic over `Platform`,
//! so the host tests run it on `FakePlatform`, whose call log is the fake of the installer.
//!
//! A package takes three kinds of entry here: hook sites (a `jmp.l` over the stock code), patches (plain bytes of 1, 2 or 4 at a data site: a table pointer or a bound, with the value
//! the service gave in `init`) and slot words. All-or-nothing per provider, the hooks and the patches together: the layout is checked and the live stock bytes are read (kept for the
//! restore) and compared with the port file's check value outside the interrupt mask, so the mask is held only for the compare of the live bytes with that copy (a site that changed
//! meanwhile refuses, nothing written), the writes, the flush of the covered lines and the slot words. A refusal writes nothing. The restore writes the saved bytes back, the patches in
//! the reverse of the order they went in, then the hooks.

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

/// One patch a service takes, from the port file's `SITES` item of the patch and the value the service declared in `init`: where the bytes are, how many (1, 2 or 4), the check value
/// of the stock bytes and the value that replaces them (big-endian, the low `len` bytes).
#[derive(Clone, Copy)]
pub struct PatchItem {
    pub addr: u32,
    pub len: u32,
    pub check: [u8; 8],
    pub value: u32,
}

/// The ColdFire items of one record: its hook sites (`hooks`, with the entries in the image at `image_base`), its patches and the slot words it fills (address, value).
pub struct CfItems<'a> {
    pub hooks: &'a [HookItem],
    pub patches: &'a [PatchItem],
    pub image_base: u32,
    pub image_size: u32,
    pub slots: &'a [(u32, u32)],
}

/// Installed sites at most (`[limits.format] install_sites` of sdk/sdk.toml): 48 today, over all providers.
pub const SITES_MAX: usize = dkneeds::INSTALL_SITES as usize;
/// Installed patches at most (`install_patches`), over all providers: the patch log.
pub const PATCHES_MAX: usize = dkneeds::INSTALL_PATCHES as usize;
/// Patches of one service file (`service_patches_max`).
pub const SERVICE_PATCHES_MAX: usize = dkneeds::SERVICE_PATCHES_MAX as usize;

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

/// One installed patch, owned by record `rec`: the log entry that gives the stock bytes back.
#[derive(Clone, Copy)]
pub struct PatchSite {
    pub rec: u8,
    pub addr: u32,
    pub len: u8,
    pub stock: [u8; 4],
}

const EMPTY_PATCH: PatchSite = PatchSite { rec: 0, addr: 0, len: 0, stock: [0; 4] };

/// The slot words the ColdFire backend filled for each record, kept so that `restore` needs only the record.
const SLOT_WORDS_MAX: usize = dkneeds::SLOTS_MAX as usize * dkrt_sdk::PROVIDER_MAX;

pub struct Installer {
    sites: [Site; SITES_MAX],
    n: usize,
    /// The patch log, in the order the patches went in.
    patches: [PatchSite; PATCHES_MAX],
    np: usize,
    slot_words: [(u8, u32); SLOT_WORDS_MAX],
    ns: usize,
    /// True in safe mode: `install` does nothing. (Stored inverted: the zero fill of the static keeps the whole loader out of `.data`.)
    disarmed: bool,
}

impl Installer {
    pub const fn new() -> Installer {
        Installer { sites: [EMPTY_SITE; SITES_MAX], n: 0, patches: [EMPTY_PATCH; PATCHES_MAX], np: 0, slot_words: [(0, 0); SLOT_WORDS_MAX], ns: 0, disarmed: false }
    }

    pub fn set_armed(&mut self, armed: bool) {
        self.disarmed = !armed;
    }

    /// Installed sites of all records.
    pub fn count(&self) -> usize {
        self.n
    }

    /// Installed patches of all records.
    pub fn patch_count(&self) -> usize {
        self.np
    }

    /// The layout of the items, no platform call: capacity, lengths and entries, and that no site overlaps an installed one or another item of the batch. Errors: `E_RANGE` (length, a patch value wider than its length,
    /// entry), `E_PLAN_FIT` (no room in the site table or the patch log), `E_PLAN_OWNER` (an installed site or patch, or an earlier item, overlaps).
    #[inline(never)]
    fn layout(&self, hooks: &[HookItem], patches: &[PatchItem], image_size: u32) -> Result<(), i32> {
        if self.n + hooks.len() > SITES_MAX || self.np + patches.len() > PATCHES_MAX {
            return Err(E_PLAN_FIT);
        }
        let mut i = 0;
        while i < hooks.len() {
            let b = &hooks[i];
            let len = b.len as usize;
            if !(6..=64).contains(&len) || len % 2 != 0 || b.entry >= image_size {
                return Err(E_RANGE);
            }
            if self.taken(b.addr, b.len) {
                return Err(E_PLAN_OWNER);
            }
            let end = b.addr.wrapping_add(b.len);
            let mut j = 0;
            while j < i {
                let o = &hooks[j];
                if b.addr < o.addr + o.len && o.addr < end {
                    return Err(E_PLAN_OWNER);
                }
                j += 1;
            }
            i += 1;
        }
        let mut i = 0;
        while i < patches.len() {
            let b = &patches[i];
            if b.len != 1 && b.len != 2 && b.len != 4 {
                return Err(E_RANGE);
            }
            // the value has to fit the declared length: a longer one would be cut to its low bytes
            if b.len < 4 && b.value >> (8 * b.len) != 0 {
                return Err(E_RANGE);
            }
            if self.taken(b.addr, b.len) {
                return Err(E_PLAN_OWNER);
            }
            let end = b.addr.wrapping_add(b.len);
            let mut j = 0;
            while j < hooks.len() {
                if b.addr < hooks[j].addr + hooks[j].len && hooks[j].addr < end {
                    return Err(E_PLAN_OWNER);
                }
                j += 1;
            }
            let mut j = 0;
            while j < i {
                let o = &patches[j];
                if b.addr < o.addr + o.len && o.addr < end {
                    return Err(E_PLAN_OWNER);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// Whether `len` bytes at `addr` overlap an installed hook site or patch.
    fn taken(&self, addr: u32, len: u32) -> bool {
        let end = addr.wrapping_add(len);
        let mut j = 0;
        while j < self.n {
            let s = &self.sites[j];
            if addr < s.addr + s.len as u32 && s.addr < end {
                return true;
            }
            j += 1;
        }
        let mut j = 0;
        while j < self.np {
            let s = &self.patches[j];
            if addr < s.addr + s.len as u32 && s.addr < end {
                return true;
            }
            j += 1;
        }
        false
    }

    /// Pass 1: check every item, write nothing: the layout, then the check value of the live bytes of each site (`E_HOOK_STOCK`: the live bytes are not the stock bytes). The service
    /// file's own check (`service_check`) runs it before `init`, so a firmware that is not stock refuses the file before anything is allocated.
    #[inline(never)]
    pub unsafe fn check<P: PackagePlatform>(&self, p: &P, hooks: &[HookItem], patches: &[PatchItem], image_size: u32) -> Result<(), i32> {
        self.layout(hooks, patches, image_size)?;
        let mut i = 0;
        while i < hooks.len() {
            if super::port::site_check(p, hooks[i].addr, hooks[i].len as usize) != hooks[i].check {
                return Err(E_HOOK_STOCK);
            }
            i += 1;
        }
        let mut i = 0;
        while i < patches.len() {
            if super::port::site_check(p, patches[i].addr, patches[i].len as usize) != patches[i].check {
                return Err(E_HOOK_STOCK);
            }
            i += 1;
        }
        Ok(())
    }

    /// Install the hooks and patches of record `rec` and fill `slots` (slot address, value). Safe mode (not armed): `Ok` with no platform call. The layout is checked, the live bytes
    /// of every site are read into the saved copies and their check value is compared, all outside the mask (SHA-256 of at most 64 bytes a site: two blocks); a refusal writes nothing and
    /// never masks. Then one mask: the live bytes are compared with the saved copies (a site changed since the read refuses, nothing written), the writes, the flush of the covered lines, the
    /// slot words, the restore of the mask.
    pub unsafe fn install<P: PackagePlatform>(
        &mut self,
        p: &P,
        rec: u8,
        hooks: &[HookItem],
        patches: &[PatchItem],
        image_base: u32,
        image_size: u32,
        slots: &[(u32, u32)],
    ) -> Result<(), i32> {
        if self.disarmed {
            return Ok(());
        }
        self.layout(hooks, patches, image_size)?;
        // the saved copies go to the free end of the tables; they count only once the writes are made. The check value is of the copy, not of a second read of the live bytes.
        let (first, firstp) = (self.n, self.np);
        let mut i = 0;
        while i < hooks.len() {
            let b = &hooks[i];
            let len = b.len as usize;
            let mut stock = [0u8; 64];
            p.read_code(b.addr, &mut stock[..len]);
            if super::port::bytes_check(&stock[..len]) != b.check {
                return Err(E_HOOK_STOCK);
            }
            self.sites[first + i] = Site { rec, addr: b.addr, len: b.len as u8, stock };
            i += 1;
        }
        let mut i = 0;
        while i < patches.len() {
            let b = &patches[i];
            let len = b.len as usize;
            let mut stock = [0u8; 4];
            p.read_code(b.addr, &mut stock[..len]);
            if super::port::bytes_check(&stock[..len]) != b.check {
                return Err(E_HOOK_STOCK);
            }
            self.patches[firstp + i] = PatchSite { rec, addr: b.addr, len: b.len as u8, stock };
            i += 1;
        }
        let sr = p.irq_mask();
        // the compare under the mask: nothing may have changed the sites since the copies were read
        let mut i = 0;
        while i < hooks.len() {
            let s = &self.sites[first + i];
            let mut now = [0u8; 64];
            p.read_code(s.addr, &mut now[..s.len as usize]);
            if now[..s.len as usize] != s.stock[..s.len as usize] {
                p.irq_restore(sr);
                return Err(E_HOOK_STOCK);
            }
            i += 1;
        }
        let mut i = 0;
        while i < patches.len() {
            let s = &self.patches[firstp + i];
            let mut now = [0u8; 4];
            p.read_code(s.addr, &mut now[..s.len as usize]);
            if now[..s.len as usize] != s.stock[..s.len as usize] {
                p.irq_restore(sr);
                return Err(E_HOOK_STOCK);
            }
            i += 1;
        }
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
            p.write_code(b.addr, &bytes[..len]);
            self.n += 1;
            i += 1;
        }
        let mut i = 0;
        while i < patches.len() {
            let b = &patches[i];
            let len = b.len as usize;
            p.write_code(b.addr, &b.value.to_be_bytes()[4 - len..]);
            self.np += 1;
            i += 1;
        }
        let mut i = first;
        while i < self.n {
            flush_site(p, self.sites[i].addr, self.sites[i].len);
            i += 1;
        }
        let mut i = firstp;
        while i < self.np {
            flush_site(p, self.patches[i].addr, self.patches[i].len);
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

    /// Write the stock bytes back for every patch and hook site of `rec`, clear the slot words it filled and drop the sites. One mask. The patches go back in the reverse of the
    /// order they went in (the log is a stack), then the hooks.
    pub unsafe fn restore<P: PackagePlatform>(&mut self, p: &P, rec: u8) {
        // a record that installed nothing (a file that failed before its install) costs no platform call
        if !self.sites[..self.n].iter().any(|s| s.rec == rec) && !self.patches[..self.np].iter().any(|s| s.rec == rec) && !self.slot_words[..self.ns].iter().any(|s| s.0 == rec) {
            return;
        }
        let sr = p.irq_mask();
        let mut i = self.np;
        while i > 0 {
            i -= 1;
            if self.patches[i].rec == rec {
                let s = self.patches[i];
                p.write_code(s.addr, &s.stock[..s.len as usize]);
                flush_site(p, s.addr, s.len);
                // the log keeps its order: the entries behind this one move up
                let mut k = i;
                while k + 1 < self.np {
                    self.patches[k] = self.patches[k + 1];
                    k += 1;
                }
                self.np -= 1;
                self.patches[self.np] = EMPTY_PATCH;
            }
        }
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
        Installer::install(self, p, rec, items.hooks, items.patches, items.image_base, items.image_size, items.slots)
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
    use std::boxed::Box;
    use std::vec::Vec;
    use std::vec;

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
        unsafe { ins.install(&f, 3, &items(&bs), &[], BASE, SIZE, &[(0x2000_0000, 0x4100_0080), (0x2000_0004, 0x4100_0090)]).unwrap() };
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
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[]) }, Err(E_HOOK_STOCK));
        assert!(f.calls.borrow().is_empty(), "the check value is compared outside the mask");
        assert_eq!(code(&f, 0x1000_0000, 8), other, "nothing was written");
        f.put_code(0x1000_0000, &stock);
        unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[]).unwrap() };
        assert_eq!(code(&f, 0x1000_0000, 6), [0x4e, 0xf9, 0x41, 0x00, 0x00, 0x40]);
    }

    #[test]
    fn a_stock_mismatch_writes_nothing() {
        let (sa, sb) = ([0xaa; 6], [0xbb; 6]);
        let bs = [bind(0x1000_0000, 6, &sa), bind(0x1000_0100, 6, &sb)];
        let f = fake(&bs);
        f.put_code(0x1000_0100, &[0; 6]);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[(0x2000_0000, 1)]) }, Err(E_HOOK_STOCK));
        assert!(f.calls.borrow().is_empty(), "refused outside the mask, nothing written");
        assert_eq!(ins.count(), 0);
        assert_eq!(code(&f, 0x1000_0000, 6), sa, "the first site is still stock");
    }

    #[test]
    fn check_makes_no_call() {
        let sa = [0xaa; 6];
        let bs = [bind(0x1000_0000, 6, &sa)];
        let f = fake(&bs);
        let ins = Installer::new();
        assert_eq!(unsafe { ins.check(&f, &items(&bs), &[], SIZE) }, Ok(()));
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn a_second_claimant_of_an_installed_site_is_refused() {
        let (sa, sc) = ([0xaa; 6], [0xcc; 6]);
        let bs = [bind(0x1000_0000, 6, &sa)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[]).unwrap() };
        f.calls.borrow_mut().clear();
        // overlapping range, stock that matches nothing is irrelevant: the owner check comes first
        let again = [bind(0x1000_0004, 6, &sc)];
        f.put_code(0x1000_0004, &sc);
        assert_eq!(unsafe { ins.install(&f, 1, &items(&again), &[], BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        assert!(f.calls.borrow().is_empty());
        assert_eq!(ins.count(), 1);
    }

    #[test]
    fn overlap_inside_one_batch_is_refused() {
        let s = [0xaa; 8];
        let bs = [bind(0x1000_0000, 8, &s), bind(0x1000_0006, 6, &s)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        assert!(!f.calls.borrow().iter().any(|c| matches!(c, Call::Write { .. })));
        assert_eq!(ins.count(), 0);
    }

    #[test]
    fn bad_length_or_entry_is_refused() {
        let s = [0xaa; 66];
        let f = FakePlatform::new(profiles::DN2);
        let ins = Installer::new();
        for len in [4u32, 7, 66] {
            assert_eq!(unsafe { ins.check(&f, &items(&[bind(0x1000_0000, len, &s)]), &[], SIZE) }, Err(E_RANGE), "len {len}");
        }
        let mut it = items(&[bind(0x1000_0000, 6, &s)]);
        it[0].entry = SIZE;
        assert_eq!(unsafe { ins.check(&f, &it, &[], SIZE) }, Err(E_RANGE));
    }

    #[test]
    fn safe_mode_makes_no_call() {
        let sa = [0xaa; 6];
        let bs = [bind(0x1000_0000, 6, &sa)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        ins.set_armed(false);
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[(0x2000_0000, 1)]) }, Ok(()));
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
        unsafe { ins.install(&f, 4, &items(&bs), &[], BASE, SIZE, &[(0x2000_0000, 0x4100_0080)]).unwrap() };
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
            ins.install(&f, 1, &items(&a), &[], BASE, SIZE, &[]).unwrap();
            ins.install(&f, 2, &items(&b), &[], BASE, SIZE, &[]).unwrap();
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
        unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[]).unwrap() };
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
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[], BASE, SIZE, &[]) }, Err(E_PLAN_FIT));
        assert_eq!(ins.count(), 0);
        // exactly the capacity fits, and the next provider then finds no room
        unsafe { ins.install(&f, 0, &items(&bs[..SITES_MAX]), &[], BASE, SIZE, &[]).unwrap() };
        assert_eq!(unsafe { ins.check(&f, &items(&bs[SITES_MAX..]), &[], SIZE) }, Err(E_PLAN_FIT));
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

    // ---- patches

    /// A patch of `len` bytes at `addr` whose stock bytes are the first `len` of `stock`; the check value is theirs.
    fn patch(addr: u32, len: u32, stock: &[u8], value: u32) -> PatchItem {
        PatchItem { addr, len, check: crate::package::loader::port::bytes_check(&stock[..len as usize]), value }
    }

    fn fake_with(patches: &[(u32, &[u8])]) -> FakePlatform {
        let f = fake(&[]);
        for (a, b) in patches {
            f.put_code(*a, b);
        }
        f
    }

    #[test]
    fn a_patch_writes_its_bytes_under_one_mask_and_restore_writes_the_stock_back() {
        let f = fake_with(&[(0x2000_0100, &[1, 2, 3, 4]), (0x2000_0200, &[9, 9]), (0x2000_0300, &[7])]);
        let ps = [patch(0x2000_0100, 4, &[1, 2, 3, 4], 0xdead_beef), patch(0x2000_0200, 2, &[9, 9], 0x1234_5678), patch(0x2000_0300, 1, &[7], 0xaabb_ccdd)];
        let mut ins = Installer::new();
        unsafe { ins.install(&f, 5, &[], &ps, BASE, SIZE, &[]).unwrap() };
        assert_eq!(code(&f, 0x2000_0100, 4), [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(code(&f, 0x2000_0200, 2), [0x56, 0x78], "a halfword takes the low two bytes of the value");
        assert_eq!(code(&f, 0x2000_0300, 1), [0xdd], "a byte takes the low byte");
        assert_eq!(
            *f.calls.borrow(),
            [
                Call::Mask,
                Call::Write { addr: 0x2000_0100, len: 4 },
                Call::Write { addr: 0x2000_0200, len: 2 },
                Call::Write { addr: 0x2000_0300, len: 1 },
                Call::Flush { addr: 0x2000_0100, len: 16 },
                Call::Flush { addr: 0x2000_0200, len: 16 },
                Call::Flush { addr: 0x2000_0300, len: 16 },
                Call::Restore,
            ]
        );
        assert_eq!((ins.patch_count(), ins.count(), f.depth.get()), (3, 0, 0));
        unsafe { ins.restore(&f, 5) };
        assert_eq!(code(&f, 0x2000_0100, 4), [1, 2, 3, 4]);
        assert_eq!(code(&f, 0x2000_0200, 2), [9, 9]);
        assert_eq!(code(&f, 0x2000_0300, 1), [7]);
        assert_eq!((ins.patch_count(), f.depth.get()), (0, 0));
    }

    #[test]
    fn a_patch_whose_stock_differs_is_refused_outside_the_mask_and_writes_nothing() {
        let f = fake_with(&[(0x2000_0100, &[1, 2, 3, 4])]);
        let p = patch(0x2000_0100, 4, &[1, 2, 3, 5], 0xdead_beef);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &[], &[p], BASE, SIZE, &[]) }, Err(E_HOOK_STOCK));
        assert!(f.calls.borrow().is_empty());
        assert_eq!(code(&f, 0x2000_0100, 4), [1, 2, 3, 4]);
        assert_eq!((ins.patch_count(), unsafe { ins.check(&f, &[], &[p], SIZE) }.err()), (0, Some(E_HOOK_STOCK)));
    }

    /// A hook and a patch of one file install together or not at all: a patch that is not stock keeps the hook out, and a hook that is not stock keeps the patch out.
    #[test]
    fn hooks_and_patches_fail_together() {
        let sa = [0xaa; 6];
        let bs = [bind(0x1000_0000, 6, &sa)];
        let f = fake(&bs);
        f.put_code(0x2000_0100, &[1, 2, 3, 4]);
        let mut ins = Installer::new();
        let bad_patch = patch(0x2000_0100, 4, &[0, 0, 0, 0], 0xdead_beef);
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[bad_patch], BASE, SIZE, &[]) }, Err(E_HOOK_STOCK));
        assert_eq!(code(&f, 0x1000_0000, 6), sa, "the hook was not written");
        assert_eq!((ins.count(), ins.patch_count()), (0, 0));
        let good_patch = patch(0x2000_0100, 4, &[1, 2, 3, 4], 0xdead_beef);
        f.put_code(0x1000_0000, &[0; 6]);
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[good_patch], BASE, SIZE, &[]) }, Err(E_HOOK_STOCK));
        assert_eq!(code(&f, 0x2000_0100, 4), [1, 2, 3, 4], "the patch was not written");
        assert_eq!((ins.count(), ins.patch_count()), (0, 0));
        assert!(!f.calls.borrow().iter().any(|c| matches!(c, Call::Write { .. })));
        // both stock: both go in
        f.put_code(0x1000_0000, &sa);
        unsafe { ins.install(&f, 0, &items(&bs), &[good_patch], BASE, SIZE, &[]).unwrap() };
        assert_eq!((ins.count(), ins.patch_count()), (1, 1));
        assert_eq!(code(&f, 0x1000_0000, 2), [0x4e, 0xf9]);
        assert_eq!(code(&f, 0x2000_0100, 4), [0xde, 0xad, 0xbe, 0xef]);
    }

    /// The live bytes are compared with the saved copy under the mask: a site that changed after the copy was read (another actor, an interrupt) refuses with nothing written and the
    /// mask given back.
    #[test]
    fn a_site_that_changed_between_the_read_and_the_mask_is_refused_under_the_mask() {
        let (sa, pa) = ([0xaa; 6], [1u8, 2, 3, 4]);
        for patch_changes in [false, true] {
            let bs = [bind(0x1000_0000, 6, &sa)];
            let f = fake(&bs);
            f.put_code(0x2000_0100, &pa);
            let (at, bytes) = if patch_changes { (0x2000_0102, vec![0x77]) } else { (0x1000_0003, vec![0x77]) };
            *f.race_at_mask.borrow_mut() = Some(Box::new(move |f| f.put_code(at, &bytes)));
            let mut ins = Installer::new();
            let r = unsafe { ins.install(&f, 0, &items(&bs), &[patch(0x2000_0100, 4, &pa, 0xdead_beef)], BASE, SIZE, &[(0x2000_0000, 1)]) };
            assert_eq!(r, Err(E_HOOK_STOCK), "patch_changes {patch_changes}");
            assert_eq!(*f.calls.borrow(), [Call::Mask, Call::Restore]);
            assert_eq!((ins.count(), ins.patch_count(), f.depth.get()), (0, 0, 0));
        }
    }

    #[test]
    fn restore_writes_the_patches_back_in_the_reverse_of_the_install_order() {
        let f = fake_with(&[(0x2000_0100, &[1, 2, 3, 4]), (0x2000_0200, &[5, 6]), (0x2000_0300, &[7])]);
        let ps = [patch(0x2000_0100, 4, &[1, 2, 3, 4], 11), patch(0x2000_0200, 2, &[5, 6], 22), patch(0x2000_0300, 1, &[7], 33)];
        let mut ins = Installer::new();
        unsafe { ins.install(&f, 1, &[], &ps, BASE, SIZE, &[]).unwrap() };
        f.calls.borrow_mut().clear();
        unsafe { ins.restore(&f, 1) };
        let writes: Vec<u32> = f.calls.borrow().iter().filter_map(|c| if let Call::Write { addr, .. } = c { Some(*addr) } else { None }).collect();
        assert_eq!(writes, [0x2000_0300, 0x2000_0200, 0x2000_0100], "the last patch goes back first");
    }

    #[test]
    fn restore_keeps_the_patches_of_other_records_and_their_order() {
        let f = fake_with(&[(0x2000_0100, &[1, 2, 3, 4]), (0x2000_0200, &[5, 6]), (0x2000_0300, &[7])]);
        let mut ins = Installer::new();
        unsafe {
            ins.install(&f, 1, &[], &[patch(0x2000_0100, 4, &[1, 2, 3, 4], 11)], BASE, SIZE, &[]).unwrap();
            ins.install(&f, 2, &[], &[patch(0x2000_0200, 2, &[5, 6], 22)], BASE, SIZE, &[]).unwrap();
            ins.install(&f, 1, &[], &[patch(0x2000_0300, 1, &[7], 33)], BASE, SIZE, &[]).unwrap();
            ins.restore(&f, 1);
        }
        assert_eq!(ins.patch_count(), 1);
        assert_eq!(code(&f, 0x2000_0100, 4), [1, 2, 3, 4]);
        assert_eq!(code(&f, 0x2000_0300, 1), [7]);
        assert_eq!(code(&f, 0x2000_0200, 2), [0, 22], "record 2 keeps its patch");
        unsafe { ins.restore(&f, 2) };
        assert_eq!((ins.patch_count(), code(&f, 0x2000_0200, 2)), (0, vec![5, 6]));
    }

    #[test]
    fn a_full_patch_log_refuses_and_writes_nothing() {
        let stock = [0u8; 4];
        let ps: Vec<PatchItem> = (0..PATCHES_MAX as u32 + 1).map(|i| patch(0x2000_0000 + i * 16, 4, &stock, i)).collect();
        let f = fake(&[]);
        let mut ins = Installer::new();
        assert_eq!(unsafe { ins.install(&f, 0, &[], &ps, BASE, SIZE, &[]) }, Err(E_PLAN_FIT));
        assert!(f.calls.borrow().is_empty());
        // exactly the capacity fits; the next record finds no room, hooks and slots of it included
        unsafe { ins.install(&f, 0, &[], &ps[..PATCHES_MAX], BASE, SIZE, &[]).unwrap() };
        let sa = [0xaa; 6];
        let bs = [bind(0x1000_0000, 6, &sa)];
        f.put_code(0x1000_0000, &sa);
        f.calls.borrow_mut().clear();
        assert_eq!(unsafe { ins.install(&f, 1, &items(&bs), &ps[PATCHES_MAX..], BASE, SIZE, &[]) }, Err(E_PLAN_FIT));
        assert!(f.calls.borrow().is_empty());
        assert_eq!((ins.count(), ins.patch_count()), (0, PATCHES_MAX));
    }

    #[test]
    fn a_patch_that_overlaps_a_hook_a_patch_or_an_installed_site_is_refused_and_a_bad_length_is_out_of_range() {
        let (sa, stock) = ([0xaa; 8], [0u8; 4]);
        let bs = [bind(0x1000_0000, 8, &sa)];
        let f = fake(&bs);
        let mut ins = Installer::new();
        // a patch inside a hook of the batch
        assert_eq!(unsafe { ins.install(&f, 0, &items(&bs), &[patch(0x1000_0004, 4, &stock, 1)], BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        // two patches of the batch
        let two = [patch(0x2000_0000, 4, &stock, 1), patch(0x2000_0002, 2, &stock, 2)];
        assert_eq!(unsafe { ins.install(&f, 0, &[], &two, BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        // a patch inside a site installed by another record, and a hook over an installed patch
        unsafe { ins.install(&f, 1, &items(&bs), &[patch(0x2000_0000, 4, &stock, 1)], BASE, SIZE, &[]).unwrap() };
        assert_eq!(unsafe { ins.install(&f, 2, &[], &[patch(0x1000_0006, 2, &stock, 1)], BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        assert_eq!(unsafe { ins.install(&f, 2, &[], &[patch(0x2000_0003, 1, &stock, 1)], BASE, SIZE, &[]) }, Err(E_PLAN_OWNER));
        for len in [0u32, 3, 8] {
            assert_eq!(unsafe { ins.check(&f, &[], &[patch(0x3000_0000, len, &[0; 8], 1)], SIZE) }, Err(E_RANGE), "len {len}");
        }
    }

    #[test]
    fn a_patch_value_that_does_not_fit_its_length_is_out_of_range_and_nothing_is_written() {
        let stock = [1u8, 2, 3, 4];
        let f = fake_with(&[(0x2000_0100, &stock)]);
        let mut ins = Installer::new();
        for (len, value) in [(1u32, 0x120u32), (2, 0x1_0000), (1, 0xffff_ffff)] {
            assert_eq!(unsafe { ins.install(&f, 0, &[], &[patch(0x2000_0100, len, &stock, value)], BASE, SIZE, &[]) }, Err(E_RANGE), "len {len} value {value:#x}");
        }
        assert!(f.calls.borrow().is_empty(), "no platform call before the refusal");
        assert_eq!((ins.patch_count(), code(&f, 0x2000_0100, 4)), (0, stock.to_vec()));
        // the largest value of each length is accepted
        for (len, value) in [(1u32, 0xffu32), (2, 0xffff), (4, 0xffff_ffff)] {
            assert_eq!(unsafe { ins.install(&f, 1, &[], &[patch(0x2000_0100, len, &stock, value)], BASE, SIZE, &[]) }, Ok(()), "len {len}");
            unsafe { ins.restore(&f, 1) };
        }
    }

    #[test]
    fn safe_mode_installs_no_patch() {
        let f = fake_with(&[(0x2000_0100, &[1, 2, 3, 4])]);
        let mut ins = Installer::new();
        ins.set_armed(false);
        assert_eq!(unsafe { ins.install(&f, 0, &[], &[patch(0x2000_0100, 4, &[1, 2, 3, 4], 9)], BASE, SIZE, &[]) }, Ok(()));
        assert!(f.calls.borrow().is_empty());
        assert_eq!((ins.patch_count(), code(&f, 0x2000_0100, 4)), (0, vec![1, 2, 3, 4]));
    }

    #[test]
    fn restore_of_a_record_that_installed_nothing_costs_no_platform_call() {
        let f = FakePlatform::new(profiles::DN2);
        let mut ins = Installer::new();
        unsafe { ins.restore(&f, 3) };
        assert!(f.calls.borrow().is_empty());
    }
}
