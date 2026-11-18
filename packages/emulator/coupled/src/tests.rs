//! `CoupledRuntime` over a fake companion: the runtime's calls into the companion and the output.
//! The runtime needs a firmware image; the tests skip when none is found (`DT2_SYX`, else `firmware/`).
use super::*;
use std::{any::Any, cell::RefCell, rc::Rc};

#[derive(Default)]
struct Log {
    calls: Vec<&'static str>,
    halted: Option<String>,
    state: Vec<u8>,
}

struct Fake {
    kind: Kind,
    log: Rc<RefCell<Log>>,
}

impl Companion for Fake {
    fn kind(&self) -> Kind {
        self.kind
    }
    fn step_begin(&mut self) {
        self.log.borrow_mut().calls.push("step_begin");
    }
    fn sync(&mut self) {
        self.log.borrow_mut().calls.push("sync");
    }
    fn poll(&mut self, _: &mut Emulator, out: &mut AudioOut) {
        self.log.borrow_mut().calls.push("poll");
        out.push(&[0.5, -0.5, 0.25, -0.25]);
    }
    fn halted(&self) -> Option<String> {
        self.log.borrow().halted.clone()
    }
    fn export_state(&mut self, _: &mut Emulator) -> Result<(Vec<u8>, u64), String> {
        Ok((self.log.borrow().state.clone(), 7))
    }
    fn report(&self, _: &Emulator) -> Value {
        json!({"fake": true})
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A ColdFire emulator on the stock DT2 image. Skips (one line on stderr) only when the file is missing,
/// as the boot tests do; an image that is there but does not load fails the test.
fn emulator() -> Option<Emulator> {
    let path = std::env::var_os("DT2_SYX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../firmware/Digitakt_II_OS1.17.syx")
        });
    let Ok(syx) = std::fs::read(&path) else {
        eprintln!(
            "SKIP unavailable firmware {} (set DT2_SYX or put the image in firmware/)",
            path.display()
        );
        return None;
    };
    Some(Emulator::new(&syx, None).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

fn runtime(kind: Kind) -> Option<(CoupledRuntime, Rc<RefCell<Log>>)> {
    let log = Rc::new(RefCell::new(Log::default()));
    let companion = Fake {
        kind,
        log: Rc::clone(&log),
    };
    let out = AudioOut::new(false, pace_seconds(0.0), || unreachable!(), || None);
    let runtime = CoupledRuntime::with_companion(emulator()?, Box::new(companion), out);
    Some((runtime, log))
}

#[test]
fn kind_names_are_the_report_strings() {
    assert_eq!(Kind::Sharc.as_str(), "sharc");
    assert_eq!(Kind::Zero.as_str(), "zero");
    assert_eq!(Kind::None.as_str(), "none");
    assert_eq!(Kind::Peer.as_str(), "peer");
    assert_eq!(Kind::Responder.as_str(), "responder");
    assert_eq!(NoCompanion.kind(), Kind::None);
    assert_eq!(ZeroCompanion.kind(), Kind::Zero);
    assert!(Kind::Sharc.makes_audio() && !Kind::Zero.makes_audio() && !Kind::None.makes_audio());
}

#[test]
fn the_inert_companions_have_no_state_report_or_halt() {
    let Some(mut emulator) = emulator() else {
        return;
    };
    for mut companion in [
        Box::new(NoCompanion) as Box<dyn Companion>,
        Box::new(ZeroCompanion),
    ] {
        assert!(companion.halted().is_none());
        assert!(companion.report(&emulator).is_null());
        assert!(companion.export_state(&mut emulator).is_err());
    }
}

#[test]
fn a_runtime_without_dsp_reports_no_audio() {
    let Some(emulator) = emulator() else {
        return;
    };
    let mut runtime = CoupledRuntime::with_emulator(emulator);
    assert_eq!(runtime.companion_kind(), Kind::None);
    assert!(!runtime.has_audio() && !runtime.is_zero());
    assert!(runtime.audio_report().is_none() && runtime.audio_status().is_none());
    assert!(runtime.dsp_export().is_none() && runtime.dsp_halted().is_none());
    assert!(runtime.capture_pcm(std::path::Path::new("unused")).is_err());
    runtime.enable_zero_dsp_timing().unwrap();
    assert!(runtime.is_zero() && !runtime.has_audio());
}

#[test]
fn step_chunk_marks_the_step_then_polls_into_the_runtime_output() {
    let Some((mut runtime, log)) = runtime(Kind::Sharc) else {
        return;
    };
    runtime.observe_pcm(true);
    runtime.step_chunk(1000);
    assert_eq!(log.borrow().calls, ["step_begin", "poll"]);
    let tap = runtime.pcm_tap().unwrap();
    assert_eq!(tap.count, 4);
    assert_eq!(tap.kept.as_deref(), Some(&[0.5, -0.5, 0.25, -0.25][..]));
}

#[test]
fn sync_audio_is_the_barrier_then_a_poll_and_snapshot_syncs() {
    let Some((mut runtime, log)) = runtime(Kind::Sharc) else {
        return;
    };
    runtime.sync_audio();
    assert_eq!(log.borrow().calls, ["sync", "poll"]);
    log.borrow_mut().calls.clear();
    runtime.snapshot();
    assert_eq!(log.borrow().calls, ["sync", "poll"]);
}

#[test]
fn a_companion_that_makes_no_audio_has_no_tap_and_no_report() {
    let Some((mut runtime, _)) = runtime(Kind::Zero) else {
        return;
    };
    runtime.observe_pcm(true);
    assert!(runtime.pcm_tap().is_none() && runtime.audio_report().is_none());
    assert!(runtime.is_zero());
}

#[test]
fn halt_and_exported_state_come_from_the_companion() {
    let Some((mut runtime, log)) = runtime(Kind::Sharc) else {
        return;
    };
    assert!(runtime.dsp_halted().is_none());
    log.borrow_mut().halted = Some("bad opcode".into());
    assert_eq!(runtime.dsp_halted().as_deref(), Some("bad opcode"));
    // An empty blob means no engine state.
    assert!(runtime.dsp_export().is_none());
    log.borrow_mut().state = vec![1, 2, 3];
    assert_eq!(runtime.dsp_export(), Some((vec![1, 2, 3], 7)));
}

#[test]
fn the_audio_report_merges_the_companion_part_with_the_output_part() {
    let Some((mut runtime, _)) = runtime(Kind::Sharc) else {
        return;
    };
    runtime.step_chunk(1000);
    let report = runtime.audio_report().unwrap();
    assert_eq!(report["fake"], true);
    assert_eq!(report["source_pcm_frames"], 2);
    assert_eq!(report["flow_started"], true);
    let diagnostics = runtime.diagnostics();
    assert_eq!(diagnostics["sharc_execution_connected"], true);
    assert_eq!(diagnostics["native_audio"]["fake"], true);
    assert_eq!(runtime.audio_status().unwrap()["fake"], true);
}

#[test]
fn a_device_without_a_second_cpu_gets_no_peer_companion() {
    let Some(emulator) = emulator() else {
        return;
    };
    assert!(peer_companion::for_emulator(&emulator, true).is_none());
    // the swap to the responder is for devices that have a peer CPU
    let mut runtime = CoupledRuntime::with_emulator(emulator);
    assert!(!runtime.has_peer_cpu());
    runtime.use_peer_responder();
    assert_eq!(runtime.companion_kind(), Kind::None);
}

#[test]
fn a_peer_companion_collects_nothing_without_pcm_and_reports_its_own_fields() {
    let Some(mut emulator) = emulator() else {
        return;
    };
    let mut out = AudioOut::new(false, pace_seconds(0.0), || unreachable!(), || None);
    let mut peer = PeerCompanion::new(true);
    peer.sync();
    peer.poll(&mut emulator, &mut out);
    assert_eq!(out.samples(), 0);
    assert!(peer.halted().is_none());
    assert!(peer.export_state(&mut emulator).is_err());
    let report = peer.report(&emulator);
    assert_eq!(report["peer"], "coldfire");
    assert_eq!(report["frames"], 0);
    assert!(
        report["halted"].is_null() && report["dsp_tiers"].is_null() && report["dsp_core"].is_null()
    );
}

#[test]
fn a_peer_has_audio_only_when_the_device_mixes_its_voices() {
    for (audio, kind) in [(true, Kind::Peer), (false, Kind::Peer)] {
        let Some(emulator) = emulator() else {
            return;
        };
        let runtime = CoupledRuntime::with_companion(
            emulator,
            Box::new(PeerCompanion::new(audio)),
            AudioOut::new(false, pace_seconds(0.0), || unreachable!(), || None),
        );
        assert_eq!(runtime.companion_kind(), kind);
        assert_eq!(runtime.has_audio(), audio);
        assert_eq!(runtime.audio_report().is_some(), audio);
    }
}

#[test]
fn a_peer_with_audio_is_not_a_sharc_session_in_the_diagnostics() {
    let Some(emulator) = emulator() else {
        return;
    };
    let mut runtime = CoupledRuntime::with_companion(
        emulator,
        Box::new(PeerCompanion::new(true)),
        AudioOut::new(false, pace_seconds(0.0), || unreachable!(), || None),
    );
    let diagnostics = runtime.diagnostics();
    assert_eq!(diagnostics["sharc_execution_connected"], false);
    assert_eq!(diagnostics["pcm_output_connected"], false);
    assert_eq!(diagnostics["native_audio"]["peer"], "coldfire");
}
