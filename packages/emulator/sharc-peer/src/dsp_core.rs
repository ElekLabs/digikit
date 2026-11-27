//! Which generated DSP core runs a firmware's DSP.
//!
//! A host holds no generated core. Each device's core is a library (`packages/emulator/sharc-core-lib`)
//! that `DIGI_SHARC_CORE_LIB_<DEV>` names (`DIGI_SHARC_CORE_LIB_DN2`, `DIGI_SHARC_CORE_LIB_DT2`; the launchers
//! take it from the store, `uv run python -m tools.devtools.verify_build cores`). The loader refuses a library
//! whose header differs from the host's (`sharc_native::core_lib::load`). Each core knows the DSP image it was
//! made for (`dn2-1.12`, `dt2-1.17`); it must be a core of the firmware's device. One engine runs either
//! core: the core is the value passed to it.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::dsp_patch::DspPatch;
use crate::{DspEngine, IdleLoop, Shared, ThreadedHandle, ThreadedPeer, open_engine_patched};
use sharc_native::GenCore;

/// The suffix of the per-core environment variables of a device (`SHARC_FAST_REGIONS_2`): the DN2 has none, the
/// DT2 has `_2`.
fn env_suffix(device: &str) -> &'static str {
    if device == "dt2" { "_2" } else { "" }
}

/// The name of the variable that names the core library of DEVICE (`dn2`, `dt2`).
pub fn core_lib_var(device: &str) -> String {
    format!("DIGI_SHARC_CORE_LIB_{}", device.to_ascii_uppercase())
}

/// The core for a device id (`dn2`, `dt2`) and the DSP image its firmware runs (`DspProfile::image`, `dn2-1.12`),
/// loaded from the library `DIGI_SHARC_CORE_LIB_<DEV>` names. Fails when the variable is missing or empty, when
/// the library is refused, and when the core is made for another device. A core of the same device but another
/// image version is accepted: the engine checks the loaded code against the core's `CODE_RANGES` and bars the
/// blocks whose code no longer matches, so it interprets what the core's blocks do not cover.
pub fn core_for_device(device: &str, dsp_image: &str) -> Result<&'static GenCore, String> {
    let var = core_lib_var(device);
    let Some(path) = std::env::var_os(&var).filter(|p| !p.is_empty()) else {
        return Err(format!(
            "{var} is not set: this build holds no DSP core; build the {device} core library with `uv run python -m tools.devtools.verify_build cores --device {device}` and set {var} to it"
        ));
    };
    let core = sharc_native::core_lib::load(std::path::Path::new(&path), env_suffix(device))
        .map_err(|e| format!("{var}: {e}; rebuild it with `uv run python -m tools.devtools.verify_build cores --device {device}`"))?;
    check_generated(&var, &path, core)?;
    check_image(&var, core.image, device, dsp_image)?;
    Ok(core)
}

/// A library built without `SHARC_GEN_DIR` (or without its image) holds the runtime fallbacks only: its engine traps at
/// the first DSP instruction. Refuse it when the session opens, with the cause.
fn check_generated(var: &str, path: &std::ffi::OsStr, core: &GenCore) -> Result<(), String> {
    if core.generated && core.has_image {
        return Ok(());
    }
    Err(format!(
        "{var}: {} was built without a generated core (SHARC_GEN_DIR unset or without image.rs): the DSP would trap at its first instruction; rebuild it with `uv run python -m tools.devtools.verify_build cores`",
        std::path::Path::new(path).display()
    ))
}

fn check_image(var: &str, core_image: &str, device: &str, dsp_image: &str) -> Result<(), String> {
    if core_image == dsp_image || core_image.starts_with(&format!("{device}-")) {
        return Ok(());
    }
    Err(format!(
        "{var}: the library is for DSP image {core_image}, {device} runs {dsp_image}"
    ))
}

/// Everything `open_engine_patched` needs; it runs on the worker thread.
pub struct EngineInputs {
    pub image: Vec<u8>,
    pub state: Vec<u8>,
    pub clock_base: u64,
    pub patch: Option<DspPatch>,
    pub idle: IdleLoop,
}

/// The DSP worker of `core`, with its DSPI2 peer.
pub fn spawn_dsp(
    core: &'static GenCore,
    inputs: EngineInputs,
    period: u32,
    link_timing: bool,
    fast_tier: bool,
) -> Result<(ThreadedPeer, ThreadedHandle, Rc<RefCell<Shared>>), String> {
    let EngineInputs {
        image,
        state,
        clock_base,
        patch,
        idle,
    } = inputs;
    if fast_tier {
        sharc_fast_cl::register();
    }
    ThreadedPeer::spawn_with_timing(
        move || {
            open_engine_patched(core, &image, &state, clock_base, patch.as_ref(), idle)
                .map(crate::NativeDsp)
        },
        period,
        idle.range,
        link_timing,
    )
}

/// Instruction budget of the cold DSP initialization, in place of a wall limit
/// (a slow host must not fail it). Normal init is much shorter (see
/// `examples/dsp_cold.rs`); the budget only ends a DSP that never reaches idle.
const COLD_INIT_LIMIT: u64 = 800_000_000;

/// Initialize a changed DSP before attaching the ColdFire. Exporting here keeps
/// the worker's restore path ordinary: its input has no patch left to apply.
pub fn initialize_cold_dsp(
    core: &'static GenCore,
    inputs: EngineInputs,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<EngineInputs, String> {
    validate_cold_state(core, &inputs.image, &inputs.state, inputs.idle)?;
    let mut engine: Box<dyn DspEngine> = Box::new(crate::NativeDsp(open_engine_patched(
        core,
        &inputs.image,
        &inputs.state,
        0,
        None,
        inputs.idle,
    )?));
    finish_cold_initialization(inputs, engine.as_mut(), cancel, progress)
}

fn validate_cold_state(
    core: &'static GenCore,
    image: &[u8],
    state: &[u8],
    idle: IdleLoop,
) -> Result<(), String> {
    // Validate before open_engine_patched concretises unknown memory or applies
    // the patch. A cold.state is canonical reset state, with no executed stores.
    let mut raw = sharc_native::Engine::from_image(image, core)
        .map_err(|e| format!("DSP image error {e}"))?;
    raw.import(state)
        .map_err(|e| format!("invalid raw DSP cold.state ({e}); regenerate ready inputs with `mise run emu:ready -- <device> --force`"))?;
    let s = &raw.s;
    if !s.mem.dirty_bytes().is_empty()
        || s.pending.is_some()
        || !s.loops.items().is_empty()
        || !s.call_stack.items().is_empty()
        || !s.pc_stack.items().is_empty()
        || !s.status_stack.items().is_empty()
        || (idle.range.0..=idle.range.1).contains(&(s.pc_sw as u32))
    {
        return Err("DSP cold.state is already initialized; regenerate ready inputs with `mise run emu:ready -- <device> --force`".into());
    }
    Ok(())
}

fn finish_cold_initialization(
    mut inputs: EngineInputs,
    engine: &mut dyn DspEngine,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<EngineInputs, String> {
    if let Some(patch) = inputs.patch.take() {
        engine.apply_dsp_patch(&patch)?;
    }
    run_cold_to_idle(engine, inputs.idle, cancel, COLD_INIT_LIMIT, progress)?;
    inputs.state = engine.export();
    inputs.clock_base = engine.icount();
    Ok(inputs)
}

fn run_cold_to_idle(
    engine: &mut dyn DspEngine,
    idle: IdleLoop,
    cancel: &AtomicBool,
    limit: u64,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    let mut done = 0u64;
    let mut rested = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if let Some(halt) = engine.halt_reason() {
            return Err(format!("DSP halted during initialization: {halt}"));
        }
        if done >= limit {
            return Err(format!(
                "DSP initialization did not reach idle after {done} instructions"
            ));
        }
        let n = engine.step((limit - done).min(250_000) as u32);
        done += u64::from(n);
        if let Some(halt) = engine.halt_reason() {
            return Err(format!("DSP halted during initialization: {halt}"));
        }
        if n == 0 {
            return Err("DSP initialization made no progress".into());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        progress(done);
        if (idle.range.0..=idle.range.1).contains(&engine.pc()) {
            rested += 1;
            if rested == 8 {
                return Ok(());
            }
        } else {
            rested = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Fake {
        pcs: VecDeque<u32>,
        pc: u32,
        steps: u64,
        halt: bool,
        stalled: bool,
        patches: usize,
        boot_value: u8,
    }
    impl Fake {
        fn new(pcs: impl IntoIterator<Item = u32>) -> Self {
            Self {
                pcs: pcs.into_iter().collect(),
                pc: 0,
                steps: 0,
                halt: false,
                stalled: false,
                patches: 0,
                boot_value: 0,
            }
        }
    }
    impl DspEngine for Fake {
        fn spi2_exchange(&mut self, _: &[u8]) -> Result<Vec<u8>, String> {
            unreachable!()
        }
        fn sport_block(&mut self) -> Result<Option<Vec<u8>>, String> {
            unreachable!()
        }
        fn step(&mut self, n: u32) -> u32 {
            if self.stalled {
                return 0;
            }
            self.pc = self.pcs.pop_front().unwrap_or(self.pc);
            self.steps += u64::from(n);
            self.boot_value = 42;
            n
        }
        fn pc(&self) -> u32 {
            self.pc
        }
        fn halt_reason(&self) -> Option<String> {
            self.halt.then(|| "test halt".into())
        }
        fn apply_dsp_patch(&mut self, _: &DspPatch) -> Result<(), String> {
            self.patches += 1;
            self.boot_value = 0;
            Ok(())
        }
        fn export(&self) -> Vec<u8> {
            vec![self.boot_value]
        }
        fn icount(&self) -> u64 {
            self.steps
        }
    }
    const IDLE: IdleLoop = IdleLoop {
        range: (10, 20),
        head: 10,
    };

    #[test]
    fn raw_state_validation_rejects_initialized_or_malformed_inputs() {
        let mut raw = sharc_native::Engine::from_image(&[], &sharc_gen::CORE).unwrap();
        raw.s.pc_sw = 42;
        validate_cold_state(&sharc_gen::CORE, &[], &raw.export(), IDLE).unwrap();
        raw.s.pc_sw = 10;
        assert!(
            validate_cold_state(&sharc_gen::CORE, &[], &raw.export(), IDLE)
                .unwrap_err()
                .contains("already initialized")
        );
        raw.s.pc_sw = 42;
        raw.poke_raw(0x2000_0000, &[1]);
        assert!(
            validate_cold_state(&sharc_gen::CORE, &[], &raw.export(), IDLE)
                .unwrap_err()
                .contains("already initialized")
        );
        assert!(
            validate_cold_state(&sharc_gen::CORE, &[], b"DT2DSP01", IDLE)
                .unwrap_err()
                .contains("regenerate ready inputs")
        );
    }

    #[test]
    fn a_core_of_another_device_is_refused() {
        check_image("V", "dn2-1.12", "dn2", "dn2-1.12").unwrap();
        check_image("V", "dt2-1.16", "dt2", "dt2-1.17").unwrap();
        let msg = check_image("V", "dt2-1.17", "dn2", "dn2-1.12").unwrap_err();
        assert!(
            msg.contains("dt2-1.17") && msg.contains("dn2-1.12"),
            "{msg}"
        );
        check_image("V", "none", "dn2", "dn2-1.12").unwrap_err();
    }

    #[test]
    fn a_library_without_a_generated_core_is_refused() {
        let msg = check_generated("V", "/x/core.dylib".as_ref(), &GenCore::NONE).unwrap_err();
        assert!(msg.contains("built without a generated core"), "{msg}");
        assert!(msg.contains("/x/core.dylib"), "{msg}");
    }

    #[test]
    fn idle_observations_must_be_consecutive() {
        let mut e = Fake::new([10, 10, 0, 10, 10, 10, 10, 10, 10, 10, 20]);
        run_cold_to_idle(&mut e, IDLE, &AtomicBool::new(false), 3_000_000, |_| {}).unwrap();
        assert_eq!(e.steps, 11 * 250_000);
    }

    #[test]
    fn failed_initialization_never_succeeds_at_idle() {
        let cancel = AtomicBool::new(false);
        let mut e = Fake::new([10]);
        e.halt = true;
        assert!(
            run_cold_to_idle(&mut e, IDLE, &cancel, 3_000_000, |_| {})
                .unwrap_err()
                .contains("halt")
        );
        e.halt = false;
        e.stalled = true;
        assert!(
            run_cold_to_idle(&mut e, IDLE, &cancel, 3_000_000, |_| {})
                .unwrap_err()
                .contains("no progress")
        );
        e.stalled = false;
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            run_cold_to_idle(&mut e, IDLE, &cancel, 3_000_000, |_| {}).unwrap_err(),
            "cancelled"
        );
        cancel.store(false, Ordering::Relaxed);
        assert!(
            run_cold_to_idle(&mut e, IDLE, &cancel, 250_000, |_| {})
                .unwrap_err()
                .contains("did not reach idle")
        );
    }

    #[test]
    fn patch_is_consumed_before_boot_and_initialized_data_survives_attachment() {
        let mut e = Fake::new([10]);
        let inputs = EngineInputs {
            image: vec![],
            state: vec![0],
            clock_base: 0,
            patch: Some(DspPatch::default()),
            idle: IDLE,
        };
        let ready =
            finish_cold_initialization(inputs, &mut e, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(e.patches, 1);
        assert_eq!(ready.state, vec![42]);
        assert_eq!(ready.clock_base, e.steps);
        assert!(
            ready.patch.is_none(),
            "worker must only import initialized state"
        );
    }
}
