//! The storage layer of the runtime: layout, the card safety gate and its one gated write, the lock with the read leases, the store (read side and
//! write side) and the routes behind `Storage for RealPlatform`. Each file calls only the ones above it in this list: `layout`, `card`, `lock`, `store`,
//! `store_writer`, `storage_adapter`.
//! The modules are re-exported at the crate root (`crate::card`, `crate::store`, `crate::store_writer`) while the callers still name them there.
pub mod backends;
pub mod card;
pub mod layout;
pub mod lock;
pub mod route;
pub mod storage_adapter;
pub mod store;
pub mod store_writer;
