//! The ColdFire mirror of the DSP stats window (docs/plans/profiling.md 13.1, P8d).
//!
//! While stats are on, the DSP writes one page of its stats window per block into the acknowledgement slot: a tag word and two data words. `dl_tick`
//! hands every ack record to `Mirror::feed`. Pages carry their index (0 to 49) and the clear generation: the mirror keeps the newest copy of each page in a
//! back buffer and publishes the buffer as one sweep when every page is there, so a block that skipped its page (a transfer being acknowledged takes the
//! slot) only delays the sweep by a cycle of 50 blocks. A new generation (the host cleared the counters) drops everything; when the runtime told the mirror
//! which generation to expect (`expect`), a page of another one is ignored (the slot of a block with stats off holds stock data). A reader (`read`) copies the
//! front buffer and retries when `seq` moved. Values inside one sweep are up to `PAGES` blocks apart, more when pages were skipped.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};

/// Words of the window: a header of 4 words, then 16 rows of 6 words.
pub const WINDOW_WORDS: usize = 100;
/// Data words of one page.
pub const PAGE_WORDS: usize = 2;
/// Pages of one sweep.
pub const PAGES: usize = WINDOW_WORDS / PAGE_WORDS;
/// Bits 0-15 of the tag word. An acknowledgement has `DL_ACK_MAGIC` (0xA5D1) there.
pub const STATS_MAGIC: u32 = 0x57A7;
const ALL: u64 = (1u64 << PAGES) - 1;

/// Tag word of a page of generation `gen`.
pub const fn tag(gen: u8, page: u8) -> u32 {
    STATS_MAGIC | (page as u32) << 16 | (gen as u32) << 24
}

const NONE: u8 = 0xff;

/// What the mirror counted, for PROFILE_DSP.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Counts {
    /// Complete sweeps published.
    pub sweeps: u32,
    /// Pages that did not follow the one before (a block gave none).
    pub gaps: u32,
    /// Records ignored: another magic or generation, a page out of range, or the same page again.
    pub ignored: u32,
}

pub struct Mirror {
    buf: [[u32; WINDOW_WORDS]; 2],
    front: usize,
    seq: u32,
    valid: bool,
    gen: u8,
    have_gen: bool,
    /// The generation the runtime expects (`None`: take the first page's).
    expect: Option<u8>,
    /// Pages of the back buffer that hold data of the sweep in progress.
    got: u64,
    last_page: u8,
    pub counts: Counts,
}

impl Mirror {
    pub const fn new() -> Mirror {
        Mirror {
            buf: [[0; WINDOW_WORDS]; 2],
            front: 0,
            seq: 0,
            valid: false,
            gen: 0,
            have_gen: false,
            expect: None,
            got: 0,
            last_page: NONE,
            counts: Counts {
                sweeps: 0,
                gaps: 0,
                ignored: 0,
            },
        }
    }

    /// Start again from nothing for clear generation `gen`: no sweep, no counts, and a page of another generation is ignored from now on.
    pub fn reset_for(&mut self, gen: u8) {
        *self = Mirror::new();
        self.expect = Some(gen);
    }

    /// One acknowledgement record. Returns true when it was a stats page (taken, a repeat included), false for anything else.
    pub fn feed(&mut self, tag: u32, a: u32, b: u32) -> bool {
        if tag & 0xffff != STATS_MAGIC {
            self.counts.ignored += 1;
            return false;
        }
        let page = ((tag >> 16) & 0xff) as u8;
        let gen = (tag >> 24) as u8;
        if page as usize >= PAGES || self.expect.is_some_and(|e| e != gen) {
            self.counts.ignored += 1;
            return false;
        }
        if !self.have_gen || gen != self.gen {
            // the counters were cleared (or this is the first page): the published sweep is stale
            self.gen = gen;
            self.have_gen = true;
            self.last_page = NONE;
            self.got = 0;
            if self.valid {
                self.valid = false;
                self.bump();
            }
        }
        if page == self.last_page {
            // the ColdFire read the same block's slot twice
            self.counts.ignored += 1;
            return true;
        }
        if self.last_page != NONE && page != (self.last_page + 1) % PAGES as u8 {
            self.counts.gaps += 1;
        }
        self.last_page = page;
        let back = 1 - self.front;
        let at = page as usize * PAGE_WORDS;
        self.buf[back][at] = a;
        self.buf[back][at + 1] = b;
        self.got |= 1 << page;
        if self.got == ALL {
            self.front = back;
            self.valid = true;
            self.bump();
            self.counts.sweeps += 1;
            self.got = 0;
        }
        true
    }

    /// The sweep just published, for a host event: its number (what `read` reports), its clear generation and the block count of its header. `None` without one.
    pub fn published(&self) -> Option<(u32, u8, u32)> {
        if !self.valid {
            return None;
        }
        // checked: this runs in the block interrupt, which must not link a bounds-check panic
        let blocks = *self.buf.get(self.front)?.first()?;
        Some((self.seq, self.gen, blocks))
    }

    /// `seq` moves with every publish; written with volatile stores, read the same way, so that the reader's recheck stays.
    fn bump(&mut self) {
        compiler_fence(Ordering::SeqCst);
        unsafe { write_volatile(&mut self.seq, self.seq.wrapping_add(1)) };
        compiler_fence(Ordering::SeqCst);
    }

    /// Copy the newest complete sweep to `out`; returns its sequence number, or None when no sweep of the current generation is complete yet.
    pub fn read(&self, out: &mut [u32; WINDOW_WORDS]) -> Option<u32> {
        loop {
            let s1 = unsafe { read_volatile(&self.seq) };
            compiler_fence(Ordering::SeqCst);
            if !unsafe { read_volatile(&self.valid) } {
                return None;
            }
            let front = unsafe { read_volatile(&self.front) };
            for (i, w) in out.iter_mut().enumerate() {
                *w = unsafe { read_volatile(&self.buf[front][i]) };
            }
            compiler_fence(Ordering::SeqCst);
            if unsafe { read_volatile(&self.seq) } == s1 {
                return Some(s1);
            }
        }
    }
}

impl Default for Mirror {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Page `p` of generation `g` with data derived from `v` so a sweep of one `v` is recognisable.
    fn page(m: &mut Mirror, g: u8, p: u8, v: u32) -> bool {
        m.feed(
            tag(g, p),
            v * 1000 + p as u32 * 2,
            v * 1000 + p as u32 * 2 + 1,
        )
    }

    fn sweep(m: &mut Mirror, g: u8, v: u32) {
        for p in 0..PAGES as u8 {
            page(m, g, p, v);
        }
    }

    fn snap(m: &Mirror) -> Option<(u32, [u32; WINDOW_WORDS])> {
        let mut o = [0; WINDOW_WORDS];
        m.read(&mut o).map(|s| (s, o))
    }

    #[test]
    fn nothing_is_published_before_a_sweep_is_complete() {
        let mut m = Mirror::new();
        for p in 0..PAGES as u8 - 1 {
            page(&mut m, 1, p, 1);
        }
        assert!(snap(&m).is_none());
        page(&mut m, 1, PAGES as u8 - 1, 1);
        let (_, w) = snap(&m).unwrap();
        assert_eq!((w[0], w[1], w[99]), (1000, 1001, 1099));
        assert_eq!(
            m.counts,
            Counts {
                sweeps: 1,
                gaps: 0,
                ignored: 0
            }
        );
    }

    #[test]
    fn a_skipped_page_delays_the_sweep_by_a_cycle_and_the_newer_copies_win() {
        let mut m = Mirror::new();
        sweep(&mut m, 1, 1);
        for p in (0..PAGES as u8).filter(|p| *p != 20) {
            page(&mut m, 1, p, 2);
        }
        let (s, w) = snap(&m).unwrap();
        assert_eq!(
            (s, w[0]),
            (1, 1000),
            "the incomplete sweep is not published"
        );
        assert_eq!(m.counts.gaps, 1);
        // the next cycle brings page 20 (and the pages before it again): the sweep is complete with the newest copy of each
        for p in 0..=20u8 {
            page(&mut m, 1, p, 3);
        }
        let (s, w) = snap(&m).unwrap();
        assert_eq!(s, 2);
        assert_eq!(
            (w[0], w[40], w[41], w[2 * 21]),
            (3000, 3040, 3041, 2042),
            "page 20 is new, the later pages are from the cycle before"
        );
        assert_eq!(m.counts.sweeps, 2);
    }

    #[test]
    fn a_transfer_ack_between_pages_is_not_a_page_and_only_delays_the_sweep() {
        let mut m = Mirror::new();
        for p in 0..10 {
            page(&mut m, 1, p, 1);
        }
        // an acknowledgement (DL_ACK_MAGIC in the low half) holds the slot for two blocks: pages 10 and 11 are not sent
        assert!(!m.feed(0x0001_A5D1, 0x0100, 0));
        assert!(!m.feed(0x0002_A5D1, 0x0100, 0));
        for p in 12..PAGES as u8 {
            page(&mut m, 1, p, 1);
        }
        assert!(snap(&m).is_none());
        assert_eq!((m.counts.ignored, m.counts.gaps), (2, 1));
        page(&mut m, 1, 0, 1);
        page(&mut m, 1, 1, 1);
        page(&mut m, 1, 2, 1);
        for p in 10..12 {
            page(&mut m, 1, p, 1);
        }
        assert_eq!(
            m.counts.sweeps, 1,
            "the missing pages came with the next cycle"
        );
    }

    #[test]
    fn the_same_page_read_twice_is_ignored() {
        let mut m = Mirror::new();
        for p in 0..PAGES as u8 {
            page(&mut m, 1, p, 1);
            if p == 7 {
                assert!(page(&mut m, 1, 7, 9), "a repeat is still a stats record");
            }
        }
        assert_eq!(
            snap(&m).unwrap().1[14],
            1014,
            "the first copy of page 7 stays"
        );
        assert_eq!(
            m.counts,
            Counts {
                sweeps: 1,
                gaps: 0,
                ignored: 1
            }
        );
    }

    #[test]
    fn a_reader_keeps_the_old_sweep_while_a_new_one_is_assembled() {
        let mut m = Mirror::new();
        sweep(&mut m, 1, 1);
        for p in 0..PAGES as u8 - 1 {
            page(&mut m, 1, p, 2);
        }
        assert_eq!(snap(&m).unwrap().1[0], 1000);
        page(&mut m, 1, PAGES as u8 - 1, 2);
        let (s, w) = snap(&m).unwrap();
        assert_eq!((s, w[0], w[99]), (2, 2000, 2099));
        sweep(&mut m, 1, 3);
        assert_eq!(snap(&m).unwrap().1[0], 3000, "the two buffers swap again");
    }

    #[test]
    fn a_new_generation_clears_the_published_sweep() {
        let mut m = Mirror::new();
        sweep(&mut m, 1, 1);
        for p in 0..5 {
            page(&mut m, 1, p, 2);
        }
        page(&mut m, 2, 5, 3);
        assert!(snap(&m).is_none(), "stale counters are not shown");
        sweep(&mut m, 2, 4);
        assert_eq!(snap(&m).unwrap().1[0], 4000);
    }

    #[test]
    fn a_sweep_can_start_at_any_page() {
        let mut m = Mirror::new();
        for p in 30..PAGES as u8 {
            page(&mut m, 1, p, 1);
        }
        assert!(snap(&m).is_none());
        for p in 0..30 {
            page(&mut m, 1, p, 1);
        }
        assert_eq!(snap(&m).unwrap().1[2 * 30], 1060);
        assert_eq!(m.counts.gaps, 0, "the cycle wraps from page 49 to 0");
    }

    #[test]
    fn a_page_out_of_range_or_of_another_generation_than_expected_is_ignored() {
        let mut m = Mirror::new();
        assert!(!m.feed(tag(1, PAGES as u8), 1, 2));
        m.reset_for(7);
        assert!(
            !page(&mut m, 8, 3, 1),
            "stock data in the slot that happens to carry the magic"
        );
        assert_eq!(m.counts.ignored, 1);
        sweep(&mut m, 7, 1);
        assert_eq!(m.counts.sweeps, 1);
    }
}
