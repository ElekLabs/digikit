//! Constants from device data, for code that cannot read a file: `patchkit gen` writes them, `gen --check` keeps them current.
//!
//! The sources are the device files (`devices/*.toml`: `[board]`, `[digikit]`, `[card.store]`) and the run-time type range of
//! `core/machines` (`types.toml`). A module asks for a file with a `[[gen]]` entry in its `module.toml` (`module::GenDef`):
//!
//! - `gas-dkdv`: the runtime's per-device data object `dkrt_dev` (struct `DkrtDevice` of `core/runtime/abi.toml`) as ColdFire assembly
//!   data, one file per device. The runtime is one image for all devices: what differs between devices is data that it reads from this
//!   object (`.include` it in the device object's `.rodata`). `gas-dkdv-offsets`: the field offsets as `.equ DKDV_<FIELD>, n` for
//!   assembly that reads the object (one file, the same for all devices).
//! - `gas`: `.equ NAME, value` lines for ColdFire assembly, one file per device (`#include "device-<dev>.inc"` in a source of
//!   the module; `patchkit build` expands it, `.include` also works).
//! - `sharc`: `#define NAME value` lines for SHARC+ assembly, one file per device (decimal values: the assembler upper-cases
//!   macro text, which breaks `0x` literals after `+`).
//! - `sharc-adapter`: `#define ADM_*` lines for the machine adapter's SHARC+ assembly, one file per device, from `[adapter.machine]`
//!   and `[modules.frame]` of the device file (docs/design/module-sdk.md 5.2). Only those names, no other constant.
//! - `gas-sample-pool`: `.equ SP_*` lines for the sample pool adapter's ColdFire assembly, one file per device, from
//!   `[adapter.sample_pool]` (slot floor and the reserved DDR range; docs/design/sample-api-1.5.md 2.3). Only those names.
//! - `sharc-sample-pool`: `#define SP_DDR_BASE_NW32` and `SP_DDR_PAGES` for the sample pool's SHARC+ receiver, one file per device that owns a
//!   DDR range (a device that borrows the stock pool gets no file), from the same fields (docs/design/dn2-sample-pool.md 2.7).
//!
//! The files carry no firmware bytes, only numbers of the device files.

use crate::error::*;
use crate::module::Library;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write;

/// One generated file: path relative to the module directory, and its text.
pub type GenFile = (String, String);

#[derive(Deserialize)]
struct Doc {
    device: Dev,
    board: Option<Board>,
    digikit: Option<Digikit>,
    capabilities: Option<Capabilities>,
    card: Option<Card>,
    limits: Option<Limits>,
    adapter: Option<AdapterDoc>,
    modules: Option<ModulesDoc>,
    input: Option<InputDoc>,
    panel: Option<PanelDoc>,
    fx: Option<FxDoc>,
    #[serde(default)]
    patch_sets: BTreeMap<String, Vec<String>>,
}
/// `[fx]`: the facts of the effect pages of run-time effects (core/fxpages, docs/design/digikit-registry.md "Effect pages"): where the kit keeps its
/// effect values, which of them the firmware does not use (effect row k owns `value_indices[8k .. 8k + 8]`), the page numbering and the stock
/// parameter rows whose display an effect knob copies.
#[derive(Deserialize)]
struct FxDoc {
    /// The address of the firmware's pointer to the live kit object (an SRAM global), and the byte offset of the kit's array of s16 effect values in it.
    kit_ptr: u64,
    array_off: u64,
    /// The page number of the first effect page (the number of stock pages).
    page_first: u64,
    /// Indices of the effect value array that no stock parameter uses, in the order the effect rows take them (a multiple of 8, at most `FX_VALUES_MAX`).
    value_indices: Vec<u64>,
    /// Stock parameter codes: a 0 .. 127 knob, a -64 .. +63 knob, and the level slot (MIX).
    template_percent: u64,
    template_bipolar: u64,
    template_mix: u64,
    /// Where the kit's storage keeps the effect values (persistence): the byte offset of the first spare halfword in the storage record of the effect value
    /// array, and the number of spare halfwords. The values (one halfword for each of `value_indices`) come first, then one effect ID for each effect row.
    store_off: u64,
    store_halfwords: u64,
    /// An array index nothing uses (not one of `value_indices`): the mark that an effect row holds values (core/fxpages).
    mark_index: u64,
    /// The byte offset of the u32 after the array's halfwords (the stock loader and saver move it with `move.l (0x98,record),(tail,array)`).
    tail_off: u64,
    /// The send page (FX routing): the stock page it copies its title and level slot from, the address of the table of the 16 Sound object pointers, the stock send row
    /// whose owner and key words a send knob copies, and the Sound array indices the send knobs use (the first `[limits] send_slots` of them). All 0 / empty where the
    /// device has no send page yet.
    #[serde(default)]
    track_page: u64,
    #[serde(default)]
    sound_off: u64,
    #[serde(default)]
    sound_stride: u64,
    /// The number of s16 parameter words in a Sound object's array (at +0x14): the debug log's parameter watch reads that many.
    #[serde(default)]
    sound_words: u64,
    #[serde(default)]
    send_template: u64,
    #[serde(default)]
    send_indices: Vec<u64>,
}
/// `[input]`: which input backend the device uses (a name of `INPUT_BACKENDS`); the profile chooses, the code of core/input has one backend each.
#[derive(Deserialize)]
struct InputDoc {
    backend: String,
    /// `[input.keys]`: logical key name = the key's id in the firmware's key events (the id at +0xc of the event). The one source of the C names (`dk_key`), the
    /// profile's key map and `DkrtDevice.key_codes`. These are firmware key indices: they equal the panel code on some devices and differ
    /// on others (finding 153), and they are not the wire codes (channel, bit) that `dk_input_key` injects.
    #[serde(default)]
    keys: BTreeMap<String, u64>,
    /// `[input.key_flags]`: the bits of the key event's flags word (+0x10), named by meaning.
    #[serde(default)]
    key_flags: Option<KeyFlagsDoc>,
}
/// `[input.key_flags]`: bit numbers (0 to 31) of the flags word of a key event: `press` is set on a press and clear on a release, `func` is set while FUNC is held,
/// `repeat` marks a held key repeating, `release` is set on a release (the stock list handler acts on it).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFlagsDoc {
    press: u64,
    func: u64,
    repeat: u64,
    release: u64,
}
/// The value of `UI_KEY_YES2` is the id of `LEVEL_PUSH`: a second key that acts as YES (the stock handler treats it like YES).
/// core/ui's four keys, by the names of `[input.keys]`: the constant they generate, and the key name. A device file without one of them is refused.
const UI_KEY_NOTES: [&str; 4] = [
    "[input.keys] YES (the key id of YES in a key event)",
    "[input.keys] LEVEL_PUSH (a second YES key)",
    "[input.keys] NO (the key id of NO)",
    "[input.keys] RIGHT (the key id of RIGHT)",
];
const UI_KEYS: [(&str, &str); 4] = [
    ("UI_KEY_YES", "YES"),
    ("UI_KEY_YES2", "LEVEL_PUSH"),
    ("UI_KEY_NO", "NO"),
    ("UI_KEY_RIGHT", "RIGHT"),
];

/// `[panel]`: the key codes: codes 1 to 8 * `linear_channels` are linear (code = channel * 8 + bit + 1), `[panel.exceptions]` lists the others as
/// code = [channel, bit]. Only these numbers are generated; the groups are for hosts that draw the panel.
#[derive(Deserialize)]
struct PanelDoc {
    linear_channels: u64,
    /// The channels the firmware's button decoder has tables for (it has no bounds check; docs/findings/58-panel-input-path.md): a key channel is below it.
    button_channels: u64,
    encoders: u64,
    #[serde(default)]
    exceptions: BTreeMap<String, [u64; 2]>,
}
#[derive(Deserialize)]
struct AdapterDoc {
    machine: Option<AdapterMachine>,
    sample_pool: Option<AdapterSamplePool>,
}
/// `[adapter.sample_pool]`: where the sample pool adapter puts samples. A device borrows the stock pool (a slot floor) or owns a DDR range
/// (base and bytes); the unused fields are 0.
#[derive(Deserialize)]
struct AdapterSamplePool {
    /// The lowest stock sample slot the adapter may take (it takes slots from 0x3ff down; 0: not used).
    slot_floor: u64,
    /// The reserved DDR range of an own allocator (base is a physical byte address; 0, 0: not used).
    ddr_base: u64,
    ddr_bytes: u64,
    /// The base of the 16 R rings of the stereo machines (16 x 32 KiB of the DSP's DDR; 0: no stereo rings). Outside the pool range, in DDR the stock never writes.
    #[serde(default)]
    ring_r_base: u64,
}
/// `[adapter.machine]`: the machine adapter's device facts. The strings are checked, the numbers become `ADM_*` defines.
#[derive(Deserialize)]
struct AdapterMachine {
    voices: u64,
    voice_unit: String,
    pitch_hw: u64,
    velocity_hw: u64,
    type_source: String,
    type_hw: Option<u64>,
    record_active: Option<u64>,
    record_mark: Option<u64>,
    note_on: String,
    trig_hw: Option<u64>,
    pair: String,
    tune_slot: Option<u64>,
    param_slots: Vec<u64>,
    reserved_slots: Vec<u64>,
}
#[derive(Deserialize)]
struct ModulesDoc {
    frame: Option<FrameDoc>,
}
#[derive(Deserialize)]
struct FrameDoc {
    base: u64,
    stride: u64,
    group: Option<u64>,
    gap: Option<u64>,
}
#[derive(Deserialize)]
struct Limits {
    modules: u64,
    machine_rows: u64,
    arena_bytes: u64,
    stage_bytes: u64,
    fx_slots: u64,
    send_slots: u64,
    param_rows: u64,
    page_rows: u64,
}
#[derive(Deserialize)]
struct Dev {
    short: String,
}
#[derive(Deserialize)]
struct Board {
    console_uart_base: u64,
    ddr_cached_base: u64,
    ddr_bytes: u64,
    ddr_uncached_alias: u64,
    /// Absent: the device has no eSDHC the runtime may poll (`DEV_HAS_SDHC` clear).
    esdhc_base: Option<u64>,
    /// The three DSP facts are present when `[capabilities] dsp = "sharc"` and absent otherwise.
    dsp_clock_hz: Option<u64>,
    dsp_blocks_per_second: Option<u64>,
}
/// `[capabilities]`: only `dsp` and `card` are read here (the device-registry schema has more keys).
#[derive(Deserialize)]
struct Capabilities {
    dsp: String,
    card: String,
}
#[derive(Deserialize)]
struct Digikit {
    id: u64,
    name: String,
    /// Absent: no key starts safe mode (`DEV_HAS_SAFE_KEY` clear).
    safe_key: Option<u64>,
    dsp_area_bytes: Option<u64>,
    dsp_tail_off: Option<u64>,
    dsp_tail_words: Option<u64>,
    keep_base: u64,
    keep_bytes: u64,
    sample_path_max: u64,
    peek: Vec<[u64; 2]>,
    /// The tracks of the device (0: not given).
    #[serde(default)]
    tracks: u64,
    #[serde(default)]
    span_clock: u64,
    #[serde(default)]
    span_clock_hz: u64,
}
#[derive(Deserialize)]
struct Card {
    store: Option<Store>,
}
#[derive(Deserialize)]
struct Store {
    first_sector: u64,
    end_sector: u64,
    selection_sectors: u64,
    settings_sectors: u64,
    header_b_sector: u64,
}

/// The device files that describe a digikit device, by short name.
#[derive(Debug, Default)]
pub struct Facts {
    docs: BTreeMap<String, Entry>,
    /// Every device file (a runtime target or not): the tables a module's `needs_device` can name, and the patch sets.
    devices: BTreeMap<String, DeviceTables>,
}

#[derive(Debug)]
struct DeviceTables {
    /// The tables of the device file as dotted paths (`input`, `input.keys`).
    tables: std::collections::BTreeSet<String>,
    /// `[patch_sets]`: the root modules of each named set.
    patch_sets: BTreeMap<String, Vec<String>>,
}

#[derive(Debug)]
struct Entry {
    board: BoardF,
    digikit: DigikitF,
    /// `None`: no `[card.store]` (`DEV_HAS_STORE` clear).
    store: Option<StoreF>,
    /// `DEV_HAS_*` bits of `core/runtime/abi.toml`, derived from `[capabilities]` and the tables present.
    caps: u64,
    limits: LimitsF,
    adapter: Option<AdapterF>,
    sample_pool: Option<SamplePoolF>,
    input: InputF,
    /// `[input.keys]`: logical key name to the key's id in the firmware's key events (empty: the device has no key names).
    keys: BTreeMap<String, u64>,
    fx: FxF,
    profile: ProfileF,
    /// DSP instructions per block left for modules (`dkprof::budget`: the block at the default margin, less the stock firmware's load). 0 where `devices/cycle-factors.json`
    /// has no entry for the device, and in the wasm build (which has no `dkprof`).
    dsp_headroom: u64,
}

/// The DSP headroom of a device: see `Entry::dsp_headroom`.
#[cfg(not(target_arch = "wasm32"))]
fn dsp_headroom(short: &str, b: &Board, voices: u64) -> u64 {
    match (b.dsp_clock_hz, b.dsp_blocks_per_second) {
        (Some(clock), Some(blocks)) => {
            crate::budget_cli::headroom(short, clock, blocks, voices).unwrap_or(0)
        }
        _ => 0, // a device without a DSP has no headroom
    }
}
#[cfg(target_arch = "wasm32")]
fn dsp_headroom(_short: &str, _b: &Board, _voices: u64) -> u64 {
    0
}
/// The device facts of the builder commit (`core/runtime` reads them from `dynreg_info` version 3): what the packer took from `[modules]` and
/// `[adapter.machine]` before modules declared through builder calls. All 0 on a device without `[modules]`.
#[derive(Debug, Default)]
struct ProfileF {
    clone_of: u64,
    level: u64,
    own_dsp: u64,
    tune_code: u64,
    tune_slot: u64,
    level_code: u64,
    level_knob: u64,
    slots: Vec<u64>,
    /// encoder, fader, pan, onoff, tune, play
    templates: [u64; 6],
}
/// The glyph templates a profile carries, in the order of the runtime's `Profile::tpl` (`dk_glyph` names of `[modules.templates]`).
const PROFILE_GLYPHS: [&str; 6] = ["encoder", "fader", "pan", "onoff", "tune", "play"];
/// The most knob positions a profile carries (`dk_machine` params per machine).
const PROFILE_SLOTS_MAX: usize = 8;

fn profile_facts(short: &str, model: Option<&crate::typed::DeviceModel>) -> Result<ProfileF> {
    let Some(m) = model else {
        return Ok(ProfileF::default());
    };
    let kn = &m.knobs;
    let to_machine = kn.to == "machine";
    if m.param_slots.len() > PROFILE_SLOTS_MAX {
        return Err(err!(
            "{short}: [adapter.machine] param_slots lists {}, a profile carries at most {PROFILE_SLOTS_MAX}",
            m.param_slots.len()
        ));
    }
    let mut templates = [0u64; 6];
    for (t, g) in templates.iter_mut().zip(PROFILE_GLYPHS) {
        *t = *m
            .templates
            .get(g)
            .ok_or_else(|| err!("{short}: [modules.templates] has no `{g}`"))? as u64;
    }
    Ok(ProfileF {
        clone_of: kn.clone_of as u64,
        level: kn.level_default as u64,
        own_dsp: u64::from(kn.own_dsp),
        tune_code: if to_machine {
            kn.tune.unwrap_or(0) as u64
        } else {
            0
        },
        tune_slot: m.tune_slot.unwrap_or(0) as u64,
        level_code: if to_machine {
            kn.level.unwrap_or(0) as u64
        } else {
            0
        },
        level_knob: kn.level_knob.unwrap_or(0) as u64,
        slots: m.param_slots.iter().map(|&x| x as u64).collect(),
        templates,
    })
}
/// A device's effect page facts (all 0 / empty on a device without run-time effect pages).
#[derive(Debug, Default)]
struct FxF {
    kit_ptr: u64,
    array_off: u64,
    page_first: u64,
    value_indices: Vec<u64>,
    template_percent: u64,
    template_bipolar: u64,
    template_mix: u64,
    store_off: u64,
    mark_index: u64,
    tail_off: u64,
    track_page: u64,
    sound_off: u64,
    sound_stride: u64,
    sound_words: u64,
    send_template: u64,
    send_indices: Vec<u64>,
}
/// A device's input facts: the backend (index into `INPUT_BACKENDS` plus 1; 0 none) and the panel's key code layout.
#[derive(Debug, Default)]
struct InputF {
    backend: u64,
    linear_channels: u64,
    button_channels: u64,
    encoders: u64,
    /// (code, channel, bit)
    exceptions: Vec<[u64; 3]>,
    /// The ids of the keys core/ui compares (`UI_KEYS` order: yes, yes2, no, right), from `[input.keys]`.
    ui_keys: [u64; 4],
    /// The flag bits of a key event (`[input.key_flags]`: press, func, repeat, release).
    key_flags: [u64; 4],
}
#[derive(Debug)]
struct SamplePoolF {
    slot_floor: u64,
    ddr_base: u64,
    ddr_bytes: u64,
    ring_r_base: u64,
}
#[derive(Debug)]
struct AdapterF {
    voices: u64,
    pitch_hw: u64,
    velocity_hw: u64,
    type_hw: Option<u64>,
    trig_hw: Option<u64>,
    record_active: Option<u64>,
    record_mark: Option<u64>,
    tune_slot: Option<u64>,
    param_slots: Vec<u64>,
    base: u64,
    stride: u64,
    group: u64,
    gap: u64,
    block_cycles: u64,
}
#[derive(Debug)]
struct LimitsF {
    modules: u64,
    machine_rows: u64,
    arena_bytes: u64,
    stage_bytes: u64,
    fx_slots: u64,
    send_slots: u64,
    // Not written into the generated files (the runtime reads the rows from core/dynreg's device object): checked against the maxima only.
    #[allow(dead_code)]
    param_rows: u64,
    #[allow(dead_code)]
    page_rows: u64,
}
#[derive(Debug)]
struct BoardF {
    console_uart_base: u64,
    ddr_cached_base: u64,
    ddr_bytes: u64,
    ddr_uncached_alias: u64,
    esdhc_base: u64,
    dsp_clock_hz: u64,
    dsp_blocks_per_second: u64,
}
#[derive(Debug)]
struct DigikitF {
    id: u64,
    name: String,
    safe_key: u64,
    dsp_area_bytes: u64,
    dsp_tail_off: u64,
    dsp_tail_words: u64,
    keep_base: u64,
    keep_bytes: u64,
    sample_path_max: u64,
    samples_in_store: bool,
    peek: Vec<[u64; 2]>,
    tracks: u64,
    span_clock: u64,
    span_clock_hz: u64,
}
#[derive(Debug)]
struct StoreF {
    first: u64,
    end: u64,
    selection_sectors: u64,
    settings_sectors: u64,
    header_b: u64,
}

impl Facts {
    pub fn from_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Facts> {
        let mut docs = BTreeMap::new();
        let mut devices = BTreeMap::new();
        let texts: Vec<&str> = texts.into_iter().collect();
        let models = crate::typed::Models::from_texts(texts.iter().copied())?;
        for t in texts {
            // a sidecar file (the LED maps) has no [device]
            let table: toml::Table = toml::from_str(t)?;
            if !table.contains_key("device") {
                continue;
            }
            let d: Doc = toml::from_str(t)?;
            let short = d.device.short.clone();
            if devices
                .insert(
                    short.clone(),
                    DeviceTables {
                        tables: table_paths(&table),
                        patch_sets: d.patch_sets.clone(),
                    },
                )
                .is_some()
            {
                return Err(err!("two device files describe {short}"));
            }
            // only a runtime target (a file with [digikit]) has shared constants; an identity-only or patch-only device is skipped
            let Some(k) = d.digikit else {
                continue;
            };
            let (Some(b), Some(l), Some(cap)) = (d.board, d.limits, d.capabilities) else {
                return Err(err!(
                    "{short}: [digikit] needs [board], [limits] and [capabilities] (a runtime target)"
                ));
            };
            let s = d.card.and_then(|c| c.store);
            let dsp = cap.dsp == "sharc";
            let dsp_facts = [
                b.dsp_clock_hz,
                b.dsp_blocks_per_second,
                k.dsp_area_bytes,
                k.dsp_tail_off,
                k.dsp_tail_words,
            ];
            if dsp && dsp_facts.iter().any(|f| f.is_none()) {
                return Err(err!(
                    "{short}: [capabilities] dsp = \"sharc\" needs [board] dsp_clock_hz and dsp_blocks_per_second, and [digikit] dsp_area_bytes, dsp_tail_off and dsp_tail_words"
                ));
            }
            if !dsp && dsp_facts.iter().any(|f| f.is_some()) {
                return Err(err!(
                    "{short}: [capabilities] dsp = \"none\" forbids [board] dsp_* and [digikit] dsp_* facts"
                ));
            }
            let cap_bits = (u64::from(dsp) * DEV_HAS_DSP)
                | (u64::from(s.is_some()) * DEV_HAS_STORE)
                | (u64::from(b.esdhc_base.is_some()) * DEV_HAS_SDHC)
                | (u64::from(k.safe_key.is_some()) * DEV_HAS_SAFE_KEY);
            let samples_in_store = match cap.card.as_str() {
                "slots" => true,
                "path-fs" => false,
                other => {
                    return Err(err!(
                        "{short}: [capabilities] card = {other:?}: a digikit device is \"slots\" (samples in the store) or \"path-fs\" (the firmware's folder filesystem)"
                    ));
                }
            };
            if k.sample_path_max == 0 || k.sample_path_max > 254 {
                return Err(err!("{short}: [digikit] sample_path_max is 1 to 254"));
            }
            if k.peek.len() > PEEK_MAX {
                return Err(err!(
                    "{short}: [digikit] peek lists at most {PEEK_MAX} ranges"
                ));
            }
            if k.peek
                .iter()
                .any(|r| r[0] >= r[1] || r[0] & 3 != 0 || r[1] & 3 != 0)
            {
                return Err(err!(
                    "{short}: [digikit] peek ranges are [lo, hi) with lo < hi, both multiples of 4"
                ));
            }
            if k.keep_base & 0xfff != 0 || k.keep_bytes == 0 || k.keep_bytes & 0xfff != 0 {
                return Err(err!(
                    "{short}: [digikit] keep_base and keep_bytes must be multiples of 4 KiB"
                ));
            }
            if k.name.len() > 3 || k.name.is_empty() || !k.name.is_ascii() {
                return Err(err!(
                    "{short}: [digikit] name must be 1 to 3 ASCII characters"
                ));
            }
            check_limits(
                &short,
                &l,
                k.dsp_area_bytes.unwrap_or(0),
                &crate::sdkgen::limits_max(),
            )?;
            let keys = key_facts(&short, d.input.as_ref().map(|i| &i.keys))?;
            let input = input_facts(&short, d.input, d.panel, &keys)?;
            let fx = fx_facts(&short, d.fx, l.fx_slots, l.send_slots)?;
            let (adapter_machine, sample_pool) = match d.adapter {
                Some(a) => (a.machine, a.sample_pool),
                None => (None, None),
            };
            let sample_pool = sample_pool_facts(&short, sample_pool)?;
            let adapter =
                adapter_facts(&short, adapter_machine, d.modules.and_then(|m| m.frame), &b)?;
            let dsp_headroom = dsp_headroom(&short, &b, adapter.as_ref().map_or(0, |a| a.voices));
            if docs
                .insert(
                    short.clone(),
                    Entry {
                        board: BoardF {
                            console_uart_base: b.console_uart_base,
                            ddr_cached_base: b.ddr_cached_base,
                            ddr_bytes: b.ddr_bytes,
                            ddr_uncached_alias: b.ddr_uncached_alias,
                            esdhc_base: b.esdhc_base.unwrap_or(0),
                            dsp_clock_hz: b.dsp_clock_hz.unwrap_or(0),
                            dsp_blocks_per_second: b.dsp_blocks_per_second.unwrap_or(0),
                        },
                        digikit: DigikitF {
                            id: k.id,
                            name: k.name,
                            safe_key: k.safe_key.unwrap_or(0),
                            dsp_area_bytes: k.dsp_area_bytes.unwrap_or(0),
                            dsp_tail_off: k.dsp_tail_off.unwrap_or(0),
                            dsp_tail_words: k.dsp_tail_words.unwrap_or(0),
                            keep_base: k.keep_base,
                            keep_bytes: k.keep_bytes,
                            sample_path_max: k.sample_path_max,
                            samples_in_store,
                            peek: k.peek,
                            tracks: k.tracks,
                            span_clock: k.span_clock,
                            span_clock_hz: k.span_clock_hz,
                        },
                        store: s.map(|s| StoreF {
                            first: s.first_sector,
                            end: s.end_sector,
                            selection_sectors: s.selection_sectors,
                            settings_sectors: s.settings_sectors,
                            header_b: s.header_b_sector,
                        }),
                        caps: cap_bits,
                        limits: LimitsF {
                            modules: l.modules,
                            machine_rows: l.machine_rows,
                            arena_bytes: l.arena_bytes,
                            stage_bytes: l.stage_bytes,
                            fx_slots: l.fx_slots,
                            send_slots: l.send_slots,
                            param_rows: l.param_rows,
                            page_rows: l.page_rows,
                        },
                        adapter,
                        sample_pool,
                        input,
                        keys,
                        fx,
                        profile: profile_facts(&short, models.by_device.get(&short))?,
                        dsp_headroom,
                    },
                )
                .is_some()
            {
                return Err(err!("two device files describe {short}"));
            }
        }
        Ok(Facts { docs, devices })
    }

    /// The logical key names in the order of their numbers: `[keys] order` of sdk/sdk.toml (`DK_KEY_NONE` is 0, the names follow from 1). A device file may use only
    /// these names; the list is the ABI and never renumbers.
    pub fn key_names(&self) -> Result<Vec<String>> {
        let names = crate::sdkgen::key_order();
        for (dev, e) in &self.docs {
            if let Some(n) = e.keys.keys().find(|n| !names.contains(n)) {
                return Err(err!(
                    "{dev}: [input.keys] {n}: not a name of [keys] order in sdk/sdk.toml (add it at the end of that list)"
                ));
            }
        }
        let slots = key_slots()?;
        if names.len() >= slots {
            return Err(err!(
                "[keys] order of sdk/sdk.toml lists {} names, the tables hold {} (DEV_KEYS of core/runtime/abi.toml)",
                names.len(),
                slots - 1
            ));
        }
        Ok(names)
    }

    /// The panel code of each logical key of `dev`, indexed by the key's number (`DEV_KEYS` entries; a key the device lacks is 0).
    fn key_codes(&self, dev: &str) -> Result<Vec<u64>> {
        let mut out = vec![0; key_slots()?];
        if let Some(e) = self.docs.get(dev) {
            for (i, n) in self.key_names()?.iter().enumerate() {
                out[i + 1] = e.keys.get(n).copied().unwrap_or(0);
            }
        }
        Ok(out)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_dir(dir: &std::path::Path) -> Result<Facts> {
        let mut texts = vec![];
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut paths: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.extension().is_some_and(|e| e == "toml") {
                    texts.push(
                        std::fs::read_to_string(&p).map_err(|e| err!("{}: {e}", p.display()))?,
                    );
                }
            }
        }
        Facts::from_texts(texts.iter().map(|s| s.as_str()))
    }
}

/// A device's capacity limits against the contract maxima of `sdk/sdk.toml` `[limits.max]`: a value above its maximum is refused, and no
/// limit is 0 where the runtime needs at least one (a device without page rows says `page_rows = 0`).
fn check_limits(
    short: &str,
    l: &Limits,
    dsp_area_bytes: u64,
    max: &crate::sdkgen::LimitsMax,
) -> Result<()> {
    for (name, v, m) in [
        ("modules", l.modules, max.modules),
        ("fx_slots", l.fx_slots, max.fx_slots),
        ("send_slots", l.send_slots, max.send_slots),
        ("param_rows", l.param_rows, max.param_rows),
        ("page_rows", l.page_rows, max.page_rows),
        ("machine_rows", l.machine_rows, max.machine_rows),
        ("stage_bytes", l.stage_bytes, max.stage_bytes),
        (
            "[digikit] dsp_area_bytes",
            dsp_area_bytes,
            max.dsp_area_bytes,
        ),
    ] {
        if v > m {
            return Err(err!(
                "{short}: [limits] {name} is {v}, above the contract maximum {m} of sdk/sdk.toml [limits.max]"
            ));
        }
    }
    if l.modules == 0 {
        return Err(err!("{short}: [limits] modules must be at least 1"));
    }
    Ok(())
}

/// Parameter slots a machine can have (the contract's `DK_MAX_PARAMS`).
const ADM_SLOTS: usize = 8;
/// The `ADM_PARAM_SLOT*` value of a parameter without a slot.
const ADM_NO_SLOT: i64 = 255;

/// The input backends core/input has code for. A device file names one in `[input] backend`; a new device with the same panel path needs
/// only that line, a different path adds a backend here and in core/input.
pub const INPUT_BACKENDS: [&str; 1] = ["panel-uart"];
/// The most exceptions to the linear key codes a device file may list.
const PANEL_EXC_MAX: usize = 8;
/// Entries of `[fx] send_indices` the generated files carry (the cells of the send page; the contract maximum of `send_slots`).
const FX_SEND_MAX: usize = 2;
/// Entries of `[fx] value_indices` the generated files carry (`DK_FX_MAX` effects of 8 values).
const FX_VALUES_MAX: usize = 32;

/// Check `[fx]`: a multiple of 8 distinct indices (below 256), enough for the device's effect slots.
fn fx_facts(short: &str, f: Option<FxDoc>, fx_slots: u64, send_slots: u64) -> Result<FxF> {
    let Some(f) = f else {
        return Ok(FxF::default());
    };
    let n = f.value_indices.len();
    if n % 8 != 0 || n > FX_VALUES_MAX || (n as u64) < 8 * fx_slots {
        return Err(err!(
            "{short}: [fx] value_indices must list a multiple of 8 indices, at least 8 for each of the {fx_slots} effect slots, at most {FX_VALUES_MAX}"
        ));
    }
    // the send page: a device that has one lists an index (a Sound array index below 256, distinct) for each of its send slots
    if f.track_page != 0 {
        let mut seen = std::collections::BTreeSet::new();
        if (f.send_indices.len() as u64) < send_slots
            || f.send_indices.len() > FX_SEND_MAX
            || f.send_indices.iter().any(|i| *i > 255 || !seen.insert(*i))
            || f.sound_stride == 0
            || f.sound_off + 16 * f.sound_stride > f.array_off
            || f.send_template > 0xffff
        {
            return Err(err!(
                "{short}: [fx] send_indices must list at least [limits] send_slots ({send_slots}) and at most {FX_SEND_MAX} distinct numbers below 256; send_template a parameter code; the 16 Sound objects (sound_off, sound_stride) end before array_off"
            ));
        }
    } else if !f.send_indices.is_empty() {
        return Err(err!("{short}: [fx] send_indices needs track_page"));
    }
    let mut seen = std::collections::BTreeSet::new();
    if f.value_indices.iter().any(|i| *i > 255 || !seen.insert(*i)) {
        return Err(err!(
            "{short}: [fx] value_indices are distinct numbers below 256"
        ));
    }
    if f.page_first == 0 || f.page_first > 255 || f.array_off & 1 != 0 || f.kit_ptr & 3 != 0 {
        return Err(err!(
            "{short}: [fx] page_first is 1 to 255, array_off is even and kit_ptr a multiple of 4"
        ));
    }
    // the storage: the values, then one ID for each effect row, in halfwords the stock firmware ignores
    if f.store_off & 1 != 0 || f.store_off > 0xffff || (n + n / 8) as u64 > f.store_halfwords {
        return Err(err!(
            "{short}: [fx] store_off is even and store_halfwords holds the {n} values and {} effect IDs ({} halfwords)",
            n / 8,
            n + n / 8
        ));
    }
    if f.tail_off & 1 != 0 || f.tail_off > 0xffff || f.mark_index * 2 + 2 > f.tail_off {
        return Err(err!(
            "{short}: [fx] tail_off is even and behind the halfword of mark_index"
        ));
    }
    if f.mark_index > 255 || f.value_indices.contains(&f.mark_index) {
        return Err(err!(
            "{short}: [fx] mark_index is an array index below 256 that is not one of value_indices"
        ));
    }
    for (name, t) in [
        ("template_percent", f.template_percent),
        ("template_bipolar", f.template_bipolar),
        ("template_mix", f.template_mix),
    ] {
        if t > 0xffff {
            return Err(err!("{short}: [fx] {name} is a parameter code"));
        }
    }
    Ok(FxF {
        kit_ptr: f.kit_ptr,
        array_off: f.array_off,
        page_first: f.page_first,
        value_indices: f.value_indices,
        template_percent: f.template_percent,
        template_bipolar: f.template_bipolar,
        template_mix: f.template_mix,
        store_off: f.store_off,
        mark_index: f.mark_index,
        tail_off: f.tail_off,
        track_page: f.track_page,
        sound_off: f.sound_off,
        sound_stride: f.sound_stride,
        sound_words: f.sound_words,
        send_template: f.send_template,
        send_indices: f.send_indices,
    })
}

/// The logical keys of `[input.keys]`: a name is capital letters, digits and `_` (it becomes `DK_KEY_<name>`), a code is 1 to 255 and names one key.
fn key_facts(short: &str, keys: Option<&BTreeMap<String, u64>>) -> Result<BTreeMap<String, u64>> {
    let Some(keys) = keys else {
        return Ok(BTreeMap::new());
    };
    let mut seen: BTreeMap<u64, &str> = BTreeMap::new();
    for (name, code) in keys {
        let ok = name.starts_with(|c: char| c.is_ascii_uppercase())
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !ok {
            return Err(err!(
                "{short}: [input.keys] {name:?}: a name is capital letters, digits and _, starting with a letter"
            ));
        }
        if !(1..=255).contains(code) {
            return Err(err!(
                "{short}: [input.keys] {name} = {code}: a code is 1 to 255"
            ));
        }
        if let Some(other) = seen.insert(*code, name) {
            return Err(err!(
                "{short}: [input.keys] {other} and {name} have the same code {code}"
            ));
        }
    }
    Ok(keys.clone())
}

/// The entries of the key table: index 0 is `DK_KEY_NONE`, then one for each logical key name (`DEV_KEYS` of core/runtime/abi.toml: the one definition).
fn key_slots() -> Result<usize> {
    abi_consts()?
        .get("DEV_KEYS")
        .map(|v| *v as usize)
        .ok_or_else(|| err!("core/runtime/abi.toml has no DEV_KEYS"))
}

fn input_facts(
    short: &str,
    i: Option<InputDoc>,
    p: Option<PanelDoc>,
    keys: &BTreeMap<String, u64>,
) -> Result<InputF> {
    let Some(i) = i else {
        return Ok(InputF::default());
    };
    let Some(backend) = INPUT_BACKENDS.iter().position(|b| *b == i.backend) else {
        return Err(err!(
            "{short}: [input] backend {:?} is not one of {INPUT_BACKENDS:?}",
            i.backend
        ));
    };
    let Some(p) = p else {
        return Err(err!("{short}: [input] needs a [panel] (the key codes)"));
    };
    if p.exceptions.len() > PANEL_EXC_MAX {
        return Err(err!(
            "{short}: [panel.exceptions] lists at most {PANEL_EXC_MAX} codes"
        ));
    }
    if p.linear_channels > p.button_channels
        || p.button_channels > 16
        || p.encoders == 0
        || p.encoders > 16
    {
        return Err(err!(
            "{short}: [panel] linear_channels <= button_channels <= 16 and encoders 1 to 16"
        ));
    }
    let mut exceptions = vec![];
    for (code, [ch, bit]) in &p.exceptions {
        let code: u64 = code
            .parse()
            .map_err(|_| err!("{short}: [panel.exceptions] key {code:?} is not a code"))?;
        if code <= p.linear_channels * 8
            || *bit > 7
            || *ch < p.linear_channels
            || *ch >= p.button_channels
        {
            return Err(err!(
                "{short}: [panel.exceptions] {code} = [{ch}, {bit}]: a code above the linear ones, on a channel after them and below button_channels, bit 0 to 7"
            ));
        }
        exceptions.push([code, *ch, *bit]);
    }
    let Some(f) = &i.key_flags else {
        return Err(err!(
            "{short}: [input] needs [input.key_flags] (press, func, repeat and release: bit numbers of the key event's flags word)"
        ));
    };
    let key_flags = [f.press, f.func, f.repeat, f.release];
    for (n, (what, bit)) in ["press", "func", "repeat", "release"]
        .iter()
        .zip(key_flags)
        .enumerate()
    {
        if bit >= 32 {
            return Err(err!(
                "{short}: [input.key_flags] {what} = {bit}: a bit number of the 32-bit flags word, 0 to 31"
            ));
        }
        if let Some(other) = key_flags[..n].iter().position(|b| *b == bit) {
            return Err(err!(
                "{short}: [input.key_flags] {what} and {} are both bit {bit}",
                ["press", "func", "repeat", "release"][other]
            ));
        }
    }
    let mut ui_keys = [0u64; 4];
    // A device that names keys names the four that core/ui needs: it compares the ids of the key events with them.
    if !keys.is_empty() {
        for (slot, (_, name)) in UI_KEYS.iter().enumerate() {
            let Some(id) = keys.get(*name) else {
                return Err(err!(
                    "{short}: [input.keys] has no {name}: core/ui needs YES, NO, RIGHT and LEVEL_PUSH (the second YES key)"
                ));
            };
            ui_keys[slot] = *id;
        }
    }
    Ok(InputF {
        ui_keys,
        key_flags,
        backend: backend as u64 + 1,
        linear_channels: p.linear_channels,
        button_channels: p.button_channels,
        encoders: p.encoders,
        exceptions,
    })
}

/// Check `[adapter.machine]` and join it with `[modules.frame]` and the block length of `[board]`.
fn adapter_facts(
    short: &str,
    m: Option<AdapterMachine>,
    f: Option<FrameDoc>,
    b: &Board,
) -> Result<Option<AdapterF>> {
    let Some(m) = m else {
        return Ok(None);
    };
    let one_of = |what: &str, v: &str, ok: &[&str]| -> Result<()> {
        if ok.contains(&v) {
            Ok(())
        } else {
            Err(err!(
                "{short}: [adapter.machine] {what} is {v:?}, expected one of {ok:?}"
            ))
        }
    };
    one_of("voice_unit", &m.voice_unit, &["track", "slot"])?;
    one_of("type_source", &m.type_source, &["cache", "frame"])?;
    one_of("note_on", &m.note_on, &["record", "trig-edge"])?;
    one_of("pair", &m.pair, &["copy-even", "none"])?;
    let need = |what: &str, v: Option<u64>, when: &str| -> Result<()> {
        if v.is_some() {
            Ok(())
        } else {
            Err(err!(
                "{short}: [adapter.machine] {what} is needed for {when}"
            ))
        }
    };
    if m.note_on == "record" {
        need("record_active", m.record_active, "note_on = \"record\"")?;
        need("record_mark", m.record_mark, "note_on = \"record\"")?;
    }
    if m.note_on == "trig-edge" {
        need("trig_hw", m.trig_hw, "note_on = \"trig-edge\"")?;
    }
    if m.type_source == "frame" {
        need("type_hw", m.type_hw, "type_source = \"frame\"")?;
    }
    if m.param_slots.len() > ADM_SLOTS {
        return Err(err!(
            "{short}: [adapter.machine] param_slots lists at most {ADM_SLOTS} slots"
        ));
    }
    if m.param_slots
        .iter()
        .chain(&m.reserved_slots)
        .any(|s| *s >= ADM_SLOTS as u64)
    {
        return Err(err!(
            "{short}: [adapter.machine] param_slots and reserved_slots are knob positions 0 to {}",
            ADM_SLOTS - 1
        ));
    }
    if m.param_slots.iter().any(|s| m.reserved_slots.contains(s)) {
        return Err(err!(
            "{short}: [adapter.machine] a slot is in both param_slots and reserved_slots"
        ));
    }
    if m.voices == 0 || m.voices > 16 {
        return Err(err!("{short}: [adapter.machine] voices is 1 to 16"));
    }
    let f = f.ok_or_else(|| err!("{short}: [adapter.machine] needs [modules.frame]"))?;
    Ok(Some(AdapterF {
        voices: m.voices,
        pitch_hw: m.pitch_hw,
        velocity_hw: m.velocity_hw,
        type_hw: m.type_hw,
        trig_hw: m.trig_hw,
        record_active: m.record_active,
        record_mark: m.record_mark,
        tune_slot: m.tune_slot,
        param_slots: m.param_slots,
        base: f.base,
        stride: f.stride,
        group: f.group.unwrap_or(0),
        gap: f.gap.unwrap_or(0),
        block_cycles: b
            .dsp_clock_hz
            .zip(b.dsp_blocks_per_second)
            .and_then(|(c, n)| block_cycles(c, n))
            .ok_or_else(|| err!("{short}: [adapter.machine] needs [board] dsp_clock_hz and dsp_blocks_per_second"))?,
    }))
}

/// Check `[adapter.sample_pool]`: a stock slot floor or a DDR range of the DSP's DDR, not both.
fn sample_pool_facts(short: &str, p: Option<AdapterSamplePool>) -> Result<Option<SamplePoolF>> {
    let Some(p) = p else {
        return Ok(None);
    };
    if p.slot_floor != 0 && (p.slot_floor > 0x3ff || p.slot_floor & 0x7f == 0) {
        return Err(err!(
            "{short}: [adapter.sample_pool] slot_floor is 0 or a stock slot 1 to 0x3ff that is not a multiple of 0x80"
        ));
    }
    if (p.ddr_base == 0) != (p.ddr_bytes == 0) {
        return Err(err!(
            "{short}: [adapter.sample_pool] ddr_base and ddr_bytes are both 0 or both set"
        ));
    }
    if p.ddr_bytes != 0 {
        if p.ddr_base & 0xfff != 0
            || p.ddr_bytes & 0xfff != 0
            || p.ddr_base < DSP_DDR_BASE
            || p.ddr_base
                .checked_add(p.ddr_bytes)
                .is_none_or(|e| e > 0x1_0000_0000)
        {
            return Err(err!(
                "{short}: [adapter.sample_pool] the DDR range must be 4 KiB aligned, in the DSP's DDR (from 0x80000000) and below 4 GiB"
            ));
        }
    }
    if p.ddr_bytes != 0
        && (p.ddr_base < SAMPLE_POOL_STOCK_END || p.ddr_base + p.ddr_bytes > DSP_DDR_END)
    {
        return Err(err!(
            "{short}: [adapter.sample_pool] the DDR range must start at or above 0x80531000 (the end of the stock writes seen, finding 106) and end at or below 0xa0000000 (the DSP's NW32 alias)"
        ));
    }
    if p.slot_floor != 0 && p.ddr_bytes != 0 {
        return Err(err!(
            "{short}: [adapter.sample_pool] a device borrows the stock pool or owns a range, not both"
        ));
    }
    if p.ring_r_base != 0 {
        let end = p.ring_r_base.checked_add(RING_R_BYTES);
        let overlaps = p.ddr_bytes != 0
            && p.ring_r_base < p.ddr_base + p.ddr_bytes
            && p.ddr_base < p.ring_r_base + RING_R_BYTES;
        if p.ring_r_base & 0xfff != 0
            || p.ring_r_base < SAMPLE_POOL_STOCK_END
            || end.is_none_or(|e| e > DSP_DDR_END)
        {
            return Err(err!(
                "{short}: [adapter.sample_pool] ring_r_base must be 4 KiB aligned, start at or above 0x80531000 (the end of the stock writes seen, finding 106) and end at or below 0xa0000000"
            ));
        }
        if overlaps {
            return Err(err!(
                "{short}: [adapter.sample_pool] the 512 KiB of the R rings at ring_r_base overlap the pool range (ddr_base, ddr_bytes)"
            ));
        }
    }
    Ok(Some(SamplePoolF {
        slot_floor: p.slot_floor,
        ddr_base: p.ddr_base,
        ddr_bytes: p.ddr_bytes,
        ring_r_base: p.ring_r_base,
    }))
}

/// The first byte of the DSP's DDR in its physical byte addresses (docs/findings/99). It is not the ColdFire's DDR of `[board]`.
const DSP_DDR_BASE: u64 = 0x8000_0000;
/// The end of the DSP's DDR that the NW32 alias reaches (physical 0xa0000000).
const DSP_DDR_END: u64 = 0xa000_0000;
/// The R rings of the stereo machines: 16 rings of 32 KiB, like the stock rings (finding 106) but in the DSP's own DDR range.
const RING_R_BYTES: u64 = 16 * 0x8000;
/// The end of the last stock write seen in the DSP's DDR (finding 106 canary map): a range the adapter owns starts at or above it.
const SAMPLE_POOL_STOCK_END: u64 = 0x8053_1000;

/// The `SP_*` constants of one device.
fn sample_pool_constants(p: &SamplePoolF) -> Vec<C> {
    let v = |name, value: u64, note| c(name, value, note);
    vec![
        v(
            "SP_SLOT_FLOOR",
            p.slot_floor,
            "[adapter.sample_pool] slot_floor (lowest stock slot the adapter takes; 0: not used)",
        ),
        v(
            "SP_DDR_BASE",
            p.ddr_base,
            "[adapter.sample_pool] ddr_base (reserved DDR range; 0: not used)",
        ),
        v(
            "SP_DDR_BYTES",
            p.ddr_bytes,
            "[adapter.sample_pool] ddr_bytes",
        ),
    ]
}

/// The DSP's view of the same range: the NW32 word address of its first byte and its size in 4 KiB pages.
fn sample_pool_dsp_constants(p: &SamplePoolF) -> Vec<C> {
    let v = |name, value: u64, note| c(name, value, note);
    vec![
        v(
            "SP_DDR_BASE_NW32",
            0x1000_0000 + (p.ddr_base - DSP_DDR_BASE) / 4,
            "NW32 word address of ddr_base (0x10000000 + (ddr_base - 0x80000000) / 4)",
        ),
        v(
            "SP_DDR_PAGES",
            p.ddr_bytes / 4096,
            "[adapter.sample_pool] ddr_bytes in 4 KiB pages: the page index of a wire tag must be below it",
        ),
    ]
}

const ADM_SLOT_NAMES: [&str; ADM_SLOTS] = [
    "ADM_PARAM_SLOT0",
    "ADM_PARAM_SLOT1",
    "ADM_PARAM_SLOT2",
    "ADM_PARAM_SLOT3",
    "ADM_PARAM_SLOT4",
    "ADM_PARAM_SLOT5",
    "ADM_PARAM_SLOT6",
    "ADM_PARAM_SLOT7",
];

/// The `ADM_*` constants of one device (what the machine adapter's assembly needs; names absent on a device are not written).
fn adapter_constants(a: &AdapterF) -> Vec<C> {
    let p = |name, value: u64, note| c(name, value, note);
    let mut v = vec![
        p("ADM_VOICES", a.voices, "[adapter.machine] voices"),
        p(
            "ADM_PITCH_HW",
            a.pitch_hw,
            "[adapter.machine] pitch_hw (frame halfword of voice 0's pitch, note << 8; voice n is + n)",
        ),
        p(
            "ADM_VELOCITY_HW",
            a.velocity_hw,
            "[adapter.machine] velocity_hw (frame halfword of voice 0's velocity, velocity << 8; voice n is + n)",
        ),
    ];
    if let Some(x) = a.type_hw {
        v.push(p(
            "ADM_TYPE_HW",
            x,
            "[adapter.machine] type_hw (frame halfword of slot 0's machine type; slot n is + n)",
        ));
    }
    if let Some(x) = a.trig_hw {
        v.push(p(
            "ADM_TRIG_HW",
            x,
            "[adapter.machine] trig_hw (frame halfword whose bit n is slot n's trig)",
        ));
    }
    if let Some(x) = a.record_active {
        v.push(p(
            "ADM_RECORD_ACTIVE",
            x,
            "[adapter.machine] record_active (byte offset in the voice record)",
        ));
    }
    if let Some(x) = a.record_mark {
        v.push(p(
            "ADM_RECORD_MARK",
            x,
            "[adapter.machine] record_mark (byte offset in the voice record)",
        ));
    }
    if let Some(x) = a.tune_slot {
        v.push(p(
            "ADM_TUNE_SLOT",
            x,
            "[adapter.machine] tune_slot (knob position of the stock tune, folded into the pitch)",
        ));
    }
    v.extend([
        p(
            "ADM_PARAM_BASE",
            a.base,
            "[modules.frame] base (halfword of knob 0 of voice 0)",
        ),
        p(
            "ADM_PARAM_STRIDE",
            a.stride,
            "[modules.frame] stride (halfwords from one voice's knobs to the next)",
        ),
        p(
            "ADM_PARAM_GROUP",
            a.group,
            "[modules.frame] group (knobs per group; 0: no groups)",
        ),
        p(
            "ADM_PARAM_GAP",
            a.gap,
            "[modules.frame] gap (halfwords between groups)",
        ),
        p(
            "ADM_NPARAM_SLOTS",
            a.param_slots.len() as u64,
            "[adapter.machine] param_slots: number of knob positions for module parameters",
        ),
    ]);
    for (i, name) in ADM_SLOT_NAMES.iter().enumerate() {
        v.push(C {
            name: *name,
            value: a.param_slots.get(i).map_or(ADM_NO_SLOT, |s| *s as i64),
            note: "[adapter.machine] param_slots: knob position of module parameter i (255: none)",
        });
    }
    v.push(p(
        "ADM_BLOCK_CYCLES",
        a.block_cycles,
        "[board] dsp_clock_hz / dsp_blocks_per_second, rounded (DSP_BLOCK_CYCLES)",
    ));
    v
}

/// Ranges of RAM the PEEK command may read, at most (a device lists fewer; the rest are 0, 0).
const PEEK_MAX: usize = 4;

struct C {
    name: &'static str,
    value: i64,
    note: &'static str,
}

fn c(name: &'static str, value: u64, note: &'static str) -> C {
    C {
        name,
        value: value as i64,
        note,
    }
}

/// The run-time machine type range of a device: `dynamic_first ..= last` of `core/machines` types.toml.
pub fn dyn_range(lib: &Library, dev: &str) -> Result<(i64, i64)> {
    let d = lib
        .get("core/machines")
        .and_then(|m| m.alloc.get("machines"))
        .and_then(|a| a.devices.get(dev))
        .ok_or_else(|| err!("{dev}: no allocation table (core/machines types.toml)"))?;
    let first = d
        .dynamic_first
        .ok_or_else(|| err!("{dev}: types.toml has no dynamic_first"))?;
    Ok((first, d.last))
}

/// The number of stock machine types of a device: `stock_types` of `core/machines` types.toml.
pub fn stock_types(lib: &Library, dev: &str) -> Result<i64> {
    lib.get("core/machines")
        .and_then(|m| m.alloc.get("machines"))
        .and_then(|a| a.devices.get(dev))
        .and_then(|d| d.stock_types)
        .ok_or_else(|| err!("{dev}: types.toml has no stock_types"))
}

/// The constants of one device.
fn constants(lib: &Library, dev: &str, e: &Entry) -> Result<Vec<C>> {
    let has_dsp = e.caps & DEV_HAS_DSP != 0;
    // run-time machine types exist only where the DSP does
    let (first, last) = if has_dsp {
        dyn_range(lib, dev)?
    } else {
        (0, -1)
    };
    let stock = if has_dsp { stock_types(lib, dev)? } else { 0 };
    let (store_first, store_sectors) = match &e.store {
        Some(s) => {
            if s.selection_sectors != 2 {
                return Err(err!(
                    "{dev}: [card.store] selection_sectors must be 2 (the runtime writes two copies of the selection record)"
                ));
            }
            let set = crate::sdkgen::settings();
            if s.settings_sectors != u64::from(set.copies) * u64::from(set.sectors) {
                return Err(err!(
                    "{dev}: [card.store] settings_sectors must be {} (sdk/sdk.toml [settings]: {} copies of {} sectors)",
                    u64::from(set.copies) * u64::from(set.sectors),
                    set.copies,
                    set.sectors
                ));
            }
            if s.header_b != s.end - s.selection_sectors - s.settings_sectors - 1 {
                return Err(err!(
                    "{dev}: [card.store] header_b_sector must be the sector before the settings record, which lies before the selection record"
                ));
            }
            (s.first, s.end - s.first)
        }
        None => (0, 0),
    };
    let b = &e.board;
    let k = &e.digikit;
    let n = (last - first + 1) as u64;
    let abi = abi_consts()?;
    for (what, have, key) in [
        ("arena_bytes", e.limits.arena_bytes, "ARENA_BYTES"),
        ("stage_bytes", e.limits.stage_bytes, "STAGE_BYTES"),
    ] {
        if have != abi[key] {
            return Err(err!(
                "{dev}: [limits] {what} is {have}, but the runtime's static array ({key} of core/runtime/abi.toml) is {}",
                abi[key]
            ));
        }
    }
    if n > abi["MACHINE_ROWS"] {
        return Err(err!(
            "{dev}: core/machines types.toml reserves {n} run-time machine types, more than the runtime's tables hold (MACHINE_ROWS {} of core/runtime/abi.toml)",
            abi["MACHINE_ROWS"]
        ));
    }
    if e.limits.machine_rows != n {
        return Err(err!(
            "{dev}: [limits] machine_rows is {}, but core/machines types.toml reserves {n} run-time machine types (dynamic_first ..= last)",
            e.limits.machine_rows
        ));
    }
    let name_word = k
        .name
        .bytes()
        .chain(std::iter::repeat(0))
        .take(4)
        .fold(0u64, |a, x| (a << 8) | x as u64);
    let mut v = vec![
        c("DEVICE_ID", k.id, "[digikit] id"),
        c(
            "DEVICE_NAME_WORD",
            name_word,
            "[digikit] name, NUL padded, as a big-endian word",
        ),
        c(
            "SAMPLES_IN_STORE",
            k.samples_in_store as u64,
            "[capabilities] card = \"slots\": the samples are files of the digikit store",
        ),
        c(
            "SAMPLE_PATH_MAX",
            k.sample_path_max,
            "[digikit] sample_path_max",
        ),
        c("TRACKS", k.tracks, "[digikit] tracks (0: not given)"),
        c(
            "STORE_FIRST",
            store_first,
            "[card.store] first_sector (0: no store)",
        ),
        c(
            "STORE_SECTORS",
            store_sectors,
            "[card.store] end_sector - first_sector (0: no store)",
        ),
        c(
            "STOCK_MACHINE_TYPES",
            stock as u64,
            "core/machines types.toml stock_types (0: no DSP)",
        ),
        c(
            "DYN_TYPE_FIRST",
            first as u64,
            "core/machines types.toml dynamic_first (0: no DSP)",
        ),
        c(
            "DYN_TYPE_LAST",
            last as u64,
            "core/machines types.toml last (0: no DSP)",
        ),
        c(
            "DYN_TYPES",
            n,
            "run-time machine types; core/dynreg reserves one machine row and core/dyndsp one voice row for each",
        ),
        c(
            "CONSOLE_UART_BASE",
            b.console_uart_base,
            "[board] console_uart_base",
        ),
        c(
            "DDR_CACHED_BASE",
            b.ddr_cached_base,
            "[board] ddr_cached_base",
        ),
        c("DDR_BYTES", b.ddr_bytes, "[board] ddr_bytes"),
        c(
            "DDR_UNCACHED_ALIAS",
            b.ddr_uncached_alias,
            "[board] ddr_uncached_alias",
        ),
        c("ESDHC_BASE", b.esdhc_base, "[board] esdhc_base (0: none)"),
        c(
            "SAFE_KEY_CODE",
            k.safe_key,
            "[digikit] safe_key (a firmware key index; 0: none)",
        ),
        c(
            "DSP_BLOCK_CYCLES",
            block_cycles(b.dsp_clock_hz, b.dsp_blocks_per_second).unwrap_or(0),
            "[board] dsp_clock_hz / dsp_blocks_per_second, rounded (the clock is open, finding 41; 0: no DSP)",
        ),
        c(
            "DKDV_CAPS",
            e.caps,
            "DEV_HAS_* bits (core/runtime/abi.toml), derived from [capabilities] and the tables present",
        ),
        c(
            "DSP_HEADROOM",
            e.dsp_headroom,
            "DSP instructions per block left for modules: the block at 85 percent (projected with the high cycle factor) less the stock firmware's load (devices/cycle-factors.json, dkprof::budget)",
        ),
        c(
            "DSP_VOICES",
            e.adapter.as_ref().map_or(0, |a| a.voices),
            "[adapter.machine] voices: the voices a machine's per-voice DSP cost counts for",
        ),
        c(
            "DL_AREA_BYTES",
            k.dsp_area_bytes,
            "[digikit] dsp_area_bytes",
        ),
        c(
            "DL_AREA_WORDS",
            k.dsp_area_bytes / 4,
            "[digikit] dsp_area_bytes / 4",
        ),
        c(
            "DL_SLOT_BYTES",
            4 * crate::sdkgen::dsp_slot_words() as u64,
            "[dsp.slots] words * 4 of sdk/sdk.toml: the slot block at the start of window 0 (DK_SLOT_BYTES of dk_stub.inc)",
        ),
        c("DL_TAIL_OFF", k.dsp_tail_off, "[digikit] dsp_tail_off"),
        c(
            "DL_TAIL_WORDS",
            k.dsp_tail_words,
            "[digikit] dsp_tail_words",
        ),
        c(
            "KEEP_BASE",
            k.keep_base,
            "[digikit] keep_base (RAM a soft reset is assumed to keep: the crash record)",
        ),
        c("KEEP_BYTES", k.keep_bytes, "[digikit] keep_bytes"),
        c(
            "LIMIT_MODULES",
            e.limits.modules,
            "[limits] modules (module records)",
        ),
        c(
            "LIMIT_MACHINE_ROWS",
            e.limits.machine_rows,
            "[limits] machine_rows (run-time machine and voice rows)",
        ),
        c(
            "LIMIT_ARENA_BYTES",
            e.limits.arena_bytes,
            "[limits] arena_bytes (the module arena)",
        ),
        c(
            "LIMIT_FX_SLOTS",
            e.limits.fx_slots,
            "[limits] fx_slots (effects in the chain: CAPACITY's limit and the free effect rows of core/dynfx)",
        ),
        c(
            "LIMIT_STAGE_BYTES",
            e.limits.stage_bytes,
            "[limits] stage_bytes (the staging buffer of a .dkm file)",
        ),
        c(
            "PEEK_N",
            k.peek.len() as u64,
            "[digikit] peek: number of ranges",
        ),
        c(
            "SPAN_CLOCK",
            k.span_clock,
            "[digikit] span_clock (address of the profiling clock; 0 none)",
        ),
        c(
            "SPAN_CLOCK_HZ",
            k.span_clock_hz,
            "[digikit] span_clock_hz (its frequency)",
        ),
    ];
    let inp = &e.input;
    v.push(c(
        "INPUT_BACKEND",
        inp.backend,
        "[input] backend (1 panel-uart; 0 none)",
    ));
    v.push(c(
        "PANEL_LINEAR",
        inp.linear_channels,
        "[panel] linear_channels (code = channel * 8 + bit + 1)",
    ));
    v.push(c(
        "PANEL_BUTTON_CHANNELS",
        inp.button_channels,
        "[panel] button_channels (the button decoder's tables: a key channel is below it)",
    ));
    v.push(c("PANEL_ENCODERS", inp.encoders, "[panel] encoders"));
    for (slot, (name, key)) in UI_KEYS.iter().enumerate() {
        v.push(C {
            name,
            value: inp.ui_keys[slot] as i64,
            note: UI_KEY_NOTES[slot],
        });
        let _ = key;
    }
    for (name, value, note) in [
        (
            "KEY_FLAG_PRESS",
            inp.key_flags[0],
            "[input.key_flags] press (set on a press, clear on a release)",
        ),
        (
            "KEY_FLAG_FUNC",
            inp.key_flags[1],
            "[input.key_flags] func (set while FUNC is held)",
        ),
        (
            "KEY_FLAG_REPEAT",
            inp.key_flags[2],
            "[input.key_flags] repeat (a held key repeating)",
        ),
        (
            "KEY_FLAG_RELEASE",
            inp.key_flags[3],
            "[input.key_flags] release (set on a release)",
        ),
    ] {
        v.push(c(name, value, note));
    }
    v.push(c(
        "PANEL_EXC_N",
        inp.exceptions.len() as u64,
        "[panel.exceptions]: number of codes",
    ));
    for k in 0..PANEL_EXC_MAX {
        let [code, ch, bit] = inp.exceptions.get(k).copied().unwrap_or([0, 0, 0]);
        v.push(C {
            name: PANEL_EXC_CODE[k],
            value: code as i64,
            note: "[panel.exceptions] code",
        });
        v.push(C {
            name: PANEL_EXC_CH[k],
            value: ch as i64,
            note: "[panel.exceptions] channel of the code",
        });
        v.push(C {
            name: PANEL_EXC_BIT[k],
            value: bit as i64,
            note: "[panel.exceptions] bit of the code",
        });
    }
    let fx = &e.fx;
    for (name, value, note) in [
        (
            "FX_KIT_PTR",
            fx.kit_ptr,
            "[fx] kit_ptr (the firmware's pointer to the live kit object)",
        ),
        (
            "FX_ARRAY_OFF",
            fx.array_off,
            "[fx] array_off (the kit's array of effect values, bytes from the kit object)",
        ),
        (
            "FX_PAGE_FIRST",
            fx.page_first,
            "[fx] page_first (the page number of the first effect page)",
        ),
        (
            "FX_TPL_PERCENT",
            fx.template_percent,
            "[fx] template_percent (a 0 .. 127 knob)",
        ),
        (
            "FX_TPL_BIPOLAR",
            fx.template_bipolar,
            "[fx] template_bipolar (a -64 .. +63 knob)",
        ),
        (
            "FX_TPL_MIX",
            fx.template_mix,
            "[fx] template_mix (the level slot: MIX)",
        ),
        (
            "FX_STORE_OFF",
            fx.store_off,
            "[fx] store_off (the first spare halfword of the kit storage's effect value record, bytes from its start)",
        ),
        (
            "FX_MARK_INDEX",
            fx.mark_index,
            "[fx] mark_index (the array index that holds the mark: an effect row has its values)",
        ),
        (
            "FX_TAIL_OFF",
            fx.tail_off,
            "[fx] tail_off (the u32 after the array's halfwords: the loader and saver copy it, bytes from the array)",
        ),
        (
            "FX_VALUES_N",
            fx.value_indices.len() as u64,
            "[fx] value_indices: number of indices",
        ),
        (
            "FX_TRACK_PAGE",
            fx.track_page,
            "[fx] track_page (the stock page the send page copies its title and level slot from; 0: no send page)",
        ),
        (
            "FX_SOUND_OFF",
            fx.sound_off,
            "[fx] sound_off (the first Sound object in the kit object, bytes from the kit object)",
        ),
        (
            "FX_SOUND_STRIDE",
            fx.sound_stride,
            "[fx] sound_stride (bytes between the 16 Sound objects)",
        ),
        (
            "FX_SOUND_WORDS",
            fx.sound_words,
            "[fx] sound_words (s16 parameter words in a Sound object's array)",
        ),
        (
            "FX_SEND_TEMPLATE",
            fx.send_template,
            "[fx] send_template (the stock send row whose owner and key words a send knob copies)",
        ),
        (
            "LIMIT_SEND_SLOTS",
            if fx.track_page == 0 {
                0
            } else {
                e.limits.send_slots
            },
            "[limits] send_slots (effects on a send bus: the cells of the send page; 0 on a device without one)",
        ),
    ] {
        v.push(c(name, value, note));
    }
    for (i, name) in FX_VALUE.iter().enumerate() {
        v.push(c(
            name,
            fx.value_indices.get(i).copied().unwrap_or(0),
            "[fx] value_indices (an effect value index that no stock parameter uses)",
        ));
    }
    for (i, name) in FX_SEND_IDX.iter().enumerate() {
        v.push(c(
            name,
            fx.send_indices.get(i).copied().unwrap_or(0),
            "[fx] send_indices (a Sound array index the send page's knob uses)",
        ));
    }
    let pf = &e.profile;
    for (name, value, note) in [
        ("PROFILE_CLONE_OF", pf.clone_of, "[modules.knobs] clone_of"),
        ("PROFILE_LEVEL", pf.level, "[modules.knobs] level_default"),
        (
            "PROFILE_OWN_DSP",
            pf.own_dsp,
            "[modules.knobs] own_dsp (0 or 1)",
        ),
        (
            "PROFILE_TUNE_CODE",
            pf.tune_code,
            "[modules.knobs] tune on a machine-row device (0: none)",
        ),
        (
            "PROFILE_TUNE_SLOT",
            pf.tune_slot,
            "[adapter.machine] tune_slot",
        ),
        (
            "PROFILE_LEVEL_CODE",
            pf.level_code,
            "[modules.knobs] level on a machine-row device (0: none)",
        ),
        (
            "PROFILE_LEVEL_KNOB",
            pf.level_knob,
            "[modules.knobs] level_knob",
        ),
        (
            "PROFILE_SLOT_COUNT",
            pf.slots.len() as u64,
            "[adapter.machine] param_slots, the count",
        ),
    ] {
        v.push(c(name, value, note));
    }
    for (i, name) in PROFILE_SLOT.iter().enumerate() {
        v.push(c(
            name,
            pf.slots.get(i).copied().unwrap_or(0),
            "[adapter.machine] param_slots (knob position of the machine's word i)",
        ));
    }
    for (name, value) in PROFILE_TPL.iter().zip(pf.templates) {
        v.push(c(
            name,
            value,
            "[modules.templates] (stock parameter row of a glyph)",
        ));
    }
    for i in 0..PEEK_MAX {
        let r = k.peek.get(i).copied().unwrap_or([0, 0]);
        v.push(C {
            name: PEEK_LO[i],
            value: r[0] as i64,
            note: "[digikit] peek range start (RAM the PEEK command may read)",
        });
        v.push(C {
            name: PEEK_HI[i],
            value: r[1] as i64,
            note: "[digikit] peek range end (exclusive)",
        });
    }
    Ok(v)
}

const PANEL_EXC_CODE: [&str; PANEL_EXC_MAX] = [
    "PANEL_EXC_CODE_0",
    "PANEL_EXC_CODE_1",
    "PANEL_EXC_CODE_2",
    "PANEL_EXC_CODE_3",
    "PANEL_EXC_CODE_4",
    "PANEL_EXC_CODE_5",
    "PANEL_EXC_CODE_6",
    "PANEL_EXC_CODE_7",
];
const PANEL_EXC_CH: [&str; PANEL_EXC_MAX] = [
    "PANEL_EXC_CH_0",
    "PANEL_EXC_CH_1",
    "PANEL_EXC_CH_2",
    "PANEL_EXC_CH_3",
    "PANEL_EXC_CH_4",
    "PANEL_EXC_CH_5",
    "PANEL_EXC_CH_6",
    "PANEL_EXC_CH_7",
];
const PANEL_EXC_BIT: [&str; PANEL_EXC_MAX] = [
    "PANEL_EXC_BIT_0",
    "PANEL_EXC_BIT_1",
    "PANEL_EXC_BIT_2",
    "PANEL_EXC_BIT_3",
    "PANEL_EXC_BIT_4",
    "PANEL_EXC_BIT_5",
    "PANEL_EXC_BIT_6",
    "PANEL_EXC_BIT_7",
];
const FX_SEND_IDX: [&str; FX_SEND_MAX] = ["FX_SEND_IDX_0", "FX_SEND_IDX_1"];
const FX_VALUE: [&str; FX_VALUES_MAX] = [
    "FX_VALUE_0",
    "FX_VALUE_1",
    "FX_VALUE_2",
    "FX_VALUE_3",
    "FX_VALUE_4",
    "FX_VALUE_5",
    "FX_VALUE_6",
    "FX_VALUE_7",
    "FX_VALUE_8",
    "FX_VALUE_9",
    "FX_VALUE_10",
    "FX_VALUE_11",
    "FX_VALUE_12",
    "FX_VALUE_13",
    "FX_VALUE_14",
    "FX_VALUE_15",
    "FX_VALUE_16",
    "FX_VALUE_17",
    "FX_VALUE_18",
    "FX_VALUE_19",
    "FX_VALUE_20",
    "FX_VALUE_21",
    "FX_VALUE_22",
    "FX_VALUE_23",
    "FX_VALUE_24",
    "FX_VALUE_25",
    "FX_VALUE_26",
    "FX_VALUE_27",
    "FX_VALUE_28",
    "FX_VALUE_29",
    "FX_VALUE_30",
    "FX_VALUE_31",
];
const PROFILE_SLOT: [&str; PROFILE_SLOTS_MAX] = [
    "PROFILE_SLOT_0",
    "PROFILE_SLOT_1",
    "PROFILE_SLOT_2",
    "PROFILE_SLOT_3",
    "PROFILE_SLOT_4",
    "PROFILE_SLOT_5",
    "PROFILE_SLOT_6",
    "PROFILE_SLOT_7",
];
const PROFILE_TPL: [&str; 6] = [
    "PROFILE_TPL_ENCODER",
    "PROFILE_TPL_FADER",
    "PROFILE_TPL_PAN",
    "PROFILE_TPL_ONOFF",
    "PROFILE_TPL_TUNE",
    "PROFILE_TPL_PLAY",
];
const PEEK_LO: [&str; PEEK_MAX] = ["PEEK_LO_0", "PEEK_LO_1", "PEEK_LO_2", "PEEK_LO_3"];
const PEEK_HI: [&str; PEEK_MAX] = ["PEEK_HI_0", "PEEK_HI_1", "PEEK_HI_2", "PEEK_HI_3"];

/// `DEV_HAS_*` of `core/runtime/abi.toml`.
const DEV_HAS_DSP: u64 = 1;
const DEV_HAS_STORE: u64 = 2;
const DEV_HAS_SDHC: u64 = 4;
const DEV_HAS_SAFE_KEY: u64 = 8;

/// DSP cycles in one block: `None` when a number is missing.
fn block_cycles(clock_hz: u64, blocks_per_second: u64) -> Option<u64> {
    (blocks_per_second != 0).then(|| (clock_hz + blocks_per_second / 2) / blocks_per_second)
}

/// `sdk/include/dk/gen/keys.h`: the logical key names of the device files (`[input.keys]`) as `dk_key` numbers. A module names a key with these and holds no
/// panel code; the profile and `dkrt_dev` map the number to the device's code. `DK_KEY_NONE` is 0 and a name that a device lacks maps to 0 there.
pub fn keys_h(facts: &Facts) -> Result<String> {
    let names = facts.key_names()?;
    let mut s = format!(
        "/* {NOTE_SDK} */\n/* sdk/include/dk/gen/keys.h -- the logical keys: panel key names of the device files ([input.keys]), numbered in the order below. */\n#ifndef DK_GEN_KEYS_H\n#define DK_GEN_KEYS_H\n\n#include <stdint.h>\n\n/* A logical key: DK_KEY_NONE or a DK_KEY_* number. */\ntypedef uint32_t dk_key;\n\n#define DK_KEY_NONE 0\n"
    );
    for (i, n) in names.iter().enumerate() {
        let _ = writeln!(s, "#define DK_KEY_{n} {}", i + 1);
    }
    let _ = writeln!(
        s,
        "#define DK_KEY_COUNT {} /* the names above, without DK_KEY_NONE */",
        names.len()
    );
    for (macro_, stem) in [("TRIG", "TRIG"), ("ENC", "ENC")] {
        let first = format!("{stem}1");
        if let Some(i) = names.iter().position(|n| *n == first) {
            let run = (1..)
                .take_while(|k| {
                    names
                        .get(i + k - 1)
                        .is_some_and(|n| *n == format!("{stem}{k}"))
                })
                .count();
            let _ = writeln!(
                s,
                "/* DK_KEY_{macro_}(n), n = 1 to {run}: the numbered keys are consecutive. */\n#define DK_KEY_{macro_}(n) ((dk_key)(DK_KEY_{stem}1 + (n) - 1))"
            );
        }
    }
    s.push_str("\n#endif /* DK_GEN_KEYS_H */\n");
    Ok(s)
}

const NOTE_SDK: &str =
    "Generated by `patchkit gen` from the device files ([input.keys]). Do not edit.";

/// The runtime's ABI file: the constants and the `DkrtDevice` struct that this generator writes the device object from.
const ABI_TEXT: &str = include_str!("../../../modules/core/runtime/abi.toml");

/// The integer constants of `core/runtime/abi.toml` by name.
fn abi_consts() -> Result<BTreeMap<String, u64>> {
    let t: toml::Table = toml::from_str(ABI_TEXT)?;
    let mut out = BTreeMap::new();
    for c in t["const"].as_array().into_iter().flatten() {
        if let (Some(n), Some(v)) = (c["name"].as_str(), c["value"].as_integer()) {
            out.insert(n.to_string(), v as u64);
        }
    }
    Ok(out)
}

/// `DkrtDevice`: its field names in order with their length in words (1, or the length of an array field `u32 name[N]`; every field is a u32), from
/// `core/runtime/abi.toml`.
fn dkdv_fields() -> Result<Vec<(String, usize)>> {
    let abi = abi_consts()?;
    let t: toml::Table = toml::from_str(ABI_TEXT)?;
    let st = t["struct"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["name"].as_str() == Some("DkrtDevice"))
        .ok_or_else(|| err!("core/runtime/abi.toml has no struct DkrtDevice"))?;
    let mut out = vec![];
    for f in st["fields"].as_array().into_iter().flatten() {
        let decl = f.as_str().unwrap_or("").split(" // ").next().unwrap_or("");
        let mut w = decl.split_whitespace();
        if w.next() != Some("u32") {
            return Err(err!("DkrtDevice: every field must be a u32: {decl:?}"));
        }
        let name = w.next().unwrap_or("");
        let (name, len) = match name.split_once('[') {
            Some((n, rest)) => {
                let l = rest.trim_end_matches(']');
                let len = l
                    .parse::<usize>()
                    .ok()
                    .or_else(|| abi.get(l).map(|v| *v as usize))
                    .ok_or_else(|| {
                        err!(
                            "DkrtDevice: field {n}: array length {l:?} is not a number or constant"
                        )
                    })?;
                (n, len)
            }
            None => (name, 1),
        };
        out.push((name.to_string(), len));
    }
    Ok(out)
}

/// The constant that fills a `DkrtDevice` field (the four header words are not in this table).
const DKDV_VALUE: [(&str, &str); 16] = [
    ("console_uart_base", "CONSOLE_UART_BASE"),
    ("ddr_uncached_alias", "DDR_UNCACHED_ALIAS"),
    ("ddr_cached_base", "DDR_CACHED_BASE"),
    ("ddr_bytes", "DDR_BYTES"),
    ("esdhc_base", "ESDHC_BASE"),
    ("safe_key", "SAFE_KEY_CODE"),
    ("keep_base", "KEEP_BASE"),
    ("keep_bytes", "KEEP_BYTES"),
    ("dsp_block_cycles", "DSP_BLOCK_CYCLES"),
    ("dyn_type_first", "DYN_TYPE_FIRST"),
    ("dyn_types", "DYN_TYPES"),
    ("tracks", "TRACKS"),
    ("key_flag_press", "KEY_FLAG_PRESS"),
    ("key_flag_func", "KEY_FLAG_FUNC"),
    ("key_flag_repeat", "KEY_FLAG_REPEAT"),
    ("key_flag_release", "KEY_FLAG_RELEASE"),
];

/// The array field of `DkrtDevice` that holds the panel code of each logical key (index = the key's number).
const DKDV_KEY_CODES: &str = "key_codes";

/// The device object of one device (`gas-dkdv`): `dkrt_dev`, the struct `DkrtDevice` as `.long` words.
fn gas_dkdv(module: &str, dev: &str, cs: &[C], key_codes: &[u64]) -> Result<String> {
    let abi = abi_consts()?;
    let fields = dkdv_fields()?;
    let value = |name: &str| {
        cs.iter()
            .find(|k| k.name == name)
            .map(|k| k.value)
            .ok_or_else(|| err!("{module}: no device constant {name}"))
    };
    let mut s = format!(
        "/* {module} on {dev}: {NOTE}. The struct DkrtDevice of core/runtime/abi.toml. */\n"
    );
    s.push_str("    .balign 4\n    .globl dkrt_dev\ndkrt_dev:\n");
    for (f, len) in &fields {
        if f == DKDV_KEY_CODES {
            if *len != key_codes.len() {
                return Err(err!(
                    "{module}: DkrtDevice.{f} has {len} words, the key table has {}",
                    key_codes.len()
                ));
            }
            for (i, code) in key_codes.iter().enumerate() {
                let _ = writeln!(
                    s,
                    "    .long {}    /* {f}[{i}]: the panel code of logical key {i} (0: none) */",
                    hex(*code as i64)
                );
            }
            continue;
        }
        let (v, note) = match f.as_str() {
            "magic" => (abi["DEV_MAGIC"] as i64, "DEV_MAGIC".to_string()),
            "version" => (abi["DEV_VERSION"] as i64, "DEV_VERSION".to_string()),
            "size" => (
                (fields.iter().map(|f| f.1).sum::<usize>() * 4) as i64,
                "the bytes of the struct".to_string(),
            ),
            "caps" => (value("DKDV_CAPS")?, "DEV_HAS_* bits".to_string()),
            f => {
                let (_, c) = DKDV_VALUE.iter().find(|(n, _)| *n == f).ok_or_else(|| {
                    err!("{module}: DkrtDevice.{f} has no source in devgen DKDV_VALUE")
                })?;
                (value(c)?, (*c).to_string())
            }
        };
        let _ = writeln!(s, "    .long {}    /* {f}: {note} */", hex(v));
    }
    Ok(s)
}

/// The field offsets of `DkrtDevice` as `.equ DKDV_<FIELD>, n` (`gas-dkdv-offsets`), for assembly that reads `dkrt_dev`.
fn gas_dkdv_offsets(module: &str) -> String {
    let mut s = format!(
        "/* {module}: {NOTE}. Byte offsets of the fields of struct DkrtDevice (core/runtime/abi.toml). */\n"
    );
    let mut word = 0;
    for (f, len) in dkdv_fields().unwrap_or_default() {
        let _ = writeln!(s, "    .equ DKDV_{}, {}", f.to_uppercase(), word * 4);
        word += len;
    }
    s
}

fn hex(v: i64) -> String {
    format!("{v:#x}")
}

const NOTE: &str =
    "generated by `patchkit gen` from the device files and core/machines types.toml; do not edit";

/// `DeviceProfile` constants of every device (the type is `dkrt_abi::platform::DeviceProfile`): what the real platform reads from the device object
/// (`device.s`, `dkrt_dev`) and what the host tests' `FakePlatform` and a service (through the device id) take their facts from, so they cannot differ.
fn rust_profiles(module: &str, lib: &Library, facts: &Facts) -> Result<String> {
    let mut s = format!(
        "// {module}: {NOTE}.\n// One DeviceProfile for each device file, from the same facts as the device objects' words.\n\nuse crate::platform::{{DeviceProfile, KeyFlagBits}};\n"
    );
    let mut names = vec![];
    for (dev, e) in &facts.docs {
        let cs = constants(lib, dev, e)?;
        let v = |name: &str| -> Result<u64> {
            cs.iter()
                .find(|k| k.name == name)
                .map(|k| k.value as u64)
                .ok_or_else(|| err!("{module}: no device constant {name}"))
        };
        let k = &e.digikit;
        let up = dev.to_uppercase();
        let key_codes = {
            let all = facts.key_codes(dev)?;
            // filled to 100 columns, as rustfmt lays out a long list of numbers
            let mut rows = String::from("[\n");
            let mut line = String::new();
            for c in all {
                let cell = format!("{c},");
                if !line.is_empty() && 8 + line.len() + 1 + cell.len() > 99 {
                    let _ = writeln!(rows, "        {line}");
                    line.clear();
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&cell);
            }
            let _ = writeln!(rows, "        {line}");
            rows.push_str("    ]");
            rows
        };
        let safe_key = if e.caps & DEV_HAS_SAFE_KEY != 0 {
            format!("Some({})", k.safe_key)
        } else {
            "None".to_string()
        };
        let _ = writeln!(
            s,
            "\n/// `{dev}`: the device files' facts: [digikit] id, name, sample_path_max, keep_*, safe_key, [board], [capabilities], [card.store] and the DSP headroom and voices.\npub const {up}: DeviceProfile = DeviceProfile {{\n    id: {},\n    name: {},\n    samples_in_store: {},\n    sample_path_max: {},\n    dsp_headroom: {},\n    dsp_voices: {},\n    has_dsp: {},\n    has_store: {},\n    has_sdhc: {},\n    safe_key: {safe_key},\n    ddr_base: {},\n    ddr_bytes: {},\n    keep_base: {},\n    keep_bytes: {},\n    esdhc_base: {},\n    dsp_block_cycles: {},\n    stock_machine_types: {},\n    dyn_type_first: {},\n    dyn_types: {},\n    tracks: {},\n    key_codes: {key_codes},\n    key_flags: KeyFlagBits {{\n        press: {},\n        func: {},\n        repeat: {},\n        release: {},\n    }},\n}};",
            v("DEVICE_ID")?,
            hex(v("DEVICE_NAME_WORD")? as i64),
            e.digikit.samples_in_store,
            v("SAMPLE_PATH_MAX")?,
            v("DSP_HEADROOM")?,
            v("DSP_VOICES")?,
            e.caps & DEV_HAS_DSP != 0,
            e.caps & DEV_HAS_STORE != 0,
            e.caps & DEV_HAS_SDHC != 0,
            hex(v("DDR_CACHED_BASE")? as i64),
            hex(v("DDR_BYTES")? as i64),
            hex(v("KEEP_BASE")? as i64),
            hex(v("KEEP_BYTES")? as i64),
            hex(v("ESDHC_BASE")? as i64),
            v("DSP_BLOCK_CYCLES")?,
            v("STOCK_MACHINE_TYPES")?,
            v("DYN_TYPE_FIRST")?,
            v("DYN_TYPES")?,
            v("TRACKS")?,
            v("KEY_FLAG_PRESS")?,
            v("KEY_FLAG_FUNC")?,
            v("KEY_FLAG_REPEAT")?,
            v("KEY_FLAG_RELEASE")?,
        );
        names.push(up);
    }
    let _ = writeln!(
        s,
        "\n/// Every device file's profile.\npub const ALL: [DeviceProfile; {}] = [{}];",
        names.len(),
        names.join(", ")
    );
    Ok(s)
}

/// A C header of the contract maxima of `sdk/sdk.toml` `[limits.max]` that a ColdFire object shared by all devices checks against: the
/// DSP area's size is a device value, but the link's word offset (17 bits) is the contract's.
fn c_shared(module: &str) -> String {
    let max = crate::sdkgen::limits_max();
    format!(
        "/* {module}: {NOTE}. Contract maxima of sdk/sdk.toml [limits.max]. */\n\
         #define DL_AREA_MAX_BYTES {:#x}u /* [limits.max] dsp_area_bytes: the largest area the 17-bit word offset of a chunk reaches */\n\
         #define DL_AREA_MAX_WORDS {:#x}u /* the same in words */\n\
         #define DL_SLOT_WORDS {}u /* [dsp.slots] words: the slot block at the start of window 0 (DK_SLOTS of dk_stub.inc) */\n",
        max.dsp_area_bytes,
        max.dsp_area_bytes / 4,
        crate::sdkgen::dsp_slot_words()
    )
}

/// The `[[repack]]` entries of a module as C macros (`REPACK_<ROLE>_DEST`, `_STAGE_END`, `_LEN` and `_SHA256`, an initializer list of 32 bytes): the layout
/// that patchkit checks the stored stream against is the one the module's code uses, from one source.
fn c_repack(module: &str, repacks: &[crate::module::RepackDef]) -> Result<String> {
    if repacks.is_empty() {
        return Err(err!("{module}: a `c-repack` gen needs a [[repack]]"));
    }
    let mut s = format!("/* {module}: {NOTE}. The [[repack]] of module.toml. */\n");
    for r in repacks {
        let role = r.role.to_ascii_uppercase();
        let sha = crate::error::unhex(&r.sha256)
            .map_err(|_| err!("{module}: [[repack]] sha256 is not hex"))?;
        let bytes: Vec<String> = sha.iter().map(|b| format!("{b:#04x}")).collect();
        let _ = writeln!(s, "#define REPACK_{role}_DEST {:#x}u", r.dest);
        let _ = writeln!(s, "#define REPACK_{role}_STAGE_END {:#x}u", r.stage_end);
        let _ = writeln!(s, "#define REPACK_{role}_LEN {}u", r.len);
        let _ = writeln!(s, "#define REPACK_{role}_SHA256 {{ {} }}", bytes.join(", "));
    }
    Ok(s)
}

fn gas(module: &str, dev: &str, cs: &[C], emit: Option<&[&str]>) -> String {
    let mut s = format!("/* {module} on {dev}: {NOTE}. */\n");
    for k in cs
        .iter()
        .filter(|k| emit.is_none_or(|e| e.contains(&k.name)))
    {
        let _ = writeln!(
            s,
            "    .equ {}, {}    /* {} */",
            k.name,
            hex(k.value),
            k.note
        );
    }
    s
}

fn sharc(module: &str, dev: &str, cs: &[C], emit: Option<&[&str]>) -> String {
    let mut s = format!(
        "// {module} on {dev}: {NOTE}.\n// Decimal values: the assembler upper-cases macro text, which breaks `0x` literals after `+`.\n"
    );
    for k in cs
        .iter()
        .filter(|k| emit.is_none_or(|e| e.contains(&k.name)))
    {
        // the note goes on its own line: the assembler's preprocessor keeps a trailing comment in the macro body, where it would
        // swallow the rest of a line that uses the macro (`R3 = DL_TAIL_WORDS - 3;`)
        let _ = writeln!(s, "// {}\n#define {} {}", k.note, k.name, k.value);
    }
    s
}

/// Does the `uses` pattern match a constant name: an exact name, or a prefix and one trailing `*`?
fn uses_match(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == pattern,
    }
}

/// The constant names a `gas` or `sharc` entry writes: those that its `uses` list names, in the order of `constants()`.
fn emitted_names(
    module: &str,
    g: &crate::module::GenDef,
    per_dev: &[(String, Vec<C>)],
) -> Result<Vec<&'static str>> {
    let at = format!("{module}: gen {}", g.path);
    if g.uses.is_empty() {
        return Err(err!(
            "{at}: a `{}` gen needs `uses`, the constants the module reads",
            g.kind
        ));
    }
    let all = &per_dev[0].1;
    for p in &g.uses {
        let stars = p.matches('*').count();
        if p.is_empty() || stars > 1 || (stars == 1 && !p.ends_with('*')) {
            return Err(err!(
                "{at}: `uses` pattern {p:?} must be a name, or a prefix and one trailing `*`"
            ));
        }
        if !all.iter().any(|k| uses_match(p, k.name)) {
            return Err(err!("{at}: `uses` {p:?} matches no device constant"));
        }
    }
    Ok(all
        .iter()
        .filter(|k| g.uses.iter().any(|p| uses_match(p, k.name)))
        .map(|k| k.name)
        .collect())
}

fn has_word(text: &str, word: &str) -> bool {
    let id = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let t = text.as_bytes();
    text.match_indices(word)
        .any(|(i, _)| (i == 0 || !id(t[i - 1])) && t.get(i + word.len()).is_none_or(|&b| !id(b)))
}

/// Every emitted name must occur as a word in a source file of the module (outputs of its own `[[gen]]` entries excluded).
fn check_sources(
    module: &str,
    g: &crate::module::GenDef,
    dir: &std::path::Path,
    outputs: &[std::path::PathBuf],
    exts: &[&str],
    names: &[&str],
) -> Result<()> {
    let mut texts = vec![];
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if !(name.starts_with('.')
                    || ["build", "target", "node_modules", "tests"].contains(&name.as_str()))
                {
                    dirs.push(p);
                }
            } else if p.extension().is_some_and(|x| exts.iter().any(|w| x == *w))
                && !outputs.contains(&p)
            {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    texts.push(t);
                }
            }
        }
    }
    let missing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| !texts.iter().any(|t| has_word(t, n)))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(err!(
        "{module}: gen {}: the module's sources never use {}; remove them from `uses`",
        g.path,
        missing.join(", ")
    ))
}

/// Is the module built for the device: a `[targets]` key or a `ports` entry that starts with `<short>-` (TODO: the port file format, devices/ports/, has no device field; the prefix is the identity, as in apps/web/src/data/modules.ts; finding 153); a module that lists neither is for every device.
fn targets_device(m: &crate::module::Manifest, short: &str) -> bool {
    let prefix = format!("{short}-");
    let listed = m.module.ports.iter().chain(m.targets.keys());
    let mut any = false;
    for t in listed {
        any = true;
        if t.starts_with(&prefix) {
            return true;
        }
    }
    !any
}

/// The tables of a device file as dotted paths (`input`, `input.keys`).
fn table_paths(t: &toml::Table) -> std::collections::BTreeSet<String> {
    fn walk(t: &toml::Table, prefix: &str, out: &mut std::collections::BTreeSet<String>) {
        for (k, v) in t {
            if let toml::Value::Table(sub) = v {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                out.insert(path.clone());
                walk(sub, &path, out);
            }
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(t, "", &mut out);
    out
}

/// A module names the tables of the device file that it reads (`[module] needs_device`, for example `input.keys`): every device (runtime target or not)
/// whose patch sets include the module (as a root or through `requires`) must have them. The generated constants of an absent table would be zeros, which no
/// module should run on. This whole-library check (`patchkit gen`) also refuses a module id of a patch set that the library does not have.
pub fn check_device_tables(lib: &Library, facts: &Facts) -> Result<()> {
    for (dev, e) in &facts.devices {
        for (set, roots) in &e.patch_sets {
            let (ids, _) = lib
                .with_requires(roots)
                .map_err(|x| err!("{dev}: patch set {set}: {x}"))?;
            for id in ids {
                let m = lib
                    .get(&id)
                    .ok_or_else(|| err!("{dev}: patch set {set}: unknown module {id}"))?;
                check_needs(dev, set, &id, &m.manifest.module.needs_device, e)?;
            }
        }
    }
    Ok(())
}

/// The same check for one module and the tables it declares (`needs`), for the generator of the module, `build` and `pack`. A device is checked when a patch set
/// lists the module; a set that cannot be resolved in this library (`pack --manifest`: no core modules) counts when the module is one of its roots, and is skipped
/// otherwise. Other modules are not looked at, so a device without a table never stops the build of a module that does not need it.
pub fn check_module_tables(lib: &Library, facts: &Facts, id: &str, needs: &[String]) -> Result<()> {
    if needs.is_empty() {
        return Ok(());
    }
    for (dev, e) in &facts.devices {
        for (set, roots) in &e.patch_sets {
            let includes = match lib.with_requires(roots) {
                Ok((ids, _)) => ids.iter().any(|i| i == id),
                Err(_) => roots.iter().any(|r| r == id),
            };
            if includes {
                check_needs(dev, set, id, needs, e)?;
            }
        }
    }
    Ok(())
}

fn check_needs(dev: &str, set: &str, id: &str, needs: &[String], e: &DeviceTables) -> Result<()> {
    for need in needs {
        if !e.tables.contains(need) {
            return Err(err!(
                "{dev}: patch set {set} includes {id}, which needs [{need}] in the device file"
            ));
        }
    }
    Ok(())
}

/// The files a module asks for with `[[gen]]` (empty: it asks for none). A module with `needs_device` is first checked against every device that patches it in.
pub fn generate(lib: &Library, facts: &Facts, module: &str) -> Result<Vec<GenFile>> {
    let m = lib
        .get(module)
        .ok_or_else(|| err!("unknown module {module}"))?;
    check_module_tables(lib, facts, module, &m.manifest.module.needs_device)?;
    if m.manifest.gens.is_empty() {
        return Ok(vec![]);
    }
    if facts.docs.is_empty() {
        return Err(err!(
            "{module}: [[gen]] needs device files with [board], [digikit] and [card.store]"
        ));
    }
    // a module gets files for the devices it is built for (`[targets]`, `ports`); a module that names none is for all of them
    let devs: Vec<&String> = facts
        .docs
        .keys()
        .filter(|d| targets_device(&m.manifest, d))
        .collect();
    if devs.is_empty() {
        return Ok(vec![]);
    }
    let mut per_dev: Vec<(String, Vec<C>)> = vec![];
    for dev in &devs {
        per_dev.push(((*dev).clone(), constants(lib, dev, &facts.docs[*dev])?));
    }
    let outputs: Vec<std::path::PathBuf> = m
        .manifest
        .gens
        .iter()
        .flat_map(|g| {
            devs.iter()
                .map(|dev| m.dir.join(g.path.replace("{dev}", dev)))
        })
        .collect();
    let mut out = vec![];
    for g in &m.manifest.gens {
        let picks = matches!(g.kind.as_str(), "gas" | "sharc");
        if !picks && !g.uses.is_empty() {
            return Err(err!(
                "{module}: a `{}` gen ({}) takes no `uses`",
                g.kind,
                g.path
            ));
        }
        let emit = if picks {
            let names = emitted_names(module, g, &per_dev)?;
            check_sources(
                module,
                g,
                &m.dir,
                &outputs,
                &["s", "S", "inc", "c", "h"],
                &names,
            )?;
            names
        } else {
            vec![]
        };
        match g.kind.as_str() {
            "gas-dkdv" => {
                if !g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `gas-dkdv` gen is one file per device: put {{dev}} in the path"
                    ));
                }
                for (dev, cs) in &per_dev {
                    let keys = facts.key_codes(dev)?;
                    out.push((
                        g.path.replace("{dev}", dev),
                        gas_dkdv(module, dev, cs, &keys)?,
                    ));
                }
            }
            "gas-dkdv-offsets" => {
                if g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `gas-dkdv-offsets` gen is one file for all devices"
                    ));
                }
                out.push((g.path.clone(), gas_dkdv_offsets(module)));
            }
            "rust-profiles" => {
                if g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `rust-profiles` gen is one file for all devices"
                    ));
                }
                out.push((g.path.clone(), rust_profiles(module, lib, facts)?));
            }
            "c-repack" => {
                if g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `c-repack` gen is one file for all devices"
                    ));
                }
                out.push((g.path.clone(), c_repack(module, &m.manifest.repack)?));
            }
            "c-shared" => {
                if g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `c-shared` gen is one file for all devices"
                    ));
                }
                out.push((g.path.clone(), c_shared(module)));
            }
            "gas" | "sharc" => {
                if !g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `{}` gen is one file per device: put {{dev}} in the path",
                        g.kind
                    ));
                }
                for (dev, cs) in &per_dev {
                    let text = if g.kind == "gas" {
                        gas(module, dev, cs, Some(&emit))
                    } else {
                        sharc(module, dev, cs, Some(&emit))
                    };
                    out.push((g.path.replace("{dev}", dev), text));
                }
            }
            "sharc-adapter" => {
                if !g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `sharc-adapter` gen is one file per device: put {{dev}} in the path"
                    ));
                }
                for dev in &devs {
                    let e = &facts.docs[*dev];
                    let a = e
                        .adapter
                        .as_ref()
                        .ok_or_else(|| err!("{module}: {dev} has no [adapter.machine]"))?;
                    let mut constants = adapter_constants(a);
                    if let Some(p) = e.sample_pool.as_ref().filter(|p| p.ring_r_base != 0) {
                        constants.push(c(
                            "ADM_RING_R_BASE",
                            p.ring_r_base,
                            "[adapter.sample_pool] ring_r_base (byte address of the 16 R rings of 32 KiB, one per slot)",
                        ));
                    }
                    out.push((
                        g.path.replace("{dev}", dev),
                        sharc(module, dev, &constants, None),
                    ));
                }
            }
            "gas-sample-pool" => {
                if !g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `gas-sample-pool` gen is one file per device: put {{dev}} in the path"
                    ));
                }
                for dev in &devs {
                    let e = &facts.docs[*dev];
                    let p = e
                        .sample_pool
                        .as_ref()
                        .ok_or_else(|| err!("{module}: {dev} has no [adapter.sample_pool]"))?;
                    out.push((
                        g.path.replace("{dev}", dev),
                        gas(module, dev, &sample_pool_constants(p), None),
                    ));
                }
            }
            "sharc-sample-pool" => {
                if !g.path.contains("{dev}") {
                    return Err(err!(
                        "{module}: a `sharc-sample-pool` gen is one file per device: put {{dev}} in the path"
                    ));
                }
                for dev in &devs {
                    let e = &facts.docs[*dev];
                    // a device that borrows the stock pool has no range and no DSP receiver
                    let Some(p) = e.sample_pool.as_ref().filter(|p| p.ddr_bytes != 0) else {
                        continue;
                    };
                    out.push((
                        g.path.replace("{dev}", dev),
                        sharc(module, dev, &sample_pool_dsp_constants(p), None),
                    ));
                }
            }
            k => return Err(err!("{module}: unknown gen kind {k:?}")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod sample_pool_tests {
    use super::*;

    fn pool(base: u64, bytes: u64) -> Option<AdapterSamplePool> {
        Some(AdapterSamplePool {
            slot_floor: 0,
            ddr_base: base,
            ddr_bytes: bytes,
            ring_r_base: 0,
        })
    }

    #[test]
    fn range_must_clear_the_stock_writes_and_stay_in_the_alias() {
        assert!(sample_pool_facts("dn2", pool(0x8053_1000, 0x1000_0000)).is_ok());
        assert!(sample_pool_facts("dn2", pool(0x9ff0_0000, 0x10_0000)).is_ok());
        // below the last stock write seen (finding 106)
        assert!(sample_pool_facts("dn2", pool(0x8053_0000, 0x1000)).is_err());
        // past the end of the NW32 alias
        assert!(sample_pool_facts("dn2", pool(0x9ff0_0000, 0x20_0000)).is_err());
        // no range at all is a device that borrows the stock pool
        assert!(sample_pool_facts("dt2", pool(0, 0)).is_ok());
    }

    #[test]
    fn the_r_rings_sit_outside_the_pool_and_inside_the_alias() {
        let rings = |base: u64, ring: u64| {
            Some(AdapterSamplePool {
                slot_floor: 0,
                ddr_base: base,
                ddr_bytes: 0x0ff8_0000,
                ring_r_base: ring,
            })
        };
        // the top of the pool range, as in devices/digitone-ii.toml
        assert!(sample_pool_facts("dn2", rings(0x8053_1000, 0x904b_1000)).is_ok());
        // inside the pool
        assert!(sample_pool_facts("dn2", rings(0x8053_1000, 0x9000_0000)).is_err());
        // half inside the pool
        assert!(sample_pool_facts("dn2", rings(0x8053_1000, 0x9049_1000)).is_err());
        // below the last stock write, unaligned, past the alias
        assert!(sample_pool_facts("dn2", rings(0x8053_1000, 0x8053_0000)).is_err());
        assert!(sample_pool_facts("dn2", rings(0x8053_1000, 0x904b_1800)).is_err());
        assert!(sample_pool_facts("dn2", rings(0x8053_1000, 0x9ff8_1000)).is_err());
        // none
        assert!(sample_pool_facts("dn2", rings(0x8053_1000, 0)).is_ok());
    }

    #[test]
    fn dsp_constants_are_the_nw32_view_of_the_range() {
        let p = SamplePoolF {
            slot_floor: 0,
            ddr_base: 0x8053_1000,
            ddr_bytes: 0x1000_0000,
            ring_r_base: 0,
        };
        let c = sample_pool_dsp_constants(&p);
        assert_eq!((c[0].name, c[0].value), ("SP_DDR_BASE_NW32", 0x1014_c400));
        assert_eq!((c[1].name, c[1].value), ("SP_DDR_PAGES", 65536));
    }
}
