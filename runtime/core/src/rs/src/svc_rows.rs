//! The rows of loaded service files (R12-4 step 4): `Registry::add` puts the `ServiceRow` a `.dks` declared into the tail of the runtime's row table
//! (`gen_services::ROWS`), `Registry::remove` takes it out again.
//!
//! The table is the in-crate rows (`gen_services::STATIC`, from `[[service]]` with `rust =`) and then the loaded rows, sorted by callback order and service name. The order of
//! callbacks is independent of the order files load in. At most `PROVIDER_MAX` rows are
//! loaded. `FAILED` (`handlers.rs`) covers the in-crate prefix only, and `is_runtime_service` reads that prefix only.
//!
//! `Rows` holds the owners and the count; the pointers are in the slice it is given, so a host test uses a table of its own.
use dkrt_abi::service::{ServiceRow, ServiceTable};
use dkrt_sdk::{E_PLAN_FIT, E_PLAN_OWNER, OK, PROVIDER_MAX};

/// The six shipped files can still be read from cards written before callback order was packed.
fn callback_order(name: &str, order: Option<u32>) -> Option<u32> {
    order.or_else(|| match name {
        "samples" => Some(100),
        "machines" => Some(200),
        "fx" => Some(300),
        "lfo" => Some(400),
        "shortcuts" => Some(500),
        "project" => Some(600),
        _ => None,
    })
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
}

impl Rows {
    pub const fn new() -> Rows {
        Rows { n: 0, owner: [0; PROVIDER_MAX], order: [None; PROVIDER_MAX] }
    }

    /// The loaded rows.
    pub fn len(&self) -> usize {
        self.n
    }

    /// Add `row` of module record `rec` to `table` (`fixed` in-crate rows, then room for `PROVIDER_MAX`): `E_PLAN_OWNER` when a row of that name is there already (a service is one owner's, in-crate or loaded), `E_PLAN_FIT` when the room is full.
    ///
    /// # Safety
    /// The pointers of `table[..fixed + len()]` point at rows.
    pub unsafe fn add(&mut self, table: &mut [*const ServiceRow], fixed: usize, rec: usize, row: &ServiceRow, order: Option<u32>) -> i32 {
        let mine = callback_order(row.name(), order);
        let mut i = 0;
        while i < fixed + self.n {
            if (*table[i]).name() == row.name() {
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
            if (sort_order(self.order[k]), (*table[fixed + k]).name()) > (sort_order(mine), row.name()) {
                at = k;
                break;
            }
            k += 1;
        }
        let mut k = self.n;
        while k > at {
            table[fixed + k] = table[fixed + k - 1];
            self.owner[k] = self.owner[k - 1];
            self.order[k] = self.order[k - 1];
            k -= 1;
        }
        table[fixed + at] = row as *const ServiceRow;
        self.owner[at] = rec as u8 + 1;
        self.order[at] = mine;
        self.n += 1;
        OK
    }

    /// Take the row of module record `rec` out of `table` (nothing when it has none).
    pub fn remove(&mut self, table: &mut [*const ServiceRow], fixed: usize, rec: usize) {
        let mut at = 0;
        while at < self.n && self.owner[at] != rec as u8 + 1 {
            at += 1;
        }
        if at == self.n {
            return;
        }
        let mut k = at;
        while k + 1 < self.n {
            table[fixed + k] = table[fixed + k + 1];
            self.owner[k] = self.owner[k + 1];
            self.order[k] = self.order[k + 1];
            k += 1;
        }
        table[fixed + self.n - 1] = core::ptr::null();
        self.owner[self.n - 1] = 0;
        self.order[self.n - 1] = None;
        self.n -= 1;
    }
}

static mut LIVE: Rows = Rows::new();

/// `Registry::add` of the runtime: the row of a service file joins the table of the services.
pub unsafe fn add(rec: usize, row: &ServiceRow, order: Option<u32>) -> i32 {
    let t = &mut *core::ptr::addr_of_mut!(crate::gen_services::ROWS);
    (*core::ptr::addr_of_mut!(LIVE)).add(t, crate::gen_services::STATIC, rec, row, order)
}

/// `Registry::remove` of the runtime.
pub unsafe fn remove(rec: usize) {
    let t = &mut *core::ptr::addr_of_mut!(crate::gen_services::ROWS);
    (*core::ptr::addr_of_mut!(LIVE)).remove(t, crate::gen_services::STATIC, rec)
}

/// The rows the core calls now: the in-crate ones and the loaded ones.
pub fn rows() -> &'static [*const ServiceRow] {
    unsafe {
        let n = crate::gen_services::STATIC + (*core::ptr::addr_of!(LIVE)).len();
        core::slice::from_raw_parts(core::ptr::addr_of!(crate::gen_services::ROWS) as *const *const ServiceRow, n)
    }
}

/// Host tests of the runtime run with the machines service as a loaded file has it (the service is a `.dks` on the device): its row joins the table once, at the last
/// provider record, the first time the services are asked for.
#[cfg(test)]
pub fn test_machines() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        add(crate::MAX_MODULES + PROVIDER_MAX - 1, crate::handlers::exports::machines_row(), None);
    });
}

/// The table of the services the core calls now (`handlers::services`): `base` with the live row list.
pub fn table(base: ServiceTable) -> ServiceTable {
    ServiceTable { rows: rows(), ..base }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_api::RealCore;
    use dkrt_abi::core_api::CoreApi;
    use dkrt_abi::service::Service;

    struct Nothing;
    impl<C: CoreApi> Service<C> for Nothing {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();
    }

    macro_rules! row {
        ($name:literal) => {{
            static R: ServiceRow = ServiceRow::of::<Nothing, RealCore>($name, &[]);
            &R
        }};
    }

    fn names(t: &[*const ServiceRow], n: usize) -> std::vec::Vec<&'static str> {
        t[..n].iter().map(|p| unsafe { (**p).name() }).collect()
    }

    #[test]
    fn rows_with_the_same_order_use_name_and_removal_closes_the_gap() {
        let mut t = [core::ptr::null(); 1 + PROVIDER_MAX];
        t[0] = row!("a");
        let mut r = Rows::new();
        unsafe {
            assert_eq!(r.add(&mut t, 1, 3, row!("d"), None), OK);
            assert_eq!(r.add(&mut t, 1, 1, row!("b"), None), OK);
            assert_eq!(r.add(&mut t, 1, 2, row!("e"), None), OK);
        }
        assert_eq!(names(&t, 1 + r.len()), ["a", "b", "d", "e"]);
        assert_eq!(r.owner[..3], [2, 4, 3]);
        // taking one out closes the gap and clears the end; a second remove does nothing
        r.remove(&mut t, 1, 3);
        assert_eq!(names(&t, 1 + r.len()), ["a", "b", "e"]);
        assert!(t[1 + r.len()].is_null());
        r.remove(&mut t, 1, 3);
        r.remove(&mut t, 1, 0);
        assert_eq!(r.len(), 2);
        assert_eq!(r.owner[..3], [2, 3, 0]);
    }

    #[test]
    fn the_explicit_maximum_precedes_an_unordered_service() {
        let mut t = [core::ptr::null(); PROVIDER_MAX];
        let mut r = Rows::new();
        unsafe {
            assert_eq!(r.add(&mut t, 0, 0, row!("a"), None), OK);
            assert_eq!(r.add(&mut t, 0, 1, row!("z"), Some(u32::MAX)), OK);
        }
        assert_eq!(names(&t, r.len()), ["z", "a"]);
    }

    #[test]
    fn the_six_shipped_rows_keep_their_order_for_every_load_order() {
        let rows = [row!("samples"), row!("machines"), row!("fx"), row!("lfo"), row!("shortcuts"), row!("project")];

        fn check(rows: &[&ServiceRow; 6], load_order: &mut [usize; 6], at: usize) {
            if at != load_order.len() {
                for i in at..load_order.len() {
                    load_order.swap(at, i);
                    check(rows, load_order, at + 1);
                    load_order.swap(at, i);
                }
                return;
            }
            let mut t = [core::ptr::null(); PROVIDER_MAX];
            let mut r = Rows::new();
            for &i in load_order.iter() {
                // Alternate old packages and freshly packed ones.
                let order = if i % 2 == 0 { Some((i as u32 + 1) * 100) } else { None };
                assert_eq!(unsafe { r.add(&mut t, 0, i, rows[i], order) }, OK);
            }
            assert_eq!(names(&t, r.len()), ["samples", "machines", "fx", "lfo", "shortcuts", "project"]);
        }

        check(&rows, &mut [0, 1, 2, 3, 4, 5], 0);
    }

    #[test]
    fn a_name_that_has_an_owner_or_does_not_fit_is_refused() {
        let mut t = [core::ptr::null(); 1 + PROVIDER_MAX];
        t[0] = row!("a");
        let mut r = Rows::new();
        unsafe {
            // an in-crate service is not provided twice, and a loaded one has one owner
            assert_eq!(r.add(&mut t, 1, 0, row!("a"), None), E_PLAN_OWNER);
            assert_eq!(r.add(&mut t, 1, 0, row!("b"), None), OK);
            assert_eq!(r.add(&mut t, 1, 1, row!("b"), None), E_PLAN_OWNER);
            assert_eq!(r.len(), 1);
            // the room is PROVIDER_MAX
            let rows = [row!("c"), row!("d"), row!("e"), row!("f"), row!("g"), row!("h"), row!("i"), row!("j")];
            let mut owner = 2;
            while r.len() < PROVIDER_MAX {
                assert_eq!(r.add(&mut t, 1, owner, rows[owner - 2], None), OK);
                owner += 1;
            }
            assert_eq!(r.add(&mut t, 1, owner, row!("k"), None), E_PLAN_FIT);
        }
        assert_eq!(r.len(), PROVIDER_MAX);
        assert_eq!(names(&t, 1 + r.len()), ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"][..1 + PROVIDER_MAX]);
    }

    // ---- the lifecycle walkers of interface B ----------------------------------------------------------------------------------

    static SEEN: std::sync::Mutex<std::vec::Vec<(&'static str, &'static str)>> = std::sync::Mutex::new(std::vec::Vec::new());
    fn seen(who: &'static str, what: &'static str) {
        SEEN.lock().unwrap_or_else(|e| e.into_inner()).push((who, what));
    }

    struct First;
    struct Refuses;
    struct Last;
    impl<C: CoreApi> Service<C> for First {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();

        unsafe fn module_begin(_: &C, _: usize) {
            seen("first", "begin");
        }
        unsafe fn module_commit(_: &C, _: usize, _: &dkrt_abi::service::ModuleCommitCtx) -> i32 {
            seen("first", "commit");
            OK
        }
    }
    impl<C: CoreApi> Service<C> for Refuses {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();

        unsafe fn module_begin(_: &C, _: usize) {
            seen("refuses", "begin");
        }
        unsafe fn module_commit(_: &C, _: usize, _: &dkrt_abi::service::ModuleCommitCtx) -> i32 {
            seen("refuses", "commit");
            dkrt_sdk::E_TYPE_USED
        }
    }
    impl<C: CoreApi> Service<C> for Last {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();

        unsafe fn module_begin(_: &C, _: usize) {
            seen("last", "begin");
        }
        unsafe fn module_commit(_: &C, _: usize, _: &dkrt_abi::service::ModuleCommitCtx) -> i32 {
            seen("last", "commit");
            OK
        }
    }

    #[test]
    fn begin_reaches_every_row_and_the_first_refused_commit_stops_the_walk() {
        use dkrt_abi::keys::KeysCopy;
        use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink};
        static CORE: RealCore = RealCore;
        static A: ServiceRow = ServiceRow::of::<First, RealCore>("first", &[]);
        static B: ServiceRow = ServiceRow::of::<Refuses, RealCore>("refuses", &[]);
        static D: ServiceRow = ServiceRow::of::<Last, RealCore>("last", &[]);
        static NONE: ServiceRow = ServiceRow::of::<Nothing, RealCore>("none", &[]);
        let rows: &'static [*const ServiceRow] = std::boxed::Box::leak(std::vec![&A as *const _, &NONE as *const _, &B as *const _, &D as *const _].into_boxed_slice());
        let table = ServiceTable::new(rows, &CORE);
        let _g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        SEEN.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let keys = KeysCopy::empty();
        let mut sink = RecordSink::measure();
        let ctx = ModuleCommitCtx { phase: commit_phase::PREPARE, base: 0x1000, image_size: 32, keys: &keys, sink: &mut sink };
        unsafe {
            table.module_begin(3);
            assert_eq!(table.module_commit(3, &ctx), dkrt_sdk::E_TYPE_USED);
        }
        let log = SEEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(
            log,
            [("first", "begin"), ("refuses", "begin"), ("last", "begin"), ("first", "commit"), ("refuses", "commit")],
            "every row hears begin, in table order; the refusal stops the commit before the last row"
        );
    }
}
