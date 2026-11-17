//! The coupled ColdFire + SHARC+ run loop, shared by the desktop app
//! (`apps/desktop/src-tauri`) and the headless runner (`coupled-run`).
//!
//! One [`CoupledRuntime`] owns the ColdFire emulator and, when audio options are
//! given, the DSP worker thread behind the DSPI2 link, the PCM handling and the
//! opt-in link timing. A host drives it with [`CoupledRuntime::step_chunk`] in
//! [`STEP_CHUNK`]-instruction steps; the app does that from its webview pump and
//! the runner as fast as it can, so both execute exactly the same loop.
use elektron_native_boot::{Emulator, Snapshot};
use emmc_card::Card;
use serde_json::{Value, json};
#[cfg(feature = "coupled-audio")]
use std::time::Instant;
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
};

/// Largest number of ColdFire instructions a host runs per `step_chunk`.
pub const STEP_CHUNK: u32 = 250_000;

#[cfg(feature = "coupled-audio")]
pub mod audio_out;
#[cfg(feature = "coupled-audio")]
pub mod cold;
#[cfg(feature = "coupled-audio")]
pub mod dsp_core;
pub mod scenario;
pub mod span_gate;
/// Most inputs kept for replay.
const INPUT_LOG_LIMIT: usize = 100_000;
/// Sink events kept in the periodic UI status report.
#[cfg(feature = "coupled-audio")]
const SINK_EVENTS_STATUS_TAIL: usize = 200;

#[cfg(feature = "coupled-audio")]
pub use audio_out::{AudioOut, PcmSink, PcmTap};
#[cfg(feature = "coupled-audio")]
pub use coupled::gates;
#[cfg(feature = "coupled-audio")]
pub use elektron_native_boot::sharc_peer::{ExecutionProfile, WorkerReport};
#[cfg(feature = "coupled-audio")]
pub use sharc_native::{RegionCost, RegionProfile, TierCounters};

#[cfg(feature = "coupled-audio")]
#[allow(dead_code, clippy::print_stdout)]
#[path = "../../boot/examples/common/mod.rs"]
pub mod common;

/// What live playback does when the emulator falls behind real time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioMode {
    /// Stop with a short fade and resume once the buffer has refilled.
    #[default]
    Rebuffer,
    /// Slow playback to the rate audio arrives (lower pitch, no gaps).
    Stretch,
    /// Play missing frames as silence (the old behaviour; sounds like noise).
    Gap,
}

impl AudioMode {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "rebuffer" => Ok(Self::Rebuffer),
            "stretch" => Ok(Self::Stretch),
            "gap" => Ok(Self::Gap),
            other => Err(format!(
                "unknown audio mode {other:?} (rebuffer, stretch or gap)"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Rebuffer => "rebuffer",
            Self::Stretch => "stretch",
            Self::Gap => "gap",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AudioOptions {
    pub image: PathBuf,
    pub state: PathBuf,
    pub snapshot: PathBuf,
    pub buffer_seconds: f64,
    pub playback: bool,
    /// The launcher-provided ready session may fall back for files picked in
    /// the normal panel. Explicit --coupled remains strict.
    pub automatic: bool,
    pub expected_syx: Option<PathBuf>,
    /// Record per-period DSP wall times and the link stage totals (also on
    /// with `DN2_PROFILE_LINK=1`). A few timer reads per 0.667 ms period.
    pub link_timing: bool,
    /// What live playback does when the emulator falls behind real time.
    pub audio_mode: AudioMode,
    /// A DSP patch file (`elektron_native_boot::dsp_patch`) overlaid on the
    /// imported DSP state. Its BASE line, if any, must match the firmware's SHA-256.
    pub dsp_patch: Option<PathBuf>,
    /// The DSP side answers zeros: no SHARC engine, SSI paced, restored from the
    /// ColdFire ready snapshot (`snapshot`). No PCM is produced.
    pub zero: bool,
}

pub struct CoupledRuntime {
    emulator: Emulator,
    #[cfg(feature = "coupled-audio")]
    audio: Option<coupled::AudioSession>,
    inputs: Vec<Value>,
    input_base: u64,
    /// Zero-DSP mode (`AudioOptions::zero`): restored from the ColdFire
    /// snapshot, the DSP side answers zeros, no audio session.
    zero: bool,
}
fn matches_automatic_profile(options: &AudioOptions, syx: &[u8]) -> bool {
    !options.automatic
        || options
            .expected_syx
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .is_some_and(|expected| expected == syx)
}
impl Deref for CoupledRuntime {
    type Target = Emulator;
    fn deref(&self) -> &Emulator {
        &self.emulator
    }
}
impl DerefMut for CoupledRuntime {
    fn deref_mut(&mut self) -> &mut Emulator {
        &mut self.emulator
    }
}
impl CoupledRuntime {
    pub fn new(
        syx: &[u8],
        card: Option<Card>,
        options: Option<&AudioOptions>,
    ) -> Result<Self, String> {
        let options = options.filter(|options| matches_automatic_profile(options, syx));
        if let Some(path) = options.and_then(|o| o.dsp_patch.as_deref()) {
            use sha2::{Digest, Sha256};
            elektron_native_boot::dsp_patch::DspPatch::load(path)?
                .check_base(&format!("{:x}", Sha256::digest(syx)))?;
        }
        let mut runtime = Self {
            emulator: Emulator::new(syx, card)?,
            #[cfg(feature = "coupled-audio")]
            audio: None,
            inputs: Vec::new(),
            input_base: 0,
            zero: false,
        };
        runtime.attach(options)?;
        runtime.input_base = runtime.emulator.icount();
        Ok(runtime)
    }
    /// A runtime for a patched build of the firmware whose SHA-256 is
    /// `base_sha256` (`Emulator::new_patched`), without DSP audio: the
    /// ColdFire side boots cold from the patched MAIN.
    pub fn new_patched(syx: &[u8], card: Option<Card>, base_sha256: &str) -> Result<Self, String> {
        let mut runtime = Self {
            emulator: Emulator::new_patched(syx, card, base_sha256)?,
            #[cfg(feature = "coupled-audio")]
            audio: None,
            inputs: Vec::new(),
            input_base: 0,
            zero: false,
        };
        runtime.input_base = runtime.emulator.icount();
        Ok(runtime)
    }
    /// A runtime for a patched build of the firmware `base_sha256` with DSP
    /// audio: the ColdFire state comes from `options.snapshot` and the DSP
    /// state from `options.state`, both made for this MAIN by
    /// [`cold::boot_patched_to_ready`]. The `automatic` profile check is not
    /// applied (the options name this build's own files).
    pub fn new_patched_audio(
        syx: &[u8],
        card: Option<Card>,
        base_sha256: &str,
        options: &AudioOptions,
    ) -> Result<Self, String> {
        let mut runtime = Self {
            emulator: Emulator::new_patched(syx, card, base_sha256)?,
            #[cfg(feature = "coupled-audio")]
            audio: None,
            inputs: Vec::new(),
            input_base: 0,
            zero: false,
        };
        runtime.attach(Some(options))?;
        runtime.input_base = runtime.emulator.icount();
        Ok(runtime)
    }
    /// Load a ColdFire snapshot into this fresh runtime and restart the
    /// scenario clock there (`elapsed_instructions` counts from the restored
    /// state, as after a coupled restore).
    pub fn restore_state(&mut self, data: &[u8]) -> Result<(), String> {
        self.emulator.load_state(data)?;
        self.input_base = self.emulator.icount();
        Ok(())
    }
    /// After `restore_state`: start the write watch when the restored state is past its trigger.
    pub fn resume_watch(&mut self) {
        self.emulator.resume_watch();
    }
    /// Restart the scenario clock here (a cold boot that reached ready).
    pub fn rebase_clock(&mut self) {
        self.input_base = self.emulator.icount();
    }
    /// Wait for the DSP frames in flight and commit them (heavier than
    /// `audio_status`; a no-op without audio).
    pub fn sync_audio(&mut self) {
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &mut self.audio {
            audio.sync();
        }
    }
    /// Whether a DSP audio session is attached.
    pub fn has_audio(&self) -> bool {
        #[cfg(feature = "coupled-audio")]
        {
            self.audio.is_some()
        }
        #[cfg(not(feature = "coupled-audio"))]
        {
            false
        }
    }

    /// Whether this runtime is in zero-DSP mode (`AudioOptions::zero`).
    pub fn is_zero(&self) -> bool {
        self.zero
    }

    /// Pace the existing SSI/DMA zero peer without a SHARC engine. Call before guest execution
    /// or snapshot restore; unlike `attach`, this does not load a stock firmware snapshot.
    pub fn enable_zero_dsp_timing(&mut self) -> Result<(), String> {
        self.emulator.enable_ssi_diagnostic(96_000)?;
        self.zero = true;
        Ok(())
    }

    /// True when the queued output audio has reached the pace target; the
    /// controller's free running waits then. False without playback.
    pub fn audio_ahead(&self) -> bool {
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &self.audio {
            return audio.queue_ahead();
        }
        false
    }
    /// Overlay a DSP patch onto the running DSP between two frames and drop
    /// every cached decode and generated block that touched the patched code
    /// (the engine re-verifies code identity on its own). Returns the frame
    /// count at which it took effect.
    pub fn inject_dsp_patch(
        &mut self,
        patch: elektron_native_boot::dsp_patch::DspPatch,
    ) -> Result<Value, String> {
        #[cfg(feature = "coupled-audio")]
        {
            let audio = self.audio.as_mut().ok_or("no DSP audio session")?;
            let ops = patch.ops.len();
            let frame = audio.handle.frames_sent();
            let dsp_icount = audio.handle.apply_patch(patch)?;
            Ok(json!({"ops": ops, "at_frame": frame, "dsp_icount": dsp_icount}))
        }
        #[cfg(not(feature = "coupled-audio"))]
        {
            let _ = patch;
            Err("no DSP audio session".into())
        }
    }
    /// DSP memory bytes (`None` where the engine does not know the byte), read
    /// between two frames.
    pub fn peek_dsp(&mut self, addr: u32, len: usize) -> Result<Vec<Option<u8>>, String> {
        #[cfg(feature = "coupled-audio")]
        {
            let audio = self.audio.as_mut().ok_or("no DSP audio session")?;
            audio.sync();
            audio.handle.peek_bytes(addr, len)
        }
        #[cfg(not(feature = "coupled-audio"))]
        {
            let _ = (addr, len);
            Err("no DSP audio session".into())
        }
    }
    /// Diagnose the session's active DSP, fenced after outstanding audio work.
    pub fn dsp_ddr_monitor(
        &mut self,
        command: elektron_native_boot::ddr_monitor::DdrMonitorCommand,
    ) -> Result<elektron_native_boot::ddr_monitor::DdrMonitorStatus, String> {
        #[cfg(feature = "coupled-audio")]
        {
            let audio = self.audio.as_mut().ok_or("no DSP audio session")?;
            audio.sync();
            audio.handle.ddr_monitor(command)
        }
        #[cfg(not(feature = "coupled-audio"))]
        {
            let _ = command;
            Err("no DSP audio session".into())
        }
    }

    /// Read one exact DSP word address, including unaligned compact MMRs.
    pub fn peek_dsp_word(&mut self, addr: u32) -> Result<Option<u32>, String> {
        #[cfg(feature = "coupled-audio")]
        {
            let audio = self.audio.as_mut().ok_or("no DSP audio session")?;
            audio.sync();
            audio.handle.peek_word(addr)
        }
        #[cfg(not(feature = "coupled-audio"))]
        {
            let _ = addr;
            Err("no DSP audio session".into())
        }
    }
    fn attach(&mut self, options: Option<&AudioOptions>) -> Result<(), String> {
        let Some(options) = options else {
            return Ok(());
        };
        if options.zero {
            // The default DSPI2 peer answers zeros; no audio session.
            let snapshot =
                std::fs::read(&options.snapshot).map_err(|e| format!("CF snapshot: {e}"))?;
            self.enable_zero_dsp_timing()?;
            self.emulator.load_state(&snapshot)?;
            self.input_base = self.emulator.icount();
            return Ok(());
        }
        #[cfg(feature = "coupled-audio")]
        {
            self.audio = Some(coupled::AudioSession::new(&mut self.emulator, options)?);
            self.input_base = self.emulator.icount();
            Ok(())
        }
        #[cfg(not(feature = "coupled-audio"))]
        {
            let _ = options;
            Err("--coupled requires the coupled-audio build feature".into())
        }
    }
    /// The exact ColdFire instruction mix since the last reset (feature `cf-mix`).
    #[cfg(feature = "cf-mix")]
    pub fn cf_mix(&self) -> Value {
        self.emulator.cf_mix()
    }
    #[cfg(feature = "cf-mix")]
    pub fn cf_mix_reset(&self) {
        self.emulator.cf_mix_reset();
    }
    pub fn step_chunk(&mut self, budget: u32) -> Snapshot {
        #[cfg(feature = "coupled-audio")]
        let cf_step_start = self
            .audio
            .as_ref()
            .is_some_and(coupled::AudioSession::link_timing_enabled)
            .then(Instant::now);
        #[cfg(feature = "coupled-audio")]
        let spike_start = self
            .audio
            .as_mut()
            .and_then(coupled::AudioSession::spike_step_start);
        let snapshot = self.emulator.step_chunk(budget);
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &mut self.audio {
            if let Some(start) = cf_step_start {
                audio.record_cf_step(start.elapsed().as_nanos());
            }
            audio.poll();
            if let Some(start) = spike_start {
                audio.spike_step_end(start);
            }
        }
        snapshot
    }
    pub fn snapshot(&mut self) -> Snapshot {
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &mut self.audio {
            audio.sync();
        }
        self.emulator.snapshot()
    }

    /// The guest status and any new screen frame without waiting for the DSP
    /// frames in flight (for the webview's poll while the controller steps).
    pub fn peek(&mut self) -> Snapshot {
        self.emulator.snapshot()
    }
    pub fn diagnostics(&mut self) -> Value {
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &mut self.audio {
            audio.sync();
        }
        let mut report =
            serde_json::to_value(self.emulator.diagnostics()).expect("diagnostic serialization");
        report["recorded_scenario"] = self.recorded_scenario();
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &self.audio {
            let health = audio.report();
            report["sharc_execution_connected"] = true.into();
            report["pcm_output_connected"] = health["sink_connected"].clone();
            report["native_audio"] = health;
        }
        // Keep the same report shape for ordinary CF-only runs.
        #[cfg(not(feature = "coupled-audio"))]
        let _ = &mut report;
        report
    }
    /// Lightweight native-output observation for the periodic desktop UI poll.
    /// It deliberately does not synchronize the peer or serialize emulator diagnostics.
    pub fn audio_status(&mut self) -> Option<Value> {
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &mut self.audio {
            audio.poll();
            let mut report = audio.report();
            // The 1 s UI poll carries only the recent sink events; the full
            // list stays in `diagnostics()` and `audio_report()`.
            if let Some(events) = report["sink"]["events"].as_array() {
                let total = events.len();
                let tail = events[total.saturating_sub(SINK_EVENTS_STATUS_TAIL)..].to_vec();
                report["sink"]["events"] = Value::Array(tail);
                report["sink"]["events_total"] = total.into();
            }
            return Some(report);
        }
        None
    }
    /// Press or release a panel button, recording it for replay.
    pub fn button(&mut self, code: u8, down: bool) -> Result<(), String> {
        self.emulator.button(code, down)?;
        self.record_input(json!({"button": code, "down": down}));
        Ok(())
    }

    /// Turn an encoder, recording it for replay.
    pub fn turn(&mut self, encoder: u8, detents: i32) -> Result<(), String> {
        self.emulator.turn(encoder, detents)?;
        self.record_input(json!({"turn": encoder, "detents": detents}));
        Ok(())
    }

    /// Queue bytes on the MIDI DIN input, one byte per wire time (31,250 baud), recording them for replay
    /// (a scenario's `midi` event). `Emulator::midi_in` takes its own period and records nothing.
    pub fn midi_in_at_wire_rate(&mut self, bytes: &[u8]) -> Result<(), String> {
        let ips = elektron_native_boot::clock::cf_ips();
        let period =
            elektron_native_boot::clock::scale_instr(elektron_native_boot::midi::WIRE_PERIOD, ips);
        self.emulator.midi_in(bytes, period)?;
        self.record_input(json!({"midi": bytes}));
        Ok(())
    }

    /// ColdFire instructions since this runtime was ready (the clock
    /// scenario events use).
    pub fn elapsed_instructions(&self) -> u64 {
        self.emulator.icount().saturating_sub(self.input_base)
    }

    fn record_input(&mut self, mut event: Value) {
        if self.inputs.len() >= INPUT_LOG_LIMIT {
            return;
        }
        event["at_instr"] = elektron_native_boot::clock::unscale_instr(
            self.elapsed_instructions(),
            elektron_native_boot::clock::cf_ips(),
        )
        .into();
        #[cfg(feature = "coupled-audio")]
        if let Some(audio) = &self.audio {
            event["dsp_frame"] = audio.handle.frames_sent().into();
        }
        self.inputs.push(event);
    }

    /// The inputs so far as a scenario for `coupled-run --replay`. Events fire
    /// at their exact instruction counts, so the replay renders the same PCM.
    pub fn recorded_scenario(&self) -> Value {
        json!({
            "name": "recorded",
            "description": "Inputs recorded from an interactive session",
            "instructions": elektron_native_boot::clock::unscale_instr(
                self.elapsed_instructions(),
                elektron_native_boot::clock::cf_ips(),
            )
            .max(1),
            "exact_events": true,
            "events": self.inputs,
        })
    }
}

/// Observations for headless hosts; the desktop app does not use them.
#[cfg(feature = "coupled-audio")]
impl CoupledRuntime {
    /// Start accumulating PCM statistics and hash (and keep the samples when
    /// `keep`). Call right after construction to cover the whole run. Does
    /// nothing without coupled audio.
    pub fn observe_pcm(&mut self, keep: bool) {
        if let Some(audio) = &mut self.audio {
            audio.out.observe(keep);
        }
    }
    /// PCM seen since `observe_pcm`, up to the last `snapshot()`/`sync`.
    pub fn pcm_tap(&self) -> Option<&PcmTap> {
        self.audio.as_ref()?.out.tap()
    }
    /// Why the DSP stopped, once it has (set when a frame exchange fails).
    pub fn dsp_halted(&self) -> Option<String> {
        self.audio.as_ref().and_then(|a| a.halted())
    }
    /// Underrun events of the audio sink since the session began (`None`
    /// without a player: no device, no sink capture).
    pub fn sink_underruns(&self) -> Option<u64> {
        self.audio.as_ref().and_then(|a| a.sink_underruns())
    }
    /// DSPI2 frames handed to the DSP worker (one per 0.667 ms of audio).
    pub fn frames_sent(&self) -> u64 {
        self.audio.as_ref().map_or(0, |a| a.handle.frames_sent())
    }
    /// The DSP worker's thread CPU time, tier counters and per-period wall
    /// times (waits for the frames in flight).
    pub fn worker_report(&self) -> Option<WorkerReport> {
        self.audio.as_ref()?.handle.worker_report()
    }
    /// Enable or clear opt-in per-entry dispatch profiling in the DSP worker.
    pub fn set_region_profile(&self, enabled: bool) -> bool {
        self.audio
            .as_ref()
            .is_some_and(|audio| audio.handle.set_region_profile(enabled))
    }
    /// Start (or clear and stop) the DSP profile of `sharc.profile`: per-entry dispatch costs of the fast and block tiers (needs the
    /// engine's `region-profile` feature) and the interpreter's instruction counts per PC.
    pub fn set_sharc_profile(&self, enabled: bool) -> bool {
        self.audio.as_ref().is_some_and(|audio| {
            audio.handle.set_region_profile(enabled) && audio.handle.set_pc_hist(enabled)
        })
    }
    /// Enable the Engine's existing execution-frequency maps. The call waits
    /// for the worker acknowledgement so later probes are ordered after it.
    pub fn set_execution_profile(&self, enabled: bool) -> bool {
        self.audio
            .as_ref()
            .is_some_and(|audio| audio.handle.set_execution_profile(enabled))
    }
    /// Fetch execution maps only on an explicit post-run request. Ordinary
    /// worker reports intentionally do not clone the TSV map strings.
    pub fn execution_profile(&self) -> Option<ExecutionProfile> {
        self.audio.as_ref()?.handle.execution_profile()
    }
    /// The DSP's canonical state blob (`SHRD`, every overlay byte as a range)
    /// and its instruction count, after every frame sent so far.
    pub fn dsp_export(&self) -> Option<(Vec<u8>, u64)> {
        let (blob, icount) = self.audio.as_ref()?.handle.export();
        (!blob.is_empty()).then_some((blob, icount))
    }
    /// The ColdFire-side state digest of the `coupled-run` report (`state_sha256`): `Emulator::cf_state_digest`. It covers the
    /// `save_state` bytes (CPU, RAM, peripherals, timers, eSDHC and card, frame and input state) without the three
    /// instruction-path counters, and the state the snapshot lacks: flash image and flash write counts, MIDI DIN port, USB
    /// port and its queues, and the pending and active guest calls. It leaves out instrumentation (spans, probe hits,
    /// `pc_probe`, traces) and the SHARC side (see the DSP state sha). Fails when `save_state` refuses.
    pub fn cf_state_digest(&mut self) -> Result<String, String> {
        self.emulator.cf_state_digest()
    }
    /// The native audio health report (includes link timing when enabled).
    pub fn audio_report(&self) -> Option<Value> {
        self.audio.as_ref().map(coupled::AudioSession::report)
    }
    /// Stop the PCM capture (the file is complete after this returns).
    pub fn stop_pcm_capture(&mut self) {
        if let Some(audio) = self.audio.as_mut() {
            audio.stop_capture();
        }
    }
    /// Capture the emulator's own PCM (before the output buffer) to `path`:
    /// a float WAV when it ends in .wav, else raw f32le. No length limit.
    pub fn capture_pcm(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.audio
            .as_mut()
            .ok_or("no native audio session")?
            .capture_pcm(path)
    }
}

#[cfg(feature = "coupled-audio")]
mod coupled {
    use super::*;
    use crate::audio_out::{AudioOut, PcmCapture, PcmSink, pace_seconds, player_settings};
    use elektron_native_boot::{
        pcm_play::{Mode, PcmPlayer, Recovery, SOURCE_RATE},
        sharc_peer::{DEFAULT_PERIOD, Shared, ThreadedHandle, ThreadedTimingProfile},
    };
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc, time::Instant};

    fn continuation(data: &[u8]) -> Result<(u64, u64, &[u8]), String> {
        if data.len() <= 24 || &data[..8] != b"DT2DSP01" {
            return Err("coupled restore requires a complete DT2DSP01 DSP sidecar".into());
        }
        Ok((
            u64::from_le_bytes(data[8..16].try_into().unwrap()),
            u64::from_le_bytes(data[16..24].try_into().unwrap()),
            &data[24..],
        ))
    }
    pub(super) struct AudioSession {
        pub(super) handle: ThreadedHandle,
        pub(super) shared: Rc<RefCell<Shared>>,
        pub(super) out: AudioOut,
        /// The DSP image name of the generated core this session runs (`dn2-1.11`, `dt2-1.16`).
        dsp_core: &'static str,
        pub(super) frames: u64,
        initial_instructions: u64,
        link_timing: bool,
        pub(super) cf_step_calls: u64,
        pub(super) cf_step_wall_ns: u64,
        pub(super) pcm_handoff_ns: u64,
        pub(super) audio_poll_wall_ns: u64,
        pub(super) audio_sync_wall_ns: u64,
        /// When the tier counters were last printed (every 10 s, link timing only).
        tiers_printed: std::cell::Cell<Option<Instant>>,
        audio_mode: crate::AudioMode,
        spike: Option<elektron_native_boot::spikelog::CfWindow>,
    }
    impl AudioSession {
        fn new_with_player(
            emulator: &mut Emulator,
            options: &AudioOptions,
            player: impl FnOnce(Mode, Recovery) -> Result<PcmPlayer, String>,
        ) -> Result<Self, String> {
            let profile = emulator.diagnostics();
            let registry = device_profile::Registry::load().map_err(|e| e.to_string())?;
            let dsp = registry
                .dsp_for(&profile.device, &profile.version)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "desktop coupled audio has no DSP profile for {} OS {}",
                        profile.device, profile.version
                    )
                })?;
            let idle = elektron_native_boot::sharc_peer::IdleLoop {
                range: (dsp.idle_start, dsp.idle_end),
                head: dsp.idle_head,
            };
            if !options.buffer_seconds.is_finite()
                || !(0.0..=10.0).contains(&options.buffer_seconds)
            {
                return Err("--audio-buffer must be finite and between 0 and 10 seconds".into());
            }
            let image = std::fs::read(&options.image).map_err(|e| format!("DSP image: {e}"))?;
            let patch = options
                .dsp_patch
                .as_deref()
                .map(elektron_native_boot::dsp_patch::DspPatch::load)
                .transpose()?;
            let sidecar = std::fs::read(&options.state).map_err(|e| format!("DSP state: {e}"))?;
            let (clock, initial_instructions, state) = continuation(&sidecar)?;
            let state = state.to_vec();
            let snapshot =
                std::fs::read(&options.snapshot).map_err(|e| format!("CF snapshot: {e}"))?;
            // The spike log needs the per-period DSP times and the reply waits.
            let link_timing = options.link_timing
                || std::env::var("DN2_PROFILE_LINK").as_deref() == Ok("1")
                || std::env::var_os("SHARC_TIER_TIME").is_some_and(|v| v != "0")
                || elektron_native_boot::spikelog::enabled();
            emulator.enable_ssi_diagnostic(96_000)?;
            emulator.load_state(&snapshot)?;
            // The fast tier's Cranelift backend is registered by the spawn; the engine uses it only when
            // SHARC_FAST_REGIONS names regions (the launcher sets that). The core is the device's own.
            let core = crate::dsp_core::core_for_device(&profile.device);
            let (peer, handle, shared) = crate::dsp_core::spawn_dsp(
                core,
                crate::dsp_core::EngineInputs {
                    image,
                    state,
                    clock_base: clock,
                    patch,
                    idle,
                },
                elektron_native_boot::sharc_peer::period_from_env(),
                link_timing,
                true,
            )?;
            if dsp.sample_link == Some(device_profile::SampleLink::Lp0) {
                peer.enable_sample_link();
            }
            shared.borrow_mut().dsp_instructions = initial_instructions;
            emulator.set_dspi2_peer(peer.boxed());
            let (mode, recovery) = player_settings(options.buffer_seconds, options.audio_mode);
            let out = AudioOut::new(
                options.playback,
                pace_seconds(options.buffer_seconds),
                || player(mode, recovery).map(|p| Box::new(p) as Box<dyn PcmSink>),
                PcmCapture::from_env,
            );
            Ok(Self {
                handle,
                shared,
                out,
                dsp_core: core.image,
                frames: 0,
                initial_instructions,
                link_timing,
                cf_step_calls: 0,
                cf_step_wall_ns: 0,
                pcm_handoff_ns: 0,
                audio_poll_wall_ns: 0,
                audio_sync_wall_ns: 0,
                tiers_printed: std::cell::Cell::new(None),
                audio_mode: options.audio_mode,
                spike: elektron_native_boot::spikelog::CfWindow::open(),
            })
        }
        pub(super) fn new(emulator: &mut Emulator, options: &AudioOptions) -> Result<Self, String> {
            Self::new_with_player(emulator, options, PcmPlayer::spawn_with_recovery)
        }
        /// Close the PCM capture, if any, after the samples produced so far.
        pub(super) fn stop_capture(&mut self) {
            self.sync();
            self.out.stop_capture();
        }
        /// Replace the PCM capture with one for `path` (no length limit).
        pub(super) fn capture_pcm(&mut self, path: &std::path::Path) -> Result<(), String> {
            // The DSP worker runs ahead of the host by a number of frames that depends on host timing. Drain them first, so that the
            // capture starts at the frame the ColdFire has sent (the same stream position in every run), not at a varying one.
            self.sync();
            self.out.start_capture(path)
        }
        pub(super) fn link_timing_enabled(&self) -> bool {
            self.link_timing
        }
        pub(super) fn record_cf_step(&mut self, elapsed_ns: u128) {
            if self.link_timing {
                self.cf_step_calls = self.cf_step_calls.saturating_add(1);
                self.cf_step_wall_ns = self
                    .cf_step_wall_ns
                    .saturating_add(elapsed_ns.min(u64::MAX as u128) as u64);
            }
        }
        fn drain(&mut self) {
            let handoff_start = self.link_timing.then(Instant::now);
            let mut shared = self.shared.borrow_mut();
            self.frames += shared.frames.len() as u64;
            self.out.push(&shared.pcm);
            // Host observation histories are not DSP state. Keep production storage bounded
            // by one host step; retain the peer's cumulative counters and halt information.
            shared.pcm.clear();
            shared.raw.clear();
            shared.frames.clear();
            if let Some(start) = handoff_start {
                self.pcm_handoff_ns = self
                    .pcm_handoff_ns
                    .saturating_add(start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
            }
        }
        pub(super) fn poll(&mut self) {
            let wall_start = self.link_timing.then(Instant::now);
            self.handle.poll();
            self.drain();
            if let Some(start) = wall_start {
                self.audio_poll_wall_ns = self
                    .audio_poll_wall_ns
                    .saturating_add(start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
            }
        }

        /// Starts timing a `step_chunk` call for the spike log.
        pub(super) fn spike_step_start(&mut self) -> Option<Instant> {
            self.spike
                .as_mut()
                .map(elektron_native_boot::spikelog::CfWindow::step_start)
        }

        /// Ends a `step_chunk` call and writes the spike-log window when it is
        /// full.
        pub(super) fn spike_step_end(&mut self, start: Instant) {
            let Some(spike) = &mut self.spike else {
                return;
            };
            if !spike.step_end(start) {
                return;
            }
            let reply_wait_ns = self
                .handle
                .timing_profile()
                .map_or(0, |t| t.host_reply_wait_ns);
            let queue = self.out.queue_json();
            spike.flush(reply_wait_ns, self.handle.frames_sent(), queue);
        }

        /// True when the device ring plus the feeder's pending input holds
        /// at least `pace_seconds`. False without a player.
        pub(super) fn halted(&self) -> Option<String> {
            self.shared.borrow().halted.clone()
        }
        pub(super) fn sink_underruns(&self) -> Option<u64> {
            self.out.sink_underruns()
        }
        pub(super) fn queue_ahead(&self) -> bool {
            self.out.queue_ahead()
        }
        pub(super) fn sync(&mut self) {
            let wall_start = self.link_timing.then(Instant::now);
            self.handle.sync();
            self.drain();
            if let Some(start) = wall_start {
                self.audio_sync_wall_ns = self
                    .audio_sync_wall_ns
                    .saturating_add(start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
            }
        }
        pub(super) fn report(&self) -> Value {
            // The live report does not drain the pipeline, so it is cheap on every report.
            let dsp_tiers = self
                .handle
                .live_report()
                .and_then(|w| {
                    w.tiers.as_ref().map(|t| {
                        let worker_cpu_s = w.cpu_ns as f64 / 1e9;
                        let audio_s = w.periods as f64 / 1500.0;
                        json!({
                            "worker_cpu_s": worker_cpu_s,
                            "audio_s": audio_s,
                            "dsp_cpu_per_audio_s": if audio_s > 0.0 {
                                Value::from(worker_cpu_s / audio_s)
                            } else {
                                Value::Null
                            },
                            "instructions": t.icount, "idle_skip": t.idle_skipped,
                            "idle_skips": t.idle_skips, "interpreter": t.interpreted,
                            "block_instructions": t.block_instructions,
                            "fast_tier": t.fast_insns,
                            "aot_block": t.block_instructions.saturating_sub(t.fast_insns),
                            "block_entries": t.block_entries, "block_traps": t.block_traps,
                            "traps": t.traps, "fast_present": t.fast_present,
                            "fast_runs": t.fast_runs, "fast_calls": t.fast_calls,
                            "fast_built": t.fast_built, "fast_refused": t.fast_refused,
                            "fast_armed": t.fast_armed,
                            "code_gen": t.code_gen, "code_verifies": t.code_verifies,
                            "code_verify_ns": t.code_verify_ns, "code_mismatch": t.code_mismatch,
                            "interp_appended": t.interp_appended,
                            "tier_time": tier_time_json(t, w.wait_ticks, w.wall_ticks, audio_s),
                        })
                    })
                })
                .unwrap_or(Value::Null);
            // Diagnostic: print the DSP tier counters to stderr every 10 s.
            if self.link_timing
                && self
                    .tiers_printed
                    .get()
                    .is_none_or(|at| at.elapsed().as_secs_f64() >= 10.0)
            {
                let mut line = json!({"dsp_tiers": dsp_tiers.clone()});
                line["source_audio_seconds"] = self.out.audio_seconds().into();
                line["session_wall_seconds"] = self.out.wall_seconds().into();
                eprintln!("native_audio_report={line}");
                self.tiers_printed.set(Some(Instant::now()));
            }
            let shared = self.shared.borrow();
            let mut report = json!({
                "schema_version": 1, "peer": "threaded", "sample_rate": SOURCE_RATE,
                "period_instructions": DEFAULT_PERIOD, "ssi_period_instructions": 96_000,
                "frames": self.frames,
                "dsp_instructions_since_restore": shared.dsp_instructions.saturating_sub(self.initial_instructions),
                "missing_blocks": shared.missing_blocks, "nonzero_replies": shared.nonzero_replies,
                "halted": shared.halted,
                "dsp_core": self.dsp_core,
                "sink_stats_note": "async feeder snapshot; excludes PCM messages still in the channel",
                "audio_mode": self.audio_mode.name(),
                "dsp_tiers": dsp_tiers,
            });
            report
                .as_object_mut()
                .expect("object")
                .extend(self.out.report_fields());
            if self.link_timing {
                let timing = self.handle.timing_profile().expect("enabled link timing");
                report["link_timing"] = link_timing_json(
                    timing,
                    self.cf_step_calls,
                    self.cf_step_wall_ns,
                    self.pcm_handoff_ns,
                    self.audio_poll_wall_ns,
                    self.audio_sync_wall_ns,
                );
            }
            report
        }
    }

    /// `SHARC_TIER_TIME=1`: host seconds per audio second of the DSP worker thread, by tier
    /// (a tier's time includes the dispatch after it up to the next tier), its wait for the
    /// ColdFire side, and the rest (SPI/SPORT exchange, models, peripherals).
    fn tier_time_json(
        t: &sharc_native::TierCounters,
        wait_ticks: u64,
        wall_ticks: u64,
        audio_s: f64,
    ) -> Value {
        if !t.tt_on || audio_s <= 0.0 {
            return Value::Null;
        }
        let per = |ticks: u64| ticks as f64 / t.tt_hz.max(1) as f64 / audio_s;
        let engine: u64 = t.tt_ticks.iter().sum();
        let busy = wall_ticks.saturating_sub(wait_ticks);
        json!({
            "units": "host seconds per audio second",
            "interpreter": per(t.tt_ticks[0]), "aot_block": per(t.tt_ticks[1]),
            "fast_tier": per(t.tt_ticks[2]), "idle_skip": per(t.tt_ticks[3]),
            "step_entry": per(t.tt_ticks[4]),
            "engine_total": per(engine), "worker_busy": per(busy),
            "worker_other": per(busy.saturating_sub(engine)),
            "worker_wait": per(wait_ticks), "worker_wall": per(wall_ticks),
        })
    }

    fn link_timing_json(
        timing: ThreadedTimingProfile,
        cf_step_calls: u64,
        cf_step_wall_ns: u64,
        pcm_handoff_ns: u64,
        audio_poll_wall_ns: u64,
        audio_sync_wall_ns: u64,
    ) -> Value {
        json!({
            "units": "nanoseconds",
            "durations_overlap": true,
            "note": "Pipelined link and nested/enclosing wall durations are not additive.",
            "frames_sent": timing.frames_sent,
            "frames_completed": timing.frames_completed,
            "host_enqueue_ns": timing.host_enqueue_ns,
            "host_reply_wait_ns": timing.host_reply_wait_ns,
            "host_done_wait_ns": timing.host_done_wait_ns,
            "host_collect_ns": timing.host_collect_ns,
            "worker_queue_ns": timing.worker_queue_ns,
            "worker_spi_ns": timing.worker_spi_ns,
            "worker_dsp_ns": timing.worker_dsp_ns,
            "worker_sport_ns": timing.worker_sport_ns,
            "cf_step_calls": cf_step_calls,
            "cf_step_wall_ns": cf_step_wall_ns,
            "pcm_handoff_ns": pcm_handoff_ns,
            "audio_poll_wall_ns": audio_poll_wall_ns,
            "audio_sync_wall_ns": audio_sync_wall_ns,
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn continuation_requires_header_and_payload_and_keeps_both_counters() {
            for data in [vec![], vec![0; 24], b"DT2DSP01".to_vec()] {
                assert!(continuation(&data).is_err());
            }
            let mut data = b"DT2DSP01".to_vec();
            data.extend(123u64.to_le_bytes());
            data.extend(456u64.to_le_bytes());
            assert!(continuation(&data).is_err());
            data.extend([7, 8]);
            assert_eq!(continuation(&data).unwrap(), (123, 456, &[7, 8][..]));
        }
    }

    /// The ignored fixture gates, callable from any crate that has the private
    /// DN2 fixtures (`DIGI_COUPLED_FIXTURES`, `DIGI_COUPLED_SYX`). The desktop
    /// crate wraps them in `#[test]`s so their names and the hashes they assert
    /// stay where tooling looks for them.
    pub mod gates {
        use super::*;
        fn captured(audio: &AudioSession) -> &[f32] {
            audio
                .out
                .tap()
                .and_then(|tap| tap.kept.as_deref())
                .expect("observe_pcm(true)")
        }
        pub fn options() -> AudioOptions {
            let dir = PathBuf::from(
                std::env::var_os("DIGI_COUPLED_FIXTURES").expect("fixture directory"),
            );
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
            let scenarios =
                crate::scenario::parse_scenarios(crate::scenario::DEFAULT_SCENARIOS).unwrap();
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
            let timing_before = runtime.audio.as_ref().unwrap().handle.timing_profile();
            let desktop_timing_before = runtime.audio.as_ref().map(|audio| {
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
                let after = runtime
                    .audio
                    .as_ref()
                    .unwrap()
                    .handle
                    .timing_profile()
                    .expect("enabled timing profile");
                let window = timing_delta(after, before);
                assert!(window.frames_completed > 0);
                assert!(window.frames_completed <= window.frames_sent);
                println!("native_audio_link_timing_window={window:?}");
                let after = runtime.audio.as_ref().unwrap();
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
            let audio = runtime.audio.as_ref().unwrap();
            let pcm: Vec<u8> = captured(audio)
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            if let Some(path) = std::env::var_os("DIGI_COUPLED_PCM_F32LE") {
                std::fs::write(path, &pcm).unwrap();
            }
            assert_eq!(captured(audio).len(), 72_896);
            let pcm_sha256 = format!("{:x}", Sha256::digest(&pcm));
            if let Some(path) = std::env::var_os("DIGI_COUPLED_DSP_EXPORT") {
                std::fs::write(path, audio.handle.export().0).unwrap();
            }
            assert_eq!(audio.report()["missing_blocks"], 0);
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
            let hold_timing_before = runtime.audio.as_ref().unwrap().handle.timing_profile();
            let held_pcm_start = captured(runtime.audio.as_ref().unwrap()).len();
            let hold_window_start = Instant::now();
            runtime.button(25, true).unwrap();
            let held_from = runtime.snapshot().status.icount;
            for _ in 0..1000 {
                if status.icount.saturating_sub(held_from) >= hold {
                    break;
                }
                status = runtime
                    .step_chunk(
                        (hold - status.icount.saturating_sub(held_from)).min(STEP_CHUNK as u64)
                            as u32,
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
                let audio = runtime.audio.as_ref().unwrap();
                let (dsp_state, dsp_icount) = audio.handle.export();
                (format!("{:x}", Sha256::digest(dsp_state)), dsp_icount)
            };
            println!(
                "desktop_trig1_state icount={} state_sha256={} dsp_icount={} dsp_sha256={}",
                final_snapshot.status.icount, state_digest, dsp_icount, dsp_digest
            );

            let audio = runtime.audio.as_ref().unwrap();
            let hold_timing = hold_timing_before
                .zip(audio.handle.timing_profile())
                .map(|(before, after)| timing_delta(after, before));
            let held_pcm = &captured(audio)[held_pcm_start..];
            let held_nonzero = held_pcm.iter().any(|sample| *sample != 0.0);
            println!(
                "desktop_trig1_hold_window_elapsed_seconds={:.6} held_samples={} native_audio_link_timing_window={hold_timing:?}",
                hold_window_elapsed.as_secs_f64(),
                held_pcm.len(),
            );
            assert!(!captured(audio).is_empty(), "Trig 1 produced no PCM");
            assert!(captured(audio).iter().all(|sample| sample.is_finite()));
            let pcm: Vec<u8> = captured(audio)
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            let digest = format!("{:x}", Sha256::digest(&pcm));
            println!(
                "desktop_trig1_pcm press_after={press_after} held={} samples={} sha256={digest}",
                status.icount.saturating_sub(held_from),
                captured(audio).len(),
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
            let mut audio = AudioSession::new_with_player(&mut emulator, &opts, |_, _| {
                Err("fake missing device".into())
            })
            .unwrap();
            emulator.step_chunk(STEP_CHUNK);
            audio.sync();
            assert!(audio.frames > 0);
            assert_eq!(audio.report()["device_error"], "fake missing device");
            assert_eq!(audio.report()["sink_connected"], false);
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
    }
}
#[cfg(all(test, feature = "coupled-audio"))]
mod gate_tests {
    /// The ready-fixture exactness gate without the desktop crate (private
    /// fixtures: DIGI_COUPLED_FIXTURES, DIGI_COUPLED_SYX).
    #[test]
    #[ignore = "private DN2 ready fixtures and locally generated DSP; bounded 100M-instruction gate"]
    fn coupled_ready_exactness() {
        crate::gates::coupled_ready_exactness();
    }
}
#[cfg(test)]
mod profile_tests {
    use super::*;

    fn automatic(profile: PathBuf) -> AudioOptions {
        AudioOptions {
            image: PathBuf::new(),
            state: PathBuf::new(),
            snapshot: PathBuf::new(),
            buffer_seconds: 0.0,
            playback: false,
            automatic: true,
            expected_syx: Some(profile),
            link_timing: false,
            audio_mode: AudioMode::default(),
            dsp_patch: None,
            zero: false,
        }
    }

    /// Zero-DSP mode restores the ColdFire ready snapshot and steps without an
    /// audio session. Skipped without DIGI_COUPLED_FIXTURES and DIGI_COUPLED_SYX.
    #[test]
    fn zero_dsp_restores_and_steps() {
        let (Some(dir), Some(syx)) = (
            std::env::var_os("DIGI_COUPLED_FIXTURES"),
            std::env::var_os("DIGI_COUPLED_SYX"),
        ) else {
            return;
        };
        let snapshot = PathBuf::from(dir).join("digi-audio-m5.snap");
        let Ok(syx) = std::fs::read(syx) else {
            return;
        };
        if !snapshot.is_file() {
            return;
        }
        let options = AudioOptions {
            snapshot,
            zero: true,
            ..Default::default()
        };
        let mut runtime = CoupledRuntime::new(&syx, None, Some(&options)).unwrap();
        assert!(runtime.is_zero() && !runtime.has_audio());
        let before = runtime.icount();
        let snap = runtime.step_chunk(STEP_CHUNK);
        assert!(snap.status.error.is_none());
        assert!(snap.status.icount > before);
    }

    #[test]
    fn automatic_profile_accepts_identical_bytes_and_rejects_another_firmware() {
        let path = std::env::temp_dir().join(format!(
            "digi-auto-profile-{}-{}.syx",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(&path, [1, 2, 3, 4]).unwrap();
        let options = automatic(path.clone());
        assert!(matches_automatic_profile(&options, &[1, 2, 3, 4]));
        assert!(!matches_automatic_profile(&options, &[1, 2, 3, 5]));
        std::fs::remove_file(path).unwrap();
    }
}
