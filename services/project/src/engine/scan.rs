//! The scan of the store's `projects/` folder for the files of one slot, and the check of what it found. UI task only.
use super::{bytes_equal, is_busy, Env, ListPoll, ListRow, SlotRecord};
use crate::dkp::{self, Head, Ignore, Image, AREA};
use crate::name::{self, Path};
use dkrt_sdk::E_NOT_FOUND;

/// The most files of one slot that are looked at.
pub const SLOT_FILES_MAX: usize = 4;
/// Rows of the listing taken in one pass.
const ROWS_PER_PASS: usize = 32;

/// A file of a slot: what its header said and whether it can be used.
#[derive(Clone, Copy)]
pub struct SlotFile {
    pub path: Path,
    /// `Ok`: the header of a file that passed the checks; `Err`: why not.
    pub head: Result<Head, Ignore>,
}

impl SlotFile {
    /// An unused entry of the scan's table: all zero bytes, so the table sits in `.bss`. A count says how many entries are in use.
    const UNUSED: SlotFile = SlotFile {
        path: Path::EMPTY,
        head: Ok(Head {
            device: 0,
            slot: 0,
            name_len: 0,
            name: [0; 16],
            stamp: 0,
            counter: 0,
            used: 0,
        }),
    };
}

/// What a scan is for: the work that follows it.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScanFor {
    Load,
    Save,
    Clear,
    Paste,
}

/// The answer of one step of a scan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanStep {
    /// The listing has not ended; step again in the next pass.
    Waiting,
    /// The listing ended: the rows are complete.
    Finished,
    /// There is nothing to look for: the work is dropped.
    Dropped,
    /// The store refused the listing or it failed half way with this status: the rows are not complete.
    Refused(i32),
}

/// Where a scan is.
#[derive(Clone, Copy)]
#[repr(u8)] // zero first: the service is a bss static
enum ScanPhase {
    /// The listing has not been asked for yet.
    Unstarted,
    /// The listing runs; rows are taken.
    Listing,
}

/// The listing in progress and what it found. One per service, reused for each piece of work (`begin`); its tables hold stale entries beyond their counts.
pub struct SlotScan {
    scan_for: ScanFor,
    slot: u32,
    /// The firmware's record of `slot` when the scan began.
    record: Option<SlotRecord>,
    phase: ScanPhase,
    /// Rows of the slot's own files.
    rows: [Path; SLOT_FILES_MAX],
    row_count: usize,
    /// The rows' files as `finish` read them.
    files: [SlotFile; SLOT_FILES_MAX],
    file_count: usize,
    /// Paste: rows of other slots whose name part equals the pasted project's.
    same_name: [Path; SLOT_FILES_MAX],
    same_name_count: usize,
    /// The name part to look for in `same_name`.
    want: [u8; name::NAME_MAX],
    want_len: usize,
    /// Paste: the file to copy, once `finish` chose it.
    paste_source: Option<Path>,
}

/// What a finished scan found, for the planner: a view of the scan's own tables.
pub struct ScanResult<'a> {
    scan_for: ScanFor,
    slot: u32,
    record: Option<SlotRecord>,
    files: &'a [SlotFile],
    paste_source: Option<Path>,
}

impl ScanResult<'_> {
    pub fn scan_for(&self) -> ScanFor {
        self.scan_for
    }
    pub fn slot(&self) -> u32 {
        self.slot
    }
    /// The firmware's record of the slot when the scan began.
    pub fn record(&self) -> Option<SlotRecord> {
        self.record
    }
    /// The files of the slot with their checks.
    pub fn files(&self) -> &[SlotFile] {
        self.files
    }
    /// Paste: the usable file of another slot with the pasted project's name part and stamp (the highest write counter if there are several).
    pub fn paste_source(&self) -> Option<Path> {
        self.paste_source
    }
}

impl SlotScan {
    pub const fn new() -> SlotScan {
        SlotScan {
            scan_for: ScanFor::Load,
            slot: 0,
            record: None,
            phase: ScanPhase::Unstarted,
            rows: [Path::EMPTY; SLOT_FILES_MAX],
            row_count: 0,
            files: [SlotFile::UNUSED; SLOT_FILES_MAX],
            file_count: 0,
            same_name: [Path::EMPTY; SLOT_FILES_MAX],
            same_name_count: 0,
            want: [0; name::NAME_MAX],
            want_len: 0,
            paste_source: None,
        }
    }

    /// Start a scan for `slot` (in place: the tables are not cleared, their counts are).
    pub fn begin(&mut self, scan_for: ScanFor, slot: u32) {
        self.scan_for = scan_for;
        self.slot = slot;
        self.record = None;
        self.phase = ScanPhase::Unstarted;
        self.row_count = 0;
        self.file_count = 0;
        self.same_name_count = 0;
        self.want_len = 0;
        self.paste_source = None;
    }

    pub fn scan_for(&self) -> ScanFor {
        self.scan_for
    }
    pub fn slot(&self) -> u32 {
        self.slot
    }

    /// One pass of the scan: start the listing, then take the rows that are ready.
    pub fn step<E: Env + ?Sized>(&mut self, env: &mut E) -> ScanStep {
        if let ScanPhase::Unstarted = self.phase {
            // a load reads: it waits while a live operation stages
            if self.scan_for == ScanFor::Load && env.live_op() != 0 {
                return ScanStep::Waiting;
            }
            self.record = env.slot_record(self.slot);
            if self.scan_for == ScanFor::Paste {
                // the project that was pasted over this slot: its name is what the other slots' files must share
                match self.record {
                    Some(record) => self.want_len = name::clean(&record.name, &mut self.want),
                    None => return ScanStep::Dropped,
                }
            }
            let status = env.list_start();
            if status < 0 && status != E_NOT_FOUND {
                return ScanStep::Refused(status);
            }
            self.phase = ScanPhase::Listing;
            if status < 0 {
                // no folder: no file
                return ScanStep::Finished;
            }
        }
        let mut row = ListRow::new();
        let mut polled = 0;
        while polled < ROWS_PER_PASS {
            polled += 1;
            match env.list_next(&mut row) {
                ListPoll::Pending => return ScanStep::Waiting,
                ListPoll::Err(status) => return ScanStep::Refused(status),
                ListPoll::End => return ScanStep::Finished,
                ListPoll::Row => self.take_row(row.as_bytes()),
            }
        }
        ScanStep::Waiting
    }

    fn take_row(&mut self, row: &[u8]) {
        let Some(row_slot) = name::slot_of_row(row) else {
            return;
        };
        if row_slot == self.slot {
            if self.row_count < SLOT_FILES_MAX {
                if let Some(path) = Path::from_bytes(row) {
                    self.rows[self.row_count] = path;
                    self.row_count += 1;
                }
            }
        } else if self.scan_for == ScanFor::Paste {
            let part = name::name_part(row);
            if part.len() == self.want_len
                && bytes_equal(part, &self.want[..self.want_len])
                && self.same_name_count < SLOT_FILES_MAX
            {
                if let Some(path) = Path::from_bytes(row) {
                    self.same_name[self.same_name_count] = path;
                    self.same_name_count += 1;
                }
            }
        }
    }

    /// The listing ended: read the header of each file of the slot (and, for a paste, choose the source). Uses `buffer` as scratch (no job runs while a scan does). `Err`: the
    /// store answered a read busy with this status, so the files are not known (the scan is refused like a busy listing).
    pub fn finish<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
    ) -> Result<ScanResult<'_>, i32> {
        let mut at = 0;
        while at < self.row_count {
            let path = self.rows[at];
            at += 1;
            if let Some(file) = probe_file(env, &path, buffer)? {
                self.files[self.file_count] = file;
                self.file_count += 1;
            }
        }
        if self.scan_for == ScanFor::Paste {
            self.paste_source = self.choose_paste_source(env, buffer)?;
        }
        Ok(ScanResult {
            scan_for: self.scan_for,
            slot: self.slot,
            record: self.record,
            files: &self.files[..self.file_count],
            paste_source: self.paste_source,
        })
    }

    /// Paste copies the stamp and the name of the project, so the source is the usable file of another slot with the same name part and stamp.
    fn choose_paste_source<E: Env + ?Sized>(
        &self,
        env: &mut E,
        buffer: &mut Image,
    ) -> Result<Option<Path>, i32> {
        let Some(record) = self.record else {
            return Ok(None);
        };
        let mut best: Option<(Path, u32)> = None;
        let mut at = 0;
        while at < self.same_name_count {
            let path = self.same_name[at];
            at += 1;
            if let Some(file) = probe_file(env, &path, buffer)? {
                if let Ok(head) = file.head {
                    if head.stamp == record.stamp
                        && best.map(|b| head.counter > b.1).unwrap_or(true)
                    {
                        best = Some((path, head.counter));
                    }
                }
            }
        }
        Ok(best.map(|b| b.0))
    }
}

impl Default for SlotScan {
    fn default() -> Self {
        Self::new()
    }
}

/// Read one file and check it: the file with its check result, `None` when it cannot be read (it is no file for the scan), `Err` when the store is busy (nothing is known).
fn probe_file<E: Env + ?Sized>(
    env: &mut E,
    path: &Path,
    buffer: &mut Image,
) -> Result<Option<SlotFile>, i32> {
    let size = env.read(path.as_bytes(), &mut buffer.0);
    if is_busy(size) {
        return Err(size);
    }
    if size < 0 {
        return Ok(None);
    }
    let size = size as usize;
    let head = dkp::check(&buffer.0[..size.min(AREA)], env.device());
    Ok(Some(SlotFile {
        path: *path,
        head: if size > AREA {
            Err(Ignore::Format)
        } else {
            head
        },
    }))
}
