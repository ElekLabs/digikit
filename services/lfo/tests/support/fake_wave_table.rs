//! `FakeWaveTable`: the image's table of added waves in memory, for the tests of the owner. It records what the owner asked of it and when (whether the graph of the wave was
//! already withdrawn when `clear` ran), so a test sees the calls without an image. All-zero bytes are its unbound state (`WaveTable`'s contract); the table the next `open` binds
//! is described by the statics below, which a test sets while it holds its lock.
#![allow(dead_code)]
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Status;
use dkrt_sdk::{E_NO_REGISTRY, E_TABLE_FULL};
use dkrt_svc_lfo::graph;
use dkrt_svc_lfo::records::SAMPLES;
use dkrt_svc_lfo::wave_table::WaveTable;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The image has no table: `open` refuses.
pub static NO_TABLE: AtomicBool = AtomicBool::new(false);
/// How many waves the table the next `open` binds has (the first is 7).
pub static WAVES: AtomicU32 = AtomicU32::new(8);

const LOG: usize = 64;
const MAX: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// `set(wave)`, with the first and last sample
    Set(u32, i16, i16),
    /// `clear(wave)`, and whether the graph of the wave was already withdrawn
    Clear(u32, bool),
}

pub struct FakeWaveTable {
    bound: bool,
    first: u32,
    waves: u32,
    n: usize,
    /// (kind: 1 set, 2 clear, the wave, first sample, last sample, graph withdrawn)
    log: [(u8, u32, i16, i16, bool); LOG],
    samples: [[i16; SAMPLES]; MAX],
    set: [bool; MAX],
}

impl FakeWaveTable {
    /// The samples wave `wave` plays now (`None`: empty, the stock sine).
    pub fn playing(&self, wave: u32) -> Option<[i16; SAMPLES]> {
        let k = wave.checked_sub(self.first)? as usize;
        (self.bound && k < MAX && self.set[k]).then(|| self.samples[k])
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.log[..self.n]
            .iter()
            .map(|&(kind, wave, a, b, gone)| {
                if kind == 1 {
                    Call::Set(wave, a, b)
                } else {
                    Call::Clear(wave, gone)
                }
            })
            .collect()
    }

    /// Forget the calls so far (the state of the waves stays).
    pub fn forget(&mut self) {
        self.n = 0;
    }

    fn note(&mut self, entry: (u8, u32, i16, i16, bool)) {
        if self.n < LOG {
            self.log[self.n] = entry;
            self.n += 1;
        }
    }
}

// SAFETY: integers, booleans and arrays of them: all-zero bytes are the unbound state.
unsafe impl WaveTable for FakeWaveTable {
    fn open<C: CoreApi>(&mut self, _core: &C) -> Result<(), Status> {
        if NO_TABLE.load(Ordering::SeqCst) {
            return Err(E_NO_REGISTRY);
        }
        self.bound = true;
        self.first = 7;
        self.waves = WAVES.load(Ordering::SeqCst);
        Ok(())
    }

    fn holds(&self, wave: u32) -> bool {
        self.bound && wave >= self.first && wave - self.first < self.waves
    }

    fn set(&mut self, wave: u32, samples: &[i16; SAMPLES]) -> Result<(), Status> {
        if !self.holds(wave) {
            return Err(E_TABLE_FULL);
        }
        let k = (wave - self.first) as usize;
        self.samples[k] = *samples;
        self.set[k] = true;
        self.note((1, wave, samples[0], samples[SAMPLES - 1], false));
        Ok(())
    }

    fn clear(&mut self, wave: u32) {
        if !self.holds(wave) {
            return;
        }
        let k = (wave - self.first) as usize;
        self.set[k] = false;
        self.note((2, wave, 0, 0, graph::tiles_of(wave).is_none()));
    }
}
