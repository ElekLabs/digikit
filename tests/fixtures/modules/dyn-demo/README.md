# test/dyn-demo

A test fixture for the digikit runtime (`core/runtime`), not for players. A `#![no_std]` Rust crate with a
`dk_module` static (`init`, `tick`, `fini`). Pack it with `patchkit pack test/dyn-demo -o dyn-demo.dkm`, put the file in
`plus_drive/<device>/digikit/modules/`, and ask the runtime to load it (`modules/core/runtime/README.md`). While it is
loaded, its `tick` counts the UI task's event-loop passes. Emulator only.
