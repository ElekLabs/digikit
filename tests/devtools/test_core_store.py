from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

from tools.devtools import core_store


def make_trees(tmp_path: Path) -> tuple[Path, Path]:
    gen = tmp_path / "gen"
    rt = tmp_path / "rt"
    gen.mkdir()
    rt.mkdir()
    (gen / "blocks_00.rs").write_text("fn a() {}")
    (rt / "lib.rs").write_text("fn rt() {}")
    return gen, rt


def key(gen: Path, rt: Path, **change: object) -> str:
    args: dict = {
        "rustc": "rustc 1",
        "triple": "t",
        "flags": "",
        "profdata": None,
    } | change
    return core_store.key_of(core_store.key_record(gen, rt, **args))


def test_key_follows_every_input(tmp_path: Path) -> None:
    gen, rt = make_trees(tmp_path)
    base = key(gen, rt)
    assert key(gen, rt) == base
    assert key(gen, rt, rustc="rustc 2") != base
    assert key(gen, rt, triple="u") != base
    assert key(gen, rt, flags="-Ctarget-cpu=native") != base
    prof = tmp_path / "p.profdata"
    prof.write_bytes(b"p")
    assert key(gen, rt, profdata=prof) != base
    (rt / "lib.rs").write_text("fn rt2() {}")
    assert key(gen, rt) != base
    (rt / "lib.rs").write_text("fn rt() {}")
    (gen / "new.rs").write_text("")
    assert key(gen, rt) != base


class Stand:
    """A stand-in library build over a changing key record."""

    def __init__(self, tmp_path: Path) -> None:
        self.tmp_path = tmp_path
        self.gen, self.rt = make_trees(tmp_path)
        self.builds = 0
        self.on_build = lambda: None

    def inputs(self) -> tuple[dict[str, object], Path]:
        record = core_store.key_record(self.gen, self.rt, "rustc 1", "t", "", None)
        return record, self.gen

    def build(self, _device: str, _gen: Path) -> Path:
        self.builds += 1
        library = self.tmp_path / "built" / core_store.lib_name()
        library.parent.mkdir(exist_ok=True)
        library.write_bytes(b"lib %d" % self.builds)
        self.on_build()
        return library

    def ensure(self, root: Path, worktrees: list[Path] | None = None) -> Path:
        return core_store.ensure_core(
            "dn2",
            root,
            inputs=self.inputs,
            build=self.build,
            worktrees=[] if worktrees is None else worktrees,
        )


def test_miss_hit_and_header(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    first = stand.ensure(root)
    assert first.read_bytes() == b"lib 1"
    assert (first.parent / "header.json").is_file()
    assert (first.parent / "key.txt").read_text().strip() == first.parent.name
    assert stand.ensure(root) == first
    assert stand.builds == 1


def test_concurrent_insert_of_the_same_key_uses_the_winner(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    key_dir = root / "dn2" / core_store.key_of(stand.inputs()[0])

    def other_process_inserts() -> None:
        key_dir.mkdir(parents=True)
        (key_dir / core_store.lib_name()).write_bytes(b"theirs")
        (key_dir / "key.txt").write_text(key_dir.name + "\n")

    stand.on_build = other_process_inserts
    library = stand.ensure(root)
    assert library.read_bytes() == b"theirs"
    assert not list((root / "dn2").glob(".tmp-*"))


def test_input_change_during_the_build_is_an_error(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    stand.on_build = lambda: (stand.rt / "lib.rs").write_text("changed")
    root = tmp_path / "store"
    with pytest.raises(core_store.CoreStoreError, match="changed during the build"):
        stand.ensure(root)
    assert not list(root.glob("dn2/*/*"))


def test_eviction_keeps_four_and_the_keys_a_worktree_names(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    group = root / "dn2"
    for n in range(6):
        entry = group / f"k{n}"
        entry.mkdir(parents=True)
        (entry / core_store.lib_name()).write_bytes(b"x")
        (entry / "key.txt").write_text(f"k{n}\n")
        os.utime(entry / "key.txt", (1000 + n, 1000 + n))
    tree = tmp_path / "tree"
    (tree / "dist").mkdir(parents=True)
    pinned = {"DIGI_SHARC_CORE_DN2": str(group / "k0" / core_store.lib_name())}
    (tree / "dist" / "daemons.json").write_text(json.dumps(pinned))
    stand.ensure(root, [tree])
    names = sorted(p.name for p in group.iterdir())
    # the new key, the 3 newest old ones and the pinned k0
    assert "k1" not in names and "k2" not in names
    assert {"k0", "k3", "k4", "k5"} <= set(names)
    assert len(names) == 5
