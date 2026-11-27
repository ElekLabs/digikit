//! The generated query codec against protobuf-es: `runtime/core/vectors/wire-vectors.json` (written by packages/wire) holds bytes made by
//! protobuf-es; the C codec generated from `tests/fixtures/wire/project_manager.proto`, compiled for the host, decodes each and encodes
//! it again, and must give protobuf-es's bytes. Refused inputs must be refused. The harness also calls the generated query table.

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
    let generated = CCodec::render(&schema, "pm", "pm.h");
    std::fs::write(dir.join("pm.h"), generated.header).unwrap();
    std::fs::write(dir.join("pm.c"), generated.source).unwrap();
    let exe = dir.join("harness");
    let build = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-I"])
        .arg(dir)
        .arg("-I")
        .arg(root().join("sdk/include"))
        .arg("-o")
        .arg(&exe)
        .arg(wire.join("harness.c"))
        .arg(root().join("sdk/src/wire/wire.c"))
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
    let input: Vec<String> = cases
        .iter()
        .map(|c| {
            format!(
                "{} {}",
                c["message"].as_str().unwrap(),
                c["input_hex"].as_str().unwrap()
            )
        })
        .collect();
    let lines = run_harness(&exe, &input);
    assert_eq!(lines.len(), cases.len());
    // the query table and call function, checked inside the harness
    assert_eq!(run_harness(&exe, &["@calls".to_string()]), ["CALLS-OK"]);
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

/// The harness's answer to each line.
fn run_harness(exe: &std::path::Path, lines: &[String]) -> Vec<String> {
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn the_generated_code_of_an_empty_message_compiles_without_warnings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("e.proto"),
        "syntax = \"proto3\";\npackage e;\nimport \"dk/options.proto\";\nservice S { rpc Ping(Empty) returns (Empty); }\nmessage Empty {}\n",
    )
    .unwrap();
    let schema = Schema::read(&[dir.path(), &root().join("sdk/proto")], "e.proto").unwrap();
    let generated = CCodec::render(&schema, "e", "e.h");
    std::fs::write(dir.path().join("e.h"), generated.header).unwrap();
    std::fs::write(dir.path().join("e.c"), generated.source).unwrap();
    let build = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c", "-I"])
        .arg(dir.path())
        .arg("-I")
        .arg(root().join("sdk/include"))
        .arg("-o")
        .arg(dir.path().join("e.o"))
        .arg(dir.path().join("e.c"))
        .output()
        .expect("this test needs a host C compiler named cc on PATH");
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
}
