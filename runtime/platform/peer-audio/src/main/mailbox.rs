use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_sdk::{E_BUSY, E_CONTRACT, E_FORMAT, E_NOT_FOUND};

const VERSION: u32 = 0x4450_4101;
const GAIN_ONE: u32 = 256;
pub const BANK_FRAMES: usize = 32_768;
pub const CHUNK_FRAMES: usize = 224;

#[derive(Clone, Copy)]
struct Bank(u32);

impl Bank {
    fn spare(published: u32, playing: u32) -> Option<Self> {
        [1, 2]
            .into_iter()
            .find(|&bank| bank != published && bank != playing)
            .map(Self)
    }
}

#[repr(u32)]
enum Command {
    Write = 1,
    Publish = 2,
    Trigger = 3,
}

/// Big-endian, shared with the peer's `wire.h`. Each CPU publishes its sequence word last.
#[repr(C)]
struct Wire {
    version: u32,
    request: u32,
    response: u32,
    command: u32,
    bank: u32,
    generation: u32,
    offset: u32,
    count: u32,
    result: i32,
    published: u32,
    playing: u32,
    published_generation: u32,
    frames: u32,
    pcm: [i16; CHUNK_FRAMES],
}

const _: () = assert!(core::mem::size_of::<Wire>() == 500);

extern "C" {
    static peer_audio_mailbox: *mut Wire;
}

pub struct Mailbox(*mut Wire);
pub struct FreeBank(Bank);

impl Mailbox {
    pub fn ready() -> Result<Self, i32> {
        // The adapter word points to uncached FlexBus memory for the lifetime of the image.
        let wire = unsafe { peer_audio_mailbox };
        if unsafe { read_volatile(addr_of!((*wire).version)) } != VERSION {
            return Err(E_CONTRACT);
        }
        Ok(Self(wire))
    }

    pub fn idle(&self) -> bool {
        let wire = self.0;
        unsafe {
            read_volatile(addr_of!((*wire).request)) == read_volatile(addr_of!((*wire).response))
        }
    }

    pub fn acknowledged(&self) -> Result<bool, i32> {
        if !self.idle() {
            return Ok(false);
        }
        match unsafe { read_volatile(addr_of!((*self.0).result)) } {
            0 => Ok(true),
            -2 => Err(E_BUSY),
            _ => Err(E_FORMAT),
        }
    }

    pub fn free_bank(&self) -> Result<FreeBank, i32> {
        if !self.idle() {
            return Err(E_BUSY);
        }
        let wire = self.0;
        let published = unsafe { read_volatile(addr_of!((*wire).published)) };
        let playing = unsafe { read_volatile(addr_of!((*wire).playing)) };
        Bank::spare(published, playing).map(FreeBank).ok_or(E_BUSY)
    }

    pub fn generation(&self) -> u32 {
        unsafe { read_volatile(addr_of!((*self.0).published_generation)) }
    }

    fn send(&self, cmd: Command, bank: Bank, generation: u32, offset: usize, count: usize) {
        let wire = self.0;
        unsafe {
            let seq = read_volatile(addr_of!((*wire).request)).wrapping_add(1);
            write_volatile(addr_of_mut!((*wire).command), cmd as u32);
            write_volatile(addr_of_mut!((*wire).bank), bank.0 - 1);
            write_volatile(addr_of_mut!((*wire).generation), generation);
            write_volatile(addr_of_mut!((*wire).offset), offset as u32);
            write_volatile(addr_of_mut!((*wire).count), count as u32);
            write_volatile(addr_of_mut!((*wire).request), seq);
        }
    }

    pub fn write(&self, bank: &FreeBank, generation: u32, start: usize, pcm: &[u8]) {
        let words = pcm.chunks_exact(2).take(CHUNK_FRAMES);
        let count = words.len();
        let dest = unsafe { addr_of_mut!((*self.0).pcm).cast::<i16>() };
        for (i, sample) in words.enumerate() {
            let value = i16::from_be_bytes([sample[0], sample[1]]);
            unsafe { write_volatile(dest.add(i), value) };
        }
        self.send(Command::Write, bank.0, generation, start, count);
    }

    pub fn publish(&self, bank: &FreeBank, generation: u32, frames: usize) {
        self.send(Command::Publish, bank.0, generation, 0, frames);
    }

    pub fn trigger(&self, generation: u32, gain: u32) -> Result<(), i32> {
        if !self.idle() {
            return Err(E_BUSY);
        }
        let current = self.generation();
        let bank = unsafe { read_volatile(addr_of!((*self.0).published)) };
        if !(1..=2).contains(&bank) || current == 0 || (generation != 0 && generation != current) {
            return Err(E_NOT_FOUND);
        }
        if gain > GAIN_ONE {
            return Err(E_FORMAT);
        }
        self.send(Command::Trigger, Bank(bank), current, 0, gain as usize);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
