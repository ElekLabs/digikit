//! What `firmware.load` needs to know about a .syx: its sections (digests),
//! which of them changed against the running one, and the DSP patch that
//! turns the running DSP image into the new one. Portable: no I/O.

use dt2_firmware_loader as container;
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use sharc_peer::dsp_patch::DspPatch;
use sharc_peer::dsp_stream::{self, LoadedMemory};

/// The MAIN OS section (ColdFire) of an image the registry does not know.
pub const SECTION_MAIN: u32 = 3;
/// The DSP boot stream of an image the registry does not know.
pub const SECTION_DSP: u32 = 7;

/// Which sections of a container are MAIN and the DSP boot stream (`[container]`, `[capabilities] dsp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roles {
    pub main: u32,
    /// None on a device without a SHARC: section 7 is then no DSP stream (a device with a second ColdFire has its program there).
    pub dsp: Option<u32>,
}

impl Roles {
    /// What an unknown or patched image gets: a device with a DSP in the usual sections.
    const UNKNOWN: Roles = Roles {
        main: SECTION_MAIN,
        dsp: Some(SECTION_DSP),
    };

    fn of_device(device: &plusdrive_format::device_profile::DeviceProfile) -> Roles {
        let (Some(caps), Some(container)) = (&device.capabilities, &device.container) else {
            return Roles::UNKNOWN;
        };
        Roles {
            main: container.main,
            dsp: if caps.dsp == "none" {
                None
            } else {
                container.dsp
            },
        }
    }

    /// The roles of the image `syx`: from the registry when it knows the file (by digest), the stock base named in its patch record, or its MAIN (a build
    /// with another DSP stream); else the defaults.
    pub fn of(syx: &[u8]) -> Roles {
        let Ok(registry) = plusdrive_format::device_profile::Registry::load() else {
            return Roles::UNKNOWN;
        };
        if let Ok((device, _)) = registry.identify(&sha256_hex(syx)) {
            return Roles::of_device(device);
        }
        // a digikit-patched image names its stock base in its patch record (its MAIN is not stock, so the MAIN lookup below cannot find it)
        if let Ok(Some(record)) = patchkit::verify::patch_record(syx)
            && let Ok((device, _)) = registry.identify(&record.base_sha256)
        {
            return Roles::of_device(device);
        }
        container::parse(syx)
            .ok()
            .and_then(|f| {
                // MAIN of every device is tried: a patched image keeps its MAIN's digest only when it is stock
                f.sections.into_iter().find_map(|s| {
                    registry
                        .boot_by_main(&s.bytes)
                        .map(|(device, _, _)| Roles::of_device(device))
                })
            })
            .unwrap_or(Roles::UNKNOWN)
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// A parsed .syx: digest of the file and of every decoded section, plus the
/// DSP stream and the MAIN digest (the sections the controller acts on).
#[derive(Clone)]
pub struct Analysis {
    pub sha256: String,
    /// Section id -> (SHA-256 of the decoded bytes, length).
    pub sections: BTreeMap<u32, (String, usize)>,
    /// Decoded DSP boot stream (section 7), if the firmware has one.
    pub dsp: Option<Vec<u8>>,
    /// Which sections are MAIN and the DSP stream.
    pub roles: Roles,
}

impl Analysis {
    pub fn of(syx: &[u8]) -> Result<Self, String> {
        Self::with_roles(syx, Roles::of(syx))
    }

    /// As `of`, with the section roles given. Without a DSP role, no section is a DSP stream (a
    /// device without a SHARC): every section is digested and none is checked or kept as DSP.
    pub fn with_roles(syx: &[u8], roles: Roles) -> Result<Self, String> {
        let firmware = container::parse(syx).map_err(|e| format!("parse SYX/ELE3: {e}"))?;
        let mut sections = BTreeMap::new();
        let mut dsp = None;
        for section in &firmware.sections {
            sections.insert(
                section.id,
                (sha256_hex(&section.bytes), section.bytes.len()),
            );
            if roles.dsp == Some(section.id) {
                // The emulator flattens the stream and never runs the boot ROM: refuse here what the ROM would.
                patchkit::bootrom::require(&section.bytes).map_err(|e| e.to_string())?;
                dsp = Some(section.bytes.clone());
            }
        }
        Ok(Self {
            sha256: sha256_hex(syx),
            sections,
            dsp,
            roles,
        })
    }

    pub fn main_sha256(&self) -> Option<&str> {
        self.sections
            .get(&self.roles.main)
            .map(|(sha, _)| sha.as_str())
    }

    /// Ids of the sections that differ from `other` (changed, added or removed).
    pub fn changed_sections(&self, other: &Analysis) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .sections
            .keys()
            .chain(other.sections.keys())
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .filter(|id| {
                self.sections.get(id).map(|s| &s.0) != other.sections.get(id).map(|s| &s.0)
            })
            .collect()
    }
}

/// The DSP patch that turns the DSP image loaded from `from` into the one
/// loaded from `to`, with counts for the report.
pub struct DspDiff {
    pub patch: DspPatch,
    pub changed_bytes: usize,
    /// Bytes the old stream loads that the new one does not (not undoable).
    pub dropped_bytes: usize,
    pub ranges: usize,
}

pub fn dsp_diff(from: &[u8], to: &[u8]) -> Result<DspDiff, String> {
    let old = LoadedMemory::from_stream(from).map_err(|e| format!("running DSP stream: {e}"))?;
    let new = LoadedMemory::from_stream(to).map_err(|e| format!("new DSP stream: {e}"))?;
    let diff = dsp_stream::diff(&old, &new, 0);
    Ok(DspDiff {
        ranges: diff.patch.ops.len(),
        changed_bytes: diff.changed_bytes,
        dropped_bytes: diff.dropped_bytes,
        patch: diff.patch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_sections_lists_differences_both_ways() {
        let a = Analysis {
            sha256: "a".into(),
            sections: BTreeMap::from([(3, ("m".into(), 1)), (7, ("d1".into(), 2))]),
            dsp: None,
            roles: Roles::UNKNOWN,
        };
        let b = Analysis {
            sha256: "b".into(),
            sections: BTreeMap::from([
                (3, ("m".into(), 1)),
                (7, ("d2".into(), 2)),
                (8, ("x".into(), 1)),
            ]),
            dsp: None,
            roles: Roles::UNKNOWN,
        };
        assert_eq!(a.changed_sections(&b), vec![7, 8]);
        assert_eq!(a.changed_sections(&a), Vec::<u32>::new());
    }
}
