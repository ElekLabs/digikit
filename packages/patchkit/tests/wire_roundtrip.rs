//! The generated query codec against protobuf-es: `runtime/core/vectors/wire-vectors.json` (written by packages/wire) holds bytes made by
//! protobuf-es; the C codec generated from `tests/fixtures/wire/project_manager.proto`, compiled for the host, decodes each and encodes
//! it again, and must give protobuf-es's bytes. Refused inputs must be refused.

use patchkit::build::wire::{CCodec, Schema};
use serde_json::Value;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn harness(dir: &std::path::Path) -> PathBuf {
    let wire = root().join("tests/fixtures/wire");
    let sdk_proto = root().join("sdk/proto");
    let schema = Schema::read(&[&wire, &sdk_proto], "project_manager.proto").unwrap();
    std::fs::write(dir.join("pm.h"), CCodec::render(&schema, "pm")).unwrap();
    let exe = dir.join("harness");
    let build = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-I"])
        .arg(dir)
        .arg("-I")
        .arg(root().join("sdk/include"))
        .arg("-o")
        .arg(&exe)
        .arg(wire.join("harness.c"))
        .output()
        .expect("this test needs a host C compiler named cc on PATH");
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    exe
}

#[test]
fn the_c_codec_decodes_and_encodes_the_bytes_of_protobuf_es() {
    let dir = tempfile::tempdir().unwrap();
    let exe = harness(dir.path());
    let vectors: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("runtime/core/vectors/wire-vectors.json")).unwrap(),
    )
    .unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert!(cases.len() > 200, "{} cases", cases.len());
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input: String = cases
        .iter()
        .map(|c| {
            format!(
                "{} {}\n",
                c["message"].as_str().unwrap(),
                c["input_hex"].as_str().unwrap()
            )
        })
        .collect();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(out.status.success());
    let out = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), cases.len());
    let mut failures = vec![];
    for (case, got) in cases.iter().zip(lines) {
        let want = match case["expect"].as_str().unwrap() {
            "refuse" => "REFUSE",
            _ => case["output_hex"].as_str().unwrap(),
        };
        if got != want {
            failures.push(format!("{}: want {want}, got {got}", case["name"]));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
