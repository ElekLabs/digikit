//! The table itself: a header and one pointer per group.
use super::*;

/// "DKRT" with the high bit of the first byte set: no other word of the runtime has this value.
pub const RESIDENT_MAGIC: u32 = 0xc44b_5254;
/// Raised when an entry is removed, reordered or changed, or the layout of a shared view changes (rules in `resident.rs`).
pub const RESIDENT_MAJOR: u16 = 2;
/// Raised when an entry is added at the end of a group, or a group at the end of the table.
pub const RESIDENT_MINOR: u16 = 4;

/// The first words of the table: what it is, which version and how many bytes the producer built.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Header {
    pub magic: u32,
    pub major: u16,
    pub minor: u16,
    /// Bytes of the `ResidentTable` the producer built (the header and the group pointers).
    pub size: u32,
}

/// The one list of the groups: it declares the table (a pointer per group, in this order), the checked groups the package reads (`Resident`) and the function that
/// opens the one from the other. A group appended here is a minor version.
macro_rules! groups {
    ($($field:ident : $ty:ty),* $(,)?) => {
        /// What the package calls in the resident core. Built once by the resident core (`resident/table.rs` of the runtime crate); a pointer is never null.
        #[repr(C)]
        pub struct ResidentTable {
            pub header: Header,
            $(pub $field: *const Group<$ty>,)*
        }

        /// The groups of an accepted table, copied out, each checked large enough for the layout this build knows (a copy, not a pointer: a call through it is one
        /// load less). The package reads the resident core through nothing else.
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct Resident {
            $(pub $field: $ty,)*
        }

        /// Where each group is in the table and in `Resident`, how large it must be and its name: one row per group, in table order.
        const GROUP_ROWS: &[GroupRow] = &[$(GroupRow {
            in_table: core::mem::offset_of!(ResidentTable, $field),
            in_resident: core::mem::offset_of!(Resident, $field),
            entries: core::mem::offset_of!(Group<$ty>, entries),
            size: core::mem::size_of::<$ty>(),
            name: stringify!($field),
        },)*];
    };
}

groups! {
    views: Views,
    platform: PlatformFns,
    boot: BootFns,
    dlog: DlogFns,
    store: StoreFns,
    store_writer: StoreWriterFns,
    card: CardFns,
    settings: SettingsFns,
    arena: ArenaFns,
    selection: SelectionFns,
    worker: WorkerFns,
    io: IoFns,
    intro: IntroFns,
    sdk_files: SdkFilesFns,
    api: ApiFns,
    fault: FaultFns,
    transport: TransportFns,
    state: StateFns,
    imports: ImportRows,
    links: LinkRows,
    host_dir: HostDirFns,
    exports: ExportFns,
}

// The table holds pointers to groups of function pointers and to the resident statics: it is built once and only read.
unsafe impl Sync for ResidentTable {}

/// One group of the table, as `Resident::open` reads it (generated from the one list of the groups, so a group is in no second place).
struct GroupRow {
    /// Offset of the group's pointer in `ResidentTable`.
    in_table: usize,
    /// Offset of the group's copy in `Resident`.
    in_resident: usize,
    /// Offset of the entries in the group (after the size word).
    entries: usize,
    /// Bytes of entries the package's layout has.
    size: usize,
    name: &'static str,
}

impl Resident {
    /// Check every group (present, at least the size this build knows) and, only when all pass, copy them into `into`. Two passes over `GROUP_ROWS`: the one place that
    /// reads a group and the one place that copies one.
    ///
    /// # Safety
    /// A non-null group pointer of `table` points at a live `Group` of its type (a size word and the entries); `into` is valid for writes of a `Resident`.
    pub(super) unsafe fn open(table: &ResidentTable, into: *mut Resident) -> Result<(), Refusal> {
        let mut i = 0;
        while i < GROUP_ROWS.len() {
            let row = &GROUP_ROWS[i];
            let group = Self::group_of(table, row);
            if group.is_null() {
                return Err(Refusal::of_group(Reason::GroupMissing, row.name, 0, 0));
            }
            let found = Group::<()>::size_at(group as *const Group<()>);
            if (found as usize) < row.size {
                return Err(Refusal::of_group(
                    Reason::GroupSize,
                    row.name,
                    found,
                    row.size as u32,
                ));
            }
            i += 1;
        }
        i = 0;
        while i < GROUP_ROWS.len() {
            let row = &GROUP_ROWS[i];
            copy_bytes(
                Self::group_of(table, row).add(row.entries),
                (into as *mut u8).add(row.in_resident),
                row.size,
            );
            i += 1;
        }
        Ok(())
    }

    /// The group pointer of `row` in `table`.
    unsafe fn group_of(table: &ResidentTable, row: &GroupRow) -> *const u8 {
        core::ptr::read(
            (table as *const ResidentTable as *const u8).add(row.in_table) as *const *const u8,
        )
    }
}

/// Copy `n` bytes. One function for every group, byte by byte and volatile: it neither becomes a `memcpy` call nor is unrolled per group (the copy runs once per boot).
#[inline(never)]
unsafe fn copy_bytes(src: *const u8, dst: *mut u8, n: usize) {
    let mut i = 0;
    while i < n {
        core::ptr::write_volatile(dst.add(i), core::ptr::read_volatile(src.add(i)));
        i += 1;
    }
}
