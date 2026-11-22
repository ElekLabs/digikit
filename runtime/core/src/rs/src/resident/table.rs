//! The one `ResidentTable` of the resident core.
use dkrt_abi::resident::{Header, ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR, RESIDENT_MINOR};

/// The table every package gets: built at compile time, read only.
pub(crate) static TABLE: ResidentTable = ResidentTable {
    header: Header { magic: RESIDENT_MAGIC, major: RESIDENT_MAJOR, minor: RESIDENT_MINOR, size: core::mem::size_of::<ResidentTable>() as u32 },
    views: &super::views::VIEWS,
    platform: &super::platform::FNS,
    boot: &super::boot::FNS,
    dlog: &super::dlog::FNS,
    store: &super::store::FNS,
    store_writer: &super::store_writer::FNS,
    card: &super::card::FNS,
    settings: &super::settings::FNS,
    arena: &super::arena::FNS,
    selection: &super::selection::FNS,
    worker: &super::worker::FNS,
    io: &super::io::FNS,
    intro: &super::intro::FNS,
    sdk_files: &super::sdk_files::FNS,
    api: &super::api::FNS,
    fault: &super::fault::FNS,
    transport: &super::transport::FNS,
    state: &super::state::FNS,
    imports: &super::imports::ROWS,
    links: &super::links::ROWS,
};
