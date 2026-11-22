//! A reload and a live load: the checks over the live set, the teardown and the load into the old record.
use super::*;

#[test]
fn a_reload_check_accepts_a_file_that_keeps_the_keys_and_changes_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let (live, allocs, base) = (l.m.live.len(), l.m.allocs, st().modules[0].base);
        // the same keys and a new one
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[2, 1, 7]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
        assert_eq!((l.m.live.len(), l.m.allocs, st().modules[0].used, st().modules[0].base), (live, allocs, 1, base));
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))));
        assert_eq!(l.table_len[0], dkneeds::keys::table_size(&dkneeds::keys::Table { keys: &[dkneeds::keys::KeyGroup { kind: dkneeds::KIND_MACHINE, keys: &[1, 2] }], ..Default::default() }), "the old table stays");
    }
}

#[test]
fn a_reload_check_refuses_a_missing_record_a_dropped_key_another_name_and_a_damaged_file() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let key = |l: &mut Fake, f: Vec<u8>| {
            l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), f);
            l.reload_check(0, PATH.as_ptr(), &wide())
        };
        assert_eq!(l.reload_check(1, PATH.as_ptr(), &wide()), E_NOT_FOUND, "a record that is not live");
        assert_eq!(l.reload_check(MAX_MODULES, PATH.as_ptr(), &wide()), E_NOT_FOUND, "a provider record is no module");
        assert_eq!(key(&mut l, keyed_file(b'a', &[1])), E_RELOAD_KEYS);
        assert_eq!(key(&mut l, keyed_file(b'z', &[1, 2])), E_FORMAT);
        let mut bad = keyed_file(b'a', &[1, 2]);
        bad[0] ^= 1;
        assert_eq!(key(&mut l, bad), E_FORMAT);
        l.m.files.remove(&PATH[..PATH.len() - 1].to_vec());
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), E_NOT_FOUND, "the new file is not in the store");
        // the old module is still there, whole
        assert_eq!((st().modules[0].used, l.m.live.len()), (1, 1));
    }
}

#[test]
fn a_reload_check_runs_the_planner_over_the_live_set_with_the_file_in_the_old_slot() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        // a second live module `b` has key 9; a new `a` that also has key 9 clashes with it (the old `a` did not)
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
        booted(&mut l, 1, PATH_B);
        l.phase = Phase::Closed;
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 9]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), E_PLAN_KEY);
        // without the clash it passes, and the old key set of `a` itself is not a clash with its own old file
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 10]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
        // a live set the capacity cannot hold with the new file is refused
        let mut tight = wide();
        let mods = tight.iter().position(|c| c.kind == dkrt_sdk::proto::CAPK_MODULES as u32).unwrap();
        tight[mods].limit = 1;
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &tight), E_PLAN_FIT);
    }
}

#[test]
fn a_second_unload_after_busy_runs_fini_and_the_undo_once_and_an_aborted_reload_gives_the_tick_back() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let mut rt = adopt(&mut l, &[0]);
        let base = st().modules[0].base;
        assert_eq!(crate::entry::tick_of(0), base + 8);
        let unloads = crate::boot::info().unloads;
        // the job's quiesce stops the tick; the DSP stays retained: the unload is busy and takes nothing down
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        dsp::ds().phase = dsp::PH_RETAINED;
        assert_eq!(rt.unload(0), E_BUSY);
        assert_eq!(crate::boot::info().unloads, unloads, "a teardown that stopped is not counted");
        assert_eq!(crate::entry::tick_of(0), 0);
        assert_eq!(st().modules[0].used, 1, "the old module is not released");
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))));
        // a reload that gives up before `fini` gives the tick back
        assert!(l.reload_abort(0));
        assert_eq!(crate::entry::tick_of(0), base + 8);
        // and the teardown can be tried again: the DSP is free this time, `fini` and the undo run once
        dsp::ds().phase = dsp::PH_IDLE;
        assert_eq!(rt.unload(0), OK);
        assert_eq!(crate::boot::info().unloads, unloads.wrapping_add(1), "a module's unload counts once (DkrtBootInfo.unloads)");
        let n = |v: &Vec<Ev>, e: Ev| v.iter().filter(|x| **x == e).count();
        assert_eq!((n(&l.m.log, Ev::Fini(0, base + 4)), n(&l.r.log, Ev::Remove(0))), (1, 1));
        assert_eq!(l.torn[0], 0, "the mask is clear once the record is free");
        // a teardown that was interrupted after `fini` ran does not run it again, and cannot give the tick back
        let mut l = live_a();
        let mut rt = adopt(&mut l, &[0]);
        l.torn[0] = TORN_TICK | TORN_FINI;
        assert!(!l.reload_abort(0));
        assert_eq!(rt.unload(0), OK);
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))), "fini is not run twice");
    }
}

#[test]
fn a_reload_loads_the_new_file_into_the_old_record_and_can_be_repeated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
        booted(&mut l, 1, PATH_B);
        l.phase = Phase::Closed;
        let mut rt = adopt(&mut l, &[0, 1]);
        for round in 0..2u32 {
            let keys: &[u32] = if round == 0 { &[1, 2, 5] } else { &[1, 2, 5, 6] };
            l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', keys));
            assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
            assert_eq!(rt.unload(0), OK);
            assert_eq!(st().modules[0].used, 0);
            assert_eq!(rt.reload_load_with(0, PATH.as_ptr(), &wide()), OK, "round {round}");
            // the same record, owner and selection index; the table is the new one; the phase is closed again
            assert_eq!((st().modules[0].used, l.sel_of(0), l.phase == Phase::Closed), (1, 0, true));
            assert_eq!(crate::entry::tick_of(0), st().modules[0].base + 8);
            let mut out = [0u32; 8];
            assert_eq!(l.keys_of(0, dkneeds::KIND_MACHINE, &mut out), keys.len());
            assert_eq!(out[..keys.len()], *keys);
            assert_eq!(st().modules[1].used, 1, "the other module is untouched");
        }
        assert_eq!(l.m.live.len(), 2, "one block each: nothing leaked over two reloads");
        // a reload is the only load after the boot: the same call in the closed phase through `load` is refused
        assert_eq!(l.load(PATH.as_ptr()), E_BOOT_ONLY);
        assert_eq!(rt.unload(0), OK);
        assert_eq!(rt.reload_load_with(MAX_MODULES, PATH.as_ptr(), &wide()), E_NOT_FOUND, "a provider record is not reloaded");
    }
}

#[test]
fn a_reload_waits_for_commands_in_flight_and_two_blocks_after_it_closed_the_entries() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let base = st().modules[0].base;
        l.p.clock.set(Some(100));
        crate::module_cmds::test_set_in_flight(1, 1);
        // the first call closes the entries: the tick stops, the commands of the module are refused, and it waits
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        assert_eq!(crate::entry::tick_of(0), 0);
        // blocks pass, but a command of the module is still in flight
        l.p.clock.set(Some(105));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        crate::module_cmds::test_set_in_flight(1, 0);
        // the command returned: one block since the entries were closed is not enough, two are
        l.p.clock.set(Some(101));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        l.p.clock.set(Some(102));
        assert_eq!(l.reload_quiesce(0), OK);
        // nothing was freed and no row touched: the old module is whole
        assert_eq!((st().modules[0].used, l.m.live.len()), (1, 1));
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..)) || *e == Ev::Free(base)));
        // the teardown that follows finishes the record and clears the mask
        assert_eq!(adopt(&mut l, &[0]).unload(0), OK);
        assert_eq!(l.torn[0], 0);
    }
}

#[test]
fn the_block_wait_survives_a_counter_wrap_and_a_platform_without_a_clock_waits_for_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        l.p.clock.set(Some(u32::MAX));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        l.p.clock.set(Some(0));
        assert_eq!(l.reload_quiesce(0), E_BUSY, "one block");
        l.p.clock.set(Some(1));
        assert_eq!(l.reload_quiesce(0), OK, "two blocks, over the wrap");
        assert!(l.reload_abort(0));
        // no block clock: the entries close and the wait is over at once
        l.p.clock.set(None);
        assert_eq!(l.reload_quiesce(0), OK);
        // the quiesce marked the module as draining (a static of `module_cmds`): give the commands back for the tests that follow
        assert!(l.reload_abort(0));
    }
}

#[test]
fn an_aborted_reload_gives_back_the_tick_and_the_commands_and_the_sinks_stay_released() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let base = st().modules[0].base;
        // module `a` (source 1) owns the span sink
        crate::dlog::test_reset_sinks();
        let prev = crate::dlog::enter_module(0, crate::dlog::PHASE_CALLBACK);
        assert_eq!(crate::dlog::dk_sink_set(crate::dlog::SINK_SPAN, 0x1000 as *mut u8), OK);
        crate::dlog::leave_module(prev);
        assert!(crate::dlog::sink_on(crate::dlog::SINK_SPAN));
        l.p.clock.set(Some(0));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        assert!(!crate::dlog::sink_on(crate::dlog::SINK_SPAN), "the sink is cleared under the mask when the entries close");
        // a command of the module is refused while it drains, and runs again after the abort
        let prev = crate::dlog::enter_module(0, crate::dlog::PHASE_CALLBACK);
        assert_eq!(crate::module_cmds::host_register(0x38, 1, Some(noop_cmd)), OK);
        crate::dlog::leave_module(prev);
        assert!(crate::module_cmds::begin_cmd(0x38).is_err());
        assert!(l.reload_abort(0));
        assert_eq!(crate::entry::tick_of(0), base + 8);
        let (h, owner) = crate::module_cmds::begin_cmd(0x38).unwrap().expect("registered");
        let _ = h;
        assert_eq!((owner, crate::module_cmds::in_flight(1)), (1, 1), "counted while it runs");
        crate::module_cmds::end_cmd(owner);
        assert_eq!(crate::module_cmds::in_flight(1), 0);
        crate::module_cmds::release_cmds(1);
        crate::dlog::test_reset_sinks();
    }
}

#[test]
fn a_reloads_settle_gets_the_keys_of_the_live_modules_and_the_new_file_in_its_slot() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
        booted(&mut l, 1, PATH_B);
        l.phase = Phase::Closed;
        // module `a` is taken down for its reload and the new file's table is kept in its slot: `b` and the slot of `a` are live
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 5]));
        assert_eq!(adopt(&mut l, &[0, 1]).unload(0), OK);
        let staged = l.m.stage(PATH.as_ptr()).unwrap();
        l.capture(0, core::slice::from_raw_parts(staged.ptr, staged.size as usize));
        let sel = live_settle_ctx(&l.tables, &l.table_len, &l.live_at(0));
        assert_eq!(sel.count, 2);
        assert_eq!((sel.nkeys[0], sel.keys[0][..3].to_vec()), (3, vec![1, 2, 5]));
        assert_eq!((sel.nkeys[1], sel.keys[1][0]), (1, 9));
        // a selection index with no live module hands over nothing, and the count stops at the last live one
        assert_eq!(l.live_at(SEL_MAX), [false, true, false, false, false, false, false, false], "the unloaded record is not live");
        let mut only_b = [false; SEL_MAX];
        only_b[1] = true;
        let sel = live_settle_ctx(&l.tables, &l.table_len, &only_b);
        assert_eq!((sel.count, sel.nkeys[0], sel.nkeys[1]), (2, 0, 1));
    }
}

#[test]
fn a_module_over_half_the_arena_is_not_refused_for_counting_itself_twice() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let own = own_use(0);
        assert!(own.0 >= dkneeds::arena_block(32).unwrap(), "the image block of the module is counted");
        // an arena that the module fills to 60 percent
        let limit = own.0 * 5 / 3;
        let arena = |used| DkrtCap { kind: dkrt_sdk::proto::CAPK_ARENA as u32, limit, used, largest: limit };
        let mut caps = vec![arena(own.0)];
        caps.extend(wide().into_iter().filter(|c| c.kind != dkrt_sdk::proto::CAPK_ARENA as u32));
        live_caps(&mut caps, own);
        assert_eq!(caps[0].used, 0, "the old module's own blocks are given back");
        // without the subtraction the same arena counts the module that is about to leave
        let mut still = vec![arena(own.0)];
        live_caps(&mut still, (0, 0));
        assert_eq!(still[0].used, own.0);
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 7]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &caps), OK);
    }
}

#[test]
fn a_reload_load_checks_the_name_and_the_keys_of_the_bytes_it_loads() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
        let mut rt = adopt(&mut l, &[0]);
        assert_eq!(rt.unload(0), OK);
        let table_before = (l.table_len[0], l.tables[0]);
        // the file is replaced while the module drains: one that drops a key, one of another module, one whose table is gone
        let mut hollow = keyed_file(b'a', &[1, 2]);
        put(&mut hollow, dkneeds::H_REG_SIZE, 0);
        seal(&mut hollow);
        for (what, f, want) in [
            ("a dropped key", keyed_file(b'a', &[2]), E_RELOAD_KEYS),
            ("another name", keyed_file(b'z', &[1, 2]), E_FORMAT),
            ("no table", hollow, E_FORMAT),
        ] {
            l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), f);
            assert_eq!(rt.reload_load_with(0, PATH.as_ptr(), &wide()), want, "{what}");
            assert_eq!(st().modules[0].used, 0, "{what}: nothing loaded");
            assert_eq!((l.table_len[0], l.tables[0]), table_before, "{what}: the kept table is the old one");
        }
        // the bytes that pass the rules load
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 3]));
        assert_eq!(rt.reload_load_with(0, PATH.as_ptr(), &wide()), OK);
    }
}

#[test]
fn a_reload_check_runs_the_file_checks_as_a_load_does_with_the_selection_index_of_the_module_set_and_restores_it() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        l.m.boot_at_stage.clear();
        let table = (l.table_len[0], l.tables[0]);
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 7]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
        // the first stage reads the table (numbering of the new keys), the second is the file checks: the services' checks of the DSP section read the index of the module that loads
        assert_eq!(l.m.boot_at_stage, vec![u32::MAX, 0]);
        assert_eq!(crate::selection::BOOT_IDX, u32::MAX, "the index is as it was");
        assert_eq!((l.table_len[0], l.tables[0]), table, "the kept table is the old one: the numbering used the new table for a moment only");
        // a file that drops a key is refused without numbering anything (the table is not touched either)
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), E_RELOAD_KEYS);
        assert_eq!((l.table_len[0], l.tables[0]), table);
    }
}

#[test]
fn a_live_load_takes_the_record_of_the_given_index_and_the_check_changes_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a_with_b();
        let (live, allocs) = (l.m.live.len(), l.m.allocs);
        // the check: no record, no table, no acceptance of the index, the selection index as it was
        assert_eq!(l.live_check_load(1, PATH_B.as_ptr(), &wide()), OK);
        assert_eq!((l.m.live.len(), l.m.allocs, st().modules[1].used, l.table_len[1], l.accepted[1]), (live, allocs, 0, 0, false));
        assert_eq!(crate::selection::BOOT_IDX, u32::MAX);
        // the load, through the manager: the record is the index's own; the table and the acceptance are kept at the index
        let mut rt = adopt(&mut l, &[0]);
        assert_eq!(rt.live_load_with(4, PATH_B.as_ptr(), &wide()), OK);
        assert_eq!((st().modules[4].used, l.sel_of(4), l.accepted[4], l.phase == Phase::Closed), (1, 4, true, true));
        assert_eq!(crate::entry::tick_of(4), st().modules[4].base + 8);
        let mut out = [0u32; 8];
        assert_eq!(l.keys_of(4, dkneeds::KIND_MACHINE, &mut out), 1);
        assert_eq!(out[0], 9);
        assert_eq!(crate::selection::BOOT_IDX, u32::MAX);
        // the module that was live is untouched, and both are live at their indices
        assert_eq!((st().modules[0].used, l.sel_of(0), l.accepted[0]), (1, 0, true));
        assert_eq!(l.live_at(SEL_MAX), [true, false, false, false, true, false, false, false]);
        assert!(cstr_eq(records::path(4).as_ptr(), PATH_B.as_ptr()), "the record keeps the path it was loaded from");
    }
}

#[test]
fn a_live_check_refuses_a_loaded_name_a_key_clash_a_full_capacity_a_damaged_file_and_an_index_past_the_selection() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a_with_b();
        let mut check = |l: &mut Fake, f: Option<Vec<u8>>, caps: &[DkrtCap]| {
            match f {
                Some(f) => l.m.files.insert(PATH_C[..PATH_C.len() - 1].to_vec(), f),
                None => l.m.files.remove(&PATH_C[..PATH_C.len() - 1].to_vec()),
            };
            l.live_check_load(1, PATH_C.as_ptr(), caps)
        };
        assert_eq!(check(&mut l, Some(keyed_file(b'a', &[7])), &wide()), E_LOADED, "the name of a loaded module");
        assert_eq!(check(&mut l, Some(keyed_file(b'c', &[1])), &wide()), E_PLAN_KEY, "a key a loaded module has");
        let mut tight = wide();
        let mods = tight.iter().position(|c| c.kind == dkrt_sdk::proto::CAPK_MODULES as u32).unwrap();
        tight[mods].limit = 1;
        assert_eq!(check(&mut l, Some(keyed_file(b'c', &[5])), &tight), E_PLAN_FIT);
        let mut bad = keyed_file(b'c', &[5]);
        bad[0] ^= 1;
        assert_eq!(check(&mut l, Some(bad), &wide()), E_FORMAT);
        assert_eq!(check(&mut l, None, &wide()), E_NOT_FOUND, "no such file");
        assert_eq!(check(&mut l, Some(keyed_file(b'c', &[5])), &wide()), OK);
        assert_eq!(l.live_check_load(SEL_MAX, PATH_C.as_ptr(), &wide()), E_SEL_COUNT);
        assert_eq!(adopt(&mut l, &[0]).live_load_with(SEL_MAX, PATH_C.as_ptr(), &wide()), E_SEL_COUNT);
        // nothing was loaded, and no refused file left a table or an acceptance behind
        assert_eq!((st().modules[1].used, l.m.live.len(), l.table_len[1], l.accepted[1]), (0, 1, 0, false));
    }
}

#[test]
fn a_live_load_that_fails_leaves_the_index_free_and_the_next_load_works() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a_with_b();
        let mut rt = adopt(&mut l, &[0]);
        l.m.init = Init::Fails(E_INIT);
        assert_eq!(rt.live_load_with(1, PATH_B.as_ptr(), &wide()), E_INIT);
        assert_eq!((st().modules[1].used, l.accepted[1], l.sel_of(1), l.phase == Phase::Closed), (0, false, u8::MAX, true));
        assert_eq!(l.m.live.len(), 1, "nothing leaked: the image block of module a only");
        let mut out = [0u32; 8];
        assert_eq!(l.keys_of(1, dkneeds::KIND_MACHINE, &mut out), 0);
        assert_eq!(crate::selection::BOOT_IDX, u32::MAX);
        // a file that is torn between the check and the load is refused at the load
        l.m.init = Init::Ok;
        let mut hollow = keyed_file(b'b', &[9]);
        put(&mut hollow, dkneeds::H_REG_SIZE, 0);
        seal(&mut hollow);
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), hollow);
        assert_eq!(rt.live_load_with(1, PATH_B.as_ptr(), &wide()), E_FORMAT);
        assert_eq!((st().modules[1].used, l.accepted[1]), (0, false));
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
        assert_eq!(rt.live_load_with(1, PATH_B.as_ptr(), &wide()), OK);
        assert_eq!((st().modules[1].used, l.sel_of(1), l.accepted[1]), (1, 1, true));
        // a loaded name is refused by the loader itself, with the index free again
        l.m.files.insert(PATH_C[..PATH_C.len() - 1].to_vec(), keyed_file(b'b', &[11]));
        assert_eq!(rt.live_load_with(2, PATH_C.as_ptr(), &wide()), E_LOADED);
        assert_eq!((l.accepted[2], st().modules[2].used), (false, 0));
    }
}

#[test]
fn a_load_after_the_boot_needs_the_live_phase_and_a_provider_record_is_never_loaded_by_it() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a_with_b();
        // outside a live job the loader loads nothing after the boot
        assert_eq!(l.load(PATH_B.as_ptr()), E_BOOT_ONLY);
        assert_eq!(l.phase == Phase::Closed, true);
        // the live phase accepts a free record and a module's own record, and nothing for a provider
        l.phase = Phase::Live;
        crate::selection::BOOT_IDX = 1;
        assert_eq!(l.load_file(PATH_B.as_ptr(), Slot::Free), OK);
        crate::selection::BOOT_IDX = u32::MAX;
        assert_eq!(l.load_file(PATH_B.as_ptr(), Slot::Provider(MAX_MODULES)), E_BOOT_ONLY);
        l.phase = Phase::Closed;
    }
}

#[test]
fn a_live_check_answers_e_missing_for_a_service_nobody_provides_before_it_looks_at_the_files_imports() {
    use dkneeds::keys::{name_hash, write_table, KeyGroup, Need, Table};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a_with_b();
        // a module that needs a service no loaded provider offers, and imports a name that nothing resolves (what the file of a machine does: its imports are the exports of the service)
        let machine = |imports: &[&[u8]], needs: &[Need]| {
            let mut v = module_file_with(&[], imports);
            v.truncate(v.len() - 8);
            let t = Table { needs, keys: &[KeyGroup { kind: dkneeds::KIND_MACHINE, keys: &[5] }], ..Default::default() };
            let at = v.len();
            v.resize(at + dkneeds::keys::table_size(&t), 0);
            write_table(&t, &mut v[at..]).unwrap();
            let n = (v.len() - at) as u32;
            put(&mut v, dkneeds::H_REG_SIZE, n);
            v[dkneeds::H_NAME as usize] = b'c';
            seal(&mut v);
            v
        };
        let need = Need { service: name_hash("no-such-service"), min_version: 0, hard: true };
        l.m.files.insert(PATH_C[..PATH_C.len() - 1].to_vec(), machine(&[b"dk_no_such_export"], &[need]));
        assert_eq!(l.live_check_load(1, PATH_C.as_ptr(), &wide()), E_MISSING, "the plan answers before the imports are resolved");
        // the same file with no unmet need is refused by its import, as a load refuses it
        l.m.files.insert(PATH_C[..PATH_C.len() - 1].to_vec(), machine(&[b"dk_no_such_export"], &[]));
        assert_eq!(l.live_check_load(1, PATH_C.as_ptr(), &wide()), E_IMPORT);
        // an optional need that is not met does not refuse
        let soft = Need { hard: false, ..need };
        l.m.files.insert(PATH_C[..PATH_C.len() - 1].to_vec(), machine(&[], &[soft]));
        assert_eq!(l.live_check_load(1, PATH_C.as_ptr(), &wide()), OK);
        assert_eq!((st().modules[1].used, l.table_len[1], l.accepted[1]), (0, 0, false), "nothing was kept");
    }
}
