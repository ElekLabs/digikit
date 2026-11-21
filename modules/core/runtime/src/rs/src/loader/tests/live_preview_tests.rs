use super::*;
use crate::core_api::RealCore;
use dkneeds::keys::{table_size, write_table, KeyGroup, Table};
use dkneeds::{KIND_FILTER, KIND_LFO_SHAPE};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::{Service, ServiceRow, SettleCtx};
use std::sync::Mutex;

const ACTIVE: usize = 2;
const CANDIDATE: usize = 4;

#[derive(Debug, PartialEq)]
struct Settle {
    preview: bool,
    restore: bool,
    count: usize,
}

static SETTLES: Mutex<Vec<Settle>> = Mutex::new(Vec::new());

struct Probe;
impl<C: CoreApi> Service<C> for Probe {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();

    unsafe fn settle(_: &C, sel: &SettleCtx) {
        SETTLES.lock().unwrap_or_else(|e| e.into_inner()).push(Settle { preview: sel.is_preview(), restore: sel.is_restore(), count: sel.count });
    }
}

impl Probe {
    fn install() -> Self {
        static ROW: ServiceRow = ServiceRow::of::<Probe, RealCore>(SVC, &[]);
        SETTLES.lock().unwrap_or_else(|e| e.into_inner()).clear();
        assert_eq!(unsafe { crate::svc_rows::add(MAX_MODULES, &ROW) }, OK);
        Self
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        unsafe { crate::svc_rows::remove(MAX_MODULES) };
    }
}

fn file(name: u8, filters: &[u32], shapes: &[u32], imports: &[&[u8]]) -> Vec<u8> {
    let mut bytes = module_file_with(&[], imports);
    let at = bytes.len() - 8;
    let table = Table { keys: &[KeyGroup { kind: KIND_FILTER, keys: filters }, KeyGroup { kind: KIND_LFO_SHAPE, keys: shapes }], ..Default::default() };
    bytes.resize(at + table_size(&table), 0);
    write_table(&table, &mut bytes[at..]).unwrap();
    let len = (bytes.len() - at) as u32;
    put(&mut bytes, dkneeds::H_REG_SIZE, len);
    bytes[H_NAME] = name;
    seal(&mut bytes);
    bytes
}

fn assert_keys(l: &Fake, index: usize, filters: &[u32], shapes: &[u32]) {
    for (kind, want) in [(KIND_FILTER, filters), (KIND_LFO_SHAPE, shapes)] {
        let mut out = [0; 8];
        let count = l.keys_of(index as u32, kind, &mut out);
        assert_eq!(&out[..count], want, "selection index {index}, kind {kind}");
    }
}

fn assert_preview(reload: bool) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::Ok, file(b'a', &[11], &[21], &[]));
        booted(&mut l, ACTIVE, PATH);
        let (old_filters, old_shapes, slot): (&[u32], &[u32], _) = if reload {
            l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), file(b'b', &[12], &[22], &[]));
            booted(&mut l, CANDIDATE, PATH_B);
            (&[12], &[22], Slot::Module(1))
        } else {
            (&[], &[], Slot::Free)
        };
        l.phase = Phase::Closed;
        let _probe = Probe::install();
        let (new_filters, new_shapes): (&[u32], &[u32]) = if reload { (&[12, 13], &[22, 23]) } else { (&[13], &[23]) };
        let candidate = file(b'b', new_filters, new_shapes, &[b"dk_no_such_export"]);
        let staged = Staged { ptr: candidate.as_ptr(), size: candidate.len() as u32 };

        let result = l.with_live_preview(CANDIDATE, &candidate, false, |l| {
            assert_keys(l, CANDIDATE, new_filters, new_shapes);
            assert_keys(l, ACTIVE, &[11], &[21]);
            l.check_staged(staged, slot, &mut Checked::empty(), OK)
        });
        assert_eq!(result, E_IMPORT);
        assert_keys(&l, CANDIDATE, old_filters, old_shapes);
        assert_keys(&l, ACTIVE, &[11], &[21]);
        let settles = core::mem::take(&mut *SETTLES.lock().unwrap_or_else(|e| e.into_inner()));
        let restore_count = if reload { CANDIDATE + 1 } else { ACTIVE + 1 };
        assert_eq!(settles, [Settle { preview: true, restore: false, count: CANDIDATE + 1 }, Settle { preview: false, restore: true, count: restore_count },]);
    }
}

#[test]
fn a_refused_first_load_exposes_typed_keys_only_during_the_preview_and_check() {
    assert_preview(false);
}

#[test]
fn a_refused_reload_restores_the_old_typed_keys() {
    assert_preview(true);
}

#[test]
fn the_real_core_reads_the_real_loaders_candidate_during_the_preview() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = dkrt_svc_rt::ImportCore;
    let mut before = [0; 8];
    let before_count = core.keys_of(CANDIDATE as u32, KIND_FILTER, &mut before);
    let candidate = file(b'b', &[13], &[23], &[]);
    let result = unsafe {
        super::super::loader().with_live_preview(CANDIDATE, &candidate, false, |_| {
            let mut keys = [0; 8];
            let count = core.keys_of(CANDIDATE as u32, KIND_FILTER, &mut keys);
            assert_eq!(&keys[..count], &[13]);
            E_IMPORT
        })
    };
    assert_eq!(result, E_IMPORT);
    let mut after = [0; 8];
    assert_eq!(core.keys_of(CANDIDATE as u32, KIND_FILTER, &mut after), before_count);
    assert_eq!(&after[..before_count], &before[..before_count]);
}

#[test]
fn a_changed_card_file_after_preview_is_refused_before_check_or_load() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let planned = file(b'b', &[13], &[23], &[]);
        let replacement = file(b'b', &[14], &[24], &[]);
        assert_eq!(planned.len(), replacement.len());
        let mut l = loader_of(Init::Ok, replacement);
        for loading in [false, true] {
            let result = l.with_candidate_file(CANDIDATE, PATH.as_ptr(), &planned, loading, |_, _| panic!("changed file reached the installer"));
            assert_eq!(result, E_SHA);
            assert_keys(&l, CANDIDATE, &[], &[]);
        }
    }
}
