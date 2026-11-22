use super::*;

#[test]
fn reads_the_latest_consecutive_stereo_window_once() {
    let source = Arc::new(PcmSource::default());
    let (mut writer, _connection) = source.connect(44_100);
    let mut cursor = PcmCursor::default();
    source.subscribe(1);

    let pcm: Vec<f32> = (0..FRAMES * 3)
        .flat_map(|frame| {
            let value = frame as f32 / (FRAMES * 3) as f32;
            [value, -value]
        })
        .collect();
    writer.push(&pcm[..FRAMES]);
    assert!(source.read(1, &mut cursor).is_none());
    writer.push(&pcm[FRAMES..]);
    let window = source.read(1, &mut cursor).unwrap();
    let generation = window.generation;
    assert_eq!(window.rate, 44_100);
    assert_eq!(
        window.left,
        pcm[FRAMES * 4..]
            .iter()
            .step_by(2)
            .copied()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        window.right,
        pcm[FRAMES * 4 + 1..]
            .iter()
            .step_by(2)
            .copied()
            .collect::<Vec<_>>()
    );
    assert!(source.read(1, &mut cursor).is_none());

    writer.push(&[f32::NAN, 3.0]);
    let window = source.read(1, &mut cursor).unwrap();
    assert_eq!(window.generation, generation);
    assert_eq!(window.left[0], pcm[FRAMES * 4 + 2]);
    assert_eq!(window.left[FRAMES - 1], 0.0);
    assert_eq!(window.right[FRAMES - 1], 1.0);
}

#[test]
fn new_subscription_and_reset_require_a_full_fresh_window() {
    let source = Arc::new(PcmSource::default());
    let (mut writer, _connection) = source.connect(48_000);
    let mut cursor = PcmCursor::default();
    let pcm = vec![0.25; FRAMES * 2];
    writer.push(&pcm);
    source.subscribe(1);
    assert!(source.read(1, &mut cursor).is_none());
    writer.push(&pcm);
    assert!(source.read(1, &mut cursor).is_some());

    source.subscribe(2);
    writer.push(&pcm[2..]);
    assert!(source.read(1, &mut cursor).is_none());
    assert!(source.read(2, &mut cursor).is_none());
    writer.push(&pcm[..2]);
    let generation = source.read(2, &mut cursor).unwrap().generation;

    writer.reset();
    writer.push(&pcm[2..]);
    assert!(source.read(2, &mut cursor).is_none());
    writer.push(&pcm[..2]);
    assert_ne!(source.read(2, &mut cursor).unwrap().generation, generation);
}

#[test]
fn stopped_writer_cannot_publish_into_a_replacement_stream() {
    let source = Arc::new(PcmSource::default());
    source.subscribe(1);
    let (mut old, old_connection) = source.connect(48_000);
    let pcm = vec![0.5; FRAMES * 2];
    old.push(&pcm);
    let mut cursor = PcmCursor::default();
    let old_generation = source.read(1, &mut cursor).unwrap().generation;
    drop(old_connection);
    assert!(source.read(1, &mut cursor).is_none());
    let (mut new, _new_connection) = source.connect(96_000);
    old.push(&pcm);
    assert!(source.read(1, &mut cursor).is_none());
    new.push(&pcm);
    let window = source.read(1, &mut cursor).unwrap();
    assert_eq!(window.rate, 96_000);
    assert_ne!(window.generation, old_generation);
}
