//! `WaveTable`: the table of added LFO waves that the image holds (`core/lfo-table`), as the owner uses it. The LFO tick and its second reader read the table; the owner
//! only gives a wave its 32 samples (`set`) and takes them away (`clear`), in the UI task. The service file binds the image's table (`dks/src/image_table.rs`), a host test
//! gives a fake (`tests/support/fake_wave_table.rs`), so the owner is tested without an image and names no address.
use crate::records::SAMPLES;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Status;

/// The table of added waves.
///
/// # Safety
/// All-zero bytes are the initial state of the implementing type, the state before `open`: the owner is made in place from zero bytes (`Provider::start_in`; no copy of the state
/// through the stack). `open` runs on that zeroed value.
pub unsafe trait WaveTable {
    /// Binds the table of the image (the first step of `start_in`). `E_NO_REGISTRY` when the image has none, or one this service does not know: the service then does not start.
    fn open<C: CoreApi>(&mut self, core: &C) -> Result<(), Status>;

    /// Does the table take wave number `wave`? (`first <= wave < first + waves`.)
    fn holds(&self, wave: u32) -> bool;

    /// Wave `wave` plays `samples` from now on (the 32 samples, interpolated). `E_TABLE_FULL` when the table has no such wave (nothing changed). UI task only.
    fn set(&mut self, wave: u32, samples: &[i16; SAMPLES]) -> Result<(), Status>;

    /// Wave `wave` is empty again and plays the stock sine. A wave the table does not hold is ignored. UI task only.
    fn clear(&mut self, wave: u32);
}
