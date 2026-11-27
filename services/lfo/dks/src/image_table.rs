//! `ImageTable`: the table of added LFO waves of the image (`core/lfo-table`, its data table `lfo_table_info`) as the owner's `WaveTable`. The service finds the table by the hash
//! of its name (`CoreApi::info`) at `start`; the words are `'LFOT'`, version 1, the first added wave, how many, the samples of a shape, and the addresses of the two calls
//! (`runtime/platform/lfo-table/src/cf/lfo-table.h`). The cell format of the waves stays private to the image: the service calls `lfo_table_set(wave, samples)` and
//! `lfo_table_clear(wave)` and nothing else.
//!
//! A host build (the assembly and contract tests of the runtime) has no image code: the two calls go to a table in memory (`host`) that a test reads.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Status;
use dkrt_sdk::{E_NO_REGISTRY, E_TABLE_FULL};
use dkrt_svc_lfo::lfo::Lfo;
use dkrt_svc_lfo::records::SAMPLES;
use dkrt_svc_lfo::wave_table::WaveTable;

/// `'LFOT'`.
const MAGIC: u32 = 0x4c46_4f54;
const VERSION: u32 = 1;
/// The words of `lfo_table_info`.
const WORDS: usize = 7;

/// The name of the image's data table (`[[info]]` of `core/lfo-table`).
const NAME: &str = "lfo_table_info";

/// The owner object of the service file: the service over the image's table of added waves.
pub type Owner = Lfo<ImageTable>;

/// The bound table: all-zero bytes are the unbound state.
pub struct ImageTable {
    first: u32,
    waves: u32,
    /// the entry of `lfo_table_set` and of `lfo_table_clear`
    set: u32,
    clear: u32,
}

impl ImageTable {
    /// The table the words describe, or none: another magic or version, a shape of other than 32 samples, no wave, or a missing entry.
    pub fn parse(w: &[u32; WORDS]) -> Option<ImageTable> {
        if w[0] != MAGIC
            || w[1] != VERSION
            || w[4] != SAMPLES as u32
            || w[3] == 0
            || w[5] == 0
            || w[6] == 0
        {
            return None;
        }
        Some(ImageTable {
            first: w[2],
            waves: w[3],
            set: w[5],
            clear: w[6],
        })
    }
}

/// The words of the image's `lfo_table_info`, or none when the image has no such entry.
fn words<C: CoreApi>(core: &C) -> Option<[u32; WORDS]> {
    // SAFETY: the image's table is `WORDS` words of read-only data
    let t = unsafe { core.info(dkneeds::keys::name_hash(NAME)) };
    if t.is_null() {
        // a host build has no image: a test that loads the file installs the stand-in table
        #[cfg(not(target_arch = "m68k"))]
        return host::words();
        #[cfg(target_arch = "m68k")]
        return None;
    }
    let mut w = [0u32; WORDS];
    let mut i = 0;
    while i < WORDS {
        // SAFETY: as above
        w[i] = unsafe { t.add(i).read() };
        i += 1;
    }
    Some(w)
}

// SAFETY: integers only: all-zero bytes are the unbound state (`holds` is false for every wave).
unsafe impl WaveTable for ImageTable {
    fn open<C: CoreApi>(&mut self, core: &C) -> Result<(), Status> {
        match words(core).and_then(|w| ImageTable::parse(&w)) {
            Some(t) => {
                *self = t;
                Ok(())
            }
            None => Err(E_NO_REGISTRY),
        }
    }

    fn holds(&self, wave: u32) -> bool {
        wave >= self.first && wave - self.first < self.waves
    }

    fn set(&mut self, wave: u32, samples: &[i16; SAMPLES]) -> Result<(), Status> {
        if !self.holds(wave) {
            return Err(E_TABLE_FULL);
        }
        // SAFETY: `set` is the entry of `lfo_table_set` of the image, called in the UI task with a wave of its range and 32 samples
        if unsafe { call::set(self.set, wave, samples) } == 0 {
            Ok(())
        } else {
            Err(E_TABLE_FULL)
        }
    }

    fn clear(&mut self, wave: u32) {
        if self.holds(wave) {
            // SAFETY: as above, for `lfo_table_clear`
            unsafe { call::clear(self.clear, wave) };
        }
    }
}

#[cfg(target_arch = "m68k")]
mod call {
    use super::SAMPLES;

    /// `int32_t lfo_table_set(uint32_t wave, const int16_t *samples)` at `entry` (the C convention of the ColdFire toolchain).
    pub(super) unsafe fn set(entry: u32, wave: u32, samples: &[i16; SAMPLES]) -> i32 {
        let f: extern "C" fn(u32, *const i16) -> i32 = core::mem::transmute(entry as usize);
        f(wave, samples.as_ptr())
    }

    /// `void lfo_table_clear(uint32_t wave)` at `entry`.
    pub(super) unsafe fn clear(entry: u32, wave: u32) {
        let f: extern "C" fn(u32) = core::mem::transmute(entry as usize);
        f(wave)
    }
}

#[cfg(not(target_arch = "m68k"))]
use host::call;

/// The host stand-in of the image's two calls: a table of 16 waves in memory, for the tests that load the file without an image.
#[cfg(not(target_arch = "m68k"))]
pub mod host {
    use super::SAMPLES;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    const WAVES: usize = 16;

    static INSTALLED: AtomicBool = AtomicBool::new(false);

    /// The image has the table (7 .. 14, two entries that are never called): the next `open` binds it. Without this call the image has none.
    pub fn install() {
        INSTALLED.store(true, Ordering::SeqCst);
    }

    /// The image has no table again.
    pub fn uninstall() {
        INSTALLED.store(false, Ordering::SeqCst);
    }

    pub(super) fn words() -> Option<[u32; super::WORDS]> {
        INSTALLED.load(Ordering::SeqCst).then_some([
            super::MAGIC,
            super::VERSION,
            7,
            8,
            super::SAMPLES as u32,
            0x1000,
            0x1010,
        ])
    }

    struct Table {
        samples: [Option<[i16; SAMPLES]>; WAVES],
        sets: usize,
        clears: usize,
    }

    static TABLE: Mutex<Table> = Mutex::new(Table {
        samples: [None; WAVES],
        sets: 0,
        clears: 0,
    });

    fn table() -> std::sync::MutexGuard<'static, Table> {
        TABLE.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) mod call {
        use super::{table, SAMPLES, WAVES};

        pub(in super::super) unsafe fn set(
            _entry: u32,
            wave: u32,
            samples: &[i16; SAMPLES],
        ) -> i32 {
            let mut t = table();
            match t.samples.get_mut(wave as usize) {
                Some(slot) if (wave as usize) < WAVES => {
                    *slot = Some(*samples);
                    t.sets += 1;
                    0
                }
                _ => -1,
            }
        }

        pub(in super::super) unsafe fn clear(_entry: u32, wave: u32) {
            let mut t = table();
            if let Some(slot) = t.samples.get_mut(wave as usize) {
                *slot = None;
                t.clears += 1;
            }
        }
    }

    /// The samples wave `wave` plays now (`None`: empty, the stock sine).
    pub fn playing(wave: u32) -> Option<[i16; SAMPLES]> {
        table().samples.get(wave as usize).copied().flatten()
    }

    /// The calls of `lfo_table_set` and `lfo_table_clear` so far.
    pub fn calls() -> (usize, usize) {
        let t = table();
        (t.sets, t.clears)
    }

    /// Every wave empty, the counts 0.
    pub fn reset() {
        let mut t = table();
        t.samples = [None; WAVES];
        t.sets = 0;
        t.clears = 0;
    }
}
