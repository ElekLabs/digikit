//! A parsed stock or patched `.syx`: container header, section table, stored and decoded sections.
//! Untouched sections keep their stored bytes verbatim; replaced ones are packed by the section rules.

use crate::error::*;
use crate::syx::{self, Device};
use dt2_firmware_loader as loader;

#[derive(Clone, Debug)]
pub struct Sec {
    pub id: u32,
    pub dest: u32,
    pub stored: Vec<u8>,
    pub data: Vec<u8>,
    pub changed: bool,
}

#[derive(Clone, Debug)]
pub struct Firmware {
    pub device: Device,
    pub device_id: u8,
    pub framing_start: Vec<u8>,
    pub framing_end: Vec<u8>,
    pub header: Vec<u8>,
    /// Container offset of the first section payload in the source image.
    pub first_offset: usize,
    pub sections: Vec<Sec>,
    pub syx_sha256: String,
}

impl Firmware {
    pub fn parse(syx_bytes: &[u8]) -> Result<Firmware> {
        let msgs = syx::messages(syx_bytes)?;
        if msgs.len() < 2 || msgs[0].len() != 16 || msgs[msgs.len() - 1].len() != 16 {
            return Err(err!("the .syx is missing its 16-byte framing messages"));
        }
        let fs = msgs[0].to_vec();
        let fe = msgs[msgs.len() - 1].to_vec();
        let device_id = fs[1..fs.len() - 1][3];
        let device = Device::from_id(device_id)?;
        let dec = loader::decode_syx(syx_bytes)?;
        let fw = loader::parse(syx_bytes)?;
        let c = &dec[fw.container_offset..];
        let header = c[..syx::COUNT_OFF].to_vec();
        let sections = fw
            .sections
            .iter()
            .map(|s| {
                let off = s.container_offset as usize;
                Sec {
                    id: s.id,
                    dest: s.destination,
                    stored: c[off..off + s.stored_length as usize].to_vec(),
                    data: s.bytes.clone(),
                    changed: false,
                }
            })
            .collect();
        Ok(Firmware {
            device,
            device_id,
            framing_start: fs,
            framing_end: fe,
            header,
            first_offset: fw
                .sections
                .iter()
                .map(|s| s.container_offset as usize)
                .min()
                .unwrap_or(0x80),
            sections,
            syx_sha256: sha256_hex(syx_bytes),
        })
    }

    pub fn section(&self, id: u32) -> Option<&Sec> {
        self.sections.iter().find(|s| s.id == id)
    }

    /// The MAIN OS section (`[container] main` of the device).
    pub fn main(&self) -> Option<&Sec> {
        self.section(self.device.main_section())
    }

    /// The SHARC+ boot stream section (`[container] dsp`); `None` on a device without a DSP, whatever its other sections hold.
    pub fn dsp(&self) -> Option<&Sec> {
        self.section(self.device.dsp_section()?)
    }

    /// Replace the decoded bytes of a section; its stored form is repacked. A section of `[container] readonly` (bootstrap,
    /// updater, boot data, FPGA bitstream, the second CPU's image) is refused: this is the one place a section is changed.
    pub fn replace(&mut self, id: u32, data: Vec<u8>) -> Result<()> {
        if self.device.is_readonly(id) {
            return Err(err!(
                "section {id} is read-only on {} ([container] readonly {:?}) and is never changed",
                self.device.short(),
                self.device.readonly_sections()
            ));
        }
        let stored = syx::store_for_section(id, &data)?;
        let s = self
            .sections
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| err!("the image has no section {id}"))?;
        s.data = data;
        s.stored = stored;
        s.changed = true;
        Ok(())
    }

    /// Replace the decoded bytes of the MAIN section and give its stored stream, made for the bootstrap's in-place unpack (`aplib::pack_section_in_place`).
    pub fn replace_main_stored(&mut self, data: Vec<u8>, stored: Vec<u8>) -> Result<()> {
        let id = self.device.main_section();
        let s = self
            .sections
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| err!("the image has no section {id}"))?;
        s.data = data;
        s.stored = stored;
        s.changed = true;
        Ok(())
    }

    /// Store a read-only section packed. Its decoded bytes do not change, so the second CPU gets the stock bytes once the module that declared the
    /// repack has unpacked them. Only a section of `[container] repack` is accepted. Returns the new stored length.
    pub fn repack(&mut self, id: u32) -> Result<usize> {
        if !self.device.is_repackable(id) {
            return Err(err!(
                "section {id} of {} cannot be stored packed ([container] repack {:?} names the sections that can)",
                self.device.short(),
                self.device.repack_roles()
            ));
        }
        let s = self
            .sections
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| err!("the image has no section {id}"))?;
        s.stored = crate::aplib::pack_section(&s.data);
        s.changed = true;
        Ok(s.stored.len())
    }

    /// The sealed container of the sections as they are stored now.
    pub fn container(&self) -> Result<Vec<u8>> {
        let entries: Vec<(u32, u32, &[u8])> = self
            .sections
            .iter()
            .map(|s| (s.id, s.dest, s.stored.as_slice()))
            .collect();
        let c = syx::build_container(&self.header, &entries, self.first_offset, self.device)?;
        Ok(syx::seal(c, self.device))
    }

    /// The container (sealed) and the full `.syx`.
    pub fn build(&self, max_container: Option<usize>) -> Result<Vec<u8>> {
        let c = self.container()?;
        if let Some(max) = max_container
            && c.len() > max
        {
            return Err(err!(
                "container is {} bytes, over the {max}-byte limit",
                c.len()
            ));
        }
        let mut stream = Vec::with_capacity(c.len() + 8);
        stream.extend_from_slice(&(c.len() as u32).to_be_bytes());
        stream.extend_from_slice(&syx::content_checksum(&c).to_be_bytes());
        stream.extend_from_slice(&c);
        Ok(syx::encode_syx(
            &stream,
            self.device_id,
            &self.framing_start,
            &self.framing_end,
        ))
    }
}
