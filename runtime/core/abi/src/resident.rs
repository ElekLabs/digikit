//! `ResidentTable`: what the system package calls in the resident core (docs/plans/resident-core-and-system-package.md, sections 2 to 4; RC2s-1 S7b, S7c).
//!
//! The package reaches the resident core through this table and nothing else (`check:layers`, rule `boundary-table`): one group of function pointers per resident
//! owner (`platform`, `boot`, `dlog`, `store`, `settings`, `arena`, ...), the views of the resident statics the package reads and writes, and the rows of the
//! module imports that the resident core serves. The resident core builds the one table (`resident/` of the runtime crate) and gives it to the package once, at
//! `package::entry`; the package keeps the checked groups behind its own facade (`package/resident.rs`).
//!
//! # Layout
//!
//! `ResidentTable` is a `Header` (`magic`, `major`, `minor`, `size` in bytes) and then one pointer per group. A group is a `Group<T>`: its own `size` in **bytes**
//! (the producer's `size_of::<T>()`) and then the entries, `T`, a `repr(C)` struct of one owner's pointers. A package reads a group only through `check`, which
//! has seen that the group's `size` covers every entry of the `T` the package was built with: no entry is read unchecked.
//!
//! # Versions
//!
//! - Appending an entry at the end of a group, or a group at the end of the table: `RESIDENT_MINOR` + 1. An older package keeps working; a newer package on an older
//!   core is refused (`Refusal::Minor`, or `GroupSize`).
//! - Removing, reordering or changing an entry (its type or meaning), or changing the layout of a shared view (below): `RESIDENT_MAJOR` + 1.
//!
//! # Shared views
//!
//! `Views` hands the package pointers to resident statics. The layout of what they point at (`PackageState` and the `Module` and `LogEntry` rows in it,
//! `DkrtBootInfo`, the `MAX_MODULES` arrays) is part of the major version: a change there is a major bump. The arena's own words are not in the package's view.
//!
//! # Compatibility
//!
//! `check` (`check.rs`) is the one function that decides: the magic is equal, the major is equal, the minor is at least the one the package was built against and
//! every group is large enough. It returns the reason (`Refusal`) and never panics; the caller logs it and refuses the package (the core runs on its own).
//!
//! Every entry is an `extern "C" fn` with integers, pointers and `repr(C)` records (an out parameter for a `Result`, `Option` or tuple), so a package in
//! another artifact can call it. The table is not part of the service ABI (`SVC_ABI`): a service never sees it.

mod api;
mod arena;
mod boot;
mod card;
mod check;
mod dlog;
mod exports;
mod fault;
mod group;
mod host_dir;
mod imports;
mod intro;
mod io;
mod links;
mod platform;
mod refusal;
mod sdk_files;
mod selection;
mod settings;
mod state;
mod store;
mod store_writer;
mod table;
mod transport;
mod views;
mod worker;

pub use api::ApiFns;
pub use arena::ArenaFns;
pub use boot::BootFns;
pub use card::CardFns;
pub use check::check;
pub use dlog::DlogFns;
pub use exports::ExportFns;
pub use fault::FaultFns;
pub use group::Group;
pub use host_dir::HostDirFns;
pub use imports::ImportRows;
pub use intro::IntroFns;
pub use io::IoFns;
pub use links::LinkRows;
pub use platform::PlatformFns;
pub use refusal::{Reason, Refusal};
pub use sdk_files::SdkFilesFns;
pub use selection::SelectionFns;
pub use settings::SettingsFns;
pub use state::StateFns;
pub use store::StoreFns;
pub use store_writer::StoreWriterFns;
pub use table::{Header, Resident, ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR, RESIDENT_MINOR};
pub use transport::TransportFns;
pub use views::Views;
pub use worker::WorkerFns;
