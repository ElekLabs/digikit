from __future__ import annotations

from pathlib import Path

import pytest

from tools.devtools import core_choice, core_store, sharc_matrix
from tools.emulator import coupled

VAR = "DIGI_SHARC_CORE_LIB_DN2"


def test_label_env_is_parsed_per_label_and_unknown_labels_are_refused() -> None:
    shared, per = core_choice.parse_label_env(
        [f"B:{VAR}=/x:y", "A:SHARC_AOT=0"], ["A", "B"]
    )
    assert shared == {} and per == {"A": {"SHARC_AOT": "0"}, "B": {VAR: "/x:y"}}
    for bad in ("C:X=1", "B:X", "X=1", "B:=1"):
        with pytest.raises(ValueError):
            core_choice.parse_label_env([bad], ["A", "B"])
    # coupled_ab's older `NAME=VALUE` applies to every label (a value may hold a colon)
    shared, per = core_choice.parse_label_env(
        ["SHARC_FAST_REGIONS=@/a:b", f"A:{VAR}=/l"], ["A", "B"], allow_global=True
    )
    assert shared == {"SHARC_FAST_REGIONS": "@/a:b"} and per["A"] == {VAR: "/l"}


def test_an_explicit_library_wins_and_nothing_is_built(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def refuse(*_a: object, **_k: object) -> dict[str, str]:
        raise AssertionError("the store must not be asked")

    monkeypatch.setattr(core_store, "library_env", refuse)
    assert core_choice.core_path("dn2", {VAR: "/mine"}) == "/mine"
    assert core_choice.announce("dn2", {"A": {}, "B": {VAR: "/b"}}, {VAR: "/a"}) == (
        "dn2: A uses core /a; B uses core /b"
    )
    assert core_choice.announce("dn2", {"A": {}, "B": {}}, {VAR: "/a"}) == (
        "dn2: both labels use core /a"
    )


def test_the_store_library_names_its_key(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    lib = tmp_path / core_store.STORE_NAME / "dn2" / ("k" * 40) / core_store.lib_name()
    monkeypatch.setattr(
        core_store, "library_env", lambda devices, *a, **k: {VAR: str(lib)}
    )
    line = core_choice.announce("dn2", {"A": {}, "B": {}}, {})
    assert line == f"dn2: both labels use core dn2/{'k' * 12}"


def test_the_matrix_provides_only_the_active_devices_and_the_environment_wins(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    seen: list[str] = []

    def store(devices: list[str], *a: object, **k: dict[str, str]) -> dict[str, str]:
        seen.extend(devices)
        return {f"DIGI_SHARC_CORE_LIB_{d.upper()}": f"/store/{d}" for d in devices}

    monkeypatch.setattr(core_store, "library_env", store)
    sharc_matrix.core_libraries.cache_clear()
    monkeypatch.setenv("DIGI_SHARC_CORE_LIB_DT2", "/explicit/dt2")
    monkeypatch.delenv(VAR, raising=False)
    sharc_matrix.activate({"cells": [{"device": "dn2"}], "instruments": ["dn2"]})
    env = sharc_matrix.child_env({})
    assert env[VAR] == "/store/dn2" and "DIGI_SHARC_CORE_LIB_DT2" not in env
    assert seen == ["dn2"]
    sharc_matrix.activate({"cells": [{"device": "dt2"}], "instruments": []})
    # the process environment wins, the cell's own variable wins over both
    assert sharc_matrix.child_env({})["DIGI_SHARC_CORE_LIB_DT2"] == "/explicit/dt2"
    assert (
        sharc_matrix.child_env({"DIGI_SHARC_CORE_LIB_DT2": "/cell"})[
            "DIGI_SHARC_CORE_LIB_DT2"
        ]
        == "/cell"
    )
    sharc_matrix.core_libraries.cache_clear()
    sharc_matrix.activate({"cells": [], "instruments": ["dt2", "dn2"]})


def test_the_binary_identity_honours_an_explicit_library_and_builds_nothing(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    binary = tmp_path / "coupled-run"
    binary.write_bytes(b"b")
    lib = tmp_path / "lib"
    lib.write_bytes(b"core")

    def refuse(*_a: object, **_k: object) -> dict[str, str]:
        raise AssertionError("the identity must not build a library")

    monkeypatch.setattr(core_store, "library_env", refuse)
    monkeypatch.setenv(VAR, str(lib))
    explicit = coupled.binary_identity(binary, "dn2")
    assert explicit["core"].startswith("file:")
    monkeypatch.delenv(VAR)
    monkeypatch.setattr(core_store, "restored_library", lambda device: None)
    assert coupled.binary_identity(binary, "dn2")["core"] == "none"
    key = tmp_path / "dn2" / "abc123"
    monkeypatch.setattr(
        core_store, "restored_library", lambda device: key / core_store.lib_name()
    )
    assert coupled.binary_identity(binary, "dn2")["core"] == "abc123"
