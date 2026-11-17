//! `job.cancel` on a coupled cold boot: the boot worker is stopped, the `firmware.load` that waits for it answers CANCELLED
//! and no job is left. Needs the coupled-audio feature (local only), the stock DN2 1.12 image (`DN2_SYX`, else `firmware/`)
//! and its ready inputs (`DIGI_COUPLED_FIXTURES`, the directory `mise run emu:ready -- dn2` fills); without them the test returns.

#![cfg(feature = "coupled-audio")]

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use digi_control::{Config, spawn};
use digi_coupled::AudioOptions;
use serde_json::json;

fn dn2_syx() -> Option<PathBuf> {
    std::env::var_os("DN2_SYX").map(PathBuf::from).or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|dir| dir.join("firmware").join("Digitone_II_OS1.12.syx"))
            .find(|path| path.exists())
    })
}

#[test]
fn a_cancelled_cold_load_answers_cancelled_and_leaves_no_job() {
    let (Some(syx), Some(fixtures)) = (
        dn2_syx(),
        std::env::var_os("DIGI_COUPLED_FIXTURES").map(PathBuf::from),
    ) else {
        return;
    };
    if !fixtures.join("cold-idle.dsp").is_file() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("digi-job-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = Config::new(Some(AudioOptions {
        image: fixtures.join("digi-audio-dn2-image.bin"),
        state: fixtures.join("digi-audio-m5.snap.dsp"),
        snapshot: fixtures.join("digi-audio-m5.snap"),
        ..Default::default()
    }));
    config.run_dir = dir.clone();
    let (handle, join) = spawn(config);

    // `start: cold` never takes the cached ready pair, so the boot runs on the worker.
    let loader = handle.clone();
    let load = std::thread::spawn(move || {
        loader.call(
            "firmware.load",
            json!({
                "syx": syx.display().to_string(),
                "runtime": true,
                "replace": true,
                "start": "cold",
            }),
        )
    });

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let status = handle.call("status", json!({})).unwrap();
        if status["job"]["kind"] == "boot" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no boot job appeared within 120 s: {status}"
        );
        assert!(
            !load.is_finished(),
            "the load ended before a boot job: {status}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let cancelled = handle.call("job.cancel", json!({})).unwrap();
    assert_eq!(cancelled["cancelled"], "boot", "{cancelled}");

    let error = load.join().unwrap().unwrap_err();
    assert_eq!(error.code, 10, "{error:?}");
    assert_eq!(error.message, "cancelled");

    let status = handle.call("status", json!({})).unwrap();
    assert!(status["job"].is_null(), "{status}");
    drop(handle);
    join.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
