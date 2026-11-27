/// The CPU an entry's site belongs to. Port data gives it; the installers route by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cpu {
    ColdFire,
    Sharc,
}

/// A site in firmware code: the CPU it runs on and its address (or the SHARC pc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiteRef {
    cpu: Cpu,
    addr: u32,
}

impl SiteRef {
    pub const fn new(cpu: Cpu, addr: u32) -> SiteRef {
        SiteRef { cpu, addr }
    }

    pub const fn cpu(&self) -> Cpu {
        self.cpu
    }

    pub const fn addr(&self) -> u32 {
        self.addr
    }
}

/// A pointer word a flashed stub reads: the CPU that reads it and the slot number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRef {
    cpu: Cpu,
    slot: u32,
}

impl SlotRef {
    pub const fn new(cpu: Cpu, slot: u32) -> SlotRef {
        SlotRef { cpu, slot }
    }

    pub const fn cpu(&self) -> Cpu {
        self.cpu
    }

    pub const fn slot(&self) -> u32 {
        self.slot
    }
}

/// Most bytes of one patch.
pub const MAX_PATCH: usize = 16;

/// The bytes a patch writes: at most [`MAX_PATCH`], held inline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchBytes {
    len: u8,
    bytes: [u8; MAX_PATCH],
}

impl PatchBytes {
    /// `None` when `bytes` is longer than [`MAX_PATCH`].
    pub fn new(bytes: &[u8]) -> Option<PatchBytes> {
        if bytes.len() > MAX_PATCH {
            return None;
        }
        let mut b = [0; MAX_PATCH];
        b[..bytes.len()].copy_from_slice(bytes);
        Some(PatchBytes {
            len: bytes.len() as u8,
            bytes: b,
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// How stock code reaches our code: a hook (a code patch at a site that jumps to `target`), a patch (plain bytes, for table pointers and bounds) or a
/// slot (a pointer word set to `target`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Hook { site: SiteRef, target: u32 },
    Patch { site: SiteRef, bytes: PatchBytes },
    Slot { slot: SlotRef, target: u32 },
}

impl Entry {
    /// The CPU whose installer handles this entry.
    pub fn cpu(&self) -> Cpu {
        match self {
            Entry::Hook { site, .. } | Entry::Patch { site, .. } => site.cpu(),
            Entry::Slot { slot, .. } => slot.cpu(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_reports_the_cpu_of_its_site_or_slot() {
        let s = SiteRef::new(Cpu::ColdFire, 0x100);
        let bytes = PatchBytes::new(&[1, 2]).unwrap();
        assert_eq!(Entry::Hook { site: s, target: 1 }.cpu(), Cpu::ColdFire);
        assert_eq!(Entry::Patch { site: s, bytes }.cpu(), Cpu::ColdFire);
        assert_eq!(
            Entry::Slot {
                slot: SlotRef::new(Cpu::Sharc, 3),
                target: 1
            }
            .cpu(),
            Cpu::Sharc
        );
    }

    #[test]
    fn patch_bytes_are_bounded() {
        assert_eq!(
            PatchBytes::new(&[9; MAX_PATCH]).unwrap().as_slice(),
            [9; MAX_PATCH]
        );
        assert!(PatchBytes::new(&[0; MAX_PATCH + 1]).is_none());
    }
}
