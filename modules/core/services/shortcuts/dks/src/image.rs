//! The image half of the service file: what it binds from the image adapters' tables. Nothing yet: the key-dispatch hook has no firmware address until the byte check.

/// `setup` of the file (the first step of `init`).
pub unsafe fn setup(_core: &dkrt_svc_rt::ImportCore) -> i32 {
    dkrt_sdk::OK
}
