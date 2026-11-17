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
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
};

/// Largest number of ColdFire instructions a host runs per `step_chunk`.
pub const STEP_CHUNK: u32 = 250_000;

pub mod audio_out;
pub mod companion;
#[cfg(feature = "sharc")]
pub mod gates;
pub mod peer_companion;
pub mod scenario;
#[cfg(feature = "sharc")]
pub mod sharc_companion;
pub mod span_gate;
/// Most inputs kept for replay.
const INPUT_LOG_LIMIT: usize = 100_000;
/// Sink events kept in the periodic UI status report.
const SINK_EVENTS_STATUS_TAIL: usize = 200;

#[cfg(feature = "sharc")]
use audio_out::PcmCapture;
use audio_out::pace_seconds;
pub use audio_out::{AudioOut, PcmSink, PcmTap};
pub use companion::{Companion, Kind, NoCompanion, ResponderCompanion, ZeroCompanion};
pub use peer_companion::PeerCompanion;
#[cfg(feature = "sharc")]
pub use sharc_companion::SharcCompanion;
#[cfg(feature = "sharc")]
pub use sharc_native::{RegionCost, RegionProfile, TierCounters};
#[cfg(feature = "sharc")]
pub use sharc_peer::{ExecutionProfile, WorkerReport, cold, dsp_core};

#[cfg(feature = "sharc")]
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
    /// A DSP patch file (`sharc_peer::dsp_patch`) overlaid on the
    /// imported DSP state. Its BASE line, if any, must match the firmware's SHA-256.
    pub dsp_patch: Option<PathBuf>,
    /// The DSP side answers zeros: no SHARC engine, SSI paced, restored from the
    /// ColdFire ready snapshot (`snapshot`). No PCM is produced.
    pub zero: bool,
}

pub struct CoupledRuntime {
    emulator: Emulator,
    /// What answers the DSPI2 link: the SHARC+ engine, the zero answerer or nothing.
    companion: Box<dyn Companion>,
    /// The one audio output path: player, tap, capture and pacing.
    out: AudioOut,
    inputs: Vec<Value>,
    input_base: u64,
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
    fn with_emulator(emulator: Emulator) -> Self {
        Self::with_parts(emulator, Box::new(NoCompanion), silent_out())
    }
    fn with_parts(emulator: Emulator, companion: Box<dyn Companion>, out: AudioOut) -> Self {
        Self {
            emulator,
            companion,
            out,
            inputs: Vec::new(),
            input_base: 0,
        }
    }
    pub fn new(
        syx: &[u8],
        card: Option<Card>,
        options: Option<&AudioOptions>,
    ) -> Result<Self, String> {
        let options = options.filter(|options| matches_automatic_profile(options, syx));
        if let Some(path) = options.and_then(|o| o.dsp_patch.as_deref()) {
            use sha2::{Digest, Sha256};
            sharc_peer::dsp_patch::DspPatch::load(path)?
                .check_base(&format!("{:x}", Sha256::digest(syx)))?;
        }
        let mut runtime = Self::with_emulator(Emulator::new(syx, card)?);
        runtime.attach(options)?;
        runtime.attach_peer(true);
        runtime.input_base = runtime.emulator.icount();
        Ok(runtime)
    }
    /// A runtime for a patched build of the firmware whose SHA-256 is
    /// `base_sha256` (`Emulator::new_patched`), without DSP audio: the
    /// ColdFire side boots cold from the patched MAIN.
    pub fn new_patched(syx: &[u8], card: Option<Card>, base_sha256: &str) -> Result<Self, String> {
        let mut runtime = Self::with_emulator(Emulator::new_patched(syx, card, base_sha256)?);
        runtime.attach_peer(false);
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
        let mut runtime = Self::with_emulator(Emulator::new_patched(syx, card, base_sha256)?);
        runtime.attach_peer(false);
        runtime.attach(Some(options))?;
        runtime.input_base = runtime.emulator.icount();
        Ok(runtime)
    }
    /// A runtime around a given companion and output (tests give fakes).
    pub fn with_companion(
        emulator: Emulator,
        companion: Box<dyn Companion>,
        out: AudioOut,
    ) -> Self {
        let mut runtime = Self::with_parts(emulator, companion, out);
        runtime.input_base = runtime.emulator.icount();
        runtime
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
        // `sync` is the barrier; the poll after it pushes the PCM it completed.
        self.companion.sync();
        self.companion.poll(&mut self.emulator, &mut self.out);
    }
    /// Which companion answers the DSPI2 link.
    pub fn companion_kind(&self) -> Kind {
        self.companion.kind()
    }
    /// Whether the companion produces PCM (a SHARC+ session, or a peer CPU that mixes the device's voices).
    pub fn has_audio(&self) -> bool {
        self.companion.makes_audio()
    }

    /// Whether the device has a peer CPU that runs a program (see `Emulator::has_peer_cpu`).
    pub fn has_peer_cpu(&self) -> bool {
        self.emulator.has_peer_cpu()
    }

    /// Replaces the peer CPU by the mailbox responder: no peer audio. Call on a fresh runtime.
    pub fn use_peer_responder(&mut self) {
        if !self.emulator.has_peer_cpu() {
            return;
        }
        self.emulator.use_peer_responder();
        self.companion = Box::new(ResponderCompanion);
        self.out = silent_out();
    }

    /// Whether this runtime is in zero-DSP mode (`AudioOptions::zero`).
    pub fn is_zero(&self) -> bool {
        self.companion.kind() == Kind::Zero
    }

    /// Pace the existing SSI/DMA zero peer without a SHARC engine. Call before guest execution
    /// or snapshot restore; unlike `attach`, this does not load a stock firmware snapshot.
    pub fn enable_zero_dsp_timing(&mut self) -> Result<(), String> {
        if self.companion.kind() != Kind::None {
            return Err(format!(
                "zero-DSP timing needs a runtime without a DSP companion (this one has {})",
                self.companion.kind().as_str()
            ));
        }
        self.emulator.enable_ssi_diagnostic(96_000)?;
        self.companion = Box::new(ZeroCompanion);
        Ok(())
    }

    /// True when the queued output audio has reached the pace target; the
    /// controller's free running waits then. False without playback.
    pub fn audio_ahead(&self) -> bool {
        self.out.queue_ahead()
    }
    /// The SHARC+ companion, if that is what this runtime has.
    #[cfg(feature = "sharc")]
    pub(crate) fn sharc(&self) -> Option<&SharcCompanion> {
        self.companion.as_any().downcast_ref()
    }
    #[cfg(feature = "sharc")]
    fn sharc_mut(&mut self) -> Result<&mut SharcCompanion, String> {
        self.companion
            .as_any_mut()
            .downcast_mut()
            .ok_or_else(|| "no DSP audio session".to_string())
    }
    /// The companion of a device whose second processor is a ColdFire (`[emulator] peer`): the peer CPU, or the mailbox
    /// responder when the device profile names it. Other devices keep the companion they have.
    /// `audio` false (patched builds): the peer runs without audio output.
    fn attach_peer(&mut self, audio: bool) {
        if let Some((companion, out)) = peer_companion::for_emulator(&self.emulator, audio) {
            self.companion = companion;
            self.out = out;
        }
    }
    /// Choose the companion for `options`: zero answers zeros, otherwise the SHARC+ the device profile names.
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
        #[cfg(feature = "sharc")]
        {
            let companion = SharcCompanion::new(&mut self.emulator, options)?;
            self.out = audio_out_for(options);
            self.companion = Box::new(companion);
            self.input_base = self.emulator.icount();
            Ok(())
        }
        #[cfg(not(feature = "sharc"))]
        {
            let _ = options;
            Err("--coupled requires the sharc build feature".into())
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
        self.companion.step_begin();
        let snapshot = self.emulator.step_chunk(budget);
        self.companion.poll(&mut self.emulator, &mut self.out);
        snapshot
    }
    pub fn snapshot(&mut self) -> Snapshot {
        self.sync_audio();
        self.emulator.snapshot()
    }

    /// The guest status and any new screen frame without waiting for the DSP
    /// frames in flight (for the webview's poll while the controller steps).
    pub fn peek(&mut self) -> Snapshot {
        self.emulator.snapshot()
    }
    pub fn diagnostics(&mut self) -> Value {
        self.sync_audio();
        let mut report =
            serde_json::to_value(self.emulator.diagnostics()).expect("diagnostic serialization");
        report["recorded_scenario"] = self.recorded_scenario();
        if let Some(health) = self.audio_health() {
            if self.companion.kind() == Kind::Sharc {
                report["sharc_execution_connected"] = true.into();
            }
            report["pcm_output_connected"] = health["sink_connected"].clone();
            report["native_audio"] = health;
        }
        report
    }
    /// The audio health report: the companion's part with the output's, `None` without audio.
    fn audio_health(&self) -> Option<Value> {
        if !self.has_audio() {
            return None;
        }
        let mut report = self.companion.report(&self.emulator);
        if let Some(fields) = report.as_object_mut() {
            fields.extend(self.out.report_fields());
        }
        self.companion.log_report(&report);
        Some(report)
    }
    /// Lightweight native-output observation for the periodic desktop UI poll.
    /// It deliberately does not synchronize the peer or serialize emulator diagnostics.
    pub fn audio_status(&mut self) -> Option<Value> {
        if !self.has_audio() {
            return None;
        }
        self.companion.poll(&mut self.emulator, &mut self.out);
        let mut report = self.audio_health()?;
        // The 1 s UI poll carries only the recent sink events; the full
        // list stays in `diagnostics()` and `audio_report()`.
        if let Some(events) = report["sink"]["events"].as_array() {
            let total = events.len();
            let tail = events[total.saturating_sub(SINK_EVENTS_STATUS_TAIL)..].to_vec();
            report["sink"]["events"] = Value::Array(tail);
            report["sink"]["events_total"] = total.into();
        }
        Some(report)
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
        let ips = self.cf_ips();
        let period =
            elektron_native_boot::clock::scale_instr(elektron_native_boot::midi::WIRE_PERIOD, ips);
        self.emulator.midi_in(bytes, period)?;
        self.record_input(json!({"midi": bytes}));
        Ok(())
    }

    /// Guest instructions per emulated second of this runtime's emulator.
    pub fn cf_ips(&self) -> u64 {
        self.emulator.cf_ips()
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
        event["at_instr"] =
            elektron_native_boot::clock::unscale_instr(self.elapsed_instructions(), self.cf_ips())
                .into();
        #[cfg(feature = "sharc")]
        if let Some(sharc) = self.sharc() {
            event["dsp_frame"] = sharc.handle.frames_sent().into();
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
                self.cf_ips(),
            )
            .max(1),
            "exact_events": true,
            "events": self.inputs,
        })
    }
}

/// The output of a runtime without PCM: no player, no capture.
fn silent_out() -> AudioOut {
    AudioOut::new(false, pace_seconds(0.0), || unreachable!(), || None)
}

/// The output for a session of `options`: the player (feature `play`), the pace target and the capture
/// the environment names.
#[cfg(feature = "sharc")]
fn audio_out_for(options: &AudioOptions) -> AudioOut {
    #[cfg(feature = "play")]
    let open = || -> Result<Box<dyn PcmSink>, String> {
        let (mode, recovery) =
            audio_out::player_settings(options.buffer_seconds, options.audio_mode);
        elektron_native_boot::pcm_play::PcmPlayer::spawn_with_recovery(mode, recovery)
            .map(|p| Box::new(p) as Box<dyn PcmSink>)
    };
    #[cfg(not(feature = "play"))]
    let open =
        || -> Result<Box<dyn PcmSink>, String> { Err("built without the play feature".into()) };
    AudioOut::new(
        options.playback,
        pace_seconds(options.buffer_seconds),
        open,
        PcmCapture::from_env,
    )
}

/// Observations for headless hosts; the desktop app does not use them.
impl CoupledRuntime {
    /// Start accumulating PCM statistics and hash (and keep the samples when
    /// `keep`). Call right after construction to cover the whole run. Does
    /// nothing without a PCM-producing companion.
    pub fn observe_pcm(&mut self, keep: bool) {
        if self.has_audio() {
            self.out.observe(keep);
        }
    }
    /// PCM seen since `observe_pcm`, up to the last `snapshot()`/`sync`.
    pub fn pcm_tap(&self) -> Option<&PcmTap> {
        self.has_audio().then(|| self.out.tap()).flatten()
    }
    /// Why the DSP stopped, once it has (set when a frame exchange fails).
    pub fn dsp_halted(&self) -> Option<String> {
        self.companion.halted()
    }
    /// Underrun events of the audio sink since the session began (`None`
    /// without a player: no device, no sink capture).
    pub fn sink_underruns(&self) -> Option<u64> {
        self.has_audio()
            .then(|| self.out.sink_underruns())
            .flatten()
    }
    /// The DSP's canonical state blob (`SHRD`, every overlay byte as a range)
    /// and its instruction count, after every frame sent so far.
    pub fn dsp_export(&mut self) -> Option<(Vec<u8>, u64)> {
        let (blob, icount) = self.companion.export_state(&mut self.emulator).ok()?;
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
        self.audio_health()
    }
    /// Stop the PCM capture (the file is complete after this returns).
    pub fn stop_pcm_capture(&mut self) {
        if self.has_audio() {
            // Drain the frames in flight into the capture first.
            self.sync_audio();
            self.out.stop_capture();
        }
    }
    /// Capture the emulator's own PCM (before the output buffer) to `path`:
    /// a float WAV when it ends in .wav, else raw f32le. No length limit.
    pub fn capture_pcm(&mut self, path: &std::path::Path) -> Result<(), String> {
        if !self.has_audio() {
            return Err("no native audio session".into());
        }
        // The DSP worker runs ahead of the host by a number of frames that depends on host timing. Drain them first, so that the
        // capture starts at the frame the ColdFire has sent (the same stream position in every run), not at a varying one.
        self.sync_audio();
        self.out.start_capture(path)
    }
}

/// SHARC+ only data: the DSP worker's counters, profiles, memory and patches.
#[cfg(feature = "sharc")]
impl CoupledRuntime {
    /// DSPI2 frames handed to the DSP worker (one per 0.667 ms of audio).
    pub fn frames_sent(&self) -> u64 {
        self.sharc().map_or(0, |s| s.handle.frames_sent())
    }
    /// The DSP worker's thread CPU time, tier counters and per-period wall
    /// times (waits for the frames in flight).
    pub fn worker_report(&self) -> Option<WorkerReport> {
        self.sharc()?.handle.worker_report()
    }
    /// Enable or clear opt-in per-entry dispatch profiling in the DSP worker.
    pub fn set_region_profile(&self, enabled: bool) -> bool {
        self.sharc()
            .is_some_and(|s| s.handle.set_region_profile(enabled))
    }
    /// Start (or clear and stop) the DSP profile of `sharc.profile`: per-entry dispatch costs of the fast and block tiers (needs the
    /// engine's `region-profile` feature) and the interpreter's instruction counts per PC.
    pub fn set_sharc_profile(&self, enabled: bool) -> bool {
        self.sharc()
            .is_some_and(|s| s.handle.set_region_profile(enabled) && s.handle.set_pc_hist(enabled))
    }
    /// Enable the Engine's existing execution-frequency maps. The call waits
    /// for the worker acknowledgement so later probes are ordered after it.
    pub fn set_execution_profile(&self, enabled: bool) -> bool {
        self.sharc()
            .is_some_and(|s| s.handle.set_execution_profile(enabled))
    }
    /// Fetch execution maps only on an explicit post-run request. Ordinary
    /// worker reports intentionally do not clone the TSV map strings.
    pub fn execution_profile(&self) -> Option<ExecutionProfile> {
        self.sharc()?.handle.execution_profile()
    }
    /// Overlay a DSP patch onto the running DSP between two frames and drop
    /// every cached decode and generated block that touched the patched code
    /// (the engine re-verifies code identity on its own). Returns the frame
    /// count at which it took effect.
    pub fn inject_dsp_patch(
        &mut self,
        patch: sharc_peer::dsp_patch::DspPatch,
    ) -> Result<Value, String> {
        let sharc = self.sharc_mut()?;
        let ops = patch.ops.len();
        let frame = sharc.handle.frames_sent();
        let dsp_icount = sharc.handle.apply_patch(patch)?;
        Ok(json!({"ops": ops, "at_frame": frame, "dsp_icount": dsp_icount}))
    }
    /// DSP memory bytes (`None` where the engine does not know the byte), read
    /// between two frames.
    pub fn peek_dsp(&mut self, addr: u32, len: usize) -> Result<Vec<Option<u8>>, String> {
        self.sharc_mut()?;
        self.sync_audio();
        self.sharc_mut()?.handle.peek_bytes(addr, len)
    }
    /// Diagnose the session's active DSP, fenced after outstanding audio work.
    pub fn dsp_ddr_monitor(
        &mut self,
        command: sharc_peer::ddr_monitor::DdrMonitorCommand,
    ) -> Result<sharc_peer::ddr_monitor::DdrMonitorStatus, String> {
        self.sharc_mut()?;
        self.sync_audio();
        self.sharc_mut()?.handle.ddr_monitor(command)
    }
    /// Read one exact DSP word address, including unaligned compact MMRs.
    pub fn peek_dsp_word(&mut self, addr: u32) -> Result<Option<u32>, String> {
        self.sharc_mut()?;
        self.sync_audio();
        self.sharc_mut()?.handle.peek_word(addr)
    }
}

/// Without the `sharc` feature there is never a DSP session.
#[cfg(not(feature = "sharc"))]
impl CoupledRuntime {
    pub fn inject_dsp_patch(
        &mut self,
        _patch: sharc_peer::dsp_patch::DspPatch,
    ) -> Result<Value, String> {
        Err("no DSP audio session".into())
    }
    pub fn peek_dsp(&mut self, _addr: u32, _len: usize) -> Result<Vec<Option<u8>>, String> {
        Err("no DSP audio session".into())
    }
    pub fn dsp_ddr_monitor(
        &mut self,
        _command: sharc_peer::ddr_monitor::DdrMonitorCommand,
    ) -> Result<sharc_peer::ddr_monitor::DdrMonitorStatus, String> {
        Err("no DSP audio session".into())
    }
    pub fn peek_dsp_word(&mut self, _addr: u32) -> Result<Option<u32>, String> {
        Err("no DSP audio session".into())
    }
}

#[cfg(all(test, feature = "sharc"))]
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

#[cfg(test)]
mod tests;
