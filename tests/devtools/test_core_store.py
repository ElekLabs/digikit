from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path

from tools.devtools import core_store, emud_key, store


def make_repo(tmp_path: Path) -> Path:
    """A git repository with one file per kind of core input."""
    repo = tmp_path / "repo"
    files = {
        "Cargo.toml": "[workspace]",
        "Cargo.lock": "lock 1",
        "packages/emulator/sharc-rt/src/lib.rs": "fn rt() {}",
        "packages/emulator/sharc-gen/src/lib.rs": "fn gen() {}",
        "packages/emulator/sharc-gen/build.rs": "fn main() {}",
        "packages/emulator/sharc-core-lib/src/lib.rs": "fn lib() {}",
        "packages/emulator/other/src/lib.rs": "fn other() {}",
    }
    for name, text in files.items():
        path = repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    return repo


def make_plan(tmp_path: Path, **env: str) -> core_store.Plan:
    gen = tmp_path / "gen"
    gen.mkdir(exist_ok=True)
    (gen / "blocks_00.rs").write_text("fn a() {}")
    return core_store.Plan(
        ["cargo", "build", "--target-dir", str(tmp_path / "t")],
        env,
        tmp_path / "t" / core_store.lib_name(),
        gen,
        None,
    )


def key(repo: Path, plan: core_store.Plan, rustc: str = "rustc 1") -> str:
    return emud_key.key_of(core_store.key_inputs(plan, "t", rustc, repo))


def test_key_follows_every_input(tmp_path: Path) -> None:
    repo = make_repo(tmp_path)
    plan = make_plan(tmp_path)
    base = key(repo, plan)
    assert key(repo, plan) == base
    for name in (
        "Cargo.lock",
        "Cargo.toml",
        "packages/emulator/sharc-gen/src/lib.rs",
        "packages/emulator/sharc-gen/build.rs",
        "packages/emulator/sharc-rt/src/lib.rs",
        "packages/emulator/sharc-core-lib/src/lib.rs",
    ):
        path = repo / name
        old = path.read_text()
        path.write_text(old + "\n// edit")
        assert key(repo, plan) != base, name
        path.write_text(old)
    # a crate the library does not link stays out of the key
    other = repo / "packages/emulator/other/src/lib.rs"
    other.write_text("changed")
    assert key(repo, plan) == base
    assert key(repo, plan, "rustc 2") != base
    (plan.gen / "new.rs").write_text("")
    assert key(repo, plan) != base


def test_key_follows_the_flags_the_build_runs_with(tmp_path: Path) -> None:
    repo = make_repo(tmp_path)
    base = key(repo, make_plan(tmp_path))
    assert key(repo, make_plan(tmp_path, RUSTFLAGS="-Ctarget-cpu=native")) != base
    assert key(repo, make_plan(tmp_path, CARGO_PROFILE_RELEASE_LTO="fat")) != base
    changed = make_plan(tmp_path)
    changed.argv += ["--features", "x"]
    assert key(repo, changed) != base
    moved = make_plan(tmp_path)
    moved.argv[-1] = str(tmp_path / "elsewhere")  # the target dir is no input
    assert key(repo, moved) == base


class Stand:
    """A stand-in library build over a changing key."""

    def __init__(self, tmp_path: Path) -> None:
        self.tmp_path = tmp_path
        self.repo = make_repo(tmp_path)
        self.plan = make_plan(tmp_path)
        self.builds = 0
        self.on_build = lambda: None

    def resolve(self) -> tuple[core_store.Plan, emud_key.EmudInputs]:
        return self.plan, core_store.key_inputs(self.plan, "t", "rustc 1", self.repo)

    def key(self) -> str:
        return emud_key.key_of(self.resolve()[1])

    def build(self, _plan: core_store.Plan, _inputs: emud_key.EmudInputs) -> Path:
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
            resolve=self.resolve,
            build=self.build,
            worktrees=[] if worktrees is None else worktrees,
        )


def test_miss_hit_and_header(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    first = stand.ensure(root)
    assert first.read_bytes() == b"lib 1"
    assert json.loads((first.parent / "header.json").read_text())["key"] == stand.key()
    assert (first.parent / "key.txt").read_text().strip() == first.parent.name
    assert stand.ensure(root) == first
    assert stand.builds == 1


def test_partial_entry_is_a_miss(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    entry = root / "dn2" / stand.key()
    entry.mkdir(parents=True)
    (entry / core_store.lib_name()).write_bytes(b"torn")
    library = stand.ensure(root)
    assert stand.builds == 1
    assert library.read_bytes() == b"lib 1"
    assert (entry / "key.txt").is_file() and (entry / "header.json").is_file()
    # a hit never creates key.txt
    (entry / "key.txt").unlink()
    stand.ensure(root)
    assert stand.builds == 2


def test_concurrent_insert_of_the_same_key_uses_the_winner(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    key_dir = root / "dn2" / stand.key()

    def other_process_inserts() -> None:
        key_dir.mkdir(parents=True)
        (key_dir / core_store.lib_name()).write_bytes(b"theirs")
        (key_dir / "header.json").write_text("{}")
        (key_dir / "key.txt").write_text(key_dir.name + "\n")

    stand.on_build = other_process_inserts
    library = stand.ensure(root)
    assert library.read_bytes() == b"theirs"
    assert not list((root / "dn2").glob(".tmp-*"))


def test_input_change_during_the_build_is_an_error(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    stand.on_build = lambda: (stand.repo / "Cargo.lock").write_text("changed")
    root = tmp_path / "store"
    try:
        stand.ensure(root)
    except core_store.CoreStoreError as exc:
        assert "changed during the build" in str(exc)
    else:
        raise AssertionError("no error")
    assert not list(root.glob("dn2/*/*"))


def make_entry(group: Path, name: str, used: float) -> None:
    entry = group / name
    entry.mkdir(parents=True)
    (entry / core_store.lib_name()).write_bytes(b"x")
    (entry / "header.json").write_text("{}")
    (entry / "key.txt").write_text(name + "\n")
    os.utime(entry / "key.txt", (used, used))


def test_eviction_keeps_four_and_the_keys_a_worktree_names(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    group = root / "dn2"
    for n in range(6):
        make_entry(group, f"k{n}", 1000 + n)
    tree = tmp_path / "tree"
    (tree / "dist").mkdir(parents=True)
    pinned = {"DIGI_SHARC_CORE_DN2": str(group / "k0" / core_store.lib_name())}
    (tree / "dist" / "daemons.json").write_text(json.dumps(pinned))
    stand.ensure(root, [tree])
    names = sorted(p.name for p in group.iterdir())
    # the new key, the 3 newest old ones and the pinned k0
    assert {"k0", "k3", "k4", "k5"} <= set(names)
    assert "k1" not in names and "k2" not in names
    assert len(names) == 5


def test_entry_used_within_the_hour_survives_eviction(tmp_path: Path) -> None:
    group = tmp_path / "dn2"
    now = time.time()
    for n in range(6):
        make_entry(group, f"k{n}", now - 7200 + n)
    make_entry(group, "fresh", now - 1000)  # used 17 minutes ago, 5th newest
    for n in range(4):  # four newer entries push `fresh` out of the last 4
        make_entry(group, f"new{n}", now - 500 + n)
    removed = store.evict(group, "new0")
    assert (group / "fresh").is_dir()
    assert "fresh" not in removed
    assert not (group / "k0").exists()


def test_old_temp_dirs_are_removed_and_fresh_ones_kept(tmp_path: Path) -> None:
    group = tmp_path / "dn2"
    make_entry(group, "k0", time.time())
    old = group / ".tmp-123"
    fresh = group / ".tmp-456"
    stale_evict = group / ".tmp-evict-1-ab"
    for d in (old, fresh, stale_evict):
        d.mkdir()
        (d / "x").write_text("x")
    long_ago = time.time() - 7200
    os.utime(old, (long_ago, long_ago))
    os.utime(stale_evict, (long_ago, long_ago))
    store.evict(group, "k0")
    assert not old.exists() and not stale_evict.exists()
    assert fresh.is_dir()
