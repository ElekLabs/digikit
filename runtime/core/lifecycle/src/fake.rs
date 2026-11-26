//! Fakes of the manager's seams for host tests (feature `fake`, as `dkrt-link` offers its `FakeLink`). The fakes build with `std` (the call log is a shared
//! `Vec`; the crate is `no_std` otherwise), so only dev-dependencies of host-test crates enable the feature; the runtime's own dependency never does. All of them record into one [`Probe`], so a test
//! reads the order of calls across seams, and can make a call fail or a restore answer busy.
//!
//! - `probe`: [`Probe`] and [`Call`], the shared call log and fault list.
//! - `placer`, `linker`, `host`, `installer`: [`FakePlacer`], [`FakeLinker`], [`FakeHost`], [`FakeInstaller`].
//! - `manifest`: [`Pkg`], a package described in a line, and [`FakeManifest`], the header tables built from them.
//! - `trace`: [`RecordingTrace`], a [`Trace`](crate::Trace) that keeps its events.
//! - `world`: [`manager`] and [`traced_manager`], a [`PackageManager`](crate::PackageManager) over all of it.

mod host;
mod installer;
mod linker;
mod manifest;
mod placer;
mod probe;
mod trace;
mod world;

pub use host::FakeHost;
pub use installer::FakeInstaller;
pub use linker::FakeLinker;
pub use manifest::{FakeManifest, Pkg};
pub use placer::FakePlacer;
pub use probe::{Call, Probe};
pub use trace::{Event, RecordingTrace};
pub use world::{manager, traced_manager, FakeManager, TracedManager};
