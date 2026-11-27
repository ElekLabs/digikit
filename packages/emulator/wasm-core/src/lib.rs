//! The browser core: the raw ABI of `abi.rs` over `elektron-native-boot`, built as the
//! `emulator-core*.wasm` modules (`tools/nx/wasm.mjs`).

#[cfg(target_arch = "wasm32")]
mod abi;
