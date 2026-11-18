//! aPLib-shaped packer for the container's section streams.
//!
//! A line-for-line port of `tools/firmware/aplibpack.py`: it must produce the same bytes as the Python packer
//! (tools/patchkit/pk_aplib_eq.py checks all stock sections). The format and the method are described there:
//! a forward dynamic program over hash-chain candidates, exact edge costs in bits, the repeat offset
//! taken from the best path that reaches a node.

use std::collections::HashMap;

const END_GAMMA: u64 = (1 << 24) + 2;
const END_BYTE: u8 = 0xFF;
const BIAS: i64 = 767;
const FAR: i64 = 3328;
const MAX_COPIES: usize = 1 << 20;
pub const DEFAULT_DEPTH: usize = 24;
pub const DEFAULT_NICE: usize = 96;

fn gamma_cost(v: i64) -> i64 {
    2 * (63 - (v as u64).leading_zeros() as i64)
}

struct Bits {
    buf: Vec<u8>,
    tag: usize,
    left: u32,
}

impl Bits {
    fn new() -> Self {
        Bits {
            buf: Vec::new(),
            tag: 0,
            left: 0,
        }
    }
    fn bit(&mut self, b: bool) {
        if self.left == 0 {
            self.tag = self.buf.len();
            self.buf.push(0);
            self.left = 8;
        }
        self.left -= 1;
        if b {
            self.buf[self.tag] |= 1 << self.left;
        }
    }
    fn byte(&mut self, x: u8) {
        self.buf.push(x);
    }
    fn gamma(&mut self, v: u64) {
        let n = 64 - v.leading_zeros(); // bit length
        // bits below the leading one, all but the last, each followed by 0
        for k in (1..n - 1).rev() {
            self.bit((v >> k) & 1 == 1);
            self.bit(false);
        }
        self.bit(v & 1 == 1);
        self.bit(true);
    }
    fn length(&mut self, field: i64) {
        if (1..=3).contains(&field) {
            self.bit(field >> 1 != 0);
            self.bit(field & 1 != 0);
        } else {
            self.bit(false);
            self.bit(false);
            self.gamma((field - 2) as u64);
        }
    }
}

fn len_bits(field: i64) -> i64 {
    if field <= 3 {
        2
    } else {
        2 + gamma_cost(field - 2)
    }
}

fn match_cost(off: i64, copies: i64, last: i64) -> i64 {
    let far = (off > FAR) as i64;
    let field = copies - 1 - far;
    let head = if off == last {
        3
    } else {
        1 + gamma_cost((off + BIAS) >> 8) + 8
    };
    head + len_bits(field)
}

fn lengths(maxlen: i64, minlen: i64, far: i64) -> Vec<i64> {
    let mut out: Vec<i64> = (minlen..=maxlen.min(minlen + 4)).collect();
    out.push(maxlen);
    let mut k = 1;
    loop {
        let top = (1i64 << (k + 1)) + 2 + far;
        if top > maxlen {
            break;
        }
        out.push(top);
        k += 1;
    }
    out.push(maxlen.min(4 + far));
    out.sort_unstable();
    out.dedup();
    out.retain(|&c| minlen <= c && c <= maxlen);
    out
}

#[derive(Default)]
struct Fx(u64);
impl std::hash::Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}
type FxMap = HashMap<u32, i64, std::hash::BuildHasherDefault<Fx>>;

fn key4(d: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]])
}
fn key3(d: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([0, d[i], d[i + 1], d[i + 2]])
}

fn mlen(data: &[u8], i: usize, j: usize, limit: usize) -> usize {
    let mut m = 0;
    while m < limit && data[i + m] == data[j + m] {
        m += 1;
    }
    m
}

/// data -> aPLib-shaped stream (no section header), ending with the marker.
pub fn pack_with(data: &[u8], depth: usize, nice: usize) -> Vec<u8> {
    let n = data.len();
    let inf: i64 = 1 << 60;
    let mut cost = vec![inf; n + 1];
    let mut prv = vec![0u32; n + 1];
    let mut koff = vec![0i64; n + 1];
    let mut lastoff = vec![1i64; n + 1];
    cost[0] = 0;
    let mut head4: FxMap = FxMap::default();
    let mut head3: FxMap = FxMap::default();
    let mut prev4 = vec![-1i64; n + 1];

    let mut i = 0usize;
    while i < n {
        let c = cost[i];
        let last = lastoff[i];
        let limit = (n - i).min(MAX_COPIES);
        if c + 9 < cost[i + 1] {
            cost[i + 1] = c + 9;
            prv[i + 1] = i as u32;
            koff[i + 1] = 0;
            lastoff[i + 1] = last;
        }
        let mut cands: Vec<(usize, i64)> = Vec::new();
        if last as usize <= i {
            let m = mlen(data, i, i - last as usize, limit);
            if m >= 2 {
                cands.push((m, last));
            }
        }
        let mut best: usize = cands.first().map_or(0, |c| c.0);
        let mut best_off = last;
        if limit >= 4 {
            let mut p = head4.get(&key4(data, i)).copied().unwrap_or(-1);
            let mut walked = 0;
            while p >= 0 && walked < depth {
                let off = i as i64 - p;
                if off != last {
                    let m = mlen(data, i, p as usize, limit);
                    if m > best {
                        cands.push((m, off));
                        best = m;
                        best_off = off;
                        if m >= nice {
                            break;
                        }
                    }
                }
                p = prev4[p as usize];
                walked += 1;
            }
        }
        if best < 3 && limit >= 3 {
            let p = head3.get(&key3(data, i)).copied().unwrap_or(-1);
            if p >= 0 && i as i64 - p != last && i as i64 - p <= FAR {
                let m = mlen(data, i, p as usize, limit);
                if m >= 3 {
                    cands.push((m, i as i64 - p));
                    if m > best {
                        best = m;
                        best_off = i as i64 - p;
                    }
                }
            }
        }
        let mut skip = 0usize;
        if !cands.is_empty() {
            if best >= nice {
                let m = best;
                let off = best_off;
                let j = i + m;
                let nc = c + match_cost(off, m as i64, last);
                if nc < cost[j] {
                    cost[j] = nc;
                    prv[j] = i as u32;
                    koff[j] = off;
                    lastoff[j] = off;
                }
                skip = m;
            } else {
                for &(m, off) in &cands {
                    let far = (off > FAR) as i64;
                    let minlen = 2 + far;
                    for cp in lengths(m as i64, minlen, far) {
                        let j = i + cp as usize;
                        let nc = c + match_cost(off, cp, last);
                        if nc < cost[j] {
                            cost[j] = nc;
                            prv[j] = i as u32;
                            koff[j] = off;
                            lastoff[j] = off;
                        }
                    }
                }
            }
        }
        let mut insert = |q: usize| {
            if q + 4 <= n {
                let k = key4(data, q);
                prev4[q] = head4.get(&k).copied().unwrap_or(-1);
                head4.insert(k, q as i64);
            }
            if q + 3 <= n {
                head3.insert(key3(data, q), q as i64);
            }
        };
        if skip > 0 {
            for q in i..(i + skip).min(i + 24) {
                insert(q);
            }
            for q in (i + 24).max((i + skip).saturating_sub(8))..(i + skip) {
                insert(q);
            }
            i += skip;
        } else {
            insert(i);
            i += 1;
        }
    }

    let mut path: Vec<(usize, usize, i64)> = Vec::new();
    let mut j = n;
    while j > 0 {
        let pi = prv[j] as usize;
        path.push((pi, j, koff[j]));
        j = pi;
    }
    path.reverse();
    let mut out = Bits::new();
    let mut last = 1i64;
    for (pi, j, off) in path {
        if off == 0 {
            out.bit(true);
            out.byte(data[pi]);
            continue;
        }
        let copies = (j - pi) as i64;
        out.bit(false);
        if off == last {
            out.gamma(2);
        } else {
            let raw = off + BIAS;
            out.gamma((raw >> 8) as u64);
            out.byte((raw & 0xFF) as u8);
            last = off;
        }
        out.length(copies - 1 - (off > FAR) as i64);
    }
    out.bit(false);
    out.gamma(END_GAMMA);
    out.byte(END_BYTE);
    out.buf
}

pub fn pack(data: &[u8]) -> Vec<u8> {
    pack_with(data, DEFAULT_DEPTH, DEFAULT_NICE)
}

/// pack(data) with the section header `[u32 stream length][u32 byte sum]`.
pub fn pack_section(data: &[u8]) -> Vec<u8> {
    let body = pack(data);
    let sum = body.iter().fold(0u32, |s, &b| s.wrapping_add(b as u32));
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&sum.to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// Reads the packed bit stream the way the firmware's depacker does (tag byte, 8 flags, MSB first).
struct Reader<'a> {
    d: &'a [u8],
    p: usize,
    end: usize,
    tag: u32,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        if self.p >= self.end {
            return Err("the packed stream ends before its end marker".into());
        }
        let v = self.d[self.p];
        self.p += 1;
        Ok(v)
    }
    fn bit(&mut self) -> Result<u32, String> {
        self.tag = (self.tag << 1) & 0x1ff;
        if self.tag & 0xff == 0 {
            let b = self.byte()? as u32;
            self.tag = (b << 1) | 1;
            return Ok(b >> 7);
        }
        Ok(self.tag >> 8)
    }
    fn gamma(&mut self) -> Result<u64, String> {
        let mut v: u64 = 1;
        loop {
            v = (v << 1) | self.bit()? as u64;
            if self.bit()? == 1 {
                return Ok(v);
            }
            if v > 0x0200_0000 {
                return Err("gamma code overflow".into());
            }
        }
    }
}

/// The slack, in bytes, of unpacking the section stream `stream` (with its 8-byte header) in place: the stream is staged at `stage`, the output
/// starts at `dest` and grows upward over the stream. The result is the smallest, over the tokens, of (stream bytes consumed) - (output bytes written
/// at or past `stage`). It is negative when a write would land on a stream byte that is not read yet, and `None` when the output never reaches
/// `stage`. This is `tools.firmware.aplibpack.inplace_margin`, which stays the reference.
pub fn inplace_margin(stream: &[u8], dest: u64, stage: u64) -> Result<Option<i64>, String> {
    if stream.len() < 8 {
        return Err("a section stream is shorter than its 8-byte header".into());
    }
    let length = u32::from_be_bytes(stream[0..4].try_into().unwrap()) as usize;
    let end = 8usize
        .checked_add(length)
        .filter(|&e| e <= stream.len())
        .ok_or("the stream header names more bytes than the stream holds")?;
    let mut s = Reader {
        d: stream,
        p: 8,
        end,
        tag: 0,
    };
    let room = stage as i64 - dest as i64;
    let (mut out, mut last): (i64, i64) = (0, 1);
    let mut margin: Option<i64> = None;
    loop {
        if s.bit()? == 1 {
            s.byte()?;
            out += 1;
        } else {
            let g = s.gamma()?;
            let off = if g == 2 {
                last
            } else {
                let raw = ((g << 8) + s.byte()? as u64) & 0xffff_ffff;
                if raw as i64 == BIAS {
                    break;
                }
                let off = raw as i64 - BIAS;
                last = off;
                off
            };
            let short = 2 * s.bit()? + s.bit()?;
            let mut ln = if short != 0 {
                short as i64
            } else {
                s.gamma()? as i64 + 2
            };
            if off > FAR {
                ln += 1;
            }
            out += ln + 1;
        }
        if out > room {
            let slack = s.p as i64 - (out - room);
            margin = Some(margin.map_or(slack, |m| m.min(slack)));
        }
    }
    Ok(margin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dt2_firmware_loader::depack_section;

    fn roundtrip(data: &[u8]) {
        let packed = pack_section(data);
        let back = depack_section(&packed, 0, 1 << 26).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn round_trips_through_the_firmware_decoder() {
        roundtrip(&[]);
        roundtrip(b"a");
        roundtrip(b"abcabcabcabcabcabc");
        roundtrip(&vec![0u8; 100_000]);
        let mut x = 12345u32;
        let noisy: Vec<u8> = (0..50_000)
            .map(|i| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                if i % 7 < 3 {
                    (x >> 24) as u8
                } else {
                    (i % 251) as u8
                }
            })
            .collect();
        roundtrip(&noisy);
    }
}

#[cfg(test)]
mod margin_tests {
    use super::*;

    #[test]
    fn the_in_place_margin_counts_stream_bytes_read_ahead_of_the_output() {
        // 64 distinct bytes pack to literals only: one tag byte per 8 bytes, then one byte per literal
        let data: Vec<u8> = (0..64).collect();
        let s = pack_section(&data);
        // the output never reaches the stage: no overlap
        assert_eq!(inplace_margin(&s, 0, 1000).unwrap(), None);
        // stage at 16: the output passes it at literal 17 (tags read: 1 + 2, stream header 8): consumed 8 + 17 + 3, written past 1
        assert_eq!(inplace_margin(&s, 0, 16).unwrap(), Some(27));
        // the stream staged at the output's start: the first literal already lands on unread bytes
        assert!(inplace_margin(&s, 0, 0).unwrap().unwrap() > 0);
        // a stream whose output grows 2 bytes per consumed byte (zeros) overruns a stage 1 KB ahead of the output
        let zeros = pack_section(&vec![0u8; 100_000]);
        assert!(inplace_margin(&zeros, 0, 1024).unwrap().unwrap() < 0);
        assert_eq!(inplace_margin(&zeros, 0, 100_000).unwrap(), None);
        assert!(inplace_margin(&s[..10], 0, 16).is_err());
    }
}
