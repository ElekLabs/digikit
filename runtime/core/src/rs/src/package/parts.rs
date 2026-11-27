//! The package parts: objects that `patchkit syspkg build --part` links into the package and that the package does not name (`dkrt_abi::part`). The package owns the directory
//! `DKPKG_PARTS`; `patchkit` writes the address of each part's `DKPART_TABLE` into a slot when it links the part. The package calls every part's `start` once at attach, its `step` once
//! per DSP pass and its `stop` at take-down, and it gives a part the one function it needs, `info` (an entry of the image's info table by name hash).
//!
//! A host test links no part: the directory is empty there, and `Parts::over` takes a directory the test builds.

use dkrt_abi::part::{PartDirectory, PartEnv, PartTable, MAX_PARTS, PARTS_MAGIC, TABLE_MAGIC};

/// The directory the package exports. Its magic keeps it in the image's data (an all-zero static would be bss, which holds no relocation); `patchkit` fills the slots.
#[no_mangle]
pub static DKPKG_PARTS: PartDirectory = PartDirectory { magic: PARTS_MAGIC, tables: [core::ptr::null(); MAX_PARTS] };

/// A part that panics ends in the package's panic path.
#[cfg(feature = "package-image")]
#[no_mangle]
pub extern "C" fn dkpkg_part_panic() -> ! {
    crate::package::resident::fault::panic()
}

unsafe extern "C" fn env_info(name_hash: u32) -> *const u32 {
    use dkrt_abi::package_platform::PackagePlatform;
    crate::package::resident::platform::PLATFORM.info(name_hash)
}

static ENV: PartEnv = PartEnv { size: core::mem::size_of::<PartEnv>() as u32, info: env_info };

/// The parts of one directory.
pub(crate) struct Parts<'a> {
    dir: &'a PartDirectory,
}

impl<'a> Parts<'a> {
    /// The parts of the package's own directory.
    pub(crate) fn linked() -> Parts<'static> {
        Parts { dir: &DKPKG_PARTS }
    }

    /// The parts of `dir` (a test's directory).
    #[cfg(test)]
    pub(crate) fn over(dir: &'a PartDirectory) -> Self {
        Parts { dir }
    }

    /// The tables of the slots that hold a part, in slot order. A slot whose table lacks the magic is skipped, never called.
    fn tables(&self) -> impl Iterator<Item = &'a PartTable> + '_ {
        self.dir.tables.iter().filter_map(|&p| {
            let t = unsafe { p.as_ref()? };
            (t.magic == TABLE_MAGIC).then_some(t)
        })
    }

    /// How many parts the directory holds.
    pub(crate) fn count(&self) -> usize {
        self.tables().count()
    }

    /// The part named `name_hash`.
    pub(crate) fn find(&self, name_hash: u32) -> Option<&'a PartTable> {
        self.tables().find(|t| t.name_hash == name_hash)
    }

    /// Call `start` of every part; the number of parts that answered an error.
    pub(crate) unsafe fn start(&self) -> u32 {
        let mut failed = 0;
        for t in self.tables() {
            if (t.start)(&ENV) < 0 {
                failed += 1;
            }
        }
        failed
    }

    /// Call `step` of every part.
    pub(crate) unsafe fn step(&self) {
        for t in self.tables() {
            (t.step)(&ENV);
        }
    }

    /// Call `stop` of every part.
    #[allow(dead_code)]
    pub(crate) unsafe fn stop(&self) {
        for t in self.tables() {
            (t.stop)(&ENV);
        }
    }
}

#[cfg(test)]
mod tests;
