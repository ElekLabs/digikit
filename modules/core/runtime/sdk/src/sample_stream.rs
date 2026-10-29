//! Allocation-free native mono PCM16 streaming packer, independent of the small
//! float decoder's frame limit. Packed words are little endian, with the even
//! frame in the low half and the odd frame in the high half (zero for odd EOF).
//!
//! Output is staging data: **all output is invalid after any error**, including
//! a later trailer/EOF error. A caller must not publish READY until `finish`
//! succeeds and it independently establishes source consistency (for example,
//! a validated snapshot or an immutable lease with backend integrity checks).
use crate::sample::{HEADER_BYTES, RATE, TRAILER_BYTES};
use crate::{E_FORMAT, E_TOO_BIG};

/// Bytes actually consumed and emitted by one successful `push`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub consumed: usize,
    pub written: usize,
}

/// A single-use parser with a 64-byte header and one pending packed word.
/// Malformed/truncated/extra input returns sticky `E_FORMAT`; a valid header
/// exceeding `max_frames` returns sticky `E_TOO_BIG`, before any output.
/// The canonical file's byte length must fit in `u32`.
pub struct NativePcm16Stream {
    header: [u8; HEADER_BYTES],
    word: [u8; 4],
    header_len: usize,
    word_len: usize,
    max_frames: u32,
    frames: u32,
    pcm_left: u32,
    trailer_len: usize,
    error: i32,
    finished: bool,
}

impl NativePcm16Stream {
    pub const fn new(max_frames: u32) -> Self {
        Self {
            header: [0; HEADER_BYTES],
            word: [0; 4],
            header_len: 0,
            word_len: 0,
            max_frames,
            frames: 0,
            pcm_left: 0,
            trailer_len: 0,
            error: 0,
            finished: false,
        }
    }

    fn fail<T>(&mut self, error: i32) -> Result<T, i32> {
        self.error = error;
        Err(error)
    }

    fn validate_header(&mut self) -> Result<(), i32> {
        let pcm = u32::from_be_bytes(self.header[4..8].try_into().unwrap());
        let rate = u32::from_be_bytes(self.header[8..12].try_into().unwrap());
        if pcm == 0
            || pcm & 1 != 0
            || rate != RATE
            || pcm
                .checked_add((HEADER_BYTES + TRAILER_BYTES) as u32)
                .is_none()
        {
            return self.fail(E_FORMAT);
        }
        for (i, &byte) in self.header.iter().enumerate() {
            if !(4..12).contains(&i) && byte != if i == 0x14 { 0x7f } else { 0 } {
                return self.fail(E_FORMAT);
            }
        }
        if pcm / 2 > self.max_frames {
            return self.fail(E_TOO_BIG);
        }
        self.frames = pcm / 2;
        self.pcm_left = pcm;
        Ok(())
    }

    /// Accept arbitrary byte chunks, stopping for output backpressure. Only
    /// complete four-byte words are written; leftover output bytes are untouched.
    /// Even a zero-length output may consume the header and one pending word.
    /// Retry the unconsumed input with fresh output space, including an empty
    /// input to drain a pending word. An error invalidates all staging output.
    pub fn push(&mut self, input: &[u8], output: &mut [u8]) -> Result<Progress, i32> {
        if self.error != 0 {
            return Err(self.error);
        }
        if self.finished {
            return if input.is_empty() {
                Ok(Progress::default())
            } else {
                self.fail(E_FORMAT)
            };
        }
        let mut progress = Progress::default();
        loop {
            if self.header_len < HEADER_BYTES {
                let take = core::cmp::min(
                    HEADER_BYTES - self.header_len,
                    input.len() - progress.consumed,
                );
                self.header[self.header_len..self.header_len + take]
                    .copy_from_slice(&input[progress.consumed..progress.consumed + take]);
                self.header_len += take;
                progress.consumed += take;
                if self.header_len < HEADER_BYTES {
                    break;
                }
                self.validate_header()?;
            }
            if self.word_len == 4 || (self.pcm_left == 0 && self.word_len != 0) {
                if output.len() - progress.written < 4 {
                    break;
                }
                let packed = [self.word[1], self.word[0], self.word[3], self.word[2]];
                output[progress.written..progress.written + 4].copy_from_slice(&packed);
                progress.written += 4;
                self.word = [0; 4];
                self.word_len = 0;
            }
            if self.pcm_left != 0 {
                let take = core::cmp::min(
                    core::cmp::min(4 - self.word_len, self.pcm_left as usize),
                    input.len() - progress.consumed,
                );
                if take == 0 {
                    break;
                }
                self.word[self.word_len..self.word_len + take]
                    .copy_from_slice(&input[progress.consumed..progress.consumed + take]);
                self.word_len += take;
                self.pcm_left -= take as u32;
                progress.consumed += take;
                continue;
            }
            if progress.consumed == input.len() {
                break;
            }
            if self.trailer_len == TRAILER_BYTES || input[progress.consumed] != 0 {
                return self.fail(E_FORMAT);
            }
            self.trailer_len += 1;
            progress.consumed += 1;
        }
        Ok(progress)
    }

    /// Assert exact EOF after all payload output and sixteen zero trailer bytes.
    /// Calling too early permanently fails, including with a word still pending.
    /// Successful empty pushes and repeated finishes are idempotent.
    pub fn finish(&mut self) -> Result<u32, i32> {
        if self.error != 0 {
            return Err(self.error);
        }
        if self.header_len != HEADER_BYTES
            || self.pcm_left != 0
            || self.word_len != 0
            || self.trailer_len != TRAILER_BYTES
        {
            return self.fail(E_FORMAT);
        }
        self.finished = true;
        Ok(self.frames)
    }
}

#[cfg(test)]
#[path = "../../../../../packages/dksample/src/native.rs"]
mod canonical;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    fn native(values: &[i16]) -> Vec<u8> {
        let pcm: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
        canonical::build_native_sample(&pcm, false).unwrap()
    }

    fn expected(values: &[i16]) -> Vec<u8> {
        values
            .chunks(2)
            .flat_map(|pair| {
                let low = pair[0] as u16 as u32;
                let high = pair.get(1).copied().unwrap_or(0) as u16 as u32;
                (low | (high << 16)).to_le_bytes()
            })
            .collect()
    }

    fn feed(
        parser: &mut NativePcm16Stream,
        mut input: &[u8],
        output_size: usize,
        packed: &mut Vec<u8>,
    ) {
        loop {
            let mut output = [0xa5; 19];
            let p = parser.push(input, &mut output[..output_size]).unwrap();
            assert!(p.consumed <= input.len());
            assert!(p.written <= output_size && p.written % 4 == 0);
            assert!(output[p.written..].iter().all(|&v| v == 0xa5));
            packed.extend_from_slice(&output[..p.written]);
            input = &input[p.consumed..];
            if input.is_empty() && p.written == 0 {
                break;
            }
            assert!(p.consumed != 0 || p.written != 0);
        }
    }

    #[test]
    fn all_fragment_boundaries_pack_extremes_and_odd_final_frame() {
        for values in [
            &[-32768, -1, 0, 1, 32767][..],
            &[32767, -32768][..],
            &[-1][..],
        ] {
            let file = native(values);
            for split in 0..=file.len() {
                for output_size in 4..=11 {
                    let mut parser = NativePcm16Stream::new(480_000);
                    let mut packed = Vec::new();
                    feed(&mut parser, &file[..split], output_size, &mut packed);
                    feed(&mut parser, &file[split..], output_size, &mut packed);
                    assert_eq!(parser.finish(), Ok(values.len() as u32));
                    assert_eq!(packed, expected(values));
                    assert_eq!(parser.finish(), Ok(values.len() as u32));
                    assert_eq!(parser.push(&[], &mut []), Ok(Progress::default()));
                }
            }
            let mut parser = NativePcm16Stream::new(480_000);
            let mut packed = Vec::new();
            for byte in &file {
                feed(&mut parser, &[*byte], 4, &mut packed);
            }
            assert_eq!(parser.finish(), Ok(values.len() as u32));
            assert_eq!(packed, expected(values));
        }
    }

    #[test]
    fn zero_and_tiny_output_hold_one_word_then_drain() {
        let file = native(&[-32768, 32767, -1]);
        for size in 0..4 {
            let mut parser = NativePcm16Stream::new(3);
            let mut small = [0xa5; 3];
            let first = parser.push(&file, &mut small[..size]).unwrap();
            assert_eq!(
                first,
                Progress {
                    consumed: 68,
                    written: 0
                }
            );
            assert_eq!(small, [0xa5; 3]);
            assert_eq!(
                parser.push(&file[first.consumed..], &mut small[..size]),
                Ok(Progress::default())
            );
            let mut packed = Vec::new();
            feed(&mut parser, &[], 4, &mut packed);
            feed(&mut parser, &file[first.consumed..], 5, &mut packed);
            assert_eq!(parser.finish(), Ok(3));
            assert_eq!(packed, expected(&[-32768, 32767, -1]));
        }
    }

    #[test]
    fn ten_second_stream_uses_fixed_output_buffer() {
        let values: Vec<i16> = (0..480_000).map(|i| i as i16).collect();
        let file = native(&values);
        let mut parser = NativePcm16Stream::new(480_000);
        let mut consumed = 0;
        let mut frame = 0;
        let mut output = [0xa5; 260];
        while consumed < file.len() {
            let end = core::cmp::min(consumed + 137, file.len());
            let p = parser
                .push(&file[consumed..end], &mut output[..259])
                .unwrap();
            consumed += p.consumed;
            for word in output[..p.written].chunks_exact(4) {
                let packed = u32::from_le_bytes(word.try_into().unwrap());
                assert_eq!(packed as u16 as i16, values[frame]);
                assert_eq!((packed >> 16) as u16 as i16, values[frame + 1]);
                frame += 2;
            }
            assert_eq!(output[259], 0xa5);
            assert!(p.consumed != 0 || p.written != 0);
        }
        assert_eq!(frame, 480_000);
        assert_eq!(parser.finish(), Ok(480_000));
    }

    fn sticky(parser: &mut NativePcm16Stream, code: i32) {
        let mut output = [0xa5; 16];
        assert_eq!(parser.push(&[], &mut output), Err(code));
        assert_eq!(parser.push(&native(&[1]), &mut output), Err(code));
        assert_eq!(parser.finish(), Err(code));
        assert_eq!(output, [0xa5; 16]);
    }

    #[test]
    fn malformed_headers_and_capacity_never_emit() {
        let valid = native(&[1]);
        let mut cases = vec![native(&[])];
        for at in 0..64 {
            let mut bad = valid.clone();
            bad[at] ^= 1;
            // PCM length and rate have explicit cases below.
            if !(4..12).contains(&at) {
                cases.push(bad);
            }
        }
        for pcm in [0, 1, 3, u32::MAX, u32::MAX - 1, u32::MAX - 79] {
            let mut bad = valid.clone();
            bad[4..8].copy_from_slice(&pcm.to_be_bytes());
            cases.push(bad);
        }
        let mut rate = valid.clone();
        rate[8..12].copy_from_slice(&44_100u32.to_be_bytes());
        cases.push(rate);
        for bad in cases {
            let mut parser = NativePcm16Stream::new(u32::MAX);
            let mut output = [0xa5; 16];
            assert_eq!(parser.push(&bad, &mut output), Err(E_FORMAT));
            assert_eq!(output, [0xa5; 16]);
            sticky(&mut parser, E_FORMAT);
        }
        for (limit, frames) in [(0, 1), (2, 3), (480_000, 480_001)] {
            let mut header = valid[..64].to_vec();
            header[4..8].copy_from_slice(&(frames * 2u32).to_be_bytes());
            let mut parser = NativePcm16Stream::new(limit);
            let mut output = [0xa5; 16];
            assert_eq!(parser.push(&header, &mut output), Err(E_TOO_BIG));
            assert_eq!(output, [0xa5; 16]);
            sticky(&mut parser, E_TOO_BIG);
        }
    }

    #[test]
    fn truncation_late_trailer_and_extra_data_are_sticky() {
        let file = native(&[-1, 2, 3]);
        for end in 0..file.len() {
            let mut parser = NativePcm16Stream::new(3);
            let mut packed = Vec::new();
            feed(&mut parser, &file[..end], 4, &mut packed);
            assert_eq!(parser.finish(), Err(E_FORMAT));
            sticky(&mut parser, E_FORMAT);
        }
        for at in file.len() - 16..file.len() {
            let mut bad = file.clone();
            bad[at] = 1;
            let mut parser = NativePcm16Stream::new(3);
            let mut output = [0xa5; 16];
            assert_eq!(parser.push(&bad, &mut output), Err(E_FORMAT));
            assert_eq!(&output[..8], &expected(&[-1, 2, 3]));
            sticky(&mut parser, E_FORMAT);
        }
        for finish_first in [false, true] {
            let mut parser = NativePcm16Stream::new(3);
            parser.push(&file, &mut [0; 8]).unwrap();
            if finish_first {
                assert_eq!(parser.finish(), Ok(3));
            }
            assert_eq!(parser.push(&[0], &mut [0; 4]), Err(E_FORMAT));
            sticky(&mut parser, E_FORMAT);
        }
        let mut parser = NativePcm16Stream::new(3);
        parser.push(&file, &mut []).unwrap();
        assert_eq!(parser.finish(), Err(E_FORMAT));
        sticky(&mut parser, E_FORMAT);
    }
}
