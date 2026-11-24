//! The module list: recorded at a save.
use crate::*;
use dkrt_sdk::logk::{PROJECT_MODULE, PROJECT_SAVED};
use dkrt_svc_project::dkp::{self, Image};
use dkrt_svc_project::engine::*;

// ---- the module list: recorded at a save, restored after a load -------------------------------------------------------------------

/// A project file with a module list.
pub(crate) fn file_with_modules(
    slot: u8,
    name: &[u8],
    stamp: u32,
    counter: u32,
    modules: &[(&[u8], u32)],
) -> Vec<u8> {
    let mut i = Box::new(Image::new());
    dkp::reset(&mut i.0, DEV, slot, &n16(name), stamp);
    dkp::set_modules(&mut i.0, modules).unwrap();
    dkp::seal(&mut i.0, counter);
    let used = dkp::head_of(&i.0).used as usize;
    i.0[..used].to_vec()
}

pub(crate) fn id_hash(id: &[u8]) -> u32 {
    ModuleId::new(id).unwrap().hash()
}

/// The module events of the log: (what, status, index, id hash), in order.
pub(crate) fn module_events(r: &Rig) -> Vec<(u32, i32, u32, u32)> {
    r.f.logs
        .iter()
        .filter(|l| l.0 == PROJECT_MODULE)
        .map(|l| (l.1[1], l.1[2] as i32, l.1[3], l.1[4]))
        .collect()
}

/// The module list of the file the service wrote for `slot`.
pub(crate) fn written_modules(r: &Rig, slot: u32, name: &[u8]) -> Vec<(Vec<u8>, u32)> {
    let bytes = &r.f.store[&path(slot, name)];
    dkp::check(bytes, DEV).unwrap();
    dkp::modules(bytes)
        .map(|(id, hash)| (id.to_vec(), hash))
        .collect()
}

/// A rig with project 7 current and the modules `a/x` and `b/y` loaded.
pub(crate) fn rig_with_two_loaded() -> Rig {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.load(7);
    for (id, kind) in [("a/x", "machine"), ("b/y", "fx")] {
        let p = format!("modules/{kind}/{}.dkm", id.replace('/', "-")).into_bytes();
        r.f.hashes.insert(p.clone(), id_hash(id.as_bytes()) ^ 1);
        r.f.loaded.push((id.as_bytes().to_vec(), p));
    }
    r
}

#[test]
fn a_save_writes_the_loaded_modules_into_the_list_and_a_project_with_only_modules_gets_a_file() {
    let mut r = rig_with_two_loaded();
    // no put: the module list alone is digikit data
    r.hook_save(7);
    r.settle();
    assert_eq!(r.f.count(PROJECT_SAVED), 1);
    assert_eq!(
        written_modules(&r, 7, b"SONG"),
        [
            (b"a/x".to_vec(), id_hash(b"a/x") ^ 1),
            (b"b/y".to_vec(), id_hash(b"b/y") ^ 1)
        ],
        "the ids in record order, each with the store hash of its file"
    );
    // the same modules again: nothing changed, nothing is written
    let writes = r.f.ops.len();
    r.hook_save(7);
    r.settle();
    assert_eq!(r.f.ops.len(), writes);
    // a module that came: the list follows
    r.f.loaded
        .push((b"c/z".to_vec(), b"modules/tool/c-z.dkm".to_vec()));
    r.hook_save(7);
    r.settle();
    assert_eq!(written_modules(&r, 7, b"SONG").len(), 3);
}

#[test]
fn a_save_with_no_loaded_module_and_no_data_writes_no_file() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.load(7);
    r.hook_save(7);
    r.settle();
    assert!(r.f.store.is_empty() && r.f.ops.is_empty());
}
