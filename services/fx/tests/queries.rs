//! Host tests of the `Query` part of the owner `Fx`: the route choices the FX ROUTES menu asks for (operation 1). The part answers from the table that `Fx::start` bound and from the
//! page and send tables of the service, so a test binds a table, marks a page in use and sets what the effect allows. One accept case and one refusal for each rule of the wire.
use core::ptr::addr_of_mut;
use dkrt_abi::provider::Query;
use dkrt_sdk::{E_CONTRACT, E_NOT_FOUND};
use dkrt_svc_fx::fx::Fx;
use dkrt_svc_fx::fxpage::{self, INFO_HASH};
use dkrt_svc_fx::fxsend::{self, RouteChoicesV1, DKRT_FXS};
use dkrt_test_support::contract::locked;
use dkrt_test_support::FakeCore;

const ROUTE_MASTER: u32 = 1;
const ROUTE_SEND: u32 = 2;

/// An image table of three effect rows and `cells` send cells.
fn bound(cells: u32) -> std::sync::MutexGuard<'static, ()> {
    let guard = locked();
    let mut t = vec![0u32; 64];
    t[0] = 0x4658_5047;
    t[1] = 4;
    t[2] = 1;
    t[4] = 0x2c;
    t[5] = 14;
    t[6] = 3;
    t[8] = 8;
    t[15] = 32;
    t[37] = 70;
    t[38] = 0x46a7;
    t[39] = cells;
    t[61] = 13;
    t[63] = 0x80;
    let mut core = FakeCore::new();
    core.device = 1;
    core.info_tables.push((INFO_HASH, t));
    unsafe {
        fxpage::unbind();
        assert_eq!(fxpage::bind(&core, [0, 0]), dkrt_sdk::OK);
        // effect row 0 has a page of module record 0 and allows both routes
        (*addr_of_mut!(fxpage::DKRT_FXP)).owner[0] = 1;
        (*addr_of_mut!(DKRT_FXS)).allow[0] = (ROUTE_MASTER | ROUTE_SEND) as u8;
    }
    guard
}

impl Drop for Reset {
    fn drop(&mut self) {
        unsafe {
            fxpage::unbind();
            (*addr_of_mut!(fxpage::DKRT_FXP)).owner[0] = 0;
            (*addr_of_mut!(DKRT_FXS)).allow[0] = 0;
        }
    }
}

struct Reset;

fn ask(op: u32, request: &[u8], reply: &mut [u8]) -> Result<usize, i32> {
    Query::query(&Fx::new(), op, request, reply)
}

fn request(row: u32, pending_sends: u32) -> Vec<u8> {
    let mut r = row.to_ne_bytes().to_vec();
    r.extend(pending_sends.to_ne_bytes());
    r
}

#[test]
fn the_reply_is_the_allowed_routes_of_the_row_and_the_call_answers_zero() {
    let _g = bound(2);
    let _r = Reset;
    let mut reply = [0xffu8; 4];
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &request(0, 0), &mut reply),
        Ok(0)
    );
    assert_eq!(u32::from_ne_bytes(reply), ROUTE_MASTER | ROUTE_SEND);
}

#[test]
fn a_send_the_image_has_no_cell_for_is_not_offered() {
    // one cell, taken by another row that is pending SEND: this row keeps only the master
    let _g = bound(1);
    let _r = Reset;
    let mut reply = [0u8; 4];
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &request(0, 1 << 1), &mut reply),
        Ok(0)
    );
    assert_eq!(u32::from_ne_bytes(reply), ROUTE_MASTER);
    // the row's own pending send keeps its cell
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &request(0, 1), &mut reply),
        Ok(0)
    );
    assert_eq!(u32::from_ne_bytes(reply), ROUTE_MASTER | ROUTE_SEND);
}

#[test]
fn an_unknown_operation_a_wrong_size_or_a_bad_row_is_refused_and_the_reply_is_left_alone() {
    let _g = bound(2);
    let _r = Reset;
    let mut reply = [0xaau8; 4];
    let good = request(0, 0);
    assert_eq!(
        ask(2, &good, &mut reply),
        Err(E_NOT_FOUND),
        "unknown operation"
    );
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &good[..4], &mut reply),
        Err(E_CONTRACT)
    );
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &good, &mut [0u8; 3]),
        Err(E_CONTRACT),
        "a reply buffer that is too short"
    );
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &good, &mut [0u8; 8]),
        Err(E_CONTRACT),
        "and one that is too long"
    );
    let rows = dkrt_abi::dsp::MAX_FX as u32;
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &request(rows, 0), &mut reply),
        Err(E_CONTRACT)
    );
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &request(0, 1 << rows), &mut reply),
        Err(E_CONTRACT),
        "a pending bit past the rows"
    );
    assert_eq!(
        ask(fxsend::ROUTE_CHOICES_V1, &request(1, 0), &mut reply),
        Err(E_NOT_FOUND),
        "a row with no page"
    );
    assert_eq!(reply, [0xaa; 4]);
}
