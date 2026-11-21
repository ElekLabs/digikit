/// A fixed output buffer of words (what `Describe::describe` fills). Words past the capacity are counted and dropped.
pub struct WordSink<'a> {
    out: &'a mut [u32],
    len: usize,
    dropped: usize,
}

impl<'a> WordSink<'a> {
    pub fn new(out: &'a mut [u32]) -> WordSink<'a> {
        WordSink {
            out,
            len: 0,
            dropped: 0,
        }
    }

    /// Append a word; false (and the word is dropped) when the buffer is full.
    pub fn push(&mut self, word: u32) -> bool {
        match self.out.get_mut(self.len) {
            Some(slot) => {
                *slot = word;
                self.len += 1;
                true
            }
            None => {
                self.dropped += 1;
                false
            }
        }
    }

    /// The words written so far.
    pub fn words(&self) -> &[u32] {
        &self.out[..self.len]
    }

    /// How many words did not fit.
    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_past_the_capacity_are_counted_not_written() {
        let mut buf = [0u32; 2];
        let mut s = WordSink::new(&mut buf);
        assert!(s.push(7) && s.push(8));
        assert!(!s.push(9));
        assert_eq!((s.words(), s.dropped()), (&[7, 8][..], 1));
    }
}
