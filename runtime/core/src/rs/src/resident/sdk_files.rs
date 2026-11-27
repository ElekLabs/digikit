//! `SdkFilesFns`: the writer front end of the SDK's file calls. Every entry is the owner's own `extern "C"` function.
use crate::storage::sdk_files as sdk;
use dkrt_abi::resident::{Group, SdkFilesFns};

pub(super) static FNS: Group<SdkFilesFns> = Group::new(SdkFilesFns {
    create: sdk::sdk_create,
    write: sdk::sdk_write,
    close: sdk::sdk_close,
    abort: sdk::sdk_abort,
    delete: sdk::sdk_delete,
    commit: sdk::sdk_commit,
    pending: sdk::sdk_pending,
});
