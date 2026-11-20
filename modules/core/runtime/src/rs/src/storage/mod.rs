//! The storage layer of the runtime. Each file calls only the ones above it in this list:
//!
//! - `layout`: where the store is on the card;
//! - `card`: the card safety gate and the one gated block write;
//! - `lock`: the store lock, the read leases and the shared buffers;
//! - `route`: the one decision of which backend serves a root on this device;
//! - `store`: the read side (find, stage with hash check, the one listing rule `listed`, health figures);
//! - `backends/fwfs`, `backends/plusdrive`: the DT2 firmware file system (reads) and the +Drive adapter (writes);
//! - `store_writer`: the write session, pending files, commit, verify, delete;
//! - `storage_adapter`: `Storage for RealPlatform` over the routes;
//! - `sdk_files`: the SDK `file_*` front end over the writer and the +Drive backend.
//!
//! The modules `card`, `store` and `store_writer` are re-exported at the crate root while callers still name them there.
pub mod backends;
pub mod card;
pub mod layout;
pub mod lock;
pub mod route;
pub mod sdk_files;
pub mod storage_adapter;
pub mod store;
pub mod store_writer;
