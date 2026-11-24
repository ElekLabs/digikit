//! The data sections: every entry in file order.
use super::*;
use dkrt_sdk::settings::{entry_at, get32};

/// Every entry of every data section, in file order: (owner, key as stored, value).
pub struct Entries<'a> {
    b: &'a [u8],
    used: usize,
    /// The next section to look at, and inside a data section the next entry and the section's end and owner.
    sec: usize,
    at: usize,
    end: usize,
    owner: (usize, usize),
}

pub fn entries(b: &[u8]) -> Entries<'_> {
    let used = (get32(b, FMT.o_used()) as usize).min(b.len());
    Entries {
        b,
        used,
        sec: HEADER,
        at: 0,
        end: 0,
        owner: (0, 0),
    }
}

impl<'a> Iterator for Entries<'a> {
    type Item = (&'a [u8], &'a [u8], &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.at < self.end {
                let e = entry_at(self.b, self.end, self.at)?;
                self.at = e.end;
                return Some((
                    &self.b[self.owner.0..self.owner.0 + self.owner.1],
                    &self.b[e.key.0..e.key.0 + e.key.1],
                    &self.b[e.value.0..e.value.0 + e.value.1],
                ));
            }
            if self.sec >= self.used {
                return None;
            }
            let s = sec_at(self.b, self.used, self.sec)?;
            self.sec = s.end;
            if let Some((o, n)) = owner_of(self.b, &s) {
                self.owner = (o, n);
                self.at = o + n;
                self.end = s.end;
            }
        }
    }
}
