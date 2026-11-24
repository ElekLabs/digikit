//! `Shared<T>` (defined in `dkrt-abi`, so the service crates can use it too): the cell for a table that several tasks and interrupts change, with its sections masked. A service file
//! holds its tables of this kind in statics next to its owner cell (`DK_SVC_OWNER`), never as fields of the owner object.
pub use dkrt_abi::shared::Shared;

#[cfg(test)]
mod tests;
