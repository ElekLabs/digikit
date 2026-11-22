use super::*;

#[test]
fn a_late_close_does_not_affect_a_new_open() {
    let analyzer = Analyzer::default();
    let (mut emulator, _emulator_connection) = analyzer.source(Source::Emulator).connect(48_000);
    let (mut remote, _remote_connection) = analyzer.source(Source::Remote).connect(44_100);
    let pcm = vec![0.2; 8192];

    let old = analyzer.open(Source::Emulator).unwrap();
    emulator.push(&pcm);
    assert!(analyzer.read(old).is_some());
    let current = analyzer.open(Source::Remote).unwrap();
    analyzer.close(old);
    emulator.push(&pcm);
    assert!(analyzer.read(old).is_none());
    assert!(analyzer.read(current).is_none());
    remote.push(&pcm);
    let window = serde_json::to_value(analyzer.read(current).unwrap()).unwrap();
    assert_eq!(window["rate"], 44_100);
    assert_eq!(window["left"].as_array().unwrap().len(), 2048);
    assert_eq!(window["right"].as_array().unwrap().len(), 2048);

    analyzer.close(current);
    analyzer.close(current);
    remote.push(&pcm);
    assert!(analyzer.read(current).is_none());
}
