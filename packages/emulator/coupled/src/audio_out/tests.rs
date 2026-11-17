use super::*;
use elektron_native_boot::pcm_play::PlaybackStats;
use std::{cell::RefCell, rc::Rc};

const PCM_CAPTURE_LIMIT: u64 = SOURCE_RATE as u64 * 2 * 10;

#[test]
fn pcm_tap_hashes_the_le_bytes_and_measures_activity() {
    use sha2::{Digest, Sha256};
    let mut tap = PcmTap::new(true);
    // One second of stereo: half silent, half a constant 0.5.
    let pcm: Vec<f32> = (0..SOURCE_RATE as usize * 2)
        .map(|i| if i < SOURCE_RATE as usize { 0.0 } else { 0.5 })
        .collect();
    // Fed in uneven pieces, as the drain does.
    tap.feed(&pcm[..1001]);
    tap.feed(&pcm[1001..]);
    let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(tap.sha256_hex(), format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(tap.count, pcm.len() as u64);
    assert_eq!(tap.nonzero, SOURCE_RATE as u64);
    assert_eq!(tap.peak, 0.5);
    assert!((tap.rms() - (0.5f64 * 0.5 / 2.0).sqrt()).abs() < 1e-9);
    assert!((tap.seconds() - 1.0).abs() < 1e-12);
    assert_eq!((tap.windows, tap.active_windows), (10, 5));
    assert_eq!(tap.kept.as_deref(), Some(&pcm[..]));
    tap.feed(&[f32::NAN]);
    assert_eq!(tap.non_finite, 1);
}
#[test]
fn pcm_capture_writer_uses_little_endian_and_respects_the_limit() {
    let mut bytes = Vec::new();
    let mut count = 0;
    write_pcm_f32le(&mut bytes, &mut count, &[-1.0, 0.0, 0.5], PCM_CAPTURE_LIMIT).unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        bytes,
        [
            (-1.0f32).to_le_bytes(),
            0.0f32.to_le_bytes(),
            0.5f32.to_le_bytes()
        ]
        .concat()
    );

    let mut bounded = Vec::new();
    let mut count = PCM_CAPTURE_LIMIT - 1;
    write_pcm_f32le(&mut bounded, &mut count, &[0.25, 0.75], PCM_CAPTURE_LIMIT).unwrap();
    assert_eq!(count, PCM_CAPTURE_LIMIT);
    assert_eq!(bounded, 0.25f32.to_le_bytes());
}
#[test]
fn pcm_capture_writer_returns_the_write_error_without_advancing_count() {
    struct FailingWriter;
    impl std::io::Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("expected write failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = 0;
    assert!(write_pcm_f32le(&mut FailingWriter, &mut count, &[0.5], PCM_CAPTURE_LIMIT).is_err());
    assert_eq!(count, 0);
}

#[derive(Default)]
struct FakeState {
    pushed: Vec<Vec<f32>>,
    queued_device_frames: u64,
    pending_source_frames: u64,
    underruns: u64,
}

struct FakeSink(Rc<RefCell<FakeState>>);

impl PcmSink for FakeSink {
    fn push(&self, pcm: &[f32]) {
        self.0.borrow_mut().pushed.push(pcm.to_vec());
    }
    fn queue_state(&self) -> QueueState {
        let s = self.0.borrow();
        QueueState {
            queued_device_frames: s.queued_device_frames,
            feeder_pending_source_frames: s.pending_source_frames,
            device_rate: 44_100,
            underrun_events: s.underruns,
            rebuffering: s.underruns > 0,
        }
    }
    fn stats(&self) -> PlaybackStats {
        PlaybackStats::default()
    }
    fn device(&self) -> String {
        "fake".into()
    }
    fn device_rate(&self) -> u32 {
        44_100
    }
}

fn fake_out(pace: f64) -> (AudioOut, Rc<RefCell<FakeState>>) {
    let state = Rc::new(RefCell::new(FakeState::default()));
    let sink = Rc::clone(&state);
    let out = AudioOut::new(true, pace, move || Ok(Box::new(FakeSink(sink))));
    (out, state)
}

#[test]
fn push_reaches_sink_and_tap_and_counts_samples() {
    let (mut out, state) = fake_out(0.5);
    out.observe(true);
    out.push(&[0.25, -0.25, 0.5, -0.5]);
    out.push(&[1.0, 1.0]);
    assert_eq!(out.samples(), 6);
    assert_eq!(
        state.borrow().pushed,
        [vec![0.25, -0.25, 0.5, -0.5], vec![1.0, 1.0]]
    );
    let tap = out.tap().unwrap();
    assert_eq!(tap.count, 6);
    assert_eq!(tap.kept.as_ref().unwrap().len(), 6);
    assert_eq!(out.report_fields()["source_pcm_frames"], 3);
    assert_eq!(out.report_fields()["sink_connected"], true);
}

#[test]
fn queue_ahead_needs_device_and_pending_audio_to_reach_the_pace_target() {
    let (out, state) = fake_out(0.5);
    assert!(!out.queue_ahead());
    // 0.25 s in the device ring (44.1 kHz) and 0.2 s pending at the source rate: 0.45 s.
    state.borrow_mut().queued_device_frames = 11_025;
    state.borrow_mut().pending_source_frames = 9_600;
    assert!(!out.queue_ahead());
    state.borrow_mut().pending_source_frames = 12_000;
    assert!(out.queue_ahead());
    assert_eq!(out.queue_json()["pace_seconds"], 0.5);
}

#[test]
fn without_a_sink_the_queue_is_never_ahead_and_underruns_are_unknown() {
    let out = AudioOut::new(false, 0.5, || unreachable!("no playback requested"));
    assert!(!out.queue_ahead());
    assert_eq!(out.sink_underruns(), None);
    assert!(out.queue_json().is_null());
    assert_eq!(out.report_fields()["sink_connected"], false);
}

#[test]
fn a_failed_device_open_is_reported_and_pcm_still_counts() {
    let mut out = AudioOut::new(true, 0.5, || Err("no device".into()));
    out.push(&[0.5; 8]);
    let report = out.report_fields();
    assert_eq!(report["device_error"], "no device");
    assert_eq!(report["playback_requested"], true);
    assert_eq!(report["sink_connected"], false);
    assert_eq!(report["flow_started"], true);
}

#[test]
fn sink_underruns_follow_the_sink() {
    let (out, state) = fake_out(0.5);
    assert_eq!(out.sink_underruns(), Some(0));
    state.borrow_mut().underruns = 2;
    assert_eq!(out.sink_underruns(), Some(2));
}

#[test]
fn rebuffer_recovery_refills_to_the_buffer_length_inside_the_ring() {
    use crate::AudioMode;
    let (mode, recovery) = player_settings(0.5, AudioMode::Rebuffer);
    assert!(matches!(mode, Mode::Buffer(s) if s == 0.5));
    assert!(matches!(recovery, Recovery::Rebuffer(s) if s == 0.5));
    // Clamped to what fits the ~1 s output ring, and at least 0.2 s.
    assert!(
        matches!(player_settings(5.0, AudioMode::Rebuffer).1, Recovery::Rebuffer(s) if s == 0.9)
    );
    let (mode, recovery) = player_settings(0.0, AudioMode::Stretch);
    assert!(matches!(mode, Mode::Live));
    assert!(matches!(recovery, Recovery::Stretch(s) if s == 0.2));
    assert!(matches!(
        player_settings(0.0, AudioMode::Gap).1,
        Recovery::Gap
    ));
}

#[test]
fn pace_target_equal_to_the_refill_level_keeps_a_stalled_run_stepping() {
    // After an underrun the player stays silent until `refill` seconds are queued. The controller
    // waits while `queue_ahead` is true, so the pace target must not lie below that level.
    let buffer = 0.5;
    let Recovery::Rebuffer(refill) = player_settings(buffer, crate::AudioMode::Rebuffer).1 else {
        unreachable!()
    };
    let (out, state) = fake_out(buffer.max(PACE_MIN_SECONDS));
    state.borrow_mut().underruns = 1;
    state.borrow_mut().queued_device_frames = (refill * 44_100.0) as u64 - 100;
    assert!(
        !out.queue_ahead(),
        "below the refill level the run keeps stepping"
    );
    state.borrow_mut().queued_device_frames = (refill * 44_100.0) as u64 + 100;
    assert!(out.queue_ahead());
}
