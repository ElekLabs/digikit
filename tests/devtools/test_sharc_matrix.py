"""Matrix checkpoints represent whole, validated pairs; no firmware required."""

import json

import pytest

from tools.devtools import sharc_matrix as matrix


def put(path, data):
    path.write_text(json.dumps(data))
    return str(path)


@pytest.fixture
def setup(tmp_path, monkeypatch):
    monkeypatch.setattr(matrix, "tool_hashes", lambda: {"runner": "stable"})
    monkeypatch.setattr(matrix.render_cache, "devices_digest", lambda env: {})
    monkeypatch.setenv("BENCHLOCK_PATH", str(tmp_path / "benchlock"))
    monkeypatch.delenv("BENCHLOCK", raising=False)
    binary = tmp_path / "binary"
    binary.write_bytes(b"fake executable")
    firmware = tmp_path / "stock.syx"
    firmware.write_bytes(b"stock")
    manifest = tmp_path / "import.json"
    put(
        manifest,
        {
            device: {
                "coupled": str(binary),
                "daemon": str(binary),
                "profile_firmware": str(firmware),
            }
            for device in matrix.DEVICES
        },
    )
    monkeypatch.setattr(
        matrix.digi_env, "fast_env", lambda *args, **kwargs: {"SHARC_FAST_AUTO": "1"}
    )
    frozen = tmp_path / "frozen"
    matrix.freeze(frozen, manifest)
    scenario = tmp_path / "scenarios.json"
    put(scenario, {"scenarios": [{"name": "note", "seconds": 3}]})
    config = {
        "cells": [
            {
                "id": "dt2-stock",
                "device": "dt2",
                "kind": "stock",
                "inputs": {key: str(firmware) for key in matrix.INPUTS},
                "scenarios": str(scenario),
                "names": ["note"],
            }
        ],
        "instruments": [],
        "pairs": 2,
    }
    config_path = tmp_path / "matrix.json"
    put(config_path, config)
    calls = []

    def replay(binary, cell, scenario, env, seconds, folder, timeout):
        folder.mkdir(parents=True)
        (folder / "report.json").write_text("original child report")
        calls.append(str(folder))
        return {
            "pcm": "pcm",
            "state": "state",
            "icount": 10,
            "dsp_cpu": 2,
            "audio_s": 3,
            "tiers": {"interpreter": 5},
        }

    monkeypatch.setattr(matrix, "replay", replay)
    return config_path, frozen, tmp_path / "run", calls, replay


def test_failure_stops_before_timings(setup, monkeypatch):
    config, frozen, out, calls, replay = setup

    def bad(*args):
        result = replay(*args)
        if str(args[-2]).endswith("/B"):
            result["state"] = "different"
        return result

    monkeypatch.setattr(matrix, "replay", bad)
    with pytest.raises(matrix.Invalid, match="mismatch"):
        matrix.run(config, frozen, frozen, out, True)
    assert len(calls) == 2
    assert all("correctness" in call for call in calls)
    assert matrix.read(out / "checkpoint.json")["steps"] == {}
    assert matrix.read(out / "summary.json")["status"] == "failed"


@pytest.mark.parametrize(
    "mutation", ["config", "input", "binary", "manifest", "tools", "devices"]
)
def test_mid_run_identity_mutation_cannot_pass(setup, monkeypatch, mutation):
    config, frozen, out, calls, replay = setup

    def changed(*args):
        result = replay(*args)
        if len(calls) == 6:  # Final side of final timing pair, before completion.
            if mutation == "config":
                config.write_text(config.read_text() + " ")
            elif mutation == "input":
                (config.parent / "stock.syx").write_bytes(b"changed stock")
            elif mutation == "binary":
                (frozen / "dt2-coupled").write_bytes(b"changed executable")
            elif mutation == "manifest":
                manifest = frozen / "frozen.json"
                manifest.write_text(manifest.read_text() + " ")
            elif mutation == "tools":
                monkeypatch.setattr(
                    matrix, "tool_hashes", lambda: {"runner": "changed"}
                )
            elif mutation == "devices":
                monkeypatch.setattr(
                    matrix.render_cache,
                    "devices_digest",
                    lambda env: {"dt2.toml": "changed"},
                )
        return result

    monkeypatch.setattr(matrix, "replay", changed)
    with pytest.raises(matrix.Invalid):
        matrix.run(config, frozen, frozen, out, True)
    state = matrix.read(out / "checkpoint.json")
    assert state["status"] == "failed"
    assert state["failure_stage"] == "completion/identity"
    assert matrix.read(out / "summary.json")["status"] == "failed"
