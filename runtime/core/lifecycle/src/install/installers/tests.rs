extern crate std;

use super::*;
use crate::install::{PatchBytes, SiteRef, SlotRef};
use crate::MAX_PACKAGES;
use dkrt_abi::provider::{Status, MAX_ENTRIES};
use std::vec::Vec;

/// Records the calls it gets; fails `install` when told to.
#[derive(Default)]
struct Recorder {
    cpu: Option<Cpu>,
    installed: Vec<Vec<Entry>>,
    restored: Vec<PackageId>,
    fail_install: Option<Status>,
    fail_restore: Option<InstallError>,
    /// `restore` of an owner that was not installed here is an error (E_RANGE).
    strict: bool,
    held: Vec<PackageId>,
}

impl Installer for Recorder {
    fn cpu(&self) -> Cpu {
        self.cpu.unwrap()
    }

    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), Status> {
        match self.fail_install {
            Some(e) => Err(e),
            None => {
                let own = self.cpu();
                self.held.push(owner);
                self.installed
                    .push(entries.iter().filter(|e| e.cpu() == own).copied().collect());
                Ok(())
            }
        }
    }

    fn restore(&mut self, owner: PackageId) -> Result<(), InstallError> {
        self.restored.push(owner);
        if self.strict && !self.held.contains(&owner) {
            return Err(InstallError::Backend(dkrt_sdk::E_RANGE));
        }
        match self.fail_restore {
            Some(e) => Err(e),
            None => {
                self.held.retain(|o| *o != owner);
                Ok(())
            }
        }
    }
}

fn cf_rec() -> Recorder {
    Recorder {
        cpu: Some(Cpu::ColdFire),
        ..Recorder::default()
    }
}

fn sharc_rec() -> Recorder {
    Recorder {
        cpu: Some(Cpu::Sharc),
        ..Recorder::default()
    }
}

const OWNER: PackageId = PackageId(4);

fn cf_hook() -> Entry {
    Entry::Hook {
        site: SiteRef::new(Cpu::ColdFire, 0x100),
        target: 1,
    }
}

fn cf_patch() -> Entry {
    Entry::Patch {
        site: SiteRef::new(Cpu::ColdFire, 0x200),
        bytes: PatchBytes::new(&[1, 2, 3, 4]).unwrap(),
    }
}

fn dsp_slot() -> Entry {
    Entry::Slot {
        slot: SlotRef::new(Cpu::Sharc, 2),
        target: 9,
    }
}

#[test]
fn each_entry_goes_to_the_installer_of_its_cpu_in_order() {
    let mut i = Installers::new(cf_rec(), sharc_rec());
    i.install(OWNER, &[cf_hook(), dsp_slot(), cf_patch()])
        .unwrap();
    assert_eq!(i.coldfire().installed, [[cf_hook(), cf_patch()]]);
    assert_eq!(i.sharc().installed, [[dsp_slot()]]);
}

#[test]
fn a_backend_without_entries_is_not_called() {
    let mut i = Installers::new(cf_rec(), sharc_rec());
    i.install(OWNER, &[cf_hook()]).unwrap();
    assert!(i.sharc().installed.is_empty() && i.sharc().restored.is_empty());
}

#[test]
fn a_failure_of_the_second_backend_restores_the_first() {
    let sharc = Recorder {
        fail_install: Some(-9),
        ..sharc_rec()
    };
    let mut i = Installers::new(cf_rec(), sharc);
    assert_eq!(
        i.install(OWNER, &[cf_hook(), dsp_slot()]),
        Err(InstallError::Backend(-9))
    );
    assert_eq!(i.coldfire().restored, [OWNER]);
    // Nothing stays installed: the owner can install again.
    i.sharc.fail_install = None;
    assert_eq!(i.install(OWNER, &[cf_hook(), dsp_slot()]), Ok(()));
}

#[test]
fn a_failure_of_the_first_backend_never_reaches_the_second() {
    let cf = Recorder {
        fail_install: Some(-9),
        ..cf_rec()
    };
    let mut i = Installers::new(cf, sharc_rec());
    assert_eq!(
        i.install(OWNER, &[cf_hook(), dsp_slot()]),
        Err(InstallError::Backend(-9))
    );
    assert!(i.sharc().installed.is_empty() && i.coldfire().restored.is_empty());
}

#[test]
fn restore_asks_both_and_answers_the_first_error() {
    let cf = Recorder {
        fail_restore: Some(InstallError::Busy { task: Some(3) }),
        ..cf_rec()
    };
    let mut i = Installers::new(cf, sharc_rec());
    i.install(OWNER, &[cf_hook(), dsp_slot()]).unwrap();
    assert_eq!(i.restore(OWNER), Err(InstallError::Busy { task: Some(3) }));
    assert_eq!(
        (i.coldfire().restored.len(), i.sharc().restored.len()),
        (1, 1)
    );
    // The owner stays installed until both say Ok.
    assert_eq!(
        i.install(OWNER, &[cf_hook()]),
        Err(InstallError::AlreadyInstalled)
    );
}

#[test]
fn more_entries_than_the_bound_are_refused_untouched() {
    let mut i = Installers::new(cf_rec(), sharc_rec());
    let many = [cf_hook(); MAX_ENTRIES + 1];
    assert_eq!(i.install(OWNER, &many), Err(InstallError::TableFull));
    assert!(i.coldfire().installed.is_empty());
}

#[test]
fn a_second_install_for_an_installed_owner_is_refused_until_restore() {
    let mut i = Installers::new(cf_rec(), sharc_rec());
    i.install(OWNER, &[cf_hook()]).unwrap();
    assert_eq!(
        i.install(OWNER, &[dsp_slot()]),
        Err(InstallError::AlreadyInstalled)
    );
    assert!(i.sharc().installed.is_empty());
    i.restore(OWNER).unwrap();
    assert_eq!(i.install(OWNER, &[dsp_slot()]), Ok(()));
}

#[test]
fn a_rollback_that_fails_leaves_the_owner_partial_and_a_later_restore_finishes_it() {
    let cf = Recorder {
        fail_restore: Some(InstallError::Busy { task: None }),
        ..cf_rec()
    };
    let sharc = Recorder {
        fail_install: Some(-9),
        ..sharc_rec()
    };
    let mut i = Installers::new(cf, sharc);
    assert_eq!(
        i.install(OWNER, &[cf_hook(), dsp_slot()]),
        Err(InstallError::Partial)
    );
    assert_eq!(
        i.install(OWNER, &[cf_hook()]),
        Err(InstallError::AlreadyInstalled)
    );
    assert_eq!(i.restore(OWNER), Err(InstallError::Busy { task: None }));
    i.coldfire.fail_restore = None;
    assert_eq!(i.restore(OWNER), Ok(()));
    assert_eq!(i.install(OWNER, &[cf_hook()]), Ok(()));
}

#[test]
fn restore_asks_only_the_backends_that_received_entries() {
    let strict = |r: Recorder| Recorder { strict: true, ..r };
    let mut i = Installers::new(strict(cf_rec()), strict(sharc_rec()));
    i.install(OWNER, &[cf_hook()]).unwrap();
    assert_eq!(i.restore(OWNER), Ok(()));
    assert_eq!(
        (i.coldfire().restored.len(), i.sharc().restored.len()),
        (1, 0)
    );
    // An unknown or already restored owner is a no-op.
    assert_eq!(i.restore(OWNER), Ok(()));
    assert_eq!(i.restore(PackageId(9)), Ok(()));
    assert_eq!(
        (i.coldfire().restored.len(), i.sharc().restored.len()),
        (1, 0)
    );
}

#[test]
fn an_owner_past_the_bound_is_out_of_range() {
    let mut i = Installers::new(cf_rec(), sharc_rec());
    let bad = PackageId(MAX_PACKAGES as _);
    assert_eq!(
        i.install(bad, &[cf_hook()]),
        Err(InstallError::OwnerOutOfRange)
    );
    assert!(i.coldfire().installed.is_empty());
}
