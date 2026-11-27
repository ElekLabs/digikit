use super::*;
extern crate std;
use std::collections::VecDeque;
use std::vec::Vec;

/// One transfer the model accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wire {
    pub win: u32,
    pub dst: u32,
    pub bytes: Vec<u8>,
}

/// `core/dsp-link` as the queue sees it: a send is refused while a transfer runs (BUSY) or the END frames follow it; the state turns DONE or ERROR after
/// `TRANSFER` blocks and stays so until the next send. Every call of the model is one block.
pub struct FakeLink {
    pub now: u32,
    busy_until: u32,
    end_until: u32,
    result: Option<Poll>,
    pub wire: Vec<Wire>,
    /// The answer for each accepted transfer, in order (`None`: DONE; `Some(code)`: ERROR); DONE when empty.
    pub script: VecDeque<Option<u32>>,
    pub refuse: bool,
    pub hard: bool,
    pub events: Vec<(u32, [u32; 4])>,
}

pub const TRANSFER: u32 = 3;
pub const END_FRAMES: u32 = 2;

impl FakeLink {
    pub fn new() -> Self {
        FakeLink {
            now: 0,
            busy_until: 0,
            end_until: 0,
            result: None,
            wire: Vec::new(),
            script: VecDeque::new(),
            refuse: false,
            hard: false,
            events: Vec::new(),
        }
    }
    pub fn words(&self) -> Vec<(u32, u32, u32)> {
        self.wire
            .iter()
            .map(|w| {
                (
                    w.win,
                    w.dst,
                    u32::from_le_bytes([w.bytes[0], w.bytes[1], w.bytes[2], w.bytes[3]]),
                )
            })
            .collect()
    }
}

impl Transport for FakeLink {
    unsafe fn send(&mut self, win: u32, dst: u32, buf: *const u8, len: u32) -> Sent {
        self.now += 1;
        if self.hard {
            return Sent::Hard;
        }
        if self.refuse || self.now < self.busy_until || self.now < self.end_until {
            return Sent::Refused;
        }
        self.wire.push(Wire {
            win,
            dst,
            bytes: core::slice::from_raw_parts(buf, len as usize).to_vec(),
        });
        self.busy_until = self.now + TRANSFER;
        self.end_until = self.busy_until + END_FRAMES;
        self.result = Some(match self.script.pop_front().flatten() {
            None => Poll::Done,
            Some(c) => Poll::Error(c),
        });
        Sent::Accepted
    }
    unsafe fn poll(&mut self) -> Poll {
        self.now += 1;
        if self.now < self.busy_until {
            return Poll::Busy;
        }
        self.result.unwrap_or(Poll::Busy)
    }
    fn event(&mut self, what: u32, a: [u32; 4]) {
        self.events.push((what, a));
    }
}

pub fn link() -> Link<FakeLink> {
    Link::new(FakeLink::new())
}
