//! The ignored fixture gates, callable from any crate that has the private
//! DN2 fixtures (`DIGI_COUPLED_FIXTURES`, `DIGI_COUPLED_SYX`). The desktop
//! crate wraps them in `#[test]`s so their names and the hashes they assert
//! stay where tooling looks for them.
use crate::audio_out::{AudioOut, pace_seconds};
use crate::companion::Companion;
use crate::sharc_companion::SharcCompanion;
use crate::{AudioOptions, CoupledRuntime, STEP_CHUNK};
use elektron_native_boot::Emulator;
use sharc_peer::ThreadedTimingProfile;
use std::path::PathBuf;
use std::time::Instant;

fn sharc(runtime: &CoupledRuntime) -> &SharcCompanion {
    runtime.sharc().expect("SHARC companion")
}
fn captured(runtime: &CoupledRuntime) -> &[f32] {
    runtime
        .out
        .tap()
        .and_then(|tap| tap.kept.as_deref())
        .expect("observe_pcm(true)")
}
pub fn options() -> AudioOptions {
    let dir = PathBuf::from(std::env::var_os("DIGI_COUPLED_FIXTURES").expect("fixture directory"));
    AudioOptions {
        image: dir.join("digi-audio-dn2-image.bin"),
        state: dir.join("digi-audio-m5.snap.dsp"),
        snapshot: dir.join("digi-audio-m5.snap"),
        buffer_seconds: 0.0,
        playback: false,
        automatic: false,
        expected_syx: None,
        link_timing: false,
        audio_mode: crate::AudioMode::default(),
        dsp_patch: None,
        zero: false,
    }
}
pub fn syx() -> Vec<u8> {
    std::fs::read(std::env::var_os("DIGI_COUPLED_SYX").expect("DN2 SYX")).unwrap()
}
fn timing_delta(
    after: ThreadedTimingProfile,
    before: ThreadedTimingProfile,
) -> ThreadedTimingProfile {
    ThreadedTimingProfile {
        frames_sent: after.frames_sent.saturating_sub(before.frames_sent),
        frames_completed: after
            .frames_completed
            .saturating_sub(before.frames_completed),
        host_enqueue_ns: after.host_enqueue_ns.saturating_sub(before.host_enqueue_ns),
        host_reply_wait_ns: after
            .host_reply_wait_ns
            .saturating_sub(before.host_reply_wait_ns),
        host_done_wait_ns: after
            .host_done_wait_ns
            .saturating_sub(before.host_done_wait_ns),
        host_collect_ns: after.host_collect_ns.saturating_sub(before.host_collect_ns),
        worker_queue_ns: after.worker_queue_ns.saturating_sub(before.worker_queue_ns),
        worker_spi_ns: after.worker_spi_ns.saturating_sub(before.worker_spi_ns),
        worker_dsp_ns: after.worker_dsp_ns.saturating_sub(before.worker_dsp_ns),
        worker_sport_ns: after.worker_sport_ns.saturating_sub(before.worker_sport_ns),
    }
}
/// Gate (private fixtures): private DN2 ready fixtures and locally generated DSP; bounded 100M-instruction gate.
pub fn coupled_ready_exactness() {
    let (digest, pcm_sha256) = coupled_ready_workload();
    // Checks the CF state digest (pinned below; it covers the DSPI RX buffer filled with DSP replies) and the PCM SHA-256 (from tests/scenarios/dn2.json).
    // re-pinned 2026-10-13: forced IRQ delivery on by default (Em approved)
    // re-pinned 2026-10-30: the DN2 1.12 ready fixture (67fee13b) changed the CF state (Em approved)
    assert_eq!(
        digest,
        "8bd3e0bf6b5fbf8720481efa752a6a5d2210d3f84094984d2d92a2cf37faa59b"
    );
    // The PCM expectation lives in tests/scenarios/dn2.json (scenario exactness-100m).
    let scenarios = crate::scenario::parse_scenarios(crate::scenario::DEFAULT_SCENARIOS).unwrap();
    let expected_pcm = scenarios
        .iter()
        .find(|sc| sc.name == "exactness-100m")
        .and_then(|sc| sc.expect_sha256.as_deref())
        .expect("scenario exactness-100m with expect_sha256");
    assert_eq!(pcm_sha256, expected_pcm);
}

/// The bounded 100M-instruction workload of the exactness gate, without the pin checks (PGO training runs it).
/// Returns the CF state digest and the PCM SHA-256, both as hex.
/// `DIGI_COUPLED_DSP_EXPORT` writes the DSP export to that path.
pub fn coupled_ready_workload() -> (String, String) {
    use sha2::{Digest, Sha256};
    let mut runtime = CoupledRuntime::new(&syx(), None, Some(&options())).unwrap();
    runtime.observe_pcm(true);
    let initial_snapshot = runtime.snapshot();
    assert!(
        initial_snapshot
            .frame
            .as_ref()
            .is_some_and(|frame| frame.iter().any(|byte| *byte != 0)),
        "ready fixture should restore a visible LCD frame"
    );
    let initial = initial_snapshot.status;
    assert!(initial.ready);
    // the pins hold for this workload only, whatever NOTE_EVENTS says (the PGO training sets trig too)
    let mut script = crate::common::Script::with_notes(Some("trig"));
    script.poll(&mut runtime.emulator, initial.icount, true);
    let timing_before = sharc(&runtime).handle.timing_profile();
    let desktop_timing_before = Some(sharc(&runtime)).map(|audio| {
        (
            audio.cf_step_calls,
            audio.cf_step_wall_ns,
            audio.pcm_handoff_ns,
            audio.audio_poll_wall_ns,
            audio.audio_sync_wall_ns,
        )
    });
    let workload_start = Instant::now();
    let mut ended = false;
    for _ in 0..1000 {
        let status = runtime.step_chunk(STEP_CHUNK).status;
        assert!(status.error.is_none(), "{:?}", status.error);
        script.poll(&mut runtime.emulator, status.icount, status.ready);
        if status.icount - initial.icount >= 100_000_000 {
            ended = true;
            break;
        }
    }
    assert!(ended, "bounded workload did not finish");
    runtime.snapshot();
    println!(
        "coupled_workload_elapsed_seconds={:.6}",
        workload_start.elapsed().as_secs_f64()
    );
    if let Some(before) = timing_before {
        let after = sharc(&runtime)
            .handle
            .timing_profile()
            .expect("enabled timing profile");
        let window = timing_delta(after, before);
        assert!(window.frames_completed > 0);
        assert!(window.frames_completed <= window.frames_sent);
        println!("native_audio_link_timing_window={window:?}");
        let after = sharc(&runtime);
        let (calls, cf_step, pcm_handoff, poll, sync) = desktop_timing_before.unwrap();
        println!(
            "native_audio_desktop_timing_window=cf_step_calls={} cf_step_wall_ns={} pcm_handoff_ns={} audio_poll_wall_ns={} audio_sync_wall_ns={}",
            after.cf_step_calls.saturating_sub(calls),
            after.cf_step_wall_ns.saturating_sub(cf_step),
            after.pcm_handoff_ns.saturating_sub(pcm_handoff),
            after.audio_poll_wall_ns.saturating_sub(poll),
            after.audio_sync_wall_ns.saturating_sub(sync),
        );
    }
    let digest = runtime.emulator.state_digest().unwrap();
    let audio = sharc(&runtime);
    let pcm: Vec<u8> = captured(&runtime)
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    if let Some(path) = std::env::var_os("DIGI_COUPLED_PCM_F32LE") {
        std::fs::write(path, &pcm).unwrap();
    }
    assert_eq!(captured(&runtime).len(), 72_896);
    let pcm_sha256 = format!("{:x}", Sha256::digest(&pcm));
    if let Some(path) = std::env::var_os("DIGI_COUPLED_DSP_EXPORT") {
        std::fs::write(path, audio.handle.export().0).unwrap();
    }
    assert_eq!(audio.report(&runtime.emulator)["missing_blocks"], 0);
    assert!(audio.shared.borrow().pcm.is_empty());
    assert!(audio.shared.borrow().raw.is_empty());
    assert!(audio.shared.borrow().frames.is_empty());
    let report = runtime.diagnostics();
    assert_eq!(report["sharc_execution_connected"], true);
    assert_eq!(report["pcm_output_connected"], false);
    println!("native_audio_health={}", report["native_audio"]);
    (digest, pcm_sha256)
}

/// Gate (private fixtures): private DN2 ready fixtures; captures the desktop Audition Trig 1 path without the shared QA script.
pub fn desktop_trig1_capture_without_qa_script() {
    use sha2::{Digest, Sha256};

    let mut runtime = CoupledRuntime::new(&syx(), None, Some(&options())).unwrap();
    runtime.observe_pcm(true);
    let initial = runtime.snapshot().status;
    assert!(initial.ready);

    let press_after = std::env::var("DIGI_DESKTOP_TRIG1_PRESS_AFTER")
        .ok()
        .map(|value| value.parse::<u64>().expect("valid press delay"))
        .unwrap_or(0);
    let hold = std::env::var("DIGI_DESKTOP_TRIG1_HOLD")
        .ok()
        .map(|value| value.parse::<u64>().expect("valid hold duration"))
        .unwrap_or(40_000_000);
    let require_nonzero = match std::env::var("DIGI_DESKTOP_TRIG1_REQUIRE_NONZERO") {
        Ok(value) if value == "1" => true,
        Ok(value) if value == "0" => false,
        Ok(value) => {
            panic!("DIGI_DESKTOP_TRIG1_REQUIRE_NONZERO must be 0 or 1, got {value}")
        }
        Err(std::env::VarError::NotPresent) => false,
        Err(error) => panic!("invalid DIGI_DESKTOP_TRIG1_REQUIRE_NONZERO: {error}"),
    };
    let press_at = initial.icount + press_after;
    let mut status = initial;
    for _ in 0..10_000 {
        if status.icount >= press_at {
            break;
        }
        status = runtime
            .step_chunk((press_at - status.icount).min(STEP_CHUNK as u64) as u32)
            .status;
        assert!(status.error.is_none(), "{:?}", status.error);
    }
    assert!(
        status.icount >= press_at,
        "did not reach Trig 1 press boundary"
    );
    runtime.snapshot();
    let hold_timing_before = sharc(&runtime).handle.timing_profile();
    let held_pcm_start = captured(&runtime).len();
    let hold_window_start = Instant::now();
    runtime.button(25, true).unwrap();
    let held_from = runtime.snapshot().status.icount;
    for _ in 0..1000 {
        if status.icount.saturating_sub(held_from) >= hold {
            break;
        }
        status = runtime
            .step_chunk(
                (hold - status.icount.saturating_sub(held_from)).min(STEP_CHUNK as u64) as u32,
            )
            .status;
        assert!(status.error.is_none(), "{:?}", status.error);
    }
    assert!(
        status.icount.saturating_sub(held_from) >= hold,
        "did not hold Trig 1 for {hold} instructions"
    );
    runtime.button(25, false).unwrap();
    let final_snapshot = runtime.snapshot();
    let hold_window_elapsed = hold_window_start.elapsed();
    let state_digest = runtime.emulator.state_digest().unwrap();
    let (dsp_digest, dsp_icount) = {
        let audio = sharc(&runtime);
        let (dsp_state, dsp_icount) = audio.handle.export();
        (format!("{:x}", Sha256::digest(dsp_state)), dsp_icount)
    };
    println!(
        "desktop_trig1_state icount={} state_sha256={} dsp_icount={} dsp_sha256={}",
        final_snapshot.status.icount, state_digest, dsp_icount, dsp_digest
    );

    let audio = sharc(&runtime);
    let hold_timing = hold_timing_before
        .zip(audio.handle.timing_profile())
        .map(|(before, after)| timing_delta(after, before));
    let held_pcm = &captured(&runtime)[held_pcm_start..];
    let held_nonzero = held_pcm.iter().any(|sample| *sample != 0.0);
    println!(
        "desktop_trig1_hold_window_elapsed_seconds={:.6} held_samples={} native_audio_link_timing_window={hold_timing:?}",
        hold_window_elapsed.as_secs_f64(),
        held_pcm.len(),
    );
    assert!(!captured(&runtime).is_empty(), "Trig 1 produced no PCM");
    assert!(captured(&runtime).iter().all(|sample| sample.is_finite()));
    let pcm: Vec<u8> = captured(&runtime)
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let digest = format!("{:x}", Sha256::digest(&pcm));
    println!(
        "desktop_trig1_pcm press_after={press_after} held={} samples={} sha256={digest}",
        status.icount.saturating_sub(held_from),
        captured(&runtime).len(),
    );
    println!("desktop_trig1_held_nonzero={held_nonzero}");
    if require_nonzero {
        assert!(held_nonzero, "held Trig 1 produced only silent PCM");
    }
    if let Some(path) = std::env::var_os("DIGI_DESKTOP_TRIG1_PCM_F32LE") {
        std::fs::write(path, pcm).unwrap();
    }
}

/// Gate (private fixtures): private DN2 fixtures; missing-device path must leave coupling intact.
pub fn missing_device_keeps_coupled_runtime_and_records_error() {
    let mut emulator = Emulator::new(&syx(), None).unwrap();
    let mut opts = options();
    opts.playback = true;
    let mut audio = SharcCompanion::new(&mut emulator, &opts).unwrap();
    let mut out = AudioOut::new(
        true,
        pace_seconds(opts.buffer_seconds),
        || Err("fake missing device".into()),
        || None,
    );
    emulator.step_chunk(STEP_CHUNK);
    audio.sync();
    audio.poll(&mut emulator, &mut out);
    assert!(audio.frames > 0);
    let report = out.report_fields();
    assert_eq!(report["device_error"], "fake missing device");
    assert_eq!(report["sink_connected"], false);
    assert!(audio.shared.borrow().halted.is_none());
}

/// Gate (private fixtures): private DN2 fixtures and an audible default output device.
pub fn default_output_receives_nonzero_emulated_pcm_and_tears_down() {
    let mut options = options();
    options.playback = true;
    {
        let mut runtime = CoupledRuntime::new(&syx(), None, Some(&options)).unwrap();
        let initial = runtime.snapshot().status;
        assert!(initial.ready);
        let mut script = crate::common::Script::from_env();
        script.poll(&mut runtime.emulator, initial.icount, true);
        for _ in 0..400 {
            let status = runtime.step_chunk(STEP_CHUNK).status;
            assert!(status.error.is_none(), "{:?}", status.error);
            script.poll(&mut runtime.emulator, status.icount, status.ready);
            if runtime
                .diagnostics()
                .get("native_audio")
                .and_then(|audio| audio.get("source_pcm_frames"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let report = runtime.diagnostics();
        let audio = &report["native_audio"];
        if audio["sink_connected"] != true {
            assert!(
                audio["device_error"].as_str().is_some(),
                "missing sink without a device error: {audio}"
            );
            eprintln!("default output unavailable: {}", audio["device_error"]);
            return;
        }
        assert_eq!(audio["sink_connected"], true, "{audio}");
        assert_eq!(audio["flow_started"], true, "{audio}");
        assert!(
            audio["source_pcm_frames"].as_u64().unwrap_or(0) > 0,
            "{audio}"
        );
        assert!(
            audio["sink"]["source_received_frames"]
                .as_u64()
                .unwrap_or(0)
                > 0,
            "{audio}"
        );
    }
}
