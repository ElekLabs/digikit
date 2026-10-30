"""rust_ws.affected_packages: changed files -> workspace packages, over the real cargo metadata."""

import pytest

from tools.devtools import rust_ws


def sel(*files):
    return rust_ws.affected_packages(list(files))


def test_package_file_gives_package_and_reverse_deps():
    assert "digi-coupled" in sel("packages/emulator/sharc/src/lib.rs")
    # a leaf: only itself
    assert sel("apps/desktop/src-tauri/src/main.rs") == ["digiemu-desktop"]
    # longest prefix: sharc-gen is not part of sharc
    assert "sharc-gen" not in sel("packages/emulator/sharc/build.rs")


def test_whole_workspace_files():
    for f in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "unknown.bin",
    ]:
        assert sel(f) is None, f
    # one such file among harmless ones is enough
    assert sel("docs/x.md", "Cargo.lock") is None


def test_no_rust_relevance():
    assert sel("docs/x.md") == []
    assert (
        sel("tools/patchkit/pk_new.py", "tests/devtools/test_x.py", "mise.toml") == []
    )
    assert sel("tools/emulator/fast_regions.json") == []
    assert sel("modules/core/runtime/README.md") == []
    assert sel() == []


@pytest.mark.parametrize(
    "path",
    [
        "modules/core/runtime/src/rs/src/dsp.rs",
        "modules/core/runtime/src/rs/src/cleanup_tests.rs",
        "modules/core/runtime/src/rs/Cargo.toml",
        "modules/core/runtime/sdk/src/sample.rs",
        "modules/core/runtime/fit/src/lib.rs",
        "modules/core/fw/rs/src/lib.rs",
        "packages/dkneeds/src/lib.rs",
    ],
)
def test_standalone_runtime_and_local_dependencies(path):
    assert "dkrt" in sel(path)


def test_standalone_source_keeps_existing_readers():
    assert sel("modules/core/runtime/src/rs/src/dsp.rs") == [
        "dkrt",
        "elektron-native-boot",
        "patchkit",
    ]
    assert "dkrt" not in sel("packages/dkupload/src/lib.rs")
    assert "dkrt" not in sel("modules/core/adapters/dsp-voice/src/dsp/dsp-voice.s")


@pytest.fixture
def execution(monkeypatch, tmp_path):
    calls = []
    monkeypatch.setattr(rust_ws.digi_env, "rust_env", lambda env=None: dict(env or {}))
    monkeypatch.setattr(rust_ws.digi_env, "generated_dir", lambda: tmp_path)
    monkeypatch.setattr(rust_ws.digi_env, "check_generated", lambda gen: None)
    monkeypatch.setattr(
        rust_ws, "cargo", lambda args, env: calls.append((args, env)) or 0
    )
    return calls


def test_standalone_only_does_not_touch_generated_core(monkeypatch, execution):
    def forbidden(*args):
        pytest.fail("standalone host tests must not inspect generated core")

    monkeypatch.setattr(rust_ws.digi_env, "generated_dir", forbidden)
    monkeypatch.setattr(rust_ws.digi_env, "check_generated", forbidden)
    monkeypatch.setenv("SHARC_GEN_DIR", "/missing/generated")
    monkeypatch.setenv("SHARC_GEN_DIR_2", "/also/missing")
    monkeypatch.setenv("CARGO_BUILD_JOBS", "3")
    assert rust_ws.main(["test", "--packages", "dkrt"]) == 0
    assert len(execution) == 1
    args, env = execution[0]
    assert args[0] == "test"
    assert args[args.index("--manifest-path") + 1] == rust_ws.STANDALONE["dkrt"]
    assert "--workspace" not in args and "nextest" not in args
    assert "SHARC_GEN_DIR" not in env and "SHARC_GEN_DIR_2" not in env
    assert env["CARGO_BUILD_JOBS"] == "3"
    assert args[args.index("--target-dir") + 1] == str(
        rust_ws.native_target.TARGET_ROOT / "nogen" / "host-dkrt"
    )


@pytest.mark.parametrize("packages", [None, "dkrt,periph"])
def test_workspace_then_standalone_are_sequential(execution, packages):
    argv = ["test"] + (["--packages", packages] if packages else [])
    assert rust_ws.main(argv) == 0
    assert len(execution) == 2
    workspace, standalone = [args for args, _ in execution]
    assert workspace[:2] == ["nextest", "run"]
    assert "dkrt" not in workspace
    if packages:
        assert workspace[workspace.index("-p") + 1] == "periph"
    else:
        assert "--workspace" in workspace and rust_ws.DESKTOP in workspace
    assert standalone[0] == "test" and "--manifest-path" in standalone


@pytest.mark.parametrize("failure_at", [1, 2])
def test_failure_propagates_and_stops(monkeypatch, execution, failure_at):
    def fail(args, env):
        execution.append((args, env))
        return 7 if len(execution) == failure_at else 0

    monkeypatch.setattr(rust_ws, "cargo", fail)
    assert rust_ws.main(["test", "--packages", "periph,dkrt"]) == 7
    assert len(execution) == failure_at


@pytest.mark.parametrize("packages", ["", rust_ws.DESKTOP])
def test_empty_or_desktop_scope_does_not_start_cargo(execution, packages):
    assert rust_ws.main(["test", "--packages", packages]) == 0
    assert execution == []


@pytest.mark.parametrize("packages", [None, "periph,dkrt"])
def test_nextest_options_do_not_reach_standalone(execution, packages):
    argv = ["test"] + (["--packages", packages] if packages else [])
    assert rust_ws.main([*argv, "--", "-E", "all()"]) == 0
    workspace, standalone = [args for args, _ in execution]
    assert workspace[workspace.index("-E") + 1] == "all()"
    assert "-E" not in standalone and "all()" not in standalone


def test_standalone_only_forwards_cargo_filter_and_options(execution):
    assert (
        rust_ws.main(
            [
                "test",
                "--packages",
                "dkrt",
                "--",
                "invalidation",
                "--",
                "--test-threads=1",
            ]
        )
        == 0
    )
    args, _ = execution[0]
    assert len(execution) == 1
    assert args[0] == "test" and "invalidation" in args
    assert args[args.index("--") + 1 :] == ["--test-threads=1"]
