//! `lfo` service, record handlers (interface B) and the table the LFO tick reads: the `lfo_shape` records of the `register` section. A record is `u32 key`
//! (the module's `DK_KEY_<ID>`: `hash24 << 8`), a 4-character name and 32 samples of `i16`, big-endian, after the 8-byte record header (`[records.kinds]` of
//! sdk/sdk.toml: 80 bytes).
//!
//! A shape is a value of the LFO WAVE parameter. The service gives each shape key a wave number from the sticky allocator (`dkrt_sdk::sticky`, the range
//! [`RANGE`], the settings key `lfo.shapes`), so a sound that names the number keeps its shape while the module stays in the map. The numbers stay below 0x80:
//! an image without `core/lfo-table` clamps a wave byte of 7 to 127 to RAND and a negative byte to TRI (finding 149), and with it a wave byte of 15 to 127 plays RAND, so a number is safe in a sound when the service is not loaded.
//!
//! The table (`shapes`) is indexed by `number - RANGE.lo`; the samples also go to the image's table of added waves (`WaveTable::set`), which is what the LFO plays. One owner per number.
use crate::lfo::Lfo;
use crate::shapes::{self, slot};
use crate::wave::{has_firmware, sync_range, wave_rows};
use crate::wave_table::WaveTable;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::rd32;
use dkrt_abi::provider::{Consumer, Records, Status};
use dkrt_abi::registry::each;
use dkrt_abi::service::{check_step, CheckCtx, Problem};
use dkrt_sdk::records::KIND_LFO_SHAPE;
use dkrt_sdk::sticky::Range;
use dkrt_sdk::*;

/// Samples of one shape.
pub const SAMPLES: usize = 32;
/// Shapes the table keeps.
pub const MAX_SHAPES: usize = 8;
/// The wave numbers of the shapes: 7 (the first above RAND) up, `MAX_SHAPES` of them, all below 0x80.
pub const RANGE: Range = Range {
    lo: 7,
    hi: 7 + MAX_SHAPES as u32 - 1,
};
/// The settings key of the sticky map: `MAX_SHAPES` words, big endian (`hash24 << 8 | number`).
pub const SETTING: &[u8] = b"lfo.shapes";
/// Bytes of the name of a shape.
pub const NAME_LEN: usize = 4;

/// The rule for the shape records of one module (`validate::check_set`); a kind other than the shape record is accepted. `Records::validate` is this function, and a host tool
/// that has no core (`patchkit records --check`) calls it directly.
pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    if kind == KIND_LFO_SHAPE {
        crate::validate::check_set(bodies, report);
    }
}

impl<C: CoreApi, T: WaveTable> Records<C> for Lfo<T> {
    fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        validate(kind, bodies, report)
    }

    fn check(&mut self, core: &C, step: u32, c: &CheckCtx) -> Result<(), Status> {
        let refusal = if step == check_step::RECORDS {
            // the module's shapes meet the rule (`validate`: count, names, key once, samples), and each key is one the module's KEYS entry lists
            let mut bodies: [&[u8]; MAX_SHAPES + 1] = [&[]; MAX_SHAPES + 1];
            let mut n = 0;
            let mut r = OK;
            let at = core.boot_index();
            // SAFETY: `c.reg` is the section the registry walker checked.
            unsafe {
                each(c.reg, |kind, b| {
                    if kind == KIND_LFO_SHAPE {
                        if n <= MAX_SHAPES {
                            bodies[n] = core::slice::from_raw_parts(b, crate::validate::BODY_LEN);
                        }
                        n += 1;
                        if !self.provided(at, rd32(b)) {
                            r = E_FORMAT;
                        }
                    }
                });
            }
            let mut first = OK;
            let shown = if n < bodies.len() { n } else { bodies.len() };
            // more than MAX_SHAPES records: the count problem is the first
            // a module without shapes has nothing to check (the walker runs this step for every module)
            if n > 0 {
                crate::validate::check_set(&bodies[..shown], &mut |p: Problem| {
                    if first == OK {
                        first = p.code;
                    }
                });
            }
            if first != OK {
                first
            } else {
                r
            }
        } else if step == check_step::ROWS {
            // every key has a wave number, and the table has room (the shapes of the module being reloaded are given back before its new file loads). A number is never
            // held by another module: `settle` gives one number to each key, and the planner refuses a repeated key.
            let mut r = OK;
            let mut add = 0;
            let at = core.boot_index();
            // SAFETY: as above.
            unsafe {
                each(c.reg, |kind, b| {
                    if kind == KIND_LFO_SHAPE && r == OK {
                        let number = self.number_of(at, rd32(b));
                        if slot(number).is_none() || !self.table().holds(number) {
                            r = E_TABLE_FULL;
                        }
                        add += 1;
                    }
                });
            }
            if r == OK
                && shapes::used() - shapes::own_shapes(core.live_op() as u8) + add > MAX_SHAPES
            {
                r = E_TABLE_FULL;
            }
            // the WAVE rows must take the maximum (`sync_range`): the image has the registry's parameter table
            if r == OK
                && add > 0
                && has_firmware()
                // SAFETY: this is the UI task; `core` is the bound core API
                && unsafe { wave_rows(core) }.is_none()
            {
                r = E_NO_REGISTRY;
            }
            r
        } else {
            OK
        };
        if refusal == OK {
            Ok(())
        } else {
            Err(refusal)
        }
    }

    fn apply(
        &mut self,
        core: &C,
        kind: u32,
        body: &[u8],
        consumer: &Consumer<'_>,
    ) -> Result<(), Status> {
        if kind != KIND_LFO_SHAPE {
            return Ok(());
        }
        if body.len() < crate::validate::BODY_LEN {
            return Err(E_FORMAT);
        }
        let owner = consumer.id().get();
        // SAFETY: `body` holds `BODY_LEN` bytes, the key is its first word
        let key = unsafe { rd32(body.as_ptr()) };
        let i = match slot(self.number_of(core.boot_index(), key)) {
            Some(i) => i,
            None => return Err(E_TABLE_FULL),
        };
        // defence in depth: `settle` gives each key its own number, so a slot is never held by another module; one that is (the module being reloaded still holds its own
        // until its teardown) is not overwritten
        let held = shapes::owner_at(i);
        if held != 0 && held != owner && held as u32 != core.live_op() {
            return Err(E_TYPE_USED);
        }
        let mut name = [0u8; NAME_LEN];
        name.copy_from_slice(&body[4..4 + NAME_LEN]);
        let mut samples = [0i16; SAMPLES];
        let mut s = 0;
        let mut neg = false;
        while s < SAMPLES {
            samples[s] = i16::from_be_bytes([body[8 + 2 * s], body[9 + 2 * s]]);
            neg |= samples[s] < 0;
            s += 1;
        }
        // the LFO plays the samples from the image's table: a table without the wave refuses before anything changes
        self.table_mut().set(RANGE.lo + i as u32, &samples)?;
        shapes::put(i, owner, !neg, name, &samples);
        // the page's picture (UI task): published last, after the table entry
        // SAFETY: `i` is a slot of the table; one writer at a time (the service's apply and detach)
        unsafe { crate::graph::publish(i, &samples, !neg) };
        // the WAVE maximum reaches the new number (UI task reads it; a write of one word)
        // SAFETY: UI task
        match unsafe { sync_range(core) } {
            OK => Ok(()),
            e => Err(e),
        }
    }
}
