use core::ffi::c_void;
use core::ptr::{addr_of_mut, write_volatile};
use dkrt_sdk::voice_host::{MAGIC, MAJOR, MINOR};

/// `dk_voice_host_v1` as the host writes it (`sdk/include/dk/host/voice_host.h`; `dkrt_sdk::voice_host` reads the same layout and a test checks that it accepts this one).
#[repr(C)]
pub struct Words {
    magic: u32,
    major: u16,
    minor: u16,
    size: u32,
    tracks: u16,
    types_max: u16,
    payload_bytes_max: u32,
    state_bytes_max: u32,
    budget: u32,
    flags: u32,
    ctx: *mut c_void,
    install: *const c_void,
    bind: *const c_void,
    retire: *const c_void,
    stats: *const c_void,
}

/// What the host offers: the limits and the four calls, as the header names them.
#[derive(Clone, Copy)]
pub struct Offer {
    pub tracks: u16,
    pub types_max: u16,
    pub payload_bytes_max: u32,
    pub state_bytes_max: u32,
    pub budget: u32,
    pub flags: u32,
    pub ctx: *mut c_void,
    pub install: *const c_void,
    pub bind: *const c_void,
    pub retire: *const c_void,
    pub stats: *const c_void,
}

/// The `voice_host` `[[info]]` table in the image: zero (magic 0) until the host serves, so a consumer that looks early finds no host. The owner of its words: the host prepares them,
/// serves with the magic written last, and withdraws with the magic cleared first, so a reader never sees a half-written table as a good one.
pub struct VoiceHostTable {
    words: *mut Words,
}

impl VoiceHostTable {
    /// The table at `words`, the words of the image's info entry.
    ///
    /// # Safety
    /// `words` is the table of the image (`size_of::<Words>()` bytes, writable, owned by this host from now on).
    pub unsafe fn at(words: *mut u32) -> VoiceHostTable {
        VoiceHostTable {
            words: words.cast(),
        }
    }

    /// Does a consumer see a host (the magic is set)?
    pub fn serving(&self) -> bool {
        unsafe { self.words.cast::<u32>().read_volatile() == MAGIC }
    }

    /// Write the version and the size, and clear the rest. The magic stays 0.
    pub fn prepare(&mut self) {
        unsafe {
            let w = self.words;
            write_volatile(addr_of_mut!((*w).magic), 0);
            write_volatile(addr_of_mut!((*w).major), MAJOR);
            write_volatile(addr_of_mut!((*w).minor), MINOR);
            write_volatile(
                addr_of_mut!((*w).size),
                core::mem::size_of::<Words>() as u32,
            );
        }
        self.write(&Offer {
            tracks: 0,
            types_max: 0,
            payload_bytes_max: 0,
            state_bytes_max: 0,
            budget: 0,
            flags: 0,
            ctx: core::ptr::null_mut(),
            install: core::ptr::null(),
            bind: core::ptr::null(),
            retire: core::ptr::null(),
            stats: core::ptr::null(),
        });
    }

    /// Publish `offer`: its words first, the magic last.
    pub fn serve(&mut self, offer: &Offer) {
        self.write(offer);
        unsafe { write_volatile(addr_of_mut!((*self.words).magic), MAGIC) };
    }

    /// Take the table back: the magic first, then the offer is cleared.
    pub fn withdraw(&mut self) {
        unsafe { write_volatile(addr_of_mut!((*self.words).magic), 0) };
        self.prepare();
    }

    fn write(&mut self, o: &Offer) {
        unsafe {
            let w = self.words;
            write_volatile(addr_of_mut!((*w).tracks), o.tracks);
            write_volatile(addr_of_mut!((*w).types_max), o.types_max);
            write_volatile(addr_of_mut!((*w).payload_bytes_max), o.payload_bytes_max);
            write_volatile(addr_of_mut!((*w).state_bytes_max), o.state_bytes_max);
            write_volatile(addr_of_mut!((*w).budget), o.budget);
            write_volatile(addr_of_mut!((*w).flags), o.flags);
            write_volatile(addr_of_mut!((*w).ctx), o.ctx);
            write_volatile(addr_of_mut!((*w).install), o.install);
            write_volatile(addr_of_mut!((*w).bind), o.bind);
            write_volatile(addr_of_mut!((*w).retire), o.retire);
            write_volatile(addr_of_mut!((*w).stats), o.stats);
        }
    }
}

#[cfg(test)]
mod tests;
