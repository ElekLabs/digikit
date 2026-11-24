//! The rows of loaded service files (R12-4 step 4): `Registry::add` puts the `ProviderRow` a `.dks` declared, resolved into a `ProviderHandle`, into the row table
//! (`gen_services::ROWS`), `Registry::remove` takes it out again.
//!
//! The table holds the loaded rows sorted by callback order (the order the manifest packs) and the hash of the service name; a file without an order follows every file with
//! one. The order of callbacks is independent of the order files load in. At most `PROVIDER_MAX` rows are loaded.
//!
//! `Rows` holds the owners and the count; the rows are in the slice it is given, so a host test uses a table of its own.
use core::ffi::c_void;
use core::ops::Range;
use dkrt_abi::provider::{ProviderHandle, ProviderRow};
use dkrt_abi::service::ServiceTable;
use dkrt_sdk::{E_PLAN_FIT, E_PLAN_OWNER, OK, PROVIDER_MAX};

/// A row the loader found in a service file: the provider row with the instance `start` made and the extent of the package it lies in (`ProviderHandle::resolve`).
pub struct RowSource<'a> {
    pub row: &'a ProviderRow,
    pub instance: *mut u8,
    pub extent: Range<usize>,
}

impl RowSource<'_> {
    /// The hash of the service's name: the file's own claim, which the loader compares with what the package provides (`E_FORMAT` for a row that does not check).
    ///
    /// # Safety
    /// As `resolve`.
    pub unsafe fn name_hash(&self) -> Result<u32, i32> {
        dkrt_abi::provider::check(self.row, self.extent.clone()).map(|checked| checked.name())
    }

    /// The row for the table, resolved against the part versions this core knows (`E_SVC_ABI` for a newer one, `E_FORMAT` for a malformed one).
    ///
    /// # Safety
    /// As `ProviderHandle::resolve`.
    pub unsafe fn resolve(self) -> Result<ProviderHandle, i32> {
        ProviderHandle::resolve(self.row, self.extent, self.instance as *mut c_void)
    }
}

/// A missing order follows every explicit value, including u32::MAX.
fn sort_order(order: Option<u32>) -> (bool, u32) {
    (order.is_none(), order.unwrap_or(0))
}

/// The loaded rows: how many, and the owner (module record + 1) of each, in table order.
pub struct Rows {
    n: usize,
    owner: [u8; PROVIDER_MAX],
    order: [Option<u32>; PROVIDER_MAX],
    /// The hash of each row's service name: the identity a row is found by and the tie-break of equal orders.
    hash: [u32; PROVIDER_MAX],
}

impl Rows {
    pub const fn new() -> Rows {
        Rows { n: 0, owner: [0; PROVIDER_MAX], order: [None; PROVIDER_MAX], hash: [0; PROVIDER_MAX] }
    }

    /// The loaded rows.
    pub fn len(&self) -> usize {
        self.n
    }

    /// Add `row` of module record `rec` to `table` (room for `PROVIDER_MAX`): `E_PLAN_OWNER` when a row of that name is there already (a service is one owner's), `E_PLAN_FIT`
    /// when the room is full. A row that does not resolve is refused with the status of `RowSource::resolve`, before anything changes.
    ///
    /// # Safety
    /// The rows of `table[..len()]` are valid, and so is `row`.
    pub unsafe fn add(&mut self, table: &mut [ProviderHandle], rec: usize, row: RowSource<'_>, order: Option<u32>) -> i32 {
        let row = match row.resolve() {
            Ok(row) => row,
            Err(e) => return e,
        };
        let hash = row.name();
        let mut i = 0;
        while i < self.n {
            if table[i].name() == hash {
                return E_PLAN_OWNER;
            }
            i += 1;
        }
        if self.n == PROVIDER_MAX {
            return E_PLAN_FIT;
        }
        // Insert before the next higher order, breaking ties by service name.
        let mut at = self.n;
        let mut k = 0;
        while k < self.n {
            if (sort_order(self.order[k]), self.hash[k]) > (sort_order(order), hash) {
                at = k;
                break;
            }
            k += 1;
        }
        let mut k = self.n;
        while k > at {
            table[k] = table[k - 1];
            self.owner[k] = self.owner[k - 1];
            self.order[k] = self.order[k - 1];
            self.hash[k] = self.hash[k - 1];
            k -= 1;
        }
        table[at] = row;
        self.owner[at] = rec as u8 + 1;
        self.order[at] = order;
        self.hash[at] = hash;
        self.n += 1;
        OK
    }

    /// The position among the loaded rows of the row of record `rec`.
    pub fn index_of(&self, rec: usize) -> Option<usize> {
        let mut at = 0;
        while at < self.n {
            if self.owner[at] == rec as u8 + 1 {
                return Some(at);
            }
            at += 1;
        }
        None
    }

    /// Take the row of module record `rec` out of `table` (nothing when it has none).
    pub fn remove(&mut self, table: &mut [ProviderHandle], rec: usize) {
        let mut at = 0;
        while at < self.n && self.owner[at] != rec as u8 + 1 {
            at += 1;
        }
        if at == self.n {
            return;
        }
        let mut k = at;
        while k + 1 < self.n {
            table[k] = table[k + 1];
            self.owner[k] = self.owner[k + 1];
            self.order[k] = self.order[k + 1];
            self.hash[k] = self.hash[k + 1];
            k += 1;
        }
        table[self.n - 1] = ProviderHandle::EMPTY;
        self.owner[self.n - 1] = 0;
        self.order[self.n - 1] = None;
        self.hash[self.n - 1] = 0;
        self.n -= 1;
    }
}

static mut LIVE: Rows = Rows::new();

/// `Registry::add` of the runtime: the row of a service file joins the table of the services.
pub unsafe fn add(rec: usize, row: RowSource<'_>, order: Option<u32>) -> i32 {
    let t = &mut *core::ptr::addr_of_mut!(crate::package::gen_services::ROWS);
    (*core::ptr::addr_of_mut!(LIVE)).add(t, rec, row, order)
}

/// `Registry::remove` of the runtime.
pub unsafe fn remove(rec: usize) {
    let t = &mut *core::ptr::addr_of_mut!(crate::package::gen_services::ROWS);
    (*core::ptr::addr_of_mut!(LIVE)).remove(t, rec)
}

/// The row the loaded service file of record `rec` declared, as a table of its own: the lifecycle calls into one service are the table's, over this one row.
pub fn row_of(rec: usize) -> Option<&'static [ProviderHandle]> {
    let at = unsafe { (*core::ptr::addr_of!(LIVE)).index_of(rec)? };
    Some(&rows()[at..=at])
}

/// The place of the row of record `rec` in the walk of the callbacks (`u32::MAX` when it has no row).
pub fn rank_of(rec: usize) -> u32 {
    match unsafe { (*core::ptr::addr_of!(LIVE)).index_of(rec) } {
        Some(at) => at as u32,
        None => u32::MAX,
    }
}

/// The rows the core calls now: the loaded ones.
pub fn rows() -> &'static [ProviderHandle] {
    unsafe {
        let n = (*core::ptr::addr_of!(LIVE)).len();
        core::slice::from_raw_parts(core::ptr::addr_of!(crate::package::gen_services::ROWS) as *const ProviderHandle, n)
    }
}

/// Whether the row of the service named `name` is in the table now.
pub fn has(name: &str) -> bool {
    let hash = dkneeds::keys::name_hash(name);
    rows().iter().any(|r| r.name() == hash)
}

/// Host tests of the runtime run with the machines service as a loaded file has it (the service is a `.dks` on the device): its row joins the table once, at the last
/// provider record, the first time the services are asked for.
#[cfg(test)]
pub fn test_machines() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        add(dkrt_abi::MAX_MODULES + PROVIDER_MAX - 1, crate::package::handlers::exports::machines_row(), Some(200));
    });
}

/// The rows a consumer's attach or detach walks: the loaded rows of `services` (the services the consumer needs), in the order of the table. The callbacks run in that
/// order for the module; a row of a service the module does not need is not called. Valid until the next call (one task, one walk at a time).
pub unsafe fn walk_rows(services: &[dkrt_abi::package::PackageId]) -> &'static [ProviderHandle] {
    let all = rows();
    let out = &mut *core::ptr::addr_of_mut!(WALK);
    let mut n = 0;
    let mut at = 0;
    while at < all.len() {
        if services.iter().any(|s| (*core::ptr::addr_of!(LIVE)).index_of(s.0 as usize) == Some(at)) {
            out[n] = all[at];
            n += 1;
        }
        at += 1;
    }
    &out[..n]
}

static mut WALK: [ProviderHandle; crate::package::gen_services::ROWS_LEN] = [ProviderHandle::EMPTY; crate::package::gen_services::ROWS_LEN];

/// The table of the services the core calls now (`handlers::services`).
pub fn table() -> ServiceTable {
    ServiceTable::new(rows(), &crate::package::core_api::REAL_CORE)
}

#[cfg(test)]
mod tests;
