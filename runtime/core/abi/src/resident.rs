//! `ResidentTable`: what the system package calls in the resident core (docs/plans/resident-core-and-system-package.md, section 2; RC2s-1 S7b).
//!
//! The package reaches the resident core through this table and nothing else (`check:layers`, rule `boundary-table`): one group of function pointers per resident
//! owner (`platform`, `boot`, `dlog`, `store`, `settings`, `arena`, ...), the views of the resident statics the package reads and writes, and the rows of the
//! module imports that the resident core serves. The resident core builds the one table (`resident/` of the runtime crate) and gives it to the package once, at
//! `package::entry`; the package keeps it behind its own facade (`package/resident.rs`).
//!
//! Every entry is an `extern "C" fn` with integers, pointers and `repr(C)` records (an out parameter for a `Result`, `Option` or tuple), so a package in
//! another artifact can call it. The table is not part of the service ABI (`SVC_ABI`): a service never sees it. A group that grows gets its new entries at the end and
//! `RESIDENT_MINOR` goes up; a changed or removed entry raises `RESIDENT_MAJOR`.

mod api;
mod arena;
mod boot;
mod card;
mod dlog;
mod fault;
mod imports;
mod intro;
mod io;
mod platform;
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
pub use dlog::DlogFns;
pub use fault::FaultFns;
pub use imports::ImportRows;
pub use intro::IntroFns;
pub use io::IoFns;
pub use platform::PlatformFns;
pub use sdk_files::SdkFilesFns;
pub use selection::SelectionFns;
pub use settings::SettingsFns;
pub use state::StateFns;
pub use store::StoreFns;
pub use store_writer::StoreWriterFns;
pub use table::{Header, ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR, RESIDENT_MINOR};
pub use transport::TransportFns;
pub use views::Views;
pub use worker::WorkerFns;
