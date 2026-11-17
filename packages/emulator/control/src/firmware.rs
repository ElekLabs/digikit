//! What `firmware.load` needs to know about a .syx: its sections (digests),
//! which of them changed against the running one, and the DSP patch that
//! turns the running DSP image into the new one. Portable: no I/O.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use sharc_peer::dsp_patch::DspPatch;
use sharc_peer::dsp_stream::{self, LoadedMemory};

/// The MAIN OS section (ColdFire).
pub const SECTION_MAIN: u32 = 3;
/// The DSP boot stream.
pub const SECTION_DSP: u32 = 7;

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
}

impl Analysis {
    pub fn of(syx: &[u8]) -> Result<Self, String> {
        let firmware =
            dt2_firmware_loader::parse(syx).map_err(|e| format!("parse SYX/ELE3: {e}"))?;
        let mut sections = BTreeMap::new();
        let mut dsp = None;
        for section in &firmware.sections {
            sections.insert(
                section.id,
                (sha256_hex(&section.bytes), section.bytes.len()),
            );
            if section.id == SECTION_DSP {
                // The emulator flattens the stream and never runs the boot ROM: refuse here what the ROM would.
                patchkit::bootrom::require(&section.bytes).map_err(|e| e.to_string())?;
                dsp = Some(section.bytes.clone());
            }
        }
        Ok(Self {
            sha256: sha256_hex(syx),
            sections,
            dsp,
        })
    }

    pub fn main_sha256(&self) -> Option<&str> {
        self.sections
            .get(&SECTION_MAIN)
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
        };
        let b = Analysis {
            sha256: "b".into(),
            sections: BTreeMap::from([
                (3, ("m".into(), 1)),
                (7, ("d2".into(), 2)),
                (8, ("x".into(), 1)),
            ]),
            dsp: None,
        };
        assert_eq!(a.changed_sections(&b), vec![7, 8]);
        assert_eq!(a.changed_sections(&a), Vec::<u32>::new());
    }
}
