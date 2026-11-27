//! The route table: which backend serves a (device profile, root, operation), and the path as that backend takes it. Three deciders of the runtime
//! each answer this question today (`storage_adapter::route`, `store_writer::root_route`, the samples service's `root_of`); each checks itself against
//! this one table, so they cannot drift apart. When one `route.rs` replaces them, the table stays and the checks call the one decider.
//!
//! `op` is `Read` for what `Storage` does (stat, read, list) and `Write` for what the store writer does (create, write, close, delete). A device with its
//! samples in the store serves the `ROOT_SAMPLES` root from the `samples/` folder of the store for both; a device with the firmware file system reads it
//! there and writes it through the +Drive adapter (`staged`: the adapter is in the image; without it the root does not exist for a write).
use dkrt_abi::platform::DeviceProfile;
use dkrt_abi::profiles::{DN2, DT2};
use dkrt_abi::storage::{ROOT_SAMPLES, ROOT_STORE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Read,
    Write,
}

/// Who serves the call. `Refused`: the root does not exist for this device and operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Store,
    Fs,
    PlusDrive,
    Refused,
}

#[derive(Clone, Copy, Debug)]
pub struct RouteRow {
    pub profile: DeviceProfile,
    /// The +Drive write adapter is in the image.
    pub staged: bool,
    pub root: u32,
    pub op: Op,
    /// Root-relative path.
    pub path: &'static str,
    pub backend: Backend,
    /// The path as the backend takes it: the store key, the firmware file system's rooted path, the +Drive's path. Empty when `Refused`.
    pub key: &'static str,
}

/// A root the runtime does not know.
pub const ROOT_OTHER: u32 = 99;

const fn row(
    profile: DeviceProfile,
    staged: bool,
    root: u32,
    op: Op,
    path: &'static str,
    backend: Backend,
    key: &'static str,
) -> RouteRow {
    RouteRow {
        profile,
        staged,
        root,
        op,
        path,
        backend,
        key,
    }
}

pub const ROUTES: [RouteRow; 20] = [
    // DN2: the samples are files of the store, under `samples/`
    row(
        DN2,
        false,
        ROOT_SAMPLES,
        Op::Read,
        "KICKS/K1",
        Backend::Store,
        "samples/KICKS/K1",
    ),
    row(
        DN2,
        false,
        ROOT_SAMPLES,
        Op::Write,
        "KICKS/K1",
        Backend::Store,
        "samples/KICKS/K1",
    ),
    row(
        DN2,
        true,
        ROOT_SAMPLES,
        Op::Write,
        "KICKS/K1",
        Backend::Store,
        "samples/KICKS/K1",
    ),
    row(
        DN2,
        false,
        ROOT_STORE,
        Op::Read,
        "modules/a.dkm",
        Backend::Store,
        "modules/a.dkm",
    ),
    row(
        DN2,
        false,
        ROOT_STORE,
        Op::Write,
        "modules/a.dkm",
        Backend::Store,
        "modules/a.dkm",
    ),
    row(DN2, false, ROOT_OTHER, Op::Read, "x", Backend::Refused, ""),
    row(DN2, false, ROOT_OTHER, Op::Write, "x", Backend::Refused, ""),
    // DT2: the samples are the firmware's file system: read there, written through the +Drive adapter
    row(
        DT2,
        false,
        ROOT_SAMPLES,
        Op::Read,
        "KICKS/K1",
        Backend::Fs,
        "/KICKS/K1",
    ),
    row(
        DT2,
        true,
        ROOT_SAMPLES,
        Op::Read,
        "KICKS/K1",
        Backend::Fs,
        "/KICKS/K1",
    ),
    row(
        DT2,
        true,
        ROOT_SAMPLES,
        Op::Write,
        "KICKS/K1",
        Backend::PlusDrive,
        "KICKS/K1",
    ),
    row(
        DT2,
        false,
        ROOT_SAMPLES,
        Op::Write,
        "KICKS/K1",
        Backend::Refused,
        "",
    ),
    row(
        DT2,
        false,
        ROOT_STORE,
        Op::Read,
        "modules/a.dkm",
        Backend::Store,
        "modules/a.dkm",
    ),
    row(
        DT2,
        false,
        ROOT_STORE,
        Op::Write,
        "modules/a.dkm",
        Backend::Store,
        "modules/a.dkm",
    ),
    row(
        DT2,
        true,
        ROOT_STORE,
        Op::Write,
        "modules/a.dkm",
        Backend::Store,
        "modules/a.dkm",
    ),
    row(DT2, false, ROOT_OTHER, Op::Read, "x", Backend::Refused, ""),
    row(DT2, false, ROOT_OTHER, Op::Write, "x", Backend::Refused, ""),
    // a path with folders, both devices
    row(
        DN2,
        false,
        ROOT_SAMPLES,
        Op::Read,
        "A/B/C",
        Backend::Store,
        "samples/A/B/C",
    ),
    row(
        DT2,
        true,
        ROOT_SAMPLES,
        Op::Read,
        "A/B/C",
        Backend::Fs,
        "/A/B/C",
    ),
    row(
        DT2,
        true,
        ROOT_SAMPLES,
        Op::Write,
        "A/B/C",
        Backend::PlusDrive,
        "A/B/C",
    ),
    row(
        DN2,
        false,
        ROOT_SAMPLES,
        Op::Write,
        "A/B/C",
        Backend::Store,
        "samples/A/B/C",
    ),
];
