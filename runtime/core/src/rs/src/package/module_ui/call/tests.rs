use super::super::*;
use super::{OUTCOME_BLOCKS, REQUEST_MAX};
use dkrt_sdk::proto::{E_MODULE_UI_METHOD, E_MODULE_UI_TOO_BIG};

/// Echo the request reversed; method 1.
unsafe extern "C" fn echo(
    _: *mut u8,
    input: *const u8,
    len: u32,
    out: *mut u8,
    out_len: *mut u32,
) -> i32 {
    assert_eq!(*out_len as usize, REPLY_MAX);
    for i in 0..len as usize {
        *out.add(i) = *input.add(len as usize - 1 - i);
    }
    *out_len = len;
    0
}

unsafe extern "C" fn refuse(_: *mut u8, _: *const u8, _: u32, _: *mut u8, _: *mut u32) -> i32 {
    7
}

unsafe extern "C" fn too_big(
    _: *mut u8,
    _: *const u8,
    _: u32,
    _: *mut u8,
    out_len: *mut u32,
) -> i32 {
    *out_len = REPLY_MAX as u32 + 1;
    0
}

fn table() -> &'static [query_table::Query; 3] {
    use query_table::Query;
    static TABLE: [Query; 3] = [
        Query::new(1, echo),
        Query::new(2, refuse),
        Query::new(4, too_big),
    ];
    &TABLE
}

/// Give module `rec` settings and the three queries, published.
fn enroll_module(reg: &mut Registry, rec: usize) {
    reg.modules[rec].descriptor =
        Descriptor::parse(b"test/m", b"\x01\x04Tool\x01\x01k\x04Name\x04\x00\x00").unwrap();
    assert_eq!(reg.set_queries(rec, table().as_ptr() as *const u8, 3), OK);
    reg.publish(rec);
}

/// A registry whose module 0 has settings and the three queries, published.
fn registry() -> Registry {
    let mut reg = Registry::EMPTY;
    enroll_module(&mut reg, 0);
    reg
}

fn call(instance: u32, ticket: u32, method: u8, request: &[u8]) -> Vec<u8> {
    call_of(0, instance, ticket, method, request)
}

fn call_of(rec: u8, instance: u32, ticket: u32, method: u8, request: &[u8]) -> Vec<u8> {
    let mut body = std::vec![CALL, rec];
    body.extend_from_slice(&instance.to_be_bytes());
    body.extend_from_slice(&ticket.to_be_bytes());
    body.push(method);
    body.extend_from_slice(request);
    body
}

fn poll(instance: u32, ticket: u32) -> Vec<u8> {
    poll_of(0, instance, ticket)
}

fn poll_of(rec: u8, instance: u32, ticket: u32) -> Vec<u8> {
    let mut body = std::vec![POLL, rec];
    body.extend_from_slice(&instance.to_be_bytes());
    body.extend_from_slice(&ticket.to_be_bytes());
    body
}

/// Run the queued call as the UI task does; the poll reply of `ticket`.
fn run(reg: &mut Registry, instance: u32, ticket: u32) -> Vec<u8> {
    let job = reg.start_call(|_| false, 0).expect("a queued call");
    let result = unsafe { job.run(core::ptr::null_mut()) };
    reg.finish_call(&job, result, 0);
    let mut out = [0; REPLY_MAX + 9];
    let n = reg.handle(&poll(instance, ticket), &mut out).unwrap();
    out[..n].to_vec()
}

/// The runtime status of a poll reply.
fn status(reply: &[u8]) -> i32 {
    i32::from_be_bytes(reply[1..5].try_into().unwrap())
}

/// The handler result of a poll reply.
fn handler(reply: &[u8]) -> i32 {
    i32::from_be_bytes(reply[5..9].try_into().unwrap())
}

#[test]
fn a_call_queues_runs_on_the_ui_pass_and_its_reply_waits_for_the_poll() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    assert_eq!(reg.handle(&call(instance, 9, 1, b"abc"), &mut out), Ok(0));
    // pending until the UI pass runs it
    let n = reg.handle(&poll(instance, 9), &mut out).unwrap();
    assert_eq!(&out[..n], [1]);
    let reply = run(&mut reg, instance, 9);
    assert_eq!(reply[0], 0);
    assert_eq!((status(&reply), handler(&reply)), (OK, OK));
    assert_eq!(&reply[9..], b"cba");
    // the poll handed the outcome over and freed it
    assert_eq!(reg.handle(&poll(instance, 9), &mut out), Err(E_NOT_FOUND));
    // an unknown ticket
    assert_eq!(reg.handle(&poll(instance, 10), &mut out), Err(E_NOT_FOUND));
}

#[test]
fn a_second_ticket_is_busy_and_a_repeat_is_not() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    assert_eq!(reg.handle(&call(instance, 1, 1, b"x"), &mut out), Ok(0));
    assert_eq!(
        reg.handle(&call(instance, 2, 1, b"x"), &mut out),
        Err(E_BUSY)
    );
    assert_eq!(reg.handle(&call(instance, 1, 1, b"x"), &mut out), Ok(0));
    assert_eq!(
        reg.handle(&call(instance, 1, 1, b"y"), &mut out),
        Err(E_MSG)
    );
    // a drained queue takes the next one
    run(&mut reg, instance, 1);
    assert_eq!(reg.handle(&call(instance, 2, 1, b"x"), &mut out), Ok(0));
}

#[test]
fn the_envelope_is_checked_before_queueing() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    assert_eq!(reg.handle(&call(instance, 0, 1, b""), &mut out), Err(E_MSG));
    assert_eq!(
        reg.handle(&call(instance, 1, 1, &[0; REQUEST_MAX + 1]), &mut out),
        Err(E_MSG)
    );
    assert_eq!(
        reg.handle(&call(instance, 1, 1, &[0; REQUEST_MAX]), &mut out),
        Ok(0)
    );
    let mut short = call(instance, 2, 1, b"");
    short.truncate(short.len() - 1);
    assert_eq!(reg.handle(&short, &mut out), Err(E_MSG));
    // another instance is not this module
    assert_eq!(
        reg.handle(&call(instance ^ 1, 3, 1, b""), &mut out),
        Err(E_NOT_FOUND)
    );
}

#[test]
fn an_unknown_method_has_its_own_error() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    for method in [0, 3, 5, 255] {
        assert_eq!(
            reg.handle(&call(instance, 1, method, b""), &mut out),
            Err(E_MODULE_UI_METHOD)
        );
    }
    // nothing was queued
    assert!(reg.start_call(|_| false, 0).is_none());
    // a module that registered no table has no method
    reg.modules[0].queries = QueryTable::EMPTY;
    assert_eq!(
        reg.handle(&call(instance, 1, 1, b""), &mut out),
        Err(E_MODULE_UI_METHOD)
    );
}

#[test]
fn the_handler_error_code_passes_through_and_an_oversize_reply_is_too_big() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    assert_eq!(reg.handle(&call(instance, 1, 2, b"x"), &mut out), Ok(0));
    let reply = run(&mut reg, instance, 1);
    assert_eq!((status(&reply), handler(&reply), reply.len()), (OK, 7, 9));
    assert_eq!(reg.handle(&call(instance, 2, 4, b"x"), &mut out), Ok(0));
    let reply = run(&mut reg, instance, 2);
    assert_eq!(
        (status(&reply), handler(&reply), reply.len()),
        (E_MODULE_UI_TOO_BIG, OK, 9)
    );
}

#[test]
fn a_call_for_an_instance_that_is_gone_finishes_without_running() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    assert_eq!(reg.handle(&call(instance, 1, 1, b"x"), &mut out), Ok(0));
    reg.modules[0].instance = instance.wrapping_add(7);
    assert!(reg.start_call(|_| false, 0).is_none());
    reg.modules[0].instance = instance;
    let n = reg.handle(&poll(instance, 1), &mut out).unwrap();
    assert_eq!((status(&out[..n]), handler(&out[..n])), (E_NOT_FOUND, OK));
}

#[test]
fn a_draining_module_keeps_its_call_queued_and_unloading_cancels_it() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    assert_eq!(reg.handle(&call(instance, 1, 1, b"x"), &mut out), Ok(0));
    assert!(reg.start_call(|_| true, 0).is_none());
    reg.withdraw(0);
    assert!(reg.start_call(|_| false, 0).is_none());
    assert_eq!(reg.handle(&poll(instance, 1), &mut out), Err(E_NOT_FOUND));
}

#[test]
fn unloading_frees_the_call_of_that_module_only() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    reg.handle(&call(instance, 1, 1, b"x"), &mut out).unwrap();
    let job = reg.start_call(|_| false, 0).unwrap();
    let result = unsafe { job.run(core::ptr::null_mut()) };
    reg.finish_call(&job, result, 0);
    reg.withdraw(3);
    assert_eq!(
        reg.handle(&call(instance, 2, 1, b"x"), &mut out),
        Err(E_BUSY)
    );
    reg.withdraw(0);
    assert_eq!(reg.handle(&poll(instance, 1), &mut out), Err(E_NOT_FOUND));
}

#[test]
fn one_call_is_in_flight_for_the_whole_device() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    enroll_module(&mut reg, 1);
    let (a, b) = (reg.modules[0].instance, reg.modules[1].instance);
    let mut out = [0; REPLY_MAX + 9];
    // queued
    assert_eq!(reg.handle(&call_of(0, a, 1, 1, b"ab"), &mut out), Ok(0));
    assert_eq!(
        reg.handle(&call_of(1, b, 1, 1, b"x"), &mut out),
        Err(E_BUSY)
    );
    // running
    let job = reg.start_call(|_| false, 0).unwrap();
    assert_eq!(
        reg.handle(&call_of(1, b, 1, 1, b"x"), &mut out),
        Err(E_BUSY)
    );
    // finished, the outcome unread
    let result = unsafe { job.run(core::ptr::null_mut()) };
    reg.finish_call(&job, result, 0);
    assert_eq!(
        reg.handle(&call_of(1, b, 1, 1, b"x"), &mut out),
        Err(E_BUSY)
    );
    // the other module's poll does not see or take it, even with the same ticket
    assert_eq!(reg.handle(&poll_of(1, b, 1), &mut out), Err(E_NOT_FOUND));
    // the owner's poll gets its own result and frees the slot
    let n = reg.handle(&poll_of(0, a, 1), &mut out).unwrap();
    assert_eq!(&out[9..n], b"ba");
    assert_eq!(reg.handle(&call_of(1, b, 1, 1, b"xyz"), &mut out), Ok(0));
    let job = reg.start_call(|_| false, 0).unwrap();
    let result = unsafe { job.run(core::ptr::null_mut()) };
    reg.finish_call(&job, result, 0);
    let n = reg.handle(&poll_of(1, b, 1), &mut out).unwrap();
    assert_eq!(&out[9..n], b"zyx");
}

#[test]
fn an_unread_outcome_is_freed_after_the_timeout() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    reg.handle(&call(instance, 1, 1, b"x"), &mut out).unwrap();
    let job = reg.start_call(|_| false, 100).unwrap();
    let result = unsafe { job.run(core::ptr::null_mut()) };
    reg.finish_call(&job, result, 100);
    reg.age_out(100 + OUTCOME_BLOCKS - 1, OUTCOME_BLOCKS);
    assert_eq!(
        reg.handle(&call(instance, 2, 1, b"x"), &mut out),
        Err(E_BUSY)
    );
    reg.age_out(100 + OUTCOME_BLOCKS, OUTCOME_BLOCKS);
    assert_eq!(reg.handle(&poll(instance, 1), &mut out), Err(E_NOT_FOUND));
    assert_eq!(reg.handle(&call(instance, 2, 1, b"x"), &mut out), Ok(0));
    // a queued or running call is not aged out
    reg.age_out(u32::MAX, 1);
    assert_eq!(reg.handle(&poll(instance, 2), &mut out), Ok(1));
}

#[test]
fn a_reload_into_the_same_slot_cannot_take_the_old_calls_result() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let mut reg = registry();
    let old = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    reg.handle(&call(old, 1, 1, b"old"), &mut out).unwrap();
    let job = reg.start_call(|_| false, 0).unwrap();
    // the module is replaced while its call is out, and the new copy asks with the same ticket
    reg.withdraw(0);
    enroll_module(&mut reg, 0);
    let new = reg.modules[0].instance;
    assert_ne!(old, new);
    reg.handle(&call(new, 1, 1, b"new"), &mut out).unwrap();
    // the old call finishes: its result belongs to no one
    let result = unsafe { job.run(core::ptr::null_mut()) };
    reg.finish_call(&job, result, 0);
    assert_eq!(reg.handle(&poll(new, 1), &mut out), Ok(1));
    assert_eq!(out[0], 1);
    let reply = run(&mut reg, new, 1);
    assert_eq!(&reply[9..], b"wen");
}

#[test]
fn a_table_on_a_two_byte_boundary_is_read() {
    use query_table::Query;
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let size = core::mem::size_of::<Query>();
    let mut bytes = std::vec![0u8; 3 * size + 4];
    let start = (0..4)
        .find(|at| (bytes.as_ptr() as usize + at) % 4 == 2)
        .unwrap();
    for (i, q) in [
        Query::new(1, echo),
        Query::new(2, refuse),
        Query::new(4, too_big),
    ]
    .into_iter()
    .enumerate()
    {
        let raw = unsafe { core::slice::from_raw_parts(&q as *const Query as *const u8, size) };
        bytes[start + i * size..start + (i + 1) * size].copy_from_slice(raw);
    }
    let mut reg = Registry::EMPTY;
    reg.modules[0].descriptor =
        Descriptor::parse(b"test/m", b"\x01\x04Tool\x01\x01k\x04Name\x04\x00\x00").unwrap();
    assert_eq!(
        reg.set_queries(0, unsafe { bytes.as_ptr().add(start) }, 3),
        OK
    );
    reg.publish(0);
    let instance = reg.modules[0].instance;
    let mut out = [0; REPLY_MAX + 9];
    reg.handle(&call(instance, 1, 4, b"x"), &mut out).unwrap();
    assert_eq!(handler(&run(&mut reg, instance, 1)), OK);
}

#[test]
fn calls_and_settings_requests_are_served_in_turn() {
    use super::super::serve;
    let (mut calls, mut settings) = (0, 0);
    let mut first = false;
    for _ in 0..10 {
        first = !first;
        serve(
            first,
            || {
                calls += 1;
                true
            },
            || {
                settings += 1;
                true
            },
        );
    }
    assert_eq!((calls, settings), (5, 5));
    // a kind with nothing pending does not hold the pass
    assert!(serve(true, || false, || true));
    assert!(serve(false, || false, || true));
    assert!(!serve(true, || false, || false));
}

#[test]
fn registration_refuses_a_missing_descriptor_a_second_table_and_a_bad_table() {
    use query_table::Query;
    let mut reg = Registry::EMPTY;
    let good = table().as_ptr() as *const u8;
    // before dk_workbench_register
    assert_eq!(reg.set_queries(0, good, 3), E_STATE);
    reg.modules[0].descriptor =
        Descriptor::parse(b"test/m", b"\x01\x04Tool\x01\x01k\x04Name\x04\x00\x00").unwrap();
    let unsorted = [Query::new(2, echo), Query::new(1, echo)];
    let repeated = [Query::new(1, echo), Query::new(1, echo)];
    let from_zero = [Query::new(0, echo)];
    let no_function = [Query::without_function(1)];
    let long: Vec<Query> = (1..=17).map(|m| Query::new(m, echo)).collect();
    for (bad, count) in [
        (core::ptr::null(), 1),
        (good, 0),
        (unsorted.as_ptr() as *const u8, 2),
        (repeated.as_ptr() as *const u8, 2),
        (from_zero.as_ptr() as *const u8, 1),
        (no_function.as_ptr() as *const u8, 1),
        (long.as_ptr() as *const u8, 17),
        ((good as usize + 1) as *const u8, 1),
    ] {
        assert_eq!(reg.set_queries(0, bad, count), E_FORMAT);
    }
    assert_eq!(reg.set_queries(0, good, 3), OK);
    assert_eq!(reg.set_queries(0, good, 3), E_STATE);
    // sixteen is the limit
    reg.withdraw(0);
    reg.modules[0].descriptor =
        Descriptor::parse(b"test/m", b"\x01\x04Tool\x01\x01k\x04Name\x04\x00\x00").unwrap();
    assert_eq!(reg.set_queries(0, long.as_ptr() as *const u8, 16), OK);
}

#[test]
fn registration_outside_a_module_context_is_refused() {
    let _guard = crate::test_lock::LOCK
        .lock_with_package()
        .unwrap_or_else(|e| e.into_inner());
    let status = unsafe {
        dk_workbench_register_queries(core::ptr::null_mut(), table().as_ptr() as *const u8, 3)
    };
    assert_eq!(status, E_CONTEXT);
}
