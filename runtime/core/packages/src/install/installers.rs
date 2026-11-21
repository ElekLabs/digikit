use super::{Cpu, Entry, InstallError, Installer};
use crate::{PackageId, MAX_PACKAGES};
use dkrt_abi::provider::MAX_ENTRIES;

/// Which backends received entries of an owner.
const CF: u8 = 1;
const DSP: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Absent,
    Installed,
    /// A failed install could not take back what the first backend wrote.
    Partial,
}

#[derive(Clone, Copy)]
struct Owner {
    state: State,
    /// Bit set of [`CF`] and [`DSP`]: the backends `restore` must ask.
    backends: u8,
}

const ABSENT: Owner = Owner {
    state: State::Absent,
    backends: 0,
};

/// One installer per CPU behind one interface. Both backends get all entries and take their own; the install stays all or nothing across both:
/// when the second backend fails, the first is restored. Owners are tracked with the backends that received their entries, so a second install for
/// an owner is refused, a half-restored owner is finished by a later `restore`, and `restore` asks only those backends.
///
/// Errors are [`InstallError`]; mapping to SDK status codes happens at the ABI boundary (RC4). The backends keep answering `Status`.
pub struct Installers<CF: Installer, DSP: Installer> {
    coldfire: CF,
    sharc: DSP,
    owners: [Owner; MAX_PACKAGES],
}

impl<CF: Installer, DSP: Installer> Installers<CF, DSP> {
    pub fn new(coldfire: CF, sharc: DSP) -> Self {
        debug_assert!(coldfire.cpu() == Cpu::ColdFire && sharc.cpu() == Cpu::Sharc);
        Installers {
            coldfire,
            sharc,
            owners: [ABSENT; MAX_PACKAGES],
        }
    }

    pub fn coldfire(&self) -> &CF {
        &self.coldfire
    }

    pub fn sharc(&self) -> &DSP {
        &self.sharc
    }
}

fn has(entries: &[Entry], cpu: Cpu) -> bool {
    entries.iter().any(|e| e.cpu() == cpu)
}

impl<CF: Installer, DSP: Installer> Installers<CF, DSP> {
    /// Install the entries of `entries` for `owner`, each on its CPU's backend, or none. A backend with no entries of its CPU is not called.
    /// `TableFull` for more than [`MAX_ENTRIES`] entries, `OwnerOutOfRange` for an owner past `MAX_PACKAGES`, `AlreadyInstalled` for an owner that is
    /// installed (or partly), `Partial` when the rollback failed, `Backend` for a backend's own refusal.
    pub fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), InstallError> {
        if entries.len() > MAX_ENTRIES {
            return Err(InstallError::TableFull);
        }
        let slot = self
            .owners
            .get_mut(owner.0 as usize)
            .ok_or(InstallError::OwnerOutOfRange)?;
        if slot.state != State::Absent {
            return Err(InstallError::AlreadyInstalled);
        }
        let cf = has(entries, Cpu::ColdFire);
        let dsp = has(entries, Cpu::Sharc);
        if cf {
            self.coldfire
                .install(owner, entries)
                .map_err(InstallError::Backend)?;
        }
        if dsp {
            if let Err(e) = self.sharc.install(owner, entries) {
                if cf && self.coldfire.restore(owner).is_err() {
                    *slot = Owner {
                        state: State::Partial,
                        backends: CF,
                    };
                    return Err(InstallError::Partial);
                }
                return Err(InstallError::Backend(e));
            }
        }
        *slot = Owner {
            state: State::Installed,
            backends: if cf { CF } else { 0 } | if dsp { DSP } else { 0 },
        };
        Ok(())
    }

    /// Undo everything `owner` installed, asking only the backends that received its entries. An owner with nothing installed (or unknown) is a
    /// no-op `Ok`. Both backends are asked even when the first answers `E_BUSY`; the first error is the answer (`Backend`) and the owner stays as it
    /// is until both say `Ok`.
    pub fn restore(&mut self, owner: PackageId) -> Result<(), InstallError> {
        let Some(slot) = self.owners.get_mut(owner.0 as usize) else {
            return Ok(());
        };
        let cf = if slot.backends & CF != 0 {
            self.coldfire.restore(owner)
        } else {
            Ok(())
        };
        let dsp = if slot.backends & DSP != 0 {
            self.sharc.restore(owner)
        } else {
            Ok(())
        };
        let done = cf.and(dsp);
        if done.is_ok() {
            *slot = ABSENT;
        }
        done.map_err(InstallError::Backend)
    }
}

#[cfg(test)]
mod tests;
