//! Request macOS audio input authorization without holding up Remote commands.

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{class, msg_send, sel};
use objc2_foundation::NSString;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

pub(super) fn check() -> Result<(), String> {
    let capture = class!(AVCaptureDevice);
    // The selectors and media type are declared by AVFoundation. On older
    // macOS versions without the authorization selector, capture needs no prompt.
    unsafe {
        let available: Bool =
            msg_send![capture, respondsToSelector: sel!(authorizationStatusForMediaType:)];
        if !available.as_bool() {
            return Ok(());
        }
        const NOT_DETERMINED: isize = 0;
        const AUTHORIZED: isize = 3;
        let status: isize = msg_send![capture, authorizationStatusForMediaType: AVMediaTypeAudio];
        match status {
            AUTHORIZED => Ok(()),
            NOT_DETERMINED => {
                let completion = RcBlock::new(|_: Bool| {});
                let _: () = msg_send![
                    capture, requestAccessForMediaType: AVMediaTypeAudio,
                    completionHandler: &*completion
                ];
                Err("Allow microphone access in the macOS prompt, then start monitoring again."
                    .into())
            }
            _ => Err(
                "Audio input permission was denied. Allow microphone access for digikit in System Settings."
                    .into(),
            ),
        }
    }
}
