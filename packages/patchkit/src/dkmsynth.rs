//! Synthetic `.dkm` files for the capacity tests of the runtime (docs/plans/capacity-isolation.md): a module of a chosen size, with a
//! chosen number of machines, parameters, pages, DSP bytes and voices, made from a small TOML spec instead of a fixture folder.
//!
//! The file is made by the same packer as a real module (`dkm::pack_data_only_max`, `dkneeds::keys::write_keys`, `dkmdsp::DspSection::encode`),
//! so the format has one source. The ColdFire code is `moveq #0,d0; rts` (the stub is `init`); everything else is data (`pad`, `fill`) or `bss`,
//! which the file does not store. The DSP image is `fill` bytes (zero words are SHARC+ NOPs) and is never run: a voice names a type and an
//! entry at byte 0, and the capacity tests do not play it.
//!
//! ```toml
//! name = "cap/a1"       # module name, 1 to 31 characters
//! pad = 0               # data bytes behind the code (a multiple of 4): makes the file larger
//! fill = 0xa5           # the byte of `pad` (and of the DSP image)
//! bss = 0               # ColdFire bss bytes (a multiple of 4)
//! machines = 0          # machine records, ids m0, m1 ... (hashed keys), data only
//! params = 0            # parameter records (the device's slots)
//! pages = 0             # page records, page i for machine i mod `machines` (Digitone II)
//! oversize = false      # allow a file over the staging buffer
//! [dsp]                 # a DSP section (optional)
//! image = 64            # bytes (a multiple of 4)
//! bss = 0               # DSP bss bytes (a multiple of 4)
//! align = 8             # 8, 16, 32 or 64
//! [[dsp.voice]]         # one per voice row
//! type = 16             # a pinned machine type number, or
//! machine = "m0"        # a machine of this file (its hashed key)
//! state_bytes = 0       # per-voice state, and how many voices of it (DSP section version 3 when any is set)
//! voices = 0
//! ```

use crate::dkm::{self, Options};
use crate::dkmdsp::{DspSection, Voice};
use crate::dko::{Dko, DkoSection, DkoSymbol};
use crate::error::*;
use crate::register;
use crate::typed::Models;
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub name: String,
    #[serde(default)]
    pub pad: u32,
    #[serde(default)]
    pub fill: u8,
    #[serde(default)]
    pub bss: u32,
    #[serde(default)]
    pub machines: u32,
    #[serde(default)]
    pub params: u32,
    #[serde(default)]
    pub pages: u32,
    #[serde(default)]
    pub oversize: bool,
    pub dsp: Option<DspSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DspSpec {
    pub image: u32,
    #[serde(default)]
    pub bss: u32,
    #[serde(default = "align8")]
    pub align: u32,
    #[serde(default)]
    pub voice: Vec<VoiceSpec>,
}

fn align8() -> u32 {
    8
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSpec {
    /// A pinned type number (the hash part of the key is 0).
    #[serde(rename = "type")]
    pub type_no: Option<u32>,
    /// A machine of the file: the voice names it by its hashed key.
    pub machine: Option<String>,
    #[serde(default)]
    pub state_bytes: u32,
    #[serde(default)]
    pub voices: u32,
}

impl Spec {
    pub fn parse(text: &str) -> Result<Spec> {
        Ok(toml::from_str(text)?)
    }
}

/// The id of machine `i` of a synthetic module.
pub fn machine_id(i: u32) -> String {
    format!("m{i}")
}

/// Make the `.dkm` of SPEC for device DEV (`dt2`, `dn2`): the device gives the parameter slots and the DSP section its device word.
pub fn synth(spec: &Spec, dev: &str, models: &Models) -> Result<Vec<u8>> {
    models
        .by_device
        .get(dev)
        .ok_or_else(|| err!("no device model for {dev:?}"))?;
    if spec.pad % 4 != 0 || spec.bss % 4 != 0 {
        return Err(err!("{}: pad and bss are multiples of 4", spec.name));
    }
    let mut sections = vec![DkoSection {
        name: ".text".into(),
        kind: "text".into(),
        align: 4,
        size: 4,
        hex: hex(&[0x70, 0x00, 0x4e, 0x75]), // moveq #0,d0; rts
    }];
    if spec.pad > 0 {
        sections.push(DkoSection {
            name: ".data".into(),
            kind: "data".into(),
            align: 4,
            size: spec.pad,
            hex: hex(&vec![spec.fill; spec.pad as usize]),
        });
    }
    if spec.bss > 0 {
        sections.push(DkoSection {
            name: ".bss".into(),
            kind: "bss".into(),
            align: 4,
            size: spec.bss,
            hex: String::new(),
        });
    }
    let mut obj = Dko {
        format: crate::dko::FORMAT,
        arch: "cf".into(),
        source_sha256: "0".repeat(64),
        toolchain: Default::default(),
        sections,
        symbols: vec![DkoSymbol {
            name: "dk_init_stub".into(),
            section: Some(".text".into()),
            value: 0,
            global: true,
        }],
        relocs: vec![],
    };
    // a made-up object holds nothing but the stub and data: the packer's record of an ISA check is for built objects
    obj.stamp_isa();

    let device = register::device_id(dev)?;
    let key = |m: &str| register::machine_key(&spec.name, m);
    if spec.pages > 0 && spec.machines == 0 {
        return Err(err!(
            "{}: a page belongs to a machine of the file, the spec has none",
            spec.name
        ));
    }
    // the keys table: the row counts and the machine keys. The stub `init` makes no builder call, so a module with machines is refused at the
    // commit (docs/plans/sdk.md 6.3): the synthetic capacity modules with machine rows cannot load until `synth` generates the calls.
    let machine_keys: Vec<u32> = (0..spec.machines).map(|i| key(&machine_id(i))).collect();
    if spec.machines == 0 && spec.params > 0 {
        return Err(err!(
            "{}: parameters belong to a machine of the file, the spec has none",
            spec.name
        ));
    }
    let mut keys = vec![0u8; dkneeds::keys::keys_size(machine_keys.len(), 0)];
    if !keys.is_empty() {
        dkneeds::keys::write_keys(
            (spec.machines as u16, spec.params as u16, spec.pages as u16),
            &machine_keys,
            &[],
            &mut keys,
        )
        .map_err(|e| err!("{}: keys table: {e:?}", spec.name))?;
    }
    let reg_bytes = keys;

    let dsp_bytes = match &spec.dsp {
        None => vec![],
        Some(d) => {
            if d.image == 0 || d.image % 4 != 0 || d.bss % 4 != 0 {
                return Err(err!(
                    "{}: dsp image and bss are multiples of 4 and the image is not empty",
                    spec.name
                ));
            }
            let mut voices = vec![];
            for v in &d.voice {
                let type_no = match (&v.type_no, &v.machine) {
                    (Some(t), None) => *t,
                    (None, Some(m)) => {
                        if !(0..spec.machines).any(|i| machine_id(i) == *m) {
                            return Err(err!(
                                "{}: voice names machine {m:?}, which the file does not have",
                                spec.name
                            ));
                        }
                        key(m)
                    }
                    _ => return Err(err!("{}: a voice has `type` or `machine`", spec.name)),
                };
                voices.push(Voice {
                    type_no,
                    entry: 0,
                    note_on: 0,
                    state_bytes: v.state_bytes,
                    clear_bytes: 0,
                    voices: v.voices,
                    ranges: [0; 8],
                    latched: 0,
                });
            }
            DspSection {
                sample_bank: None,
                sample_tracks: None,
                device,
                image: vec![spec.fill; d.image as usize],
                bss: d.bss,
                align: d.align,
                imports: vec![],
                relocs: vec![],
                voices,
                fxs: vec![],
            }
            .encode()
        }
    };
    let opts = Options {
        name: spec.name.clone(),
        version: 0x00_01_00,
        trace: false,
        stereo: false,
        gate: false,
    };
    let max = if spec.oversize {
        usize::MAX
    } else {
        dkm::max_file()
    };
    dkm::pack_data_only_max(&obj, &reg_bytes, &dsp_bytes, &opts, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dkm::Dkm;

    fn models() -> Models {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices");
        Models::from_dir(&root).unwrap()
    }

    fn make(text: &str, dev: &str) -> Vec<u8> {
        synth(&Spec::parse(text).unwrap(), dev, &models()).unwrap()
    }

    #[test]
    fn a_plain_module_is_a_stub_with_the_chosen_size() {
        let f = make(
            "name = \"cap/a\"\npad = 100\nbss = 4096\nfill = 0xa5\n",
            "dt2",
        );
        let d = Dkm::parse(&f).unwrap();
        assert_eq!(d.name, "cap/a");
        // the loader allocates image + bss: code 4 + pad 100
        assert_eq!((d.image.len(), d.bss), (104, 4096));
        assert_eq!(&d.image[..4], &[0x70, 0x00, 0x4e, 0x75]);
        assert!(d.image[4..].iter().all(|b| *b == 0xa5));
        assert_eq!(d.init, Some(0));
        assert!(d.register.is_empty() && d.dsp.is_empty());
        // deterministic
        assert_eq!(
            f,
            make(
                "name = \"cap/a\"\npad = 100\nbss = 4096\nfill = 0xa5\n",
                "dt2"
            )
        );
    }

    #[test]
    fn the_keys_table_has_the_rows_and_the_machine_keys_on_both_devices() {
        for dev in ["dt2", "dn2"] {
            let pages = if dev == "dn2" { 8 } else { 0 };
            let f = make(
                &format!("name = \"cap/r\"\nmachines = 8\nparams = 16\npages = {pages}\n"),
                dev,
            );
            let d = Dkm::parse(&f).unwrap();
            let k = dkneeds::keys::Keys::parse(&d.register).unwrap();
            assert_eq!(
                (k.machine_rows(), k.param_rows(), k.page_rows()),
                (8, 16, pages)
            );
            // a machine's key is the hashed key of "<module>:m<i>"
            assert_eq!(
                k.machine_keys().nth(3),
                Some(register::machine_key("cap/r", "m3"))
            );
        }
        let e = synth(
            &Spec::parse("name = \"x\"\npages = 2\n").unwrap(),
            "dn2",
            &models(),
        )
        .unwrap_err();
        assert!(e.0.contains("page belongs to a machine"), "{}", e.0);
    }

    #[test]
    fn a_dsp_section_has_the_sizes_and_voices_asked_for() {
        let f = make(
            "name = \"cap/d\"\nmachines = 1\n[dsp]\nimage = 64\nbss = 4096\nalign = 16\n\
             [[dsp.voice]]\ntype = 17\n[[dsp.voice]]\nmachine = \"m0\"\nstate_bytes = 32\nvoices = 2\n",
            "dn2",
        );
        let d = Dkm::parse(&f).unwrap();
        let s = DspSection::parse(&d.dsp).unwrap();
        assert_eq!((s.image.len(), s.bss, s.align), (64, 4096, 16));
        assert_eq!(s.device, register::device_id("dn2").unwrap());
        assert_eq!(s.voices.len(), 2);
        assert_eq!(s.voices[0].type_no, 17);
        assert_eq!(s.voices[1].type_no, register::machine_key("cap/d", "m0"));
        assert_eq!((s.voices[1].state_bytes, s.voices[1].voices), (32, 2));
    }

    #[test]
    fn an_oversize_file_needs_the_flag_and_bad_specs_are_refused() {
        let big = "name = \"cap/big\"\npad = 70000\n";
        let e = synth(&Spec::parse(big).unwrap(), "dt2", &models()).unwrap_err();
        assert!(e.0.contains("runtime stages"), "{}", e.0);
        let f = make(&format!("{big}oversize = true\n"), "dt2");
        assert!(f.len() > dkm::max_file());
        for bad in [
            "name = \"x\"\npad = 3\n",
            "name = \"\"\n",
            "name = \"x\"\n[dsp]\nimage = 0\n",
            "name = \"x\"\n[dsp]\nimage = 8\n[[dsp.voice]]\n",
            "name = \"x\"\n[dsp]\nimage = 8\n[[dsp.voice]]\nmachine = \"m9\"\n",
            "name = \"x\"\nsurprise = 1\n",
        ] {
            let r = Spec::parse(bad).and_then(|s| synth(&s, "dt2", &models()));
            assert!(r.is_err(), "accepted: {bad}");
        }
    }
}
