use super::*;

#[test]
fn take_duration_and_xruns_validate_the_audio_window() {
    let expected = Duration::from_millis(1800);
    assert!(!incomplete(85_440, expected, 0));
    assert!(incomplete(75_840, expected, 0));
    assert!(incomplete(85_440, expected, 1));
}

#[test]
fn sink_captures_the_first_pair_unmodified_and_signals_readiness_on_the_first_frame() {
    let recorder = Recorder::new();
    let (tx, rx) = mpsc::sync_channel(1);
    let mut sink = recorder.sink(tx);
    sink.write::<f32>(&[], 3);
    assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    sink.write(&[0.5_f32, -0.5, 1.0, 0.25, -0.25, 1.0], 3);
    assert_eq!(rx.try_recv(), Ok(()));
    assert_eq!(
        recorder.pcm.lock().unwrap().samples,
        [0.5, -0.5, 0.25, -0.25]
    );
}
