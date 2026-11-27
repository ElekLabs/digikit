use crate::voice_host_table::VoiceHostTable;
use dkrt_sdk::voice_host::NAME_HASH;

/// The state of the host part: it owns the `voice_host` table and nothing else yet. `start` finds the table in the image, `step` runs once per DSP pass, `stop` takes the table back.
pub struct Host {
    table: Option<VoiceHostTable>,
}

impl Host {
    pub const fn new() -> Host {
        Host { table: None }
    }

    /// Find the `voice_host` table through `info` (the image's info table by name hash) and prepare it. An image without the table has no host: the part stays idle and answers 0.
    ///
    /// # Safety
    /// `info` answers null or the writable words of the image's entry.
    pub unsafe fn start(&mut self, info: &dyn Fn(u32) -> *mut u32) -> i32 {
        let words = info(NAME_HASH);
        if words.is_null() {
            return 0;
        }
        let mut table = VoiceHostTable::at(words);
        table.prepare();
        self.table = Some(table);
        0
    }

    /// One pass of the UI loop. Nothing to do until the host serves voices.
    pub fn step(&mut self) -> i32 {
        0
    }

    /// Take the table back (the magic first).
    pub fn stop(&mut self) -> i32 {
        if let Some(t) = self.table.as_mut() {
            t.withdraw();
        }
        self.table = None;
        0
    }

    /// Has the host found its table?
    pub fn started(&self) -> bool {
        self.table.is_some()
    }
}

#[cfg(test)]
mod tests;
