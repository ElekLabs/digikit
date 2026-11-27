//! The sample slot image of a device that keeps its samples in numbered slots: a 0x40-byte header (magic `SAMP`, the length in 16-bit samples at +8, the name at
//! +0xC), then mono 16-bit samples at 48 kHz. One copy for the browser and the desktop (`ds_slot`) and for the host tests. The firmware reads only the magic,
//! the length and the name (finding 142), so +4 and +0x1C..0x3F are zero.
use crate::wav::{WavError, WavLimits, read_wav, resample};

pub const SLOT_HEADER_SIZE: usize = 0x40;
/// A name is at most 15 bytes: it ends with a NUL inside its 16 bytes.
pub const SLOT_NAME_MAX: usize = 15;
const RATE: u32 = 48_000;

/// What the device takes: `max_bytes` is the most audio bytes; `big_endian` the byte order of the 16-bit words (open on the device, a data key).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotFormat {
    pub rate: u32,
    pub max_bytes: usize,
    pub big_endian: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SlotError {
    Wav(WavError),
    Format(&'static str),
    Empty,
    TooLong { samples: usize, max_samples: usize },
}

impl SlotError {
    pub fn text(&self) -> String {
        match self {
            SlotError::Wav(e) => format!("{e:?}"),
            SlotError::Format(s) => (*s).to_string(),
            SlotError::Empty => "the sample has no audio".to_string(),
            SlotError::TooLong {
                samples,
                max_samples,
            } => {
                format!("too long for a slot: {samples} samples at 48 kHz, at most {max_samples}")
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SlotSample {
    /// The header and the audio: what the slot holds.
    pub bytes: Vec<u8>,
    /// The name as written (printable ASCII, at most 15 bytes).
    pub name: String,
    pub src_rate: u32,
    pub channels: u16,
    pub src_frames: usize,
    pub frames: usize,
    pub data_len: usize,
}

/// The name the header holds: printable ASCII kept, anything else `_`, cut to 15 bytes; `sample` when nothing is left.
pub fn slot_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { '_' })
        .take(SLOT_NAME_MAX)
        .collect();
    if s.trim().is_empty() {
        "sample".to_string()
    } else {
        s
    }
}

/// Mix to mono, resample to 48 kHz, round to 16 bits and wrap in the slot header. Refuses (before any resampling work) a result over `max_bytes`.
pub fn wav_to_slot(wav: &[u8], name: &str, fmt: SlotFormat) -> Result<SlotSample, SlotError> {
    if fmt.rate != RATE {
        return Err(SlotError::Format("the converter makes 48000 Hz"));
    }
    if fmt.max_bytes == 0 || !fmt.max_bytes.is_multiple_of(2) {
        return Err(SlotError::Format("max_bytes is a positive even number"));
    }
    let limits = WavLimits::default();
    let (src_rate, channels) = read_wav(wav, &limits).map_err(SlotError::Wav)?;
    let src_frames = channels[0].len();
    if src_frames == 0 {
        return Err(SlotError::Empty);
    }
    let max_samples = fmt.max_bytes / 2;
    // the resampler's output count is ceil(frames * 48000 / rate)
    let expect = (src_frames as u128 * u128::from(RATE)).div_ceil(u128::from(src_rate));
    if expect > max_samples as u128 {
        return Err(SlotError::TooLong {
            samples: usize::try_from(expect).unwrap_or(usize::MAX),
            max_samples,
        });
    }
    let n = channels.len() as f64;
    let mono: Vec<f64> = (0..src_frames)
        .map(|i| channels.iter().map(|c| c[i]).sum::<f64>() / n)
        .collect();
    let out = resample(&mono, src_rate, &limits).map_err(SlotError::Wav)?;
    if out.is_empty() {
        return Err(SlotError::Empty);
    }
    if out.len() > max_samples {
        return Err(SlotError::TooLong {
            samples: out.len(),
            max_samples,
        });
    }
    let name = slot_name(name);
    let data_len = out.len() * 2;
    let mut bytes = vec![0u8; SLOT_HEADER_SIZE + data_len];
    bytes[0..4].copy_from_slice(b"SAMP");
    bytes[8..12].copy_from_slice(&(out.len() as u32).to_be_bytes());
    bytes[0xC..0xC + name.len()].copy_from_slice(name.as_bytes());
    for (i, v) in out.iter().enumerate() {
        let s = (v * 32768.0).round_ties_even().clamp(-32768.0, 32767.0) as i16;
        let w = if fmt.big_endian {
            s.to_be_bytes()
        } else {
            s.to_le_bytes()
        };
        bytes[SLOT_HEADER_SIZE + i * 2..SLOT_HEADER_SIZE + i * 2 + 2].copy_from_slice(&w);
    }
    Ok(SlotSample {
        bytes,
        name,
        src_rate,
        channels: channels.len() as u16,
        src_frames,
        frames: out.len(),
        data_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(ch: u16, rate: u32, frames: &[[i16; 2]]) -> Vec<u8> {
        let mut pcm = Vec::new();
        for f in frames {
            for s in f.iter().take(ch as usize) {
                pcm.extend(s.to_le_bytes());
            }
        }
        let mut v = b"RIFF".to_vec();
        v.extend((36 + pcm.len() as u32).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(ch.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * u32::from(ch) * 2).to_le_bytes());
        v.extend((ch * 2).to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend((pcm.len() as u32).to_le_bytes());
        v.extend(pcm);
        v
    }
    const FMT: SlotFormat = SlotFormat {
        rate: 48_000,
        max_bytes: 0x7ffc0,
        big_endian: true,
    };

    #[test]
    fn a_mono_48k_wav_becomes_header_and_big_endian_samples() {
        let s = wav_to_slot(
            &wav(1, 48_000, &[[0x0102, 0], [-2, 0], [0x7fff, 0]]),
            "kick.wav",
            FMT,
        )
        .unwrap();
        assert_eq!(s.bytes.len(), 0x40 + 6);
        assert_eq!(&s.bytes[0..4], b"SAMP");
        assert_eq!(&s.bytes[4..8], &[0; 4]);
        assert_eq!(&s.bytes[8..12], &3u32.to_be_bytes());
        assert_eq!(&s.bytes[0xC..0xC + 9], b"kick.wav\0");
        assert!(s.bytes[0x1c..0x40].iter().all(|b| *b == 0));
        assert_eq!(&s.bytes[0x40..], &[0x01, 0x02, 0xff, 0xfe, 0x7f, 0xff]);
        assert_eq!((s.frames, s.data_len, s.channels), (3, 6, 1));
    }

    #[test]
    fn little_endian_flips_every_word() {
        let f = SlotFormat {
            big_endian: false,
            ..FMT
        };
        let s = wav_to_slot(&wav(1, 48_000, &[[0x0102, 0]]), "a", f).unwrap();
        assert_eq!(&s.bytes[0x40..], &[0x02, 0x01]);
    }

    #[test]
    fn stereo_is_mixed_down_to_the_mean() {
        let s = wav_to_slot(&wav(2, 48_000, &[[0x4000, 0], [0x4000, 0x2000]]), "a", FMT).unwrap();
        assert_eq!(&s.bytes[0x40..], &[0x20, 0x00, 0x30, 0x00]);
    }

    #[test]
    fn another_rate_is_resampled_to_48k() {
        let frames: Vec<[i16; 2]> = (0..441).map(|i| [(i * 50) as i16, 0]).collect();
        let s = wav_to_slot(&wav(1, 44_100, &frames), "a", FMT).unwrap();
        assert_eq!(s.frames, 480);
        assert_eq!(&s.bytes[8..12], &480u32.to_be_bytes());
    }

    #[test]
    fn the_maximum_fits_and_one_more_sample_is_refused() {
        let max = FMT.max_bytes / 2;
        let ok = vec![[1i16, 0]; max];
        assert_eq!(
            wav_to_slot(&wav(1, 48_000, &ok), "a", FMT)
                .unwrap()
                .bytes
                .len(),
            0x80000
        );
        let over = vec![[1i16, 0]; max + 1];
        assert_eq!(
            wav_to_slot(&wav(1, 48_000, &over), "a", FMT),
            Err(SlotError::TooLong {
                samples: max + 1,
                max_samples: max
            })
        );
        // refused before the resampler runs: a 96 kHz file of 2 * max + 2 frames
        let long = vec![[1i16, 0]; 2 * max + 2];
        assert!(matches!(
            wav_to_slot(&wav(1, 96_000, &long), "a", FMT),
            Err(SlotError::TooLong { .. })
        ));
    }

    #[test]
    fn names_are_printable_ascii_of_at_most_15_bytes() {
        assert_eq!(slot_name("0123456789abcdefXYZ"), "0123456789abcde");
        assert_eq!(slot_name("k\u{e9}ck"), "k_ck");
        assert_eq!(slot_name("   "), "sample");
        assert_eq!(slot_name(""), "sample");
    }

    #[test]
    fn refuses_what_it_does_not_make() {
        assert!(matches!(
            wav_to_slot(b"RIFF", "a", FMT),
            Err(SlotError::Wav(_))
        ));
        assert!(matches!(
            wav_to_slot(&wav(1, 48_000, &[]), "a", FMT),
            Err(SlotError::Empty)
        ));
        assert!(matches!(
            wav_to_slot(
                &wav(1, 48_000, &[[0, 0]]),
                "a",
                SlotFormat {
                    rate: 44_100,
                    ..FMT
                }
            ),
            Err(SlotError::Format(_))
        ));
        assert!(matches!(
            wav_to_slot(
                &wav(1, 48_000, &[[0, 0]]),
                "a",
                SlotFormat {
                    max_bytes: 3,
                    ..FMT
                }
            ),
            Err(SlotError::Format(_))
        ));
    }
}
