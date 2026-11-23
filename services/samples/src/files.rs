//! The sample files of the device: which root they live under and the identity of one file (`FileId`) from `Storage::stat`.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::storage::EntryKind;
use dkrt_sdk::files::SAMPLES;
use dkrt_sdk::sample_tracks::{rooted, unrooted, FileId, ID_HASH_VALID, PATH, ROOT_DN2, ROOT_DT2};
use dkrt_sdk::E_SEL_PATH;

/// Are the samples files of the store (a device with `samples_in_store` in its profile), not of the firmware's file system? The profile is found by the device id.
pub(crate) fn files_in_store<C: CoreApi>(core: &C) -> bool {
    let id = core.device_id();
    dkrt_abi::profiles::ALL
        .iter()
        .any(|p| p.id == id && p.samples_in_store)
}

/// The device's sample root (`select` paths are root-relative; the runtime keeps the rooted form).
pub(crate) fn root_of<C: CoreApi>(core: &C) -> &'static [u8] {
    if files_in_store(core) {
        ROOT_DN2
    } else {
        ROOT_DT2
    }
}

/// The identity of the sample file `path` (device-rooted, not NUL-terminated) from `Storage::stat`: its size and, when the storage gives one, the
/// file hash of its table entry (the listing row's). No mtime exists on either device (`mtime` 0): without a hash the identity is size and path. When
/// `head` is given, the first 64 bytes of the file. The storage routes (store, firmware file system) are the platform's.
///
/// # Safety
/// The core's storage calls (`CoreApi`).
pub(crate) unsafe fn probe<C: CoreApi>(
    core: &C,
    path: &[u8],
    head: Option<&mut [u8; 64]>,
) -> Result<FileId, i32> {
    if path.is_empty() || path.len() >= PATH {
        return Err(E_SEL_PATH);
    }
    let rel = unrooted(root_of(core), path);
    if rel.is_empty() {
        return Err(E_SEL_PATH);
    }
    let st = match head {
        Some(head) => core.storage_stat_head(SAMPLES, rel, head)?.0,
        None => core.storage_stat(SAMPLES, rel)?,
    };
    if st.kind != EntryKind::File {
        return Err(E_SEL_PATH);
    }
    let mut id = FileId {
        root: SAMPLES,
        size: st.size,
        mtime: 0,
        flags: 0,
        hash_kind: 0,
        hash: [0; 32],
    };
    if st.hash_valid {
        id.flags = ID_HASH_VALID;
        id.hash_kind = st.hash_kind as u32;
        id.hash = st.hash;
    }
    Ok(id)
}

/// The device-rooted copy of a module's root-relative path (a listing row) in `full`, and its length; or a negative error.
///
/// # Safety
/// `path` is null or a NUL-terminated string (or readable up to `PATH` bytes).
pub(crate) unsafe fn rooted_path<C: CoreApi>(
    core: &C,
    path: *const u8,
    full: &mut [u8; PATH],
) -> Result<usize, i32> {
    if path.is_null() {
        return Err(E_SEL_PATH);
    }
    let mut copy = [0u8; PATH];
    let mut n = 0;
    while n < copy.len() {
        let v = *path.add(n);
        if v == 0 {
            break;
        }
        copy[n] = v;
        n += 1;
    }
    if n == copy.len() {
        return Err(E_SEL_PATH);
    }
    // The path is root-relative (a listing row); the runtime keeps and checks the device-rooted form. A DT2 path that starts with '/' stays.
    rooted(root_of(core), &copy[..n], full)
}

#[cfg(test)]
mod tests;
