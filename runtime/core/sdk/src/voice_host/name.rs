/// The name of the info table a host publishes.
pub const NAME: &str = "voice_host";

/// FNV-1a of [`NAME`], the key of `CoreApi::info` (the same hash as `dkneeds::keys::name_hash`).
pub const NAME_HASH: u32 = crate::info_name::fnv1a(NAME.as_bytes());
