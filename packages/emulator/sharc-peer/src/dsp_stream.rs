//! The DSP boot stream (firmware section 7, an ADI boot stream) as loaded
//! memory, and the difference between two of them as a DSP patch.
//!
//! This is the Rust side of `tools/sharc_dev/sharcldr.py` (`parse_blocks`,
//! `LoadedMemory`): 16-byte little-endian headers (block code, target
//! address, byte count, argument) with a zero XOR over the 16 bytes and a core
//! signature in the top byte; a FILL block (bit 8) has no payload and repeats
//! its argument; later blocks win over earlier ones. Addresses are loader byte
//! addresses, the same ones the engine image and `dsp_patch` use, so a diff
//! of two streams is directly a list of `BYTES` operations. No I/O, no
//! threads: it builds for wasm.

use std::collections::{BTreeMap, HashMap};

use crate::dsp_patch::{DspPatch, PatchOp};

const HEADER_LEN: usize = 16;
const FILL_BIT: u32 = 1 << 8;
const PAGE: usize = 4096;
/// Largest FILL block accepted (a corrupt header must not allocate freely).
const FILL_LIMIT: u32 = 64 * 1024 * 1024;

struct Page {
    data: Box<[u8; PAGE]>,
    present: Box<[u64; PAGE / 64]>,
}

impl Page {
    fn new() -> Self {
        Self {
            data: Box::new([0; PAGE]),
            present: Box::new([0; PAGE / 64]),
        }
    }
    fn has(&self, i: usize) -> bool {
        self.present[i / 64] >> (i % 64) & 1 != 0
    }
    fn set(&mut self, i: usize, v: u8) {
        self.data[i] = v;
        self.present[i / 64] |= 1 << (i % 64);
    }
}

/// The bytes a boot stream writes, by address.
#[derive(Default)]
pub struct LoadedMemory {
    pages: HashMap<u32, Page>,
    blocks: usize,
}

impl LoadedMemory {
    /// Walk the stream like `sharcldr.parse_blocks`: stop at the first invalid
    /// header or a payload that overruns the data.
    pub fn from_stream(data: &[u8]) -> Result<Self, String> {
        let mut memory = Self::default();
        let mut offset = 0usize;
        while offset + HEADER_LEN <= data.len() {
            let header = &data[offset..offset + HEADER_LEN];
            if !matches!(header[3], 0xad | 0xac | 0xab) || header.iter().fold(0, |x, b| x ^ b) != 0
            {
                break;
            }
            let word = |i: usize| u32::from_le_bytes(header[i * 4..i * 4 + 4].try_into().unwrap());
            let (code, target, count, argument) = (word(0), word(1), word(2), word(3));
            let fill = code & FILL_BIT != 0;
            let payload = offset + HEADER_LEN;
            let length = if fill { 0 } else { count as usize };
            if payload + length > data.len() {
                break;
            }
            if fill {
                if count > FILL_LIMIT {
                    return Err(format!("FILL block of {count} bytes at {target:#x}"));
                }
                let pattern = argument.to_le_bytes();
                for k in 0..count {
                    memory.write(target.wrapping_add(k), pattern[(k % 4) as usize]);
                }
            } else {
                for (k, byte) in data[payload..payload + length].iter().enumerate() {
                    memory.write(target.wrapping_add(k as u32), *byte);
                }
            }
            memory.blocks += 1;
            offset = payload + length;
        }
        if memory.blocks == 0 {
            return Err("no valid boot-stream blocks".into());
        }
        Ok(memory)
    }

    fn write(&mut self, address: u32, value: u8) {
        self.pages
            .entry(address / PAGE as u32)
            .or_insert_with(Page::new)
            .set(address as usize % PAGE, value);
    }

    /// The byte at `address`, or None when the stream never writes it.
    pub fn get(&self, address: u32) -> Option<u8> {
        let page = self.pages.get(&(address / PAGE as u32))?;
        let i = address as usize % PAGE;
        page.has(i).then(|| page.data[i])
    }

    pub fn blocks(&self) -> usize {
        self.blocks
    }

    pub fn len(&self) -> usize {
        self.pages
            .values()
            .map(|p| {
                p.present
                    .iter()
                    .map(|w| w.count_ones() as usize)
                    .sum::<usize>()
            })
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What differs between two loaded streams.
pub struct StreamDiff {
    /// `BYTES` operations writing `new`'s bytes where they differ from `old`.
    pub patch: DspPatch,
    pub changed_bytes: usize,
    /// Bytes `old` writes and `new` does not (they cannot be undone by a patch).
    pub dropped_bytes: usize,
}

/// The operations that turn memory loaded from `old` into memory loaded from
/// `new`. Runs of changed bytes are joined across gaps of at most `join`
/// unchanged bytes (the gap bytes are rewritten with their new values, which
/// equal the old ones).
pub fn diff(old: &LoadedMemory, new: &LoadedMemory, join: u32) -> StreamDiff {
    // Changed addresses in order, as (address, new byte).
    let mut changed: BTreeMap<u32, u8> = BTreeMap::new();
    let mut dropped = 0usize;
    for (&number, page) in &new.pages {
        let base = number * PAGE as u32;
        for i in 0..PAGE {
            if !page.has(i) {
                continue;
            }
            let address = base + i as u32;
            if old.get(address) != Some(page.data[i]) {
                changed.insert(address, page.data[i]);
            }
        }
    }
    for (&number, page) in &old.pages {
        let base = number * PAGE as u32;
        for i in 0..PAGE {
            if page.has(i) && new.get(base + i as u32).is_none() {
                dropped += 1;
            }
        }
    }
    let changed_bytes = changed.len();
    let mut ops = Vec::new();
    let mut run: Option<(u32, Vec<u8>)> = None;
    for (&address, &byte) in &changed {
        let joins = run
            .as_ref()
            .is_some_and(|(start, bytes)| address - (*start + bytes.len() as u32) <= join);
        if joins {
            let (start, bytes) = run.as_mut().expect("joined run");
            // Fill the gap with the new stream's own bytes.
            for gap in *start + bytes.len() as u32..address {
                bytes.push(new.get(gap).unwrap_or(0));
            }
            bytes.push(byte);
        } else {
            if let Some((start, bytes)) = run.take() {
                ops.push(PatchOp::Bytes {
                    addr: start,
                    bytes,
                    was: None,
                });
            }
            run = Some((address, vec![byte]));
        }
    }
    if let Some((start, bytes)) = run {
        ops.push(PatchOp::Bytes {
            addr: start,
            bytes,
            was: None,
        });
    }
    StreamDiff {
        patch: DspPatch {
            base_sha256: None,
            ops,
        },
        changed_bytes,
        dropped_bytes: dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(code: u32, target: u32, count: u32, argument: u32, payload: &[u8]) -> Vec<u8> {
        let mut header = Vec::new();
        for w in [code, target, count, argument] {
            header.extend(w.to_le_bytes());
        }
        // Core 0 signature, then the check byte makes the XOR zero.
        header[3] = 0xad;
        header[2] = 0;
        let x = header.iter().fold(0, |x, b| x ^ b);
        header[2] = x;
        // byte 2 is part of block_code's third byte: keep the code's own bits.
        let mut out = header;
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn later_blocks_win_and_fill_repeats_its_argument() {
        let mut stream = block(0, 0x1000, 4, 0, &[1, 2, 3, 4]);
        stream.extend(block(FILL_BIT, 0x1002, 4, 0xaabbccdd, &[]));
        let m = LoadedMemory::from_stream(&stream).unwrap();
        assert_eq!(m.get(0x1000), Some(1));
        assert_eq!(m.get(0x1001), Some(2));
        assert_eq!(m.get(0x1002), Some(0xdd));
        assert_eq!(m.get(0x1003), Some(0xcc));
        assert_eq!(m.get(0x1004), Some(0xbb));
        assert_eq!(m.get(0x1005), Some(0xaa));
        assert_eq!(m.get(0x1006), None);
        assert_eq!(m.blocks(), 2);
    }

    #[test]
    fn diff_lists_changed_runs_and_counts_dropped_bytes() {
        let old =
            LoadedMemory::from_stream(&block(0, 0x2000, 8, 0, &[0, 1, 2, 3, 4, 5, 6, 7])).unwrap();
        let new = LoadedMemory::from_stream(&block(0, 0x2000, 6, 0, &[0, 9, 2, 3, 8, 5])).unwrap();
        let d = diff(&old, &new, 0);
        assert_eq!(d.changed_bytes, 2);
        assert_eq!(d.dropped_bytes, 2);
        assert_eq!(d.patch.ops.len(), 2);
        let d = diff(&old, &new, 2);
        assert_eq!(
            d.patch.ops,
            vec![PatchOp::Bytes {
                addr: 0x2001,
                bytes: vec![9, 2, 3, 8],
                was: None
            }]
        );
    }

    #[test]
    fn diff_of_streams_with_an_appended_block_writes_the_new_range() {
        // `new` is `old` with a block inserted before the final one.
        let mut old = block(0, 0x2000, 4, 0, &[1, 2, 3, 4]);
        let final_block = block(0x8000, 0x10, 0, 0, &[]);
        let mut new = old.clone();
        new.extend(block(0, 0x4000, 8, 0, &[9, 8, 7, 6, 5, 4, 3, 2]));
        old.extend(final_block.clone());
        new.extend(final_block);
        let (old, new) = (
            LoadedMemory::from_stream(&old).unwrap(),
            LoadedMemory::from_stream(&new).unwrap(),
        );
        assert_eq!((old.blocks(), new.blocks()), (2, 3));
        let d = diff(&old, &new, 0);
        assert_eq!(d.changed_bytes, 8);
        assert_eq!(d.dropped_bytes, 0);
        assert_eq!(
            d.patch.ops,
            vec![PatchOp::Bytes {
                addr: 0x4000,
                bytes: vec![9, 8, 7, 6, 5, 4, 3, 2],
                was: None
            }]
        );
        // and back: the removed block's bytes cannot be undone by a patch
        let back = diff(&new, &old, 0);
        assert_eq!((back.changed_bytes, back.dropped_bytes), (0, 8));
    }
}
