//! The checks of a file's `PAYLOADS` items over the loader's fakes: the bounds, the count and the owner as a hard need (`check_body`), and the owner that is loaded, adopts the part and
//! takes the body (`check_payload_owners`). The packed files and the walks are the assembly tests (`assembly_tests/payloads.rs`).
use super::*;
use crate::package::fixture_provider::{instance, Fixture};
use crate::package::provider_rows;
use core::sync::atomic::{AtomicU32, Ordering};
use dkneeds::keys::{name_hash, write_table, Need, Payload, Table};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{ConsumerId, Payloads, Provider, Step};

const OWNER: &str = "payload-owner";
/// An offset from here up is the file's own (not relative to the blob): the bounds tests give offsets that no blob reaches.
const ABSOLUTE: u32 = 0x8000_0000;

/// A module file whose header table has `needs` and the payload items `items` (`offset` relative to `blob`, which follows the table, unless it is `ABSOLUTE` or more).
fn payload_file(needs: &[Need], items: &[Payload], blob: &[u8]) -> Vec<u8> {
    let mut v = module_file();
    v.truncate(v.len() - 8);
    let at = v.len();
    let size = dkneeds::keys::table_size(&Table { needs, payloads: items, ..Default::default() });
    let items: Vec<Payload> = items.iter().map(|p| Payload { offset: if p.offset >= ABSOLUTE { p.offset } else { p.offset + (at + size) as u32 }, ..*p }).collect();
    let t = Table { needs, payloads: &items, ..Default::default() };
    v.resize(at + size, 0);
    write_table(&t, &mut v[at..]).unwrap();
    put(&mut v, dkneeds::H_REG_SIZE, size as u32);
    v.extend_from_slice(blob);
    seal(&mut v);
    v
}

fn owner_need(hard: bool) -> Need {
    Need { service: name_hash(OWNER), min_version: 0, hard }
}

fn item(kind: u16, offset: u32, bytes: u32) -> Payload {
    Payload { service: name_hash(OWNER), kind, offset, bytes }
}

/// Check the file as a load does (`check_file`): the status.
unsafe fn checked(file: Vec<u8>) -> (i32, Fake, Checked) {
    let mut l = loader_of(Init::Ok, file);
    let mut c = Checked::empty();
    let r = l.check_file(PATH.as_ptr(), Slot::Free, &mut c);
    (r, l, c)
}

#[test]
fn the_bounds_the_count_and_the_owner_as_a_hard_need_are_checked_with_the_file() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let blob = [1u8, 2, 3, 4, 5, 6, 7, 8];
        // a body inside the file, its owner a hard need: accepted
        assert_eq!(checked(payload_file(&[owner_need(true)], &[item(1, 0, 4), item(2, 4, 4)], &blob)).0, OK);
        // a body that ends at the end of the file is inside, one byte more is not; an offset past the end, or one that wraps, is not
        assert_eq!(checked(payload_file(&[owner_need(true)], &[item(1, 4, 4)], &blob)).0, OK);
        assert_eq!(checked(payload_file(&[owner_need(true)], &[item(1, 4, 5)], &blob)).0, E_FORMAT);
        assert_eq!(checked(payload_file(&[owner_need(true)], &[item(1, 9, 0)], &blob)).0, E_FORMAT);
        assert_eq!(checked(payload_file(&[owner_need(true)], &[item(1, 0, u32::MAX)], &blob)).0, E_FORMAT);
        assert_eq!(checked(payload_file(&[owner_need(true)], &[item(1, u32::MAX - 2, 8)], &blob)).0, E_FORMAT);
        // more than the format allows
        let many: Vec<Payload> = (0..=dkneeds::keys::MAX_PAYLOADS as u16).map(|k| item(k, 0, 1)).collect();
        assert_eq!(checked(payload_file(&[owner_need(true)], &many, &blob)).0, E_FORMAT);
        let most: Vec<Payload> = (0..dkneeds::keys::MAX_PAYLOADS as u16).map(|k| item(k, 0, 1)).collect();
        assert_eq!(checked(payload_file(&[owner_need(true)], &most, &blob)).0, OK);
        // the owner is a need, but a soft one, or no need at all: the file cannot rely on it
        assert_eq!(checked(payload_file(&[owner_need(false)], &[item(1, 0, 4)], &blob)).0, E_MISSING);
        assert_eq!(checked(payload_file(&[], &[item(1, 0, 4)], &blob)).0, E_MISSING);
        // a file with no payload is as it was
        assert_eq!(checked(payload_file(&[], &[], &[])).0, OK);
    }
}

static LAST: AtomicU32 = AtomicU32::new(0);

/// An owner that takes kind 1 and refuses kind 9 with `-77`; the last check it got is in `LAST` (kind in the low half, body length above, consumer in the top byte).
struct Owner;

impl<C: CoreApi> Provider<C> for Owner {
    fn start_in(_: &C, slot: &mut core::mem::MaybeUninit<Owner>) -> Result<(), i32> {
        slot.write(Owner);
        Ok(())
    }
}

impl<C: CoreApi> Payloads<C> for Owner {
    fn check(&mut self, _: &C, kind: u16, body: &[u8], consumer: ConsumerId) -> Result<(), i32> {
        LAST.store(kind as u32 | (body.len() as u32) << 16 | (consumer.get() as u32) << 24, Ordering::SeqCst);
        if kind == 9 {
            Err(-77)
        } else {
            Ok(())
        }
    }
    fn install(&mut self, _: &C, _: u16, _: Option<&[u8]>, _: ConsumerId) -> Result<Step, i32> {
        Ok(Step::Done)
    }
    fn retire(&mut self, _: &C, _: ConsumerId) -> Step {
        Step::Done
    }
}

/// The owner's row in the table (record `MAX_MODULES + 5`), with or without the payloads part; removed by the caller.
unsafe fn load_owner(with_part: bool) {
    let f = Fixture::of::<Owner>(OWNER);
    let f = if with_part { f.payloads::<Owner>() } else { f };
    assert_eq!(provider_rows::add(MAX_MODULES + 5, crate::package::provider_rows::RowSource { row: f.row(), instance: instance(), extent: 0..usize::MAX }, None), OK);
}

#[test]
fn the_owner_must_be_loaded_adopt_the_part_and_take_the_body() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let blob = [9u8, 8, 7];
        let ok = payload_file(&[owner_need(true)], &[item(1, 0, 3)], &blob);
        let refused = payload_file(&[owner_need(true)], &[item(9, 0, 3)], &blob);
        let (r, mut l, c) = checked(ok.clone());
        assert_eq!(r, OK);
        // no row: a load needs one, a live check of a file whose owner is not up yet leaves it to the place step
        assert_eq!(l.check_payload_owners(&c, 3, true), E_MISSING);
        assert_eq!(l.check_payload_owners(&c, 3, false), OK);

        // a row without the part is no owner of a body
        load_owner(false);
        assert_eq!(l.check_payload_owners(&c, 3, true), E_MISSING);
        assert_eq!(l.check_payload_owners(&c, 3, false), E_MISSING);
        provider_rows::remove(MAX_MODULES + 5);

        // with the part the owner is asked about each body: its kind, the body's bytes and the consumer
        load_owner(true);
        LAST.store(0, Ordering::SeqCst);
        assert_eq!(l.check_payload_owners(&c, 3, true), OK);
        assert_eq!(LAST.load(Ordering::SeqCst), 1 | 3 << 16 | 3 << 24);
        // and its refusal is the answer
        let (r, mut l2, c2) = checked(refused);
        assert_eq!(r, OK);
        assert_eq!(l2.check_payload_owners(&c2, 3, true), -77);
        assert_eq!(l2.check_payload_owners(&c2, 3, false), -77);
        provider_rows::remove(MAX_MODULES + 5);
        let _ = &mut l;
    }
}
