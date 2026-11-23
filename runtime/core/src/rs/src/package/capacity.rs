//! `dkrt_capacity`: the entries of the CAPACITY command and the DIGIKIT menu's MEMORY page, and the limit each kind takes from the loaded services or the image.

use crate::package::handlers::services;
use crate::package::resident::platform::PLATFORM;
use crate::package::resident::state::{init_once, st};
use crate::package::resident::{arena, boot, card, settings, store};
use dkrt_abi::MAX_MODULES;
use dkrt_sdk::{PACKAGE_BYTES, STAGE_BYTES};
use crate::package::{dsp};
use core::ptr::read_volatile;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_sdk::*;

/// The value of capacity kind number `k` (the index in `CAP_KINDS`): `base` is the runtime's own row for it. A loaded service that counts the kind answers for it
/// and owns `used`; else the image's `cap_<kind>` info entry (an `[[info]]` of the image adapter that reserves the capacity, a pointer to a u32 limit word)
/// gives the limit with `used` 0; else `base` (zeros for a kind with no row). The runtime names no service and no adapter: only the generic `cap_<kind>` name.
pub(crate) unsafe fn kind_value<P: PackagePlatform>(p: &P, table: &dkrt_abi::service::ServiceTable, k: usize, base: (u8, u32, u32, u32)) -> (u8, u32, u32, u32) {
    use dkrt_sdk::proto::{CAP_CEILING, CAP_INFO_HASHES, CAP_KINDS};
    let kind = CAP_KINDS[k];
    if let Some((limit, used)) = table.cap(kind) {
        return (kind, limit, used, 0);
    }
    let w = p.info(CAP_INFO_HASHES[k]);
    if w.is_null() {
        return base;
    }
    // never more than the consumers' arrays hold (`ceiling` of the kind in sdk.toml)
    (kind, read_volatile(w).min(CAP_CEILING[k]), 0, 0)
}

/// `dkrt_capacity(out, max)`: the entries of the CAPACITY command (one `DkrtCap` per resource, kinds in sdk/sdk.toml `[capacity.kinds]`), at most
/// `max` of them; returns how many it wrote. The SysEx command and the DIGIKIT menu's MEMORY page read the same figures.
pub unsafe extern "C" fn dkrt_capacity(out: *mut DkrtCap, max: u32) -> u32 {
    init_once();
    use dkrt_sdk::proto::*;
    let s = st();
    let (af, al) = arena::stats();
    let (df, dl) = dsp::area_stats();
    let area = dsp::area_bytes();
    let arena = arena::size();
    let mut mods = 0;
    let mut i = 0;
    while i < MAX_MODULES {
        if s.modules[i].used != 0 {
            mods += 1;
        }
        i += 1;
    }
    let stage = STAGE_BYTES as u32;
    // the store: its area in KiB (0 when the card is too small for it), and what the headers say is used (a card with no header: nothing)
    let (st_total, st_used) = store::capacity();
    let (set_limit, set_used) = settings::capacity();
    // the DSP load: the headroom and the voices of the device profile; the load is the boot plan's (the module set is fixed per session)
    let (dsp_used, dsp_top) = crate::package::loader::planned_dsp();
    // the services declare their own counts (machine, voice, parameter and page rows, effect and send slots, map entries), per kind
    let table = services();
    // the values of each kind (a table, not a `match`: a jump table would put data in .text)
    let entries: [(u8, u32, u32, u32); 10] = [
        (CAPK_MODULES, MAX_MODULES as u32, mods, 0),
        (CAPK_SELECTION, dkrt_sdk::SEL_MAX as u32, boot::selected(), 0),
        (CAPK_ARENA, arena, arena - af, al),
        (CAPK_STAGE, stage, 0, stage),
        (CAPK_DSP_AREA, area, area - df, dl),
        (CAPK_STORE, st_total, st_used, 0),
        (CAPK_SETTINGS, set_limit, set_used, 0),
        (CAPK_DSP_COST, PLATFORM.dsp_headroom(), dsp_used, dsp_top),
        (CAPK_DSP_VOICES, PLATFORM.dsp_voices(), 0, 0),
        // the system package block, apart from the arena: its bytes and what the placed package takes of them
        (CAPK_PACKAGE, PACKAGE_BYTES as u32, s.package_bytes, 0),
    ];
    // one entry per kind of `[capacity.kinds]`, in its order; a kind with no row above reports zeros
    let mut k = 0;
    while k < CAP_KINDS.len() && (k as u32) < max {
        let kind = CAP_KINDS[k];
        let mut v = (kind, 0, 0, 0);
        let mut e = 0;
        while e < entries.len() {
            if entries[e].0 == kind {
                v = entries[e];
            }
            e += 1;
        }
        v = kind_value(&crate::package::resident::platform::PLATFORM, &table, k, v);
        *out.add(k) = DkrtCap { kind: kind as u32, limit: v.1, used: v.2, largest: v.3 };
        k += 1;
    }
    k as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::FakePlatform;
    use dkrt_abi::profiles;
    use dkrt_sdk::proto::*;

    fn hash(kind: u8) -> u32 {
        CAP_INFO_HASHES[CAP_KINDS.iter().position(|k| *k == kind).unwrap()]
    }

    fn caps(p: &FakePlatform, table: &dkrt_abi::service::ServiceTable) -> std::vec::Vec<DkrtCap> {
        (0..CAP_KINDS.len())
            .map(|k| {
                let v = unsafe { kind_value(p, table, k, (CAP_KINDS[k], 0, 0, 0)) };
                DkrtCap { kind: v.0 as u32, limit: v.1, used: v.2, largest: v.3 }
            })
            .collect()
    }

    fn limits(c: &[DkrtCap]) -> [(u32, u32); 3] {
        let g = |kind: u8| c.iter().find(|e| e.kind == kind as u32).map(|e| (e.limit, e.used)).unwrap();
        [g(CAPK_MACHINE_ROWS), g(CAPK_PARAM_ROWS), g(CAPK_PAGE_ROWS)]
    }

    /// With no machines service loaded the limits are the `cap_<kind>` words of the image's info table, and a module that uses one of each is accepted.
    #[test]
    fn the_image_info_table_gives_the_limits_when_no_service_is_loaded() {
        let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        let p = FakePlatform::new(profiles::DT2);
        for (kind, v) in [(CAPK_MACHINE_ROWS, 8), (CAPK_PARAM_ROWS, 16), (CAPK_PAGE_ROWS, 8)] {
            p.info_entries.borrow_mut().push((hash(kind), std::vec![v]));
            p.info_data.borrow_mut().push(hash(kind));
        }
        let none = dkrt_abi::service::ServiceTable::new(&[], &crate::package::core_api::REAL_CORE);
        let c = caps(&p, &none);
        assert_eq!(limits(&c), [(8, 0), (16, 0), (8, 0)]);
        // every kind that needs room has it: only the three rows are checked here
        let n = dkrt_fit::Needs { machine_rows: 1, param_rows: 1, page_rows: 1, ..Default::default() };
        let rows: std::vec::Vec<DkrtCap> = c.iter().filter(|e| [CAPK_MACHINE_ROWS, CAPK_PARAM_ROWS, CAPK_PAGE_ROWS].contains(&(e.kind as u8))).copied().collect();
        assert_eq!(dkrt_fit::fits_now(&n, &rows), None, "uses {{1,1,1}} are accepted");
        // an info value over the ceiling of its kind is reported as the ceiling
        p.info_entries.borrow_mut().push((hash(CAPK_FX_SLOTS), std::vec![1000]));
        let c = caps(&p, &none);
        let fx = c.iter().find(|e| e.kind == CAPK_FX_SLOTS as u32).unwrap();
        assert_eq!((fx.limit, fx.used), (CAPMAX_FX_SLOTS, 0));
        assert!(CAPMAX_FX_SLOTS < 1000);
        // without the entries (no adapter reserves them) the limits are 0 and the module is refused
        let empty = FakePlatform::new(profiles::DT2);
        assert_eq!(limits(&caps(&empty, &none)), [(0, 0), (0, 0), (0, 0)]);
    }

    /// A loaded service's answer wins over the info entry and owns `used`.
    #[test]
    fn a_loaded_services_answer_wins_over_the_info_entry() {
        use dkrt_abi::service::{Service, ServiceRow, ServiceTable};
        struct Machines;
        impl<C: crate::package::core_api::CoreApi> Service<C> for Machines {
            dkrt_abi::accepts_records!();
            dkrt_abi::accepts_teardown!();

            unsafe fn cap(_: &C, kind: u8) -> Option<(u32, u32)> {
                (kind == CAPK_MACHINE_ROWS).then_some((12, 2))
            }
        }
        static ROWS: [ServiceRow; 1] = [ServiceRow::of::<Machines, crate::package::core_api::RealCore>("machines", &[])];
        static ROWS_P: dkrt_abi::service::RowPtrs<1> = dkrt_abi::service::RowPtrs::of(&ROWS);
        let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        let p = FakePlatform::new(profiles::DT2);
        for (kind, v) in [(CAPK_MACHINE_ROWS, 8), (CAPK_PARAM_ROWS, 16)] {
            p.info_entries.borrow_mut().push((hash(kind), std::vec![v]));
            p.info_data.borrow_mut().push(hash(kind));
        }
        let table = ServiceTable::new(&ROWS_P.0, &crate::package::core_api::REAL_CORE);
        let c = caps(&p, &table);
        let l = limits(&c);
        assert_eq!(l[0], (12, 2), "the service's answer wins and owns used");
        assert_eq!(l[1], (16, 0), "a kind the service does not count comes from the info entry");
    }
}

/// CAPACITY: per resource of the runtime, its limit, what is used and the largest free piece. Body: none. Reply: `u8 version` (1), `u8 n`, then
/// `n` entries of `u8 kind` (`CAPK_*`, sdk/sdk.toml `[capacity.kinds]`), `u32 limit`, `u32 used`, `u32 largest` (0 where a largest piece has
/// no meaning). A host skips a kind it does not know; new kinds are appended.
pub(crate) unsafe fn cmd_capacity(seq: u32) -> i32 {
    let mut entries = [dkrt_sdk::DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; dkrt_sdk::CAP_MAX];
    let n = crate::package::capacity::dkrt_capacity(entries.as_mut_ptr(), dkrt_sdk::CAP_MAX as u32) as usize;
    let rb = crate::package::resident::transport::rbody().add(1);
    *rb = 1;
    *rb.add(1) = n as u8;
    let mut at = 2usize;
    let mut k = 0;
    while k < n {
        let e = &entries[k];
        *rb.add(at) = e.kind as u8;
        dkrt_abi::mem::wr32(rb.add(at + 1), e.limit);
        dkrt_abi::mem::wr32(rb.add(at + 5), e.used);
        dkrt_abi::mem::wr32(rb.add(at + 9), e.largest);
        at += 13;
        k += 1;
    }
    crate::package::resident::transport::reply(dkrt_sdk::proto::CMD_CAPACITY, seq, dkrt_sdk::OK, at)
}
