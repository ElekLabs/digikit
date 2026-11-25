//! Capture bridge. The caller owns the workload, WAV formatting and report.
mod capture;

use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::StreamTrait;
use dkaudio::{exact_device, inputs, open_input, stereo_config};
use serde_json::json;

use capture::{LIMIT, RATE, Recorder};

fn emit(value: serde_json::Value) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{value}")
        .and_then(|()| stdout.flush())
        .map_err(|e| e.to_string())
}

fn capture(input: &str, file: &Path) -> Result<(), String> {
    let candidates = inputs()?;
    let selected = candidates
        .iter()
        .find(|dev| dev.id() == input)
        .or_else(|| {
            let mut exact = candidates
                .iter()
                .filter(|dev| dev.name().eq_ignore_ascii_case(input));
            let item = exact.next()?;
            exact.next().is_none().then_some(item)
        })
        .ok_or("Choose an exact, unambiguous audio input name or ID from dkaudio inputs.")?;
    let id = selected
        .id()
        .parse::<cpal::DeviceId>()
        .map_err(|e| e.to_string())?;
    let device = exact_device(&cpal::default_host(), &id)?;
    let supported = stereo_config(&device, true, Some(RATE))?;
    let recorder = Recorder::new();
    let (tx, ready) = mpsc::sync_channel(1);
    let stream = open_input(&device, supported, recorder.sink(tx), recorder.errors())?;
    let started = Instant::now();
    stream
        .play()
        .map_err(|e| format!("Start audio input: {e}"))?;
    ready
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "The audio input did not deliver PCM.")?;

    let (tx, stop) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let ok = io::stdin().read_line(&mut line).is_ok() && line.trim() == "stop";
        let _ = tx.send(ok);
    });
    emit(json!({"event":"ready"}))?;
    if stop.recv_timeout(LIMIT) != Ok(true) {
        return Err("USB audio capture was cancelled or timed out.".into());
    }
    let elapsed = started.elapsed();
    stream
        .pause()
        .map_err(|e| format!("Stop audio input: {e}"))?;
    drop(stream);
    let frames = recorder.finish(elapsed, file)?;
    emit(json!({
        "source": "usb-audio", "input": selected.name(), "input_id": selected.id(),
        "route": "master L/R", "wall_seconds": elapsed.as_secs_f64(), "frames": frames,
    }))
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd] if cmd == "inputs" => emit(json!(inputs()?)),
        [cmd, input, file] if cmd == "capture" => capture(input, Path::new(file)),
        _ => Err("Usage: dkaudio inputs | dkaudio capture INPUT_ID_OR_NAME FILE.f32le".into()),
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dkaudio: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
