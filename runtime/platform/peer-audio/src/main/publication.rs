use dkrt_sdk::{E_BUSY, E_CONTRACT, E_READ};

use crate::mailbox::{FreeBank, Mailbox};
use crate::sample_file::{SampleFile, Sector};

const WAIT_PASSES: u16 = 600;

extern "C" {
    fn dkrt_poll_add(f: usize) -> i32;
    fn dkrt_poll_remove(f: usize);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Send,
    Write,
    Publish,
}

struct Publication {
    file: SampleFile,
    bank: FreeBank,
    generation: u32,
    cursor: usize,
    phase: Phase,
    waits: u16,
}

impl Publication {
    fn step(&mut self, mailbox: &Mailbox, sector: &mut Sector) -> Result<bool, i32> {
        if self.phase != Phase::Send {
            if !mailbox.acknowledged()? {
                self.waits += 1;
                return if self.waits == WAIT_PASSES {
                    Err(E_READ)
                } else {
                    Ok(false)
                };
            }
            if self.phase == Phase::Publish {
                return Ok(true);
            }
        }
        self.waits = 0;
        if self.cursor == self.file.frames() {
            mailbox.publish(&self.bank, self.generation, self.cursor);
            self.phase = Phase::Publish;
        } else {
            let pcm = self.file.read(self.cursor, sector)?;
            mailbox.write(&self.bank, self.generation, self.cursor, pcm);
            self.cursor += pcm.len() / 2;
            self.phase = Phase::Write;
        }
        Ok(false)
    }
}

struct Completion {
    error: i32,
    late_generation: Option<u32>,
}

struct Publisher {
    pending: Option<Publication>,
    completed: Option<Completion>,
    next_generation: u32,
    sector: Sector,
}

impl Publisher {
    fn status(&self, generation: Option<u32>) -> i32 {
        if self.pending.is_some() {
            return E_BUSY;
        }
        if let Some(failure) = &self.completed {
            if generation.is_none() || generation != failure.late_generation {
                return failure.error;
            }
        }
        generation.map_or(E_BUSY, |current| current as i32)
    }
}

static mut PUBLISHER: Publisher = Publisher {
    pending: None,
    completed: None,
    next_generation: 1,
    sector: Sector::empty(),
};

extern "C" fn poll() {
    // Each entry point belongs to the same UI task and card calls leave its state alone.
    let state = unsafe { &mut *(&raw mut PUBLISHER) };
    let Some(load) = &mut state.pending else {
        return;
    };
    let result = Mailbox::ready().and_then(|mailbox| load.step(&mailbox, &mut state.sector));
    if let Ok(false) = result {
        return;
    }
    state.completed = result.err().map(|error| Completion {
        error,
        late_generation: (load.phase == Phase::Publish).then_some(load.generation),
    });
    state.pending = None;
    unsafe { dkrt_poll_remove(poll as *const () as usize) };
}

fn begin(state: &mut Publisher, path: *const u8) -> Result<(), i32> {
    if path.is_null() {
        return Err(E_CONTRACT);
    }
    if state.pending.is_some() {
        return Err(E_BUSY);
    }
    let bank = Mailbox::ready()?.free_bank()?;
    let file = unsafe { SampleFile::open(path, &mut state.sector) }?;
    let poll_result = unsafe { dkrt_poll_add(poll as *const () as usize) };
    if poll_result != 0 {
        return Err(poll_result);
    }
    let generation = state.next_generation;
    state.next_generation = if generation == i32::MAX as u32 {
        1
    } else {
        generation + 1
    };
    state.pending = Some(Publication {
        file,
        bank,
        generation,
        cursor: 0,
        phase: Phase::Send,
        waits: 0,
    });
    state.completed = None;
    Ok(())
}

/// Queue a native mono file from the store. The current PCM remains available until commit.
#[no_mangle]
pub unsafe extern "C" fn dk_peer_audio_load(path: *const u8) -> i32 {
    begin(&mut *(&raw mut PUBLISHER), path).err().unwrap_or(0)
}

/// Trigger the currently published generation. A caller may pass zero to select the latest.
#[no_mangle]
pub unsafe extern "C" fn dk_peer_audio_trigger(generation: u32, gain: u32) -> i32 {
    if (*(&raw const PUBLISHER)).pending.is_some() {
        return E_BUSY;
    }
    Mailbox::ready()
        .and_then(|mailbox| mailbox.trigger(generation, gain))
        .err()
        .unwrap_or(0)
}

/// Positive generation on completion, zero before the first load, or a runtime error.
#[no_mangle]
pub unsafe extern "C" fn dk_peer_audio_status() -> i32 {
    let mailbox = match Mailbox::ready() {
        Ok(mailbox) => mailbox,
        Err(error) => return error,
    };
    let state = &*(&raw const PUBLISHER);
    // An outstanding command still blocks new sends. A timed-out load remains visible in status.
    // A late successful commit becomes authoritative as soon as the peer acknowledges it.
    state.status(mailbox.idle().then(|| mailbox.generation()))
}

#[cfg(test)]
mod tests;
