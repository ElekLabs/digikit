from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from tools.devtools import emud_key


def make(tmp_path: Path, **over: object) -> emud_key.EmudInputs:
    src = tmp_path / "src"
    gen = tmp_path / "gen"
    src.mkdir(exist_ok=True)
    gen.mkdir(exist_ok=True)
    prof = tmp_path / "p.profdata"
    # Written once: a test changes a file between calls and make() must not undo it.
    for path, text in (
        (src / "a.rs", "a"),
        (src / "b.rs", "b"),
        (gen / "x.rs", "x"),
        (prof, "prof"),
    ):
        if not path.exists():
            path.write_text(text)
    base = emud_key.EmudInputs(
        files={"a.rs": src / "a.rs", "b.rs": src / "b.rs"},
        gen_dirs={"gen": gen},
        profdata=prof,
        rustc="rustc 1",
        flags={"features": "sharc,play"},
    )
    for name, value in over.items():
        setattr(base, name, value)
    return base


def test_key_changes_with_each_input(tmp_path: Path) -> None:
    base = emud_key.key_of(make(tmp_path))
    (tmp_path / "src" / "a.rs").write_text("a2")
    assert emud_key.key_of(make(tmp_path)) != base
    (tmp_path / "src" / "a.rs").write_text("a")
    assert emud_key.key_of(make(tmp_path)) == base
    (tmp_path / "gen" / "x.rs").write_text("x2")
    assert emud_key.key_of(make(tmp_path)) != base
    (tmp_path / "gen" / "x.rs").write_text("x")
    (tmp_path / "p.profdata").write_text("prof2")
    assert emud_key.key_of(make(tmp_path)) != base
    (tmp_path / "p.profdata").write_text("prof")
    assert emud_key.key_of(make(tmp_path, flags={"features": "other"})) != base
    assert emud_key.key_of(make(tmp_path, rustc="rustc 2")) != base
    assert emud_key.key_of(make(tmp_path, profdata=None)) != base


def test_test_key_ignores_files_of_a_device_without_a_cell(tmp_path: Path) -> None:
    devices = tmp_path / "devices"
    devices.mkdir()
    (devices / "cell.toml").write_text(
        '[device]\nshort = "c1"\n[acceptance]\nready_dir = "x"\n'
    )
    (devices / "nocell.toml").write_text('[device]\nshort = "n1"\n')
    names = ["devices/cell.toml", "devices/nocell.toml"]

    def key() -> str:
        return emud_key.test_key_of(make(tmp_path), names, "", tmp_path)

    base = key()
    (devices / "nocell.toml").write_text('[device]\nshort = "n1"\n# edit\n')
    assert key() == base
    (devices / "cell.toml").write_text(
        '[device]\nshort = "c1"\n[acceptance]\nready_dir = "y"\n'
    )
    assert key() != base


def test_test_key_of_a_device_ignores_files_of_every_other_device(
    tmp_path: Path,
) -> None:
    devices = tmp_path / "devices"
    devices.mkdir()
    for stem, short in (("one", "c1"), ("two", "c2")):
        (devices / f"{stem}.toml").write_text(
            f'[device]\nshort = "{short}"\n[acceptance]\nready_dir = "x"\n'
        )
    names = ["devices/one.toml", "devices/two.toml"]

    def key(keep: str) -> str:
        return emud_key.test_key_of(make(tmp_path), names, "", tmp_path, keep=keep)

    base = {short: key(short) for short in ("c1", "c2")}
    (devices / "two.toml").write_text(
        '[device]\nshort = "c2"\n[acceptance]\nready_dir = "y"\n'
    )
    assert key("c1") == base["c1"]
    assert key("c2") != base["c2"]


def test_key_ignores_file_order(tmp_path: Path) -> None:
    one = make(tmp_path)
    two = make(tmp_path, files=dict(reversed(list(one.files.items()))))
    assert emud_key.key_of(one) == emud_key.key_of(two)


def test_mise_task_edits_preserve_key_but_rust_pin_changes_do_not(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from tools.emulator import digi_env, native_pgo
    from tools.patchkit import pk_emud_pgo

    seed = make(tmp_path)
    repo = tmp_path / "repo"
    repo.mkdir()
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    mise = repo / "mise.toml"
    mise.write_text(f'[tools]\nrust = "{digi_env.TOOLCHAIN}"\n')
    source = repo / "packages/emulator/control/src/main.rs"
    source.parent.mkdir(parents=True)
    source.write_text("fn main() {}\n")
    repo_files = emud_key.repo_files
    monkeypatch.setattr(emud_key, "repo_files", lambda: repo_files(repo))
    monkeypatch.setattr(emud_key, "rustc_version", lambda: "same rustc identity")
    monkeypatch.setattr(
        pk_emud_pgo, "generated_core", lambda _device: seed.gen_dirs["gen"]
    )
    monkeypatch.setattr(native_pgo, "current_profile", lambda _device, _gen=None: None)

    before = emud_key.key_of(emud_key.inputs("dn2", {}))
    mise.write_text(mise.read_text() + '\n[tasks.example]\nrun = "echo changed"\n')
    assert emud_key.key_of(emud_key.inputs("dn2", {})) == before

    # Distinct rustup pins can report the same rustc -vV; the pin itself still matters.
    monkeypatch.setattr(digi_env, "TOOLCHAIN", "another-toolchain")
    assert emud_key.key_of(emud_key.inputs("dn2", {})) != before

    # Untracked Cargo sources remain covered, while a newly read mise file fails the audit.
    inputs = make(tmp_path, files=repo_files(repo))
    assert emud_key.audit(inputs, f"daemon: {source}\n", tmp_path / "target") == []
    assert emud_key.audit(inputs, f"daemon: {mise}\n", tmp_path / "target") == [
        str(mise)
    ]
    before = emud_key.key_of(inputs)
    source.write_text('fn main() { println!("changed"); }\n')
    assert emud_key.key_of(inputs) != before


def test_parse_depfile_escapes_and_continuations() -> None:
    text = "/t/digi-emud: /r/a.rs /r/with\\ space.rs \\\n  /r/b.rs\n"
    assert emud_key.parse_depfile(text) == [
        "/r/a.rs",
        "/r/with space.rs",
        "/r/b.rs",
    ]


def test_audit_flags_uncovered_and_accepts_build_output(tmp_path: Path) -> None:
    inputs = make(tmp_path)
    target = tmp_path / "target"
    (target / "build").mkdir(parents=True)
    out = target / "build" / "gen.rs"
    out.write_text("o")
    stray = tmp_path / "stray.txt"
    stray.write_text("s")
    dep = (
        f"{target}/digi-emud: {tmp_path}/src/a.rs {tmp_path}/gen/x.rs "
        f"{tmp_path}/p.profdata {out} {stray}\n"
    )
    assert emud_key.audit(inputs, dep, target) == [str(stray)]
    ok = dep.replace(f" {stray}", "")
    assert emud_key.audit(inputs, ok, target) == []


def test_audit_checks_every_file_below_a_directory_dependency(tmp_path: Path) -> None:
    inputs = make(tmp_path)
    (tmp_path / "src" / "extra.rs").write_text("e")
    dep = f"{tmp_path}/t/digi-emud: {tmp_path / 'src'}\n"
    assert emud_key.audit(inputs, dep, tmp_path / "t") == [
        str((tmp_path / "src" / "extra.rs").resolve())
    ]


def test_test_key_ignores_test_sources_only(tmp_path: Path) -> None:
    names = ["a/src/lib.rs", "a/src/tests.rs", "a/src/tests/x.rs", "a/src/b_tests.rs"]
    for n in names:
        (tmp_path / n).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / n).write_text("1")

    def key() -> str:
        return emud_key.test_key_of(make(tmp_path), names, "", tmp_path)

    base = key()
    for n in names[1:]:
        (tmp_path / n).write_text("2")
    assert key() == base
    (tmp_path / names[0]).write_text("2")
    assert key() != base
    assert not emud_key.is_test_source("a/src/contest.rs")
