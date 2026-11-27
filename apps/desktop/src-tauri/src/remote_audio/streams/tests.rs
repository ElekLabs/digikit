use super::*;

#[test]
fn fatal_stream_error_wins_and_transient_errors_are_ignored() {
    let signal = Arc::new(StreamSignal::default());
    let mut input = on_error(&signal, false);
    let mut output = on_error(&signal, true);
    input(ErrorKind::Xrun.into());
    assert_eq!(signal.failure.load(Ordering::Acquire), 0);
    output(ErrorKind::DeviceChanged.into());
    input(ErrorKind::DeviceNotAvailable.into());
    assert_eq!(signal.failure.load(Ordering::Acquire), 0x83);
}

#[test]
#[ignore = "requires explicit real input and default output"]
fn hardware_audio_monitor() {
    let input_match =
        std::env::var("DIGI_REMOTE_AUDIO_INPUT").expect("set the exact input name or ID");
    let output_match =
        std::env::var("DIGI_REMOTE_AUDIO_OUTPUT").expect("set the exact default output name");
    let inputs: Vec<_> = dkaudio::inputs()
        .unwrap()
        .into_iter()
        .filter(|input| input.name() == input_match || input.id() == input_match)
        .collect();
    assert_eq!(
        inputs.len(),
        1,
        "the requested input must match exactly one device"
    );
    let host = cpal::default_host();
    let output = host.default_output_device().expect("default audio output");
    assert_eq!(output.description().unwrap().name(), output_match);

    let require_signal = std::env::var("DIGI_REMOTE_AUDIO_REQUIRE_SIGNAL").as_deref() == Ok("1");
    use digi_coupled::pcm_observer::PcmCursor;

    let pcm_source = Arc::new(PcmSource::default());
    let monitor = super::super::RemoteAudio::new(pcm_source.clone()).unwrap();
    let seconds: usize = std::env::var("DIGI_REMOTE_AUDIO_SECONDS")
        .ok()
        .map(|value| value.parse().expect("whole seconds"))
        .unwrap_or(2);
    assert!((1..=30).contains(&seconds));
    let mut previous_generation = None;
    for session in 1..=2 {
        pcm_source.subscribe(session);
        let mut cursor = PcmCursor::default();
        assert!(pcm_source.read(session, &mut cursor).is_none());
        let (start, mut started) = tauri::async_runtime::channel(1);
        monitor
            .send(super::super::Command::Start(
                inputs[0].id().to_owned(),
                start,
            ))
            .unwrap();
        assert_eq!(started.blocking_recv().unwrap().unwrap(), output_match);
        let (observe, mut observed) = tauri::async_runtime::channel(1);
        monitor
            .send(super::super::Command::Observe(observe))
            .unwrap();
        let signal = observed
            .blocking_recv()
            .unwrap()
            .expect("running stream signals");

        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(seconds as u64 + 5);
        let window = loop {
            if let Some(window) = pcm_source.read(session, &mut cursor) {
                break serde_json::to_value(window).unwrap();
            }
            assert_eq!(signal.failure.load(Ordering::Acquire), 0);
            assert!(std::time::Instant::now() < deadline, "no analyzer window");
            thread::park_timeout(std::time::Duration::from_millis(20));
        };
        assert_eq!(window["rate"], signal.rates[0]);
        let generation = window["generation"].as_u64().unwrap();
        assert_ne!(previous_generation, Some(generation));
        previous_generation = Some(generation);
        for channel in ["left", "right"] {
            let samples = window[channel].as_array().unwrap();
            assert_eq!(samples.len(), 2048);
            assert!(
                samples
                    .iter()
                    .all(|sample| { (-1.0..=1.0).contains(&sample.as_f64().unwrap()) })
            );
        }
        let frames_after_read = signal.played.load(Ordering::Relaxed);
        if session == 1 {
            pcm_source.subscribe(0);
        }
        while signal.captured.load(Ordering::Relaxed) < 4096
            || signal.played.load(Ordering::Relaxed)
                < frames_after_read + seconds * signal.rates[1] as usize
            || require_signal
                && signal
                    .peaks
                    .iter()
                    .any(|peak| peak.load(Ordering::Relaxed) == 0)
        {
            assert_eq!(signal.failure.load(Ordering::Acquire), 0);
            assert!(
                std::time::Instant::now() < deadline,
                "audio callbacks did not deliver the requested stereo signal"
            );
            thread::park_timeout(std::time::Duration::from_millis(20));
        }

        let [left, right] = signal
            .peaks
            .each_ref()
            .map(|peak| f32::from_bits(peak.load(Ordering::Relaxed)));
        eprintln!(
            "{} Hz input → {output_match} at {} Hz. Captured {} and transferred {} stereo frames. Output peaks L={left:.5} R={right:.5}.",
            signal.rates[0],
            signal.rates[1],
            signal.captured.load(Ordering::Relaxed),
            signal.played.load(Ordering::Relaxed),
        );

        let (stop, mut stopped) = tauri::async_runtime::channel(1);
        monitor.send(super::super::Command::Stop(stop)).unwrap();
        stopped.blocking_recv().unwrap().unwrap();
        assert!(
            pcm_source
                .read(session, &mut PcmCursor::default())
                .is_none()
        );
        let (observe, mut observed) = tauri::async_runtime::channel(1);
        monitor
            .send(super::super::Command::Observe(observe))
            .unwrap();
        assert!(observed.blocking_recv().unwrap().is_none());
        // Only this test should still hold the observation signal, because
        // both data/error callback closures were released before Stop replied.
        assert_eq!(Arc::strong_count(&signal), 1);
    }
}
