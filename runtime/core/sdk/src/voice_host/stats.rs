/// Counters of one voice type, the row of PROFILE_DSP (`dk_voice_stats`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Stats {
    pub(super) calls: u32,
    sum: u32,
    max_call: u32,
    max_block: u32,
    overruns: u32,
    faults: u32,
}

impl Stats {
    /// Calls counted.
    pub fn calls(&self) -> u32 {
        self.calls
    }
    /// Sum of the call costs, in the host's units.
    pub fn sum(&self) -> u32 {
        self.sum
    }
    /// The largest single call.
    pub fn max_call(&self) -> u32 {
        self.max_call
    }
    /// The largest total of one block.
    pub fn max_block(&self) -> u32 {
        self.max_block
    }
    pub fn overruns(&self) -> u32 {
        self.overruns
    }
    pub fn faults(&self) -> u32 {
        self.faults
    }
}
