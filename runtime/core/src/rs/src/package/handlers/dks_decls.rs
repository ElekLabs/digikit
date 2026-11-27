//! The declarations of the real `init` of each Rust service file (`dk_svc_declare` of `service!`, the glue of the file's own crate built for the host), for the assembly
//! test that compares them with the packed header table. The runtime names the service file crates only here (layercheck `service-crates`).

/// `dk_svc_declare` of a service file: the declarations of `init` without `setup`, for the device that `device` gives.
pub type DeclareFn = unsafe fn(*mut u8, fn() -> u32) -> i32;

/// The row of the fx file: what its `service!` call declares.
pub fn fx_row() -> &'static dkrt_abi::provider::ProviderRow {
    dkrt_svc_fx_dks::dk_svc_row()
}

/// The owner of the fx file in its cell, made as the file's `init` makes it before the declarations, except that no table of the effect pages is bound: a host test binds it
/// when it first needs it (`fxpage::info`), as the file's own code does. `E_LOADED` when the cell holds one.
pub fn fx_start() -> Result<*mut u8, i32> {
    dkrt_svc_fx_dks::DK_SVC_OWNER
        .start(|slot| {
            slot.write(dkrt_svc_fx::fx::Fx::new());
            Ok(())
        })
        .map(|instance| instance as *mut u8)
}

/// What the file's `fini` does: the owner is dropped (nothing when there is none).
pub fn fx_stop() {
    dkrt_svc_fx_dks::dk_svc_stop()
}

/// The builder call `dk_module_ctx_add_fx` of the file, for the module whose context is `ctx`: the export that a module's `init` calls.
///
/// # Safety
/// `ctx` is any pointer (the core only compares it).
pub unsafe fn fx_add_fx(ctx: *mut u8, index: u32) {
    dkrt_svc_fx_dks::builder::dk_module_ctx_add_fx(ctx, index, b"Test\0".as_ptr(), b"TST\0".as_ptr());
}

/// The row of the machines file: what its `service!` call declares.
pub fn machines_file_row() -> &'static dkrt_abi::provider::ProviderRow {
    dkrt_svc_machines_dks::dk_svc_row()
}

/// The owner of the machines file in its cell, made as the file's `init` makes it before the declarations, except that no image table is bound (the hooks do not run on a host).
/// The instance of a started owner is kept: a case that needs the file's owner and finds one uses it. `E_LOADED` never comes back.
pub fn machines_start() -> *mut u8 {
    let live = dkrt_svc_machines_dks::DK_SVC_OWNER.instance();
    if !live.is_null() {
        return live as *mut u8;
    }
    dkrt_svc_machines_dks::DK_SVC_OWNER
        .start(|slot| {
            slot.write(dkrt_svc_machines::machines::Machines::new());
            Ok(())
        })
        .expect("the owner of the machines file starts") as *mut u8
}

/// What the machines file's `fini` does: the owner is dropped (nothing when there is none).
pub fn machines_stop() {
    // `stop` builds the machine selection view's list again: a host has no view
    crate::package::cleanup_tests::without_rows_info(|| dkrt_svc_machines_dks::dk_svc_stop())
}

/// The row of the machines file as a loaded service file has it, over its owner (started when no one is).
pub fn machines_source() -> crate::package::provider_rows::RowSource<'static> {
    crate::package::provider_rows::RowSource { row: machines_file_row(), instance: machines_start(), extent: 0..usize::MAX }
}

/// The builder call `dk_module_ctx_add_machine` of the file, for the module whose context is `ctx`: the export that a module's `init` calls (the owner is started when no one is).
///
/// # Safety
/// `ctx` is any pointer (the core only compares it).
pub unsafe fn machines_add_machine(ctx: *mut u8, index: u32) {
    machines_start();
    dkrt_svc_machines_dks::builder::dk_module_ctx_add_machine(ctx, index, b"Test\0".as_ptr(), b"TST\0".as_ptr());
}

/// The Rust service files: (file name, declarations).
pub fn declarations() -> [(&'static str, DeclareFn); 6] {
    [
        ("machines", dkrt_svc_machines_dks::dk_svc_declare),
        ("fx", dkrt_svc_fx_dks::dk_svc_declare),
        ("lfo", dkrt_svc_lfo_dks::dk_svc_declare),
        ("samples", dkrt_svc_samples_dks::dk_svc_declare),
        ("shortcuts", dkrt_svc_shortcuts_dks::dk_svc_declare),
        ("project", dkrt_svc_project_dks::dk_svc_declare),
    ]
}
