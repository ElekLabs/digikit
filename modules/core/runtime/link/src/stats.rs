//! The ColdFire mirror of the DSP stats window (docs/plans/profiling.md 13.1, P8d).
//!
//! While stats are on, the DSP writes one page of its stats window per block into the acknowledgement slot: a tag word and two data words. `dl_tick`
//! hands every ack record to `Mirror::feed`. The mirror assembles the pages of one sweep (0 to 49, in order) in a back buffer and publishes only a complete
//! sweep: the back buffer becomes the front buffer and `seq` moves. A skipped page, a new generation (the host cleared the counters) or a page 0 in the
//! middle of a sweep ends that sweep without publishing it. A transfer being acknowledged takes the slot, so its blocks give no page and the sweep in
//! progress is dropped. A reader (`read`) copies the front buffer and retries when `seq` moved. Values inside one sweep are up to `PAGES` blocks apart.

/// Words of the window: a header of 4 words, then 16 rows of 6 words.
pub const WINDOW_WORDS: usize = 100;
/// Data words of one page.
pub const PAGE_WORDS: usize = 2;
/// Pages of one sweep.
pub const PAGES: usize = WINDOW_WORDS / PAGE_WORDS;
/// Bits 0-15 of the tag word. An acknowledgement has `DL_ACK_MAGIC` (0xA5D1) there.
pub const STATS_MAGIC: u32 = 0x57A7;

/// Tag word of a page of generation `gen`.
pub const fn tag(gen: u8, page: u8) -> u32 {
    STATS_MAGIC | (page as u32) << 16 | (gen as u32) << 24
}

const WAIT: u8 = 0xff;

/// What the mirror counted, for PROFILE_DSP.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Counts {
    /// Complete sweeps published.
    pub sweeps: u32,
    /// Sweeps ended by a skipped page, a generation change or a new page 0.
    pub gaps: u32,
    /// Records ignored: another magic, a page out of range, or the same page again.
    pub ignored: u32,
}

pub struct Mirror {
    buf: [[u32; WINDOW_WORDS]; 2],
    front: usize,
    seq: u32,
    valid: bool,
    gen: u8,
    have_gen: bool,
    /// The page the sweep in progress needs next (`WAIT`: none in progress, only page 0 starts one).
    next: u8,
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
            next: WAIT,
            last_page: WAIT,
            counts: Counts {
                sweeps: 0,
                gaps: 0,
                ignored: 0,
            },
        }
    }

    fn end_sweep(&mut self) {
        if self.next != WAIT {
            self.counts.gaps += 1;
        }
        self.next = WAIT;
    }

    /// One acknowledgement record. Returns true when it was a stats page (taken, a repeat included), false for anything else.
    pub fn feed(&mut self, tag: u32, a: u32, b: u32) -> bool {
        if tag & 0xffff != STATS_MAGIC {
            self.counts.ignored += 1;
            return false;
        }
        let page = ((tag >> 16) & 0xff) as u8;
        let gen = (tag >> 24) as u8;
        if page as usize >= PAGES {
            self.counts.ignored += 1;
            return true;
        }
        if !self.have_gen || gen != self.gen {
            // the counters were cleared (or this is the first page): the published sweep is stale
            self.end_sweep();
            self.gen = gen;
            self.have_gen = true;
            self.last_page = WAIT;
            if self.valid {
                self.valid = false;
                self.seq = self.seq.wrapping_add(1);
            }
        }
        if page == self.last_page {
            // the ColdFire read the same block's slot twice
            self.counts.ignored += 1;
            return true;
        }
        self.last_page = page;
        let back = 1 - self.front;
        if page == 0 {
            self.end_sweep();
            self.next = 0;
        } else if self.next != page {
            self.end_sweep();
            return true;
        }
        let at = page as usize * PAGE_WORDS;
        self.buf[back][at] = a;
        self.buf[back][at + 1] = b;
        self.next = page + 1;
        if self.next as usize == PAGES {
            self.front = back;
            self.valid = true;
            self.seq = self.seq.wrapping_add(1);
            self.counts.sweeps += 1;
            self.next = WAIT;
        }
        true
    }

    /// Copy the newest complete sweep to `out`; returns its sequence number, or None when no sweep of the current generation is complete yet.
    pub fn read(&self, out: &mut [u32; WINDOW_WORDS]) -> Option<u32> {
        loop {
            let s1 = self.seq;
            if !self.valid {
                return None;
            }
            *out = self.buf[self.front];
            if self.seq == s1 {
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
    fn a_skipped_page_drops_the_sweep_and_keeps_the_last_complete_one() {
        let mut m = Mirror::new();
        sweep(&mut m, 1, 1);
        for p in (0..PAGES as u8).filter(|p| *p != 20) {
            page(&mut m, 1, p, 2);
        }
        let (s, w) = snap(&m).unwrap();
        assert_eq!((s, w[0]), (1, 1000), "the torn sweep is not published");
        assert_eq!(m.counts.gaps, 1);
        sweep(&mut m, 1, 3);
        assert_eq!(
            snap(&m).unwrap().1[0],
            3000,
            "the next sweep from page 0 is"
        );
    }

    #[test]
    fn a_transfer_ack_between_pages_is_not_a_page_and_costs_the_sweep() {
        let mut m = Mirror::new();
        for p in 0..10 {
            page(&mut m, 1, p, 1);
        }
        // an acknowledgement (DL_ACK_MAGIC in the low half) holds the slot for two blocks
        assert!(!m.feed(0x0001_A5D1, 0x0100, 0));
        assert!(!m.feed(0x0002_A5D1, 0x0100, 0));
        for p in 12..PAGES as u8 {
            page(&mut m, 1, p, 1);
        }
        assert!(snap(&m).is_none());
        assert_eq!(m.counts.sweeps, 0);
        assert_eq!(m.counts.ignored, 2);
        assert_eq!(m.counts.gaps, 1);
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
        assert_eq!(m.counts.gaps, 1);
        sweep(&mut m, 2, 4);
        assert_eq!(snap(&m).unwrap().1[0], 4000);
    }

    #[test]
    fn a_sweep_starts_only_at_page_0_and_page_0_restarts_one() {
        let mut m = Mirror::new();
        for p in 30..PAGES as u8 {
            page(&mut m, 1, p, 1);
        }
        assert!(snap(&m).is_none());
        assert_eq!(m.counts.gaps, 0, "no sweep was in progress");
        for p in 0..4 {
            page(&mut m, 1, p, 1);
        }
        sweep(&mut m, 1, 2);
        assert_eq!(snap(&m).unwrap().1[0], 2000);
        assert_eq!(m.counts.gaps, 1, "page 0 inside a sweep ended it");
    }

    #[test]
    fn a_page_out_of_range_is_ignored() {
        let mut m = Mirror::new();
        assert!(m.feed(tag(1, PAGES as u8), 1, 2));
        assert_eq!(m.counts.ignored, 1);
    }
}
