/// Counters of what the link did, for `DKRT_DSP`-style peeks and the tests.
#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    pub live_sent: u32,
    pub live_errors: u32,
    pub req_done: u32,
    pub req_failed: u32,
}
