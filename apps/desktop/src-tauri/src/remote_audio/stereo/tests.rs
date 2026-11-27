use super::*;

#[test]
fn maps_first_stereo_pair_and_silences_underruns_and_extra_outputs() {
    let (mut capture, mut playback) = monitor(1000, 1000);
    let mut before_capture = [42_u16; 8];
    assert_eq!(playback.fill(&mut before_capture, 4), 0);
    assert_eq!(before_capture, [u16::EQUILIBRIUM; 8]);

    capture.write(&[8192_i16, -16_384, i16::MAX, i16::MIN].repeat(4), 4);
    let mut prebuffer = [42_i16; 200];
    assert_eq!(playback.fill(&mut prebuffer, 2), 0);
    assert_eq!(prebuffer, [0; 200]);

    let mut output = [42_i16; 32];
    assert_eq!(playback.fill(&mut output, 4), 4);
    assert_eq!(&output[..16], [8192, -16_384, 0, 0].repeat(4));
    assert_eq!(&output[16..], &[0; 16]);
}

#[test]
fn maps_full_scale_without_wrapping_packed_24_bit_output() {
    let (mut capture, mut playback) = monitor(1000, 1000);
    capture.write(&[1.0_f32, -1.0, 1.0, -1.0, f32::NAN, f32::INFINITY], 2);
    playback.fill(&mut [0.0_f32; 200], 2);

    let mut signed = [cpal::I24::EQUILIBRIUM; 2];
    assert_eq!(playback.fill(&mut signed, 2), 1);
    assert_eq!(signed.map(cpal::I24::inner), [0x7f_ffff, -0x80_0000]);

    let mut unsigned = [cpal::U24::EQUILIBRIUM; 2];
    assert_eq!(playback.fill(&mut unsigned, 2), 1);
    assert_eq!(unsigned.map(cpal::U24::inner), [0xff_ffff, 0]);

    let mut nonfinite = [1.0_f32; 2];
    assert_eq!(playback.fill(&mut nonfinite, 2), 1);
    assert_eq!(nonfinite, [0.0; 2]);
}

#[test]
fn carries_large_capture_quanta_to_smaller_playback_callbacks_in_both_rate_directions() {
    for (input_rate, output_rate) in [(48_000, 44_100), (44_100, 48_000)] {
        let (mut capture, mut playback) = monitor(input_rate, output_rate);
        let input = [0.25_f32, -0.5].repeat(4096);
        let output_frames = (4096 * output_rate / input_rate) as usize;
        let startup_frames = playback.startup_frames;
        let mut transferred = 0;
        for quantum in 0..3 {
            capture.write(&input, 2);
            let mut output = vec![0.0_f32; output_frames * 2];
            for callback in output.chunks_mut(128 * 2) {
                transferred += playback.fill(callback, 2);
            }
            if quantum > 0 {
                assert!((output[output.len() - 2] - 0.25).abs() < 0.001);
                assert!((output[output.len() - 1] + 0.5).abs() < 0.001);
            }
        }
        assert_eq!(transferred, 3 * output_frames - startup_frames);
    }
}

#[test]
fn monitors_again_after_upsampling_overruns_the_queue() {
    let (mut capture, mut playback) = monitor(44_100, 48_000);
    let input = [0.25_f32, -0.5].repeat(4096);
    for _ in 0..6 {
        capture.write(&input, 2);
    }

    let mut output = vec![1.0_f32; 12_000 * 2];
    let transferred = playback.fill(&mut output, 2);
    assert!((1..12_000).contains(&transferred));
    assert_eq!(&output[output.len() - 2..], &[0.0, 0.0]);

    capture.write(&input, 2);
    assert_eq!(playback.fill(&mut output[..256], 2), 128);
    assert!((output[0] - 0.25).abs() < 0.001);
    assert!((output[1] + 0.5).abs() < 0.001);
}

#[test]
fn observes_the_first_stereo_pair_before_resampling() {
    use digi_coupled::pcm_observer::{PcmCursor, PcmSource};
    use std::sync::Arc;

    let source = Arc::new(PcmSource::default());
    let (writer, _connection) = source.connect(44_100);
    source.subscribe(1);
    let (mut capture, mut playback) = monitor(44_100, 48_000);
    capture.observe(writer);
    let input: Vec<f32> = (0..4096)
        .flat_map(|frame| {
            let value = frame as f32 / 4096.0;
            [value, -0.5, 1.0]
        })
        .collect();
    capture.write(&input, 3);

    let window = source.read(1, &mut PcmCursor::default()).unwrap();
    let window = serde_json::to_value(window).unwrap();
    assert_eq!(window["rate"], 44_100);
    assert_eq!(window["left"][0], 0.5);
    assert_eq!(window["left"][2047], 4095.0 / 4096.0);
    assert!(
        window["right"]
            .as_array()
            .unwrap()
            .iter()
            .all(|sample| sample == -0.5)
    );
    let mut output = vec![0.0_f32; (playback.startup_frames + 1024) * 2];
    assert_eq!(playback.fill(&mut output, 2), 1024);
}
