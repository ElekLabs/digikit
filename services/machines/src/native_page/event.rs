/// `dk_page_event` (sdk/include/dk/page/page.h): what a LOCAL encoder turn hands to the content's event callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct PageEvent {
    version: u32,
    size: u32,
    track: u32,
    control: u32,
    delta: i32,
    coarse: u32,
}

pub const EVENT_BYTES: usize = 24;

impl PageEvent {
    pub const fn new(track: u32, control: u32, delta: i32, coarse: bool) -> Self {
        PageEvent {
            version: 1,
            size: EVENT_BYTES as u32,
            track,
            control,
            delta,
            coarse: coarse as u32,
        }
    }
}

#[cfg(test)]
mod tests;
