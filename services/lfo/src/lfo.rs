//! `Lfo<T>`: the owner object of the service. It keeps the keys of the boot's selection and the wave numbers `settle` gave them, the image's table of added waves (`T`, a
//! [`WaveTable`]) and is the `Provider` of the file: `start` (in the file's `init`: it binds the table, and refuses without one), `settle` (the boot's plan of numbers, kept in
//! the settings key `lfo.shapes`) and `detach` (a module's shapes go: the page's picture first, then the wave back to the sine). The `Records` part is in `records.rs`. The
//! table of shapes stays a named static (`shapes::SHAPES`: the UI hooks of the file reach it by symbol), and so does the table of the WAVE rows (`wave::WAVE`, set at `start`).
use crate::records::{MAX_SHAPES, RANGE, SETTING};
use crate::shapes;
use crate::wave_table::WaveTable;
use core::mem::MaybeUninit;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{ConsumerId, Provider, Status};
use dkrt_abi::service::SettleCtx;
use dkrt_sdk::records::KIND_LFO_SHAPE;
use dkrt_sdk::sticky::{assign, clean, Event, MAX_KEYS};
use dkrt_sdk::*;

#[cfg(target_arch = "m68k")]
extern "C" {
    /// Four words of the file's assembler (`dks/src/hooks-ui.s`): the stock parameter table and the rows of the three WAVE parameters (`dk_fw_` words, resolved by the port
    /// file). A host build has no firmware, so it has no such symbol.
    static DKRT_LFO_FW: [u32; 4];
}

/// The owner object: no key settled, no module in the selection, the table not bound. All-zero bytes are this state (`Provider::start_in`; `WaveTable`'s contract).
pub struct Lfo<T: WaveTable> {
    /// keys in the lists below
    n: usize,
    /// selection index of the module of each key
    at: [u8; MAX_KEYS],
    key: [u32; MAX_KEYS],
    /// the wave number `settle` gave each key
    num: [u32; MAX_KEYS],
    /// modules of the selection
    modules: usize,
    /// the image's table of added waves
    table: T,
}

impl<T: WaveTable> Lfo<T> {
    /// The table of added waves (for a test or a query).
    pub fn table(&self) -> &T {
        &self.table
    }

    /// The table of added waves, to change it.
    pub fn table_mut(&mut self) -> &mut T {
        &mut self.table
    }

    /// The wave number of the shape key `key` of the module at selection index `at` (0: none was given).
    pub fn number_of(&self, at: u32, key: u32) -> u32 {
        let mut i = 0;
        while i < self.n {
            if self.key[i] == key && self.at[i] as u32 == at {
                return self.num[i];
            }
            i += 1;
        }
        0
    }

    /// Is `key` among the shape keys the module at selection index `at` provides (its `KEYS` entry)? True when no `settle` ran for that index (a host without a boot).
    pub(crate) fn provided(&self, at: u32, key: u32) -> bool {
        if at as usize >= self.modules {
            return true;
        }
        let mut i = 0;
        while i < self.n {
            if self.at[i] as u32 == at && self.key[i] == key {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// The firmware words of the WAVE rows, given to `wave` (what the file's `setup` did before the owner). A host build has no firmware: nothing is set, and the service
/// writes no row.
fn bind_firmware() {
    #[cfg(target_arch = "m68k")]
    // SAFETY: the four words are the file's own data, resolved by the loader before `init`
    unsafe {
        crate::wave::set_firmware(
            core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[0])),
            [
                core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[1])),
                core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[2])),
                core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[3])),
            ],
        );
    }
}

fn words(raw: &[u8; MAX_SHAPES * 4]) -> [u32; MAX_SHAPES] {
    let mut map = [0u32; MAX_SHAPES];
    let mut i = 0;
    while i < MAX_SHAPES {
        map[i] = u32::from_be_bytes([raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]]);
        i += 1;
    }
    map
}

fn stored<C: CoreApi>(core: &C) -> [u32; MAX_SHAPES] {
    let mut raw = [0u8; MAX_SHAPES * 4];
    // SAFETY: a plain read of a setting into `raw`
    if unsafe { core.settings_get(SETTING, &mut raw) } == raw.len() as i32 {
        words(&raw)
    } else {
        [0; MAX_SHAPES]
    }
}

fn save<C: CoreApi>(core: &C, map: &[u32; MAX_SHAPES]) -> bool {
    let mut raw = [0u8; MAX_SHAPES * 4];
    let mut i = 0;
    while i < MAX_SHAPES {
        raw[4 * i..4 * i + 4].copy_from_slice(&map[i].to_be_bytes());
        i += 1;
    }
    // SAFETY: a plain write of a setting
    unsafe { core.settings_put(SETTING, &raw) == OK }
}

impl<C: CoreApi, T: WaveTable> Provider<C> for Lfo<T> {
    /// The state is made where it lives: all-zero bytes are the initial state (`WaveTable`'s contract; `tests/state.rs` checks it), so this is one fill. Then the table of the
    /// image is bound: without one the service does not start (`E_NO_REGISTRY`), so no shape is ever accepted that no LFO could play.
    fn start_in(core: &C, slot: &mut MaybeUninit<Lfo<T>>) -> Result<(), Status> {
        bind_firmware();
        // SAFETY: every field of `Lfo` is valid as zero bytes (integers, arrays of them and the table, by the contract of `WaveTable`).
        let lfo = unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            &mut *slot.as_mut_ptr()
        };
        lfo.table.open(core)
    }

    /// A module unloads (also one that never attached, or only in part): its shapes leave, the page's picture first, then the wave goes back to the sine, and the WAVE maximum
    /// follows the highest number that is left (the stock pair when none is).
    fn detach(&mut self, core: &C, consumer: ConsumerId) {
        let owner = consumer.get();
        let mut i = 0;
        while i < MAX_SHAPES {
            if shapes::owner_at(i) == owner {
                // the hook gives the page the stock picture first
                // SAFETY: `i` is a slot of the table; one writer at a time
                unsafe { crate::graph::retract(i) };
                self.table.clear(RANGE.lo + i as u32);
                shapes::free(i);
            }
            i += 1;
        }
        // the highest number may be lower now, or no shape is left: the stock pair goes back
        // SAFETY: UI task
        unsafe { crate::wave::sync_range(core) };
    }

    /// The boot's plan of numbers: each shape key of the selection gets a number from the sticky map in the setting `lfo.shapes`, and the map is saved when it changed.
    fn settle(&mut self, core: &C, sel: &SettleCtx) {
        self.n = 0;
        self.modules = sel.count.min(dkrt_sdk::SEL_MAX);
        let from = stored(core);
        let mut map = [0u32; MAX_SHAPES];
        clean(RANGE, &from, &mut map);
        let mut k = 0;
        while k < self.modules {
            // the shape keys of the module (none for one the plan refused)
            let mut keys = [0u32; 8];
            let have = core.keys_of(k as u32, KIND_LFO_SHAPE, &mut keys);
            let mut i = 0;
            while i < have && self.n < MAX_KEYS {
                self.at[self.n] = k as u8;
                self.key[self.n] = keys[i];
                self.num[self.n] = 0;
                self.n += 1;
                i += 1;
            }
            k += 1;
        }
        let n = self.n;
        let dirty = assign(
            RANGE,
            &from,
            &mut map,
            &self.key,
            &mut self.num,
            n,
            &mut |_: &Event| {},
        );
        // a refused write is tried again at the next boot: the same map gives the same numbers
        if dirty != 0 {
            save(core, &map);
        }
    }
}
