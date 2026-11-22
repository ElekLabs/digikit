use super::*;
use crate::test_lock::LOCK;
use dkrt_sdk::records::KIND_PARAM;

const KEY: u32 = 0x00ab_cd00;
const FXID: u32 = 0x1234;

fn keys(mk: &[u32], fk: &[u32], rows: (u32, u32, u32)) -> KeysCopy {
    let mut k = KeysCopy::empty();
    k.machine_rows = rows.0;
    k.param_rows = rows.1;
    k.page_rows = rows.2;
    k.nm = mk.len();
    k.nf = fk.len();
    k.mk[..mk.len()].copy_from_slice(mk);
    k.fk[..fk.len()].copy_from_slice(fk);
    k
}

#[test]
fn a_dsp_section_must_match_the_keys_table() {
    let k = keys(&[KEY], &[FXID, 0x77], (1, 0, 0));
    assert_eq!(check_dsp_keys(&k, 1, |_| KEY, 2), OK);
    assert_eq!(check_dsp_keys(&k, 1, |_| KEY ^ 1, 2), E_DSP_TYPE, "a voice of a machine that is not in the table");
    assert_eq!(check_dsp_keys(&k, 1, |_| KEY, 1), E_DECL_ID, "an effect record fewer than the table's effects");
    assert_eq!(check_dsp_keys(&k, 0, |_| 0, 3), E_DECL_ID, "an effect record more than the table's effects");
}

#[test]
fn a_generic_record_of_an_unknown_or_a_typed_kind_is_refused_and_the_error_is_kept() {
    // `add_record` reads the service table, a static that the tests of the service rows replace under this lock
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let body = [0u8; 0x100];
    // no service owns kind 999
    let mut a = Collect::new();
    assert_eq!(a.add_record(999, body.as_ptr(), 8), E_FORMAT);
    assert_eq!(a.err, E_FORMAT);
    assert_eq!(a.ng, 0);
    // a typed kind has its builders: even with the right length it is not taken as a generic record
    let mut b = Collect::new();
    assert_eq!(b.add_record(KIND_PARAM, body.as_ptr(), 0x50 - 8), E_FORMAT);
    assert_eq!(b.ng, 0);
    // without an active collection the import refuses and changes nothing
    unsafe {
        end();
        assert_eq!(dk_record_add(core::ptr::null_mut(), KIND_PARAM, body.as_ptr(), 8), E_CONTEXT);
    }
}

#[test]
fn a_header_table_of_runtime_0_7_or_none_is_a_contract_error_and_a_damaged_one_a_format_error() {
    // no table (the old "no machine, no effect" file), and the old table that begins with the row counts
    assert_eq!(KeysCopy::from_bytes(&[]).err(), Some(E_CONTRACT));
    let old = [0u8, 2, 0, 3, 0, 4, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1];
    assert_eq!(KeysCopy::from_bytes(&old).err(), Some(E_CONTRACT));
    // the current table: rows from USES, keys from KEYS
    let mut t = [0u8; 128];
    let n = dkneeds::keys::write_keys((2, 5, 1), 0, &[0x100, 0x200], &[7], &mut t).unwrap();
    let k = KeysCopy::from_bytes(&t[..n]).unwrap();
    assert_eq!((k.machine_rows, k.param_rows, k.page_rows, k.nm, k.nf), (2, 5, 1, 2, 1));
    assert_eq!((k.mk[1], k.fk[0]), (0x200, 7));
    // a damaged table (cut) is a format error, not a contract error
    assert_eq!(KeysCopy::from_bytes(&t[..n - 4]).err(), Some(E_FORMAT));
}

#[test]
fn what_init_declared_must_be_what_the_header_table_carries() {
    use dkneeds::keys::{write_table, KindDecl, Pair, Slot, Table};
    // a header table with one kind (kind 90, 24 bytes, handler at image offset 0x40), one slot (entry 0x60) and one export
    let t = Table {
        kinds: &[KindDecl { kind: 90, len: 24, entry: 0x40 }],
        slots: &[Slot { id: 3, flags: 1, entry: 0x60 }],
        exports: &[Pair { a: 0xaabb, b: 0x50 }],
        // a slot with an entry needs a version 2 table, which PROVIDES makes
        provides: &[Pair { a: 0x5e, b: 0 }],
        ..Table::default()
    };
    let mut b = [0u8; 128];
    let n = write_table(&t, &mut b).unwrap();
    let keys = KeysCopy::from_bytes(&b[..n]).unwrap();
    let base = 0x1000;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // nothing declared: the table's entries are missing
    assert!(!decl_matches(&Collect::new(), &keys, base));
    // the same items, with the addresses of the loaded image
    let declared = || {
        let mut c = Collect::new();
        c.decl.add(Declare::KIND, 90, 24, base + 0x40);
        c.decl.add(Declare::SLOT, 3, 1, base + 0x60);
        c.decl.add(Declare::EXPORT, 0xaabb, base + 0x50, 0);
        c
    };
    assert!(decl_matches(&declared(), &keys, base));
    assert_eq!(decl_diff(&declared(), &keys, base), None);
    // the diff names the first item that is not in both: the export the table has and `init` did not declare (the 0x100 bit), or the hook `init` declared more
    let mut d = declared();
    d.decl.n -= 1;
    assert_eq!(decl_diff(&d, &keys, base), Some((Declare::EXPORT as u32 | 0x100, 0xaabb)));
    let mut d = declared();
    d.decl.add(Declare::HOOK, 1, base, 0);
    assert_eq!(decl_diff(&d, &keys, base), Some((Declare::HOOK as u32, 1)));
    // another handler, another length, an item more
    let mut d = declared();
    d.decl.items[0].c = base + 0x44;
    assert!(!decl_matches(&d, &keys, base));
    let mut d = declared();
    d.decl.items[0].b = 28;
    assert!(!decl_matches(&d, &keys, base));
    // the slot's entry is compared too
    let mut d = declared();
    d.decl.items[1].c = base + 0x64;
    assert!(!decl_matches(&d, &keys, base));
    let mut d = declared();
    d.decl.add(Declare::HOOK, 1, base, 0);
    assert!(!decl_matches(&d, &keys, base));
    // a module with no table entries and no declarations passes
    let none = KeysCopy::empty();
    assert!(decl_matches(&Collect::new(), &none, base));
    // the row of a service file is no entry of the table: it does not count against it
    let mut d = declared();
    d.decl.row(&ROW);
    assert!(decl_matches(&d, &keys, base));
    // outside init the imports refuse
    unsafe {
        end();
        assert_eq!(dk_service_declare_slot(core::ptr::null_mut(), 1, 0, core::ptr::null()), E_CONTEXT);
    }
}

#[test]
fn a_service_file_declares_one_row_in_init_and_only_there() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // outside init
        end();
        assert_eq!(dk_service_declare_row(core::ptr::null_mut(), &ROW), E_CONTEXT);
        // inside init the context must be live; a null row and a second row are refused and kept as the load's error
        let mut b = Box::new(Collect::new());
        begin(&mut *b);
        let ctx = crate::package::entry::bind(0) as *mut u8;
        assert_eq!(dk_service_declare_row(ctx, core::ptr::null()), E_DECL_KIND);
        assert_eq!(b.err, E_DECL_KIND);
        b.err = 0;
        assert_eq!(dk_service_declare_row(core::ptr::null_mut(), &ROW), E_CONTEXT);
        assert_eq!(dk_service_declare_row(ctx, &ROW), OK);
        assert!(core::ptr::eq(b.decl.row_ptr().unwrap(), &ROW));
        assert_eq!(dk_service_declare_row(ctx, &ROW), E_DECL_KIND);
        end();
        crate::package::entry::unbind(0);
        assert_eq!(b.decl.n, 1);
    }
}

static ROW: dkrt_abi::service::ServiceRow = dkrt_abi::service::ServiceRow::of::<Nothing, crate::package::core_api::RealCore>("nothing", &[]);
struct Nothing;
impl<C: dkrt_abi::core_api::CoreApi> dkrt_abi::service::Service<C> for Nothing {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();
}

#[test]
fn the_generic_records_are_appended_to_the_block_the_services_write_in_the_order_they_were_added() {
    use dkrt_abi::service::RecordSink;
    use dkrt_sdk::records::{KIND_LFO_SHAPE, LEN_LFO_SHAPE};
    // the record length comes from the lfo service, a loaded file: its row must be in the table, and no other test may change the table meanwhile
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _l = crate::package::handlers::lfo_tests::Loaded::new();
    let mut c = Collect::new();
    let mut shape = std::vec![0u8; LEN_LFO_SHAPE as usize - 8];
    shape[3] = 7;
    assert_eq!(c.add_record(KIND_LFO_SHAPE, shape.as_ptr(), shape.len() as u32), OK);
    shape[3] = 9;
    assert_eq!(c.add_record(KIND_LFO_SHAPE, shape.as_ptr(), shape.len() as u32), OK);
    let mut measured = RecordSink::measure();
    c.write_generic(&mut measured);
    assert_eq!((measured.at, measured.count), (RecordSink::HEADER + 2 * LEN_LFO_SHAPE as usize, 2));
    let mut buf = std::vec![0u8; measured.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    c.write_generic(&mut out);
    assert_eq!((out.at, out.count), (measured.at, 2), "the pass that writes sizes what the pass that measures counted");
    let at = RecordSink::HEADER;
    assert_eq!(&buf[at..at + 4], &KIND_LFO_SHAPE.to_be_bytes());
    assert_eq!(&buf[at + 4..at + 8], &LEN_LFO_SHAPE.to_be_bytes());
    assert_eq!(buf[at + 8 + 3], 7);
    assert_eq!(buf[at + LEN_LFO_SHAPE as usize + 8 + 3], 9);
}
