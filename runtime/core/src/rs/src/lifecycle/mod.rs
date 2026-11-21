//! The package manager of the runtime (docs/plans/runtime-contract-and-lifecycle.md, RC2) will live here, over the package graph of `dkrt-lifecycle`.
//! Today the folder holds the assembly tests of that graph: nothing in the runtime object uses the graph yet, and `dkrt-lifecycle` is a dev-dependency
//! of this crate only.

mod assembly_tests;
