//! The table itself: a header and one group per resident owner.
use super::*;

/// "DKRT" with the high bit of the first byte set: no other word of the runtime has this value.
pub const RESIDENT_MAGIC: u32 = 0xc44b_5254;
/// Raised when an entry is changed or removed.
pub const RESIDENT_MAJOR: u16 = 1;
/// Raised when an entry is added at the end of a group.
pub const RESIDENT_MINOR: u16 = 0;

/// The first words of the table: what it is, which version and how many bytes the producer built.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Header {
    pub magic: u32,
    pub major: u16,
    pub minor: u16,
    /// Bytes of the `ResidentTable` the producer built.
    pub size: u32,
}

/// What the package calls in the resident core. Built once by the resident core (`resident/table.rs` of the runtime crate); a field is never null.
#[repr(C)]
pub struct ResidentTable {
    pub header: Header,
    pub views: Views,
    pub platform: PlatformFns,
    pub boot: BootFns,
    pub dlog: DlogFns,
    pub store: StoreFns,
    pub store_writer: StoreWriterFns,
    pub card: CardFns,
    pub settings: SettingsFns,
    pub arena: ArenaFns,
    pub selection: SelectionFns,
    pub worker: WorkerFns,
    pub io: IoFns,
    pub intro: IntroFns,
    pub sdk_files: SdkFilesFns,
    pub api: ApiFns,
    pub fault: FaultFns,
    pub transport: TransportFns,
    pub state: StateFns,
    pub imports: ImportRows,
}

// The table holds function pointers and pointers to the resident statics: it is built once and only read.
unsafe impl Sync for ResidentTable {}
