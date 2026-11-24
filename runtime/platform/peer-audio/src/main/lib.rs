//! MAIN's runtime PCM publisher. The UI task is the only caller.
#![no_std]

mod mailbox;
mod publication;
mod sample_file;

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
