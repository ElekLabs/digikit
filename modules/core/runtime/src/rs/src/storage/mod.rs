//! The storage layer of the runtime: the card safety gate, the store (read side and write side) and the routes behind `Storage for RealPlatform`.
//! The modules are re-exported at the crate root (`crate::card`, `crate::store`, `crate::store_writer`) while the callers still name them there.
pub mod card;
pub mod storage_adapter;
pub mod store;
pub mod store_writer;
