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

/// One token of the packed path: `data[from..to]` as one literal (`off == 0`, `to == from + 1`) or as a match at distance `off`.
#[derive(Clone, Copy, Debug)]
pub struct Tok {
    pub from: usize,
    pub to: usize,
    pub off: i64,
}

/// data -> aPLib-shaped stream (no section header), ending with the marker.
pub fn pack_with(data: &[u8], depth: usize, nice: usize) -> Vec<u8> {
    let toks = plan_with(data, depth, nice);
    emit(data, &toks, data.len())
}

/// The optimal path of `data` as tokens (the dynamic program of `pack_with`).
pub fn plan_with(data: &[u8], depth: usize, nice: usize) -> Vec<Tok> {
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

    let mut path: Vec<Tok> = Vec::new();
    let mut j = n;
    while j > 0 {
        let pi = prv[j] as usize;
        path.push(Tok {
            from: pi,
            to: j,
            off: koff[j],
        });
        j = pi;
    }
    path.reverse();
    path
}

/// The stream of a path with everything from output position `literal_from` on written as literals: a token that starts at or after it becomes literals,
/// and a match that straddles it is cut there (a shorter match keeps the part before the cut, the rest is literals), so the tail is as short as asked.
/// A tail of literals consumes one stream byte per output byte, which is what an in-place unpack needs at the end. `literal_from == data.len()` is the
/// plain stream.
pub fn emit(data: &[u8], toks: &[Tok], literal_from: usize) -> Vec<u8> {
    let mut out = Bits::new();
    let mut last = 1i64;
    for t in toks {
        let mut to = t.to;
        if t.off != 0 && t.from < literal_from && t.to > literal_from {
            // the cut falls inside this match: keep a shorter match when it is still a match, else all of it goes to literals
            let min = 2 + (t.off > FAR) as usize;
            if literal_from - t.from >= min {
                to = literal_from;
            } else {
                for &b in &data[t.from..t.to] {
                    out.bit(true);
                    out.byte(b);
                }
                continue;
            }
        }
        if t.off == 0 || t.from >= literal_from {
            for &b in &data[t.from..t.to] {
                out.bit(true);
                out.byte(b);
            }
            continue;
        }
        let copies = (to - t.from) as i64;
        out.bit(false);
        if t.off == last {
            out.gamma(2);
        } else {
            let raw = t.off + BIAS;
            out.gamma((raw >> 8) as u64);
            out.byte((raw & 0xFF) as u8);
            last = t.off;
        }
        out.length(copies - 1 - (t.off > FAR) as i64);
        for &b in &data[to..t.to] {
            out.bit(true);
            out.byte(b);
        }
    }
    out.bit(false);
    out.gamma(END_GAMMA);
    out.byte(END_BYTE);
    out.buf
}

pub fn pack(data: &[u8]) -> Vec<u8> {
    pack_with(data, DEFAULT_DEPTH, DEFAULT_NICE)
}

/// `body` (a packed stream) with the section header `[u32 stream length][u32 byte sum]`.
pub fn section_of(body: Vec<u8>) -> Vec<u8> {
    let sum = body.iter().fold(0u32, |s, &b| s.wrapping_add(b as u32));
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&sum.to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// pack(data) with the section header `[u32 stream length][u32 byte sum]`.
pub fn pack_section(data: &[u8]) -> Vec<u8> {
    section_of(pack(data))
}

/// A section stream made for an unpack in place (the bootstrap stages the stream at `stage` and unpacks it from `dest` over it).
#[derive(Debug)]
pub struct InPlace {
    pub stream: Vec<u8>,
    /// Output bytes at the end that are stored as literals to keep the margin (0: the plain stream).
    pub literal_tail: usize,
    /// The margin of `stream` (`None`: the output never reaches `stage`).
    pub margin: Option<i64>,
    /// The length of the plain stream, for what the tail costs.
    pub plain_len: usize,
}

/// `pack_section(data)` when its in-place margin at `stage` is at least `floor` (or the output never reaches `stage`); otherwise the same path with the
/// shortest tail of the data stored as literals that brings the margin to `floor`. The bootstrap's depacker reads such literals like any other. Err when
/// even a stream of literals only does not reach the floor.
pub fn pack_section_in_place(
    data: &[u8],
    dest: u64,
    stage: u64,
    floor: i64,
) -> Result<InPlace, String> {
    let toks = plan_with(data, DEFAULT_DEPTH, DEFAULT_NICE);
    let plain = section_of(emit(data, &toks, data.len()));
    let plain_len = plain.len();
    let ok = |m: &Option<i64>| m.is_none_or(|m| m >= floor);
    let margin = inplace_margin(&plain, dest, stage)?;
    if ok(&margin) {
        return Ok(InPlace {
            stream: plain,
            literal_tail: 0,
            margin,
            plain_len,
        });
    }
    // the stream with the data from output position `from` on as literals: `from` = 0 is all literals, `from` = data.len() the plain stream
    let at = |from: usize| {
        let s = section_of(emit(data, &toks, from));
        let m = inplace_margin(&s, dest, stage);
        (s, m, data.len() - from)
    };
    let (s0, m0, tail0) = at(0);
    let m0 = m0?;
    if !ok(&m0) {
        return Err(format!(
            "even a stream of literals only has margin {} at stage {stage:#x} (floor {floor}): the unpacked image ({} bytes from {dest:#x}) is too large for this stage",
            m0.unwrap_or(0),
            data.len()
        ));
    }
    // binary search for the largest `from` (the shortest tail) that keeps the margin: lo is known good, hi is known bad (the plain stream)
    let (mut lo, mut hi) = (0usize, data.len());
    let mut best = (s0, m0, tail0);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        let (s, m, tail) = at(mid);
        let m = m?;
        if ok(&m) {
            lo = mid;
            best = (s, m, tail);
        } else {
            hi = mid;
        }
    }
    Ok(InPlace {
        stream: best.0,
        literal_tail: best.2,
        margin: best.1,
        plain_len,
    })
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

#[cfg(test)]
mod tail_tests {
    use super::*;
    use dt2_firmware_loader::depack_section;

    /// Pseudo-random bytes (no matches inside).
    fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect()
    }

    /// Data with a far match (distance above FAR) and matches that set a new offset after another.
    fn mixed() -> Vec<u8> {
        let a = noise(9000, 1);
        let mut d = a.clone();
        d.extend_from_slice(&a[..6000]); // distance 9000 > FAR: a far match
        d.extend(noise(50, 2));
        d.extend_from_slice(&a[1000..4000]); // another distance: a new offset
        d.extend(noise(30, 3));
        d.extend_from_slice(&a[200..2200]); // and another
        d.extend(noise(40, 4));
        d
    }

    #[test]
    fn a_cut_inside_a_far_match_or_a_match_with_a_new_offset_still_decodes_to_the_input() {
        let data = mixed();
        let toks = plan_with(&data, DEFAULT_DEPTH, DEFAULT_NICE);
        let far = toks
            .iter()
            .find(|t| t.off > FAR && t.to - t.from > 100)
            .expect("a far match");
        let shifted: Vec<&Tok> = toks
            .iter()
            .filter(|t| t.off != 0 && t.off != far.off && t.to - t.from > 100)
            .collect();
        assert!(!shifted.is_empty(), "a match with another offset");
        let mut cuts = vec![far.from + 1, far.from + (far.to - far.from) / 2, far.to - 1];
        for t in shifted {
            cuts.extend([t.from + 3, t.from + (t.to - t.from) / 2, t.to - 1]);
        }
        for cut in cuts {
            let s = section_of(emit(&data, &toks, cut));
            assert_eq!(
                depack_section(&s, 0, 1 << 20).unwrap(),
                data,
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn the_in_place_tail_reaches_the_floor_when_the_cut_falls_in_such_matches() {
        let data = mixed();
        // a stage inside the data and a floor: where the plain stream's margin is below the floor a tail is needed and taken
        let mut needed = 0;
        for (dest, stage, floor) in [(0u64, 11_000u64, 64i64), (0, 13_000, 200), (0, 9_500, 32)] {
            let made = pack_section_in_place(&data, dest, stage, floor).unwrap();
            assert_eq!(depack_section(&made.stream, 0, 1 << 20).unwrap(), data);
            assert!(made.margin.is_none_or(|m| m >= floor), "{:?}", made.margin);
            assert_eq!(
                inplace_margin(&made.stream, dest, stage).unwrap(),
                made.margin
            );
            let plain = inplace_margin(&pack_section(&data), dest, stage).unwrap();
            if plain.is_some_and(|m| m < floor) {
                needed += 1;
                assert!(
                    made.literal_tail > 0,
                    "the plain margin {plain:?} is below {floor}: a tail is taken"
                );
                assert!(
                    made.literal_tail < data.len(),
                    "and it is a tail, not all of the data"
                );
                assert!(made.stream.len() > pack_section(&data).len());
            } else {
                assert_eq!(
                    made.literal_tail, 0,
                    "the plain stream is kept when it holds"
                );
            }
        }
        assert!(
            needed > 0,
            "a tail was needed and taken in at least one case"
        );
    }
}
