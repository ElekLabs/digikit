//! The SHARC+ engine as a [`Companion`]: the worker thread behind the DSPI2 link, its frame and PCM
//! collection, the link timing counters and the health report.
//!
//! It lives in this crate, not in `sharc-peer`, because the trait takes an [`AudioOut`] and `AudioOut` is
//! here: `sharc-peer` cannot depend on `coupled`. `boot` and the other ColdFire crates stay free of SHARC.
use crate::AudioOptions;
use crate::audio_out::{AudioOut, SOURCE_RATE};
use crate::companion::{Companion, Kind};
use elektron_native_boot::Emulator;
use serde_json::{Value, json};
use sharc_peer::{DEFAULT_PERIOD, Shared, ThreadedHandle, ThreadedTimingProfile};
use std::{any::Any, cell::RefCell, rc::Rc, time::Instant};

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

pub struct SharcCompanion {
    pub(crate) handle: ThreadedHandle,
    pub(crate) shared: Rc<RefCell<Shared>>,
    /// The DSP image name of the generated core this session runs (`dn2-1.11`, `dt2-1.16`).
    dsp_core: &'static str,
    pub(crate) frames: u64,
    initial_instructions: u64,
    link_timing: bool,
    pub(crate) cf_step_calls: u64,
    pub(crate) cf_step_wall_ns: u64,
    pub(crate) pcm_handoff_ns: u64,
    pub(crate) audio_poll_wall_ns: u64,
    pub(crate) audio_sync_wall_ns: u64,
    /// When the tier counters were last printed (every 10 s, link timing only).
    tiers_printed: std::cell::Cell<Option<Instant>>,
    audio_mode: crate::AudioMode,
    spike: Option<elektron_native_boot::spikelog::CfWindow>,
    /// Marks of `step_begin`, consumed by the `poll` that follows the ColdFire step.
    cf_step_start: Option<Instant>,
    spike_start: Option<Instant>,
    /// Set by `sync`: the barrier's wall time so far. The `poll` that follows commits what the barrier
    /// completed, and both count as sync time (as when the drain ran inside the sync).
    sync_wall_ns: Option<u64>,
    /// `sync` ran and its `poll` is still to come.
    after_sync: bool,
}

impl SharcCompanion {
    /// Start the SHARC+ for `emulator`: the device profile names the DSP image, the idle loop and the sample
    /// link; `options` name the files. The ColdFire state is restored from `options.snapshot`.
    pub fn new(emulator: &mut Emulator, options: &AudioOptions) -> Result<Self, String> {
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
        let idle = sharc_peer::IdleLoop {
            range: (dsp.idle_start, dsp.idle_end),
            head: dsp.idle_head,
        };
        if !options.buffer_seconds.is_finite() || !(0.0..=10.0).contains(&options.buffer_seconds) {
            return Err("--audio-buffer must be finite and between 0 and 10 seconds".into());
        }
        let image = std::fs::read(&options.image).map_err(|e| format!("DSP image: {e}"))?;
        let patch = options
            .dsp_patch
            .as_deref()
            .map(sharc_peer::dsp_patch::DspPatch::load)
            .transpose()?;
        let sidecar = std::fs::read(&options.state).map_err(|e| format!("DSP state: {e}"))?;
        let (clock, initial_instructions, state) = continuation(&sidecar)?;
        let state = state.to_vec();
        let snapshot = std::fs::read(&options.snapshot).map_err(|e| format!("CF snapshot: {e}"))?;
        // The spike log needs the per-period DSP times and the reply waits.
        let link_timing = options.link_timing
            || std::env::var("DN2_PROFILE_LINK").as_deref() == Ok("1")
            || std::env::var_os("SHARC_TIER_TIME").is_some_and(|v| v != "0")
            || elektron_native_boot::spikelog::enabled();
        emulator.enable_ssi_diagnostic(96_000)?;
        emulator.load_state(&snapshot)?;
        // The fast tier's Cranelift backend is registered by the spawn; the engine uses it only when
        // SHARC_FAST_REGIONS names regions (the launcher sets that). The core is the device's own.
        let core = sharc_peer::dsp_core::core_for_device(&profile.device, &dsp.image)?;
        let (peer, handle, shared) = sharc_peer::dsp_core::spawn_dsp(
            core,
            sharc_peer::dsp_core::EngineInputs {
                image,
                state,
                clock_base: clock,
                patch,
                idle,
            },
            sharc_peer::period_from_env(),
            link_timing,
            true,
        )?;
        if dsp.sample_link == Some(device_profile::SampleLink::Lp0) {
            peer.enable_sample_link();
        }
        shared.borrow_mut().dsp_instructions = initial_instructions;
        emulator.set_dspi2_peer(peer.boxed());
        Ok(Self {
            handle,
            shared,
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
            cf_step_start: None,
            spike_start: None,
            sync_wall_ns: None,
            after_sync: false,
        })
    }

    fn record_cf_step(&mut self, elapsed_ns: u128) {
        if self.link_timing {
            self.cf_step_calls = self.cf_step_calls.saturating_add(1);
            self.cf_step_wall_ns = self
                .cf_step_wall_ns
                .saturating_add(elapsed_ns.min(u64::MAX as u128) as u64);
        }
    }

    fn drain(&mut self, out: &mut AudioOut) {
        let handoff_start = self.link_timing.then(Instant::now);
        let mut shared = self.shared.borrow_mut();
        self.frames += shared.frames.len() as u64;
        out.push(&shared.pcm);
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

    /// Ends a `step_chunk` call and writes the spike-log window when it is full.
    fn spike_step_end(&mut self, start: Instant, out: &AudioOut) {
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
        spike.flush(reply_wait_ns, self.handle.frames_sent(), out.queue_json());
    }
}

impl Companion for SharcCompanion {
    fn kind(&self) -> Kind {
        Kind::Sharc
    }

    fn step_begin(&mut self) {
        self.cf_step_start = self.link_timing.then(Instant::now);
        self.spike_start = self
            .spike
            .as_mut()
            .map(elektron_native_boot::spikelog::CfWindow::step_start);
    }

    fn sync(&mut self) {
        let wall_start = self.link_timing.then(Instant::now);
        self.handle.sync();
        self.sync_wall_ns =
            wall_start.map(|start| start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        self.after_sync = true;
    }

    fn poll(&mut self, _emulator: &mut Emulator, out: &mut AudioOut) {
        if let Some(start) = self.cf_step_start.take() {
            self.record_cf_step(start.elapsed().as_nanos());
        }
        let wall_start = self.link_timing.then(Instant::now);
        self.handle.poll();
        self.drain(out);
        let elapsed =
            wall_start.map(|start| start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        if std::mem::take(&mut self.after_sync) {
            // The commit of a barrier: sync time, as before the drain moved out of `sync`.
            let total = self
                .sync_wall_ns
                .take()
                .unwrap_or(0)
                .saturating_add(elapsed.unwrap_or(0));
            if self.link_timing {
                self.audio_sync_wall_ns = self.audio_sync_wall_ns.saturating_add(total);
            }
        } else if let Some(ns) = elapsed {
            self.audio_poll_wall_ns = self.audio_poll_wall_ns.saturating_add(ns);
        }
        if let Some(start) = self.spike_start.take() {
            self.spike_step_end(start, out);
        }
    }

    fn halted(&self) -> Option<String> {
        self.shared.borrow().halted.clone()
    }

    fn export_state(&mut self, _emulator: &mut Emulator) -> Result<(Vec<u8>, u64), String> {
        Ok(self.handle.export())
    }

    fn report(&self, _emulator: &Emulator) -> Value {
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

    /// Diagnostic: print the DSP tier counters to stderr every 10 s (link timing only). The audio and wall
    /// seconds are the output's.
    fn log_report(&self, merged: &Value) {
        if self.link_timing
            && self
                .tiers_printed
                .get()
                .is_none_or(|at| at.elapsed().as_secs_f64() >= 10.0)
        {
            let mut line = json!({"dsp_tiers": merged["dsp_tiers"].clone()});
            line["source_audio_seconds"] = merged["source_audio_seconds"].clone();
            line["session_wall_seconds"] = merged["session_wall_seconds"].clone();
            eprintln!("native_audio_report={line}");
            self.tiers_printed.set(Some(Instant::now()));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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
