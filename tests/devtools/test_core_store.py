from __future__ import annotations

import json
import os
import subprocess
import time
from dataclasses import replace
from pathlib import Path

import pytest

from tools.devtools import core_store, emud_key, store

LOCKFILE = """
version = 4

[[package]]
name = "patchkit"
version = "1.0.0"

[[package]]
name = "sharc-core-lib"
version = "0.0.1"
dependencies = ["sharc-gen", "sharc-rt"]

[[package]]
name = "sharc-gen"
version = "0.0.1"
dependencies = ["sharc-rt"]

[[package]]
name = "sharc-rt"
version = "0.0.1"
dependencies = ["external"]

[[package]]
name = "external"
version = "1.0.0"
source = "registry+https://example.invalid/index"
checksum = "old"
"""


def make_repo(tmp_path: Path) -> Path:
    """A git repository with one file per kind of core input."""
    repo = tmp_path / "repo"
    files = {
        "Cargo.toml": "[workspace]",
        "Cargo.lock": LOCKFILE,
        "packages/emulator/sharc-rt/src/lib.rs": "fn rt() {}",
        "packages/emulator/sharc-gen/src/lib.rs": "fn gen() {}",
        "packages/emulator/sharc-gen/build.rs": "fn main() {}",
        "packages/emulator/sharc-core-lib/src/lib.rs": "fn lib() {}",
        "packages/emulator/other/src/lib.rs": "fn other() {}",
        "tools/devtools/core_store.py": "recipe 1",
        "tools/emulator/digi_env.py": "recipe 1",
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
    (gen / "image.rs").write_text('pub const IMAGE: &str = "dn2-1.12";\n')
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
    # the tooling that builds it is not an input: what it decides (argv, environment, flags) is in the plan
    for script in ("tools/devtools/core_store.py", "tools/emulator/digi_env.py"):
        (repo / script).write_text("recipe 2")
    assert key(repo, plan) == base
    assert key(repo, plan, "rustc 2") != base
    (plan.gen / "new.rs").write_text("")
    assert key(repo, plan) != base


def test_key_follows_only_the_core_lockfile_closure(tmp_path: Path) -> None:
    main = make_repo(tmp_path / "main")
    lane = make_repo(tmp_path / "lane")
    plan = make_plan(tmp_path)
    # A second version elsewhere makes Cargo spell out the core reference.
    # Neither this nor a Patchkit-only dependency invalidates the core.
    lock = lane / "Cargo.lock"
    unrelated = LOCKFILE.replace(
        'name = "patchkit"\nversion = "1.0.0"',
        'name = "patchkit"\nversion = "1.0.0"\ndependencies = ["external 2.0.0"]',
    ).replace('dependencies = ["external"]', 'dependencies = ["external 1.0.0"]')
    unrelated += """
[[package]]
name = "external"
version = "2.0.0"
source = "registry+https://example.invalid/index"
checksum = "unused"
"""
    lock.write_text(unrelated)
    assert key(main, plan) == key(lane, plan)
    lock.write_text(unrelated.replace('checksum = "old"', 'checksum = "replaced"'))
    assert key(main, plan) != key(lane, plan)


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


def instrument_plan(tmp_path: Path, lane: str) -> core_store.Plan:
    """An instrumented-library plan as built in lane LANE: the raw dir and the target dir are lane paths."""
    plan = make_plan(tmp_path)
    target = tmp_path / lane / "target"
    raw = target / "profraw"
    plan.argv[-1] = str(target)
    plan.env = {"RUSTFLAGS": f"-Cprofile-generate={raw}"}
    plan.instrument = raw
    return plan


def test_the_instrumented_key_is_the_same_in_every_lane(tmp_path: Path) -> None:
    repo = make_repo(tmp_path)
    one = instrument_plan(tmp_path, "lane-a")
    two = instrument_plan(tmp_path, "lane-b")
    assert one.env != two.env
    assert key(repo, one) == key(repo, two)
    assert key(repo, one) != key(
        repo, make_plan(tmp_path)
    )  # not the plain library's key
    before = key(repo, one)
    (one.gen / "new.rs").write_text(
        ""
    )  # both plans share this gen dir: compare one plan's key before and after
    assert key(repo, one) != before  # a changed gen file is another key


class Stand:
    """A stand-in library build over a changing key."""

    def __init__(self, tmp_path: Path) -> None:
        self.tmp_path = tmp_path
        self.repo = make_repo(tmp_path)
        self.plan = make_plan(tmp_path)
        self.builds = 0
        self.on_build = lambda: None

    @property
    def device(self) -> str:
        return core_store.gen_image(self.plan.gen).partition("-")[0]

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
            self.device,
            root,
            resolve=self.resolve,
            build=self.build,
            header=lambda _library: ({"abi_version": 2}, None),
            worktrees=[] if worktrees is None else worktrees,
        )


def test_miss_hit_and_header(tmp_path: Path) -> None:
    stand = Stand(tmp_path)
    root = tmp_path / "store"
    first = stand.ensure(root)
    assert first.read_bytes() == b"lib 1"
    record = json.loads((first.parent / "header.json").read_text())
    assert record["key"] == stand.key()
    assert record["image"] == "dn2-1.12"
    assert record["header"] == {"abi_version": 2}
    assert (first.parent / "key.txt").read_text().strip() == first.parent.name
    assert stand.ensure(root) == first
    assert stand.builds == 1


def test_exact_legacy_hit_seeds_a_hit_for_another_worktree(tmp_path: Path) -> None:
    stand = Stand(tmp_path / "base")
    root = tmp_path / "store"
    _, inputs = stand.resolve()
    # A cache entry written before the lockfile projection.
    legacy = replace(
        inputs,
        files={**inputs.files, "Cargo.lock": stand.repo / "Cargo.lock"},
        flags={k: v for k, v in inputs.flags.items() if k != core_store.LOCK_CLOSURE},
    )
    old_key = emud_key.key_of(legacy)
    group = root / stand.device
    make_entry(group, old_key, time.time())
    assert stand.ensure(root).read_bytes() == b"x"
    assert stand.builds == 0

    lane = Stand(tmp_path / "lane")
    (lane.repo / "Cargo.lock").write_text(
        LOCKFILE.replace(
            'name = "patchkit"\nversion = "1.0.0"',
            'name = "patchkit"\nversion = "2.0.0"',
        )
    )
    assert lane.ensure(root) == stand.ensure(root)
    assert lane.builds == 0
    (lane.repo / "packages/emulator/sharc-rt/src/lib.rs").write_text("fn changed() {}")
    assert lane.ensure(root) != stand.ensure(root)
    assert lane.builds == 1


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
    stand.on_build = lambda: (stand.repo / "Cargo.lock").write_text(
        LOCKFILE.replace('checksum = "old"', 'checksum = "changed"')
    )
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
    pinned = {"DIGI_SHARC_CORE_LIB_DN2": str(group / "k0" / core_store.lib_name())}
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


def make_gen(path: Path, image: str) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    (path / "image.rs").write_text(f'pub const IMAGE: &str = "{image}";\n')
    return path


def test_a_generic_gen_dir_of_another_device_is_not_used(tmp_path: Path) -> None:
    dt2 = make_gen(tmp_path / "dt2gen", "dt2-1.16")
    dn2 = make_gen(tmp_path / "dn2gen", "dn2-1.11")
    # SHARC_GEN_DIR names a DT2 core: ignored for DN2, used for DT2
    env = {"SHARC_GEN_DIR": str(dt2)}
    assert (
        core_store.device_gen("dn2", env)
        == core_store.ROOT / "out/native/dn2-audio/gen"
    )
    assert core_store.device_gen("dt2", env) == dt2
    # an explicit SHARC_GEN_DIR_<DEV> of another device is refused with a message
    with pytest.raises(core_store.CoreStoreError, match="not a DN2 core"):
        core_store.device_gen("dn2", {"SHARC_GEN_DIR_DN2": str(dt2)})
    assert core_store.device_gen("dn2", {"SHARC_GEN_DIR_DN2": str(dn2)}) == dn2


def test_a_daemon_without_its_core_library_is_an_error_with_the_build_command(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from tools.devtools import verify_build
    from tools.patchkit import pk_harness

    monkeypatch.setenv("DIGI_SHARC_CORE_STORE", str(tmp_path / "store"))

    def refuse(_device: str, _root: Path, **_kw: object) -> Path:
        raise SystemExit("generated core is stale")

    monkeypatch.setattr(verify_build.core_store, "ensure_core", refuse)
    monkeypatch.setattr(
        verify_build, "daemon_pgo", lambda _device: (None, "off", "", False)
    )
    with pytest.raises(pk_harness.HarnessError) as error:
        verify_build.core_env(["dt2"])
    message = str(error.value)
    assert "dt2 core library unavailable: generated core is stale" in message
    assert "verify_build cores --device dt2" in message


def test_library_env_builds_a_miss_once_and_lets_the_environment_win(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    asked: list[str] = []

    def fake(device: str, _root: Path, **_kw: object) -> Path:
        asked.append(device)
        return tmp_path / device / "key0" / core_store.lib_name()

    monkeypatch.setattr(core_store, "ensure_core", fake)
    env = core_store.library_env(["dn2", "dt2"], tmp_path, environ={})
    assert env == {
        "DIGI_SHARC_CORE_LIB_DN2": str(tmp_path / "dn2/key0" / core_store.lib_name()),
        "DIGI_SHARC_CORE_LIB_DT2": str(tmp_path / "dt2/key0" / core_store.lib_name()),
    }
    # a variable already set names the library: nothing is built for that device
    asked.clear()
    set_by_hand = {"DIGI_SHARC_CORE_LIB_DN2": "/mine"}
    assert list(
        core_store.library_env(["dn2", "dt2"], tmp_path, environ=set_by_hand)
    ) == ["DIGI_SHARC_CORE_LIB_DT2"]
    assert asked == ["dt2"]


def test_library_env_names_the_build_command_when_a_library_cannot_be_built(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def refuse(_device: str, _root: Path, **_kw: object) -> Path:
        raise core_store.CoreStoreError("cargo failed")

    monkeypatch.setattr(core_store, "ensure_core", refuse)
    with pytest.raises(
        core_store.CoreStoreError, match="verify_build cores --device dn2"
    ):
        core_store.library_env(["dn2"], tmp_path, environ={})


def test_a_pgo_library_has_its_own_key_from_the_profile_content(
    tmp_path: Path,
) -> None:
    gen = tmp_path / "gen"
    gen.mkdir()
    (gen / "image.rs").write_text('pub const IMAGE: &str = "dn2-1.12";\n')
    profile = tmp_path / "p.profdata"
    profile.write_text("profile 1")
    repo = make_repo(tmp_path)

    def pgo_key() -> str:
        plan = core_store.build_plan("dn2", gen=gen, profdata=profile)
        return emud_key.key_of(core_store.key_inputs(plan, "t", "rustc 1", repo))

    plain = core_store.build_plan("dn2", gen=gen)
    plan = core_store.build_plan("dn2", gen=gen, profdata=profile)
    # profile-use flags go through --target, so they skip build scripts, into a target dir of their own
    assert "--target" in plan.argv and "--target" not in plain.argv
    assert f"-Cprofile-use={profile}" in plan.env["RUSTFLAGS"]
    assert (
        plan.target != plain.target and plan.library.parent.parent.parent == plan.target
    )
    plain_key = emud_key.key_of(core_store.key_inputs(plain, "t", "rustc 1", repo))
    first = pgo_key()
    assert first != plain_key
    # the key holds the profile's content, not its path
    moved = tmp_path / "other" / "p.profdata"
    moved.parent.mkdir()
    moved.write_text("profile 1")
    profile_path = profile
    profile = moved
    assert pgo_key() == first
    profile = profile_path
    profile.write_text("profile 2")
    assert pgo_key() != first


def test_an_instrumented_library_builds_apart_with_profile_generate(
    tmp_path: Path,
) -> None:
    gen = tmp_path / "gen"
    gen.mkdir()
    plan = core_store.build_plan("dn2", gen=gen, instrument=tmp_path / "raw")
    assert plan.env["RUSTFLAGS"] == f"-Cprofile-generate={tmp_path / 'raw'}"
    assert plan.profdata is None and "--target" in plan.argv
    assert plan.target != core_store.build_plan("dn2", gen=gen).target


def test_library_env_gives_a_device_with_a_profile_its_pgo_entry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    seen: dict[str, Path | None] = {}

    def fake(device: str, _root: Path, *, resolve=None, **_kw: object) -> Path:
        plan, _ = resolve() if resolve else (None, None)
        seen[device] = plan.profdata if plan else None
        return tmp_path / device / "k" / core_store.lib_name()

    monkeypatch.setattr(core_store, "ensure_core", fake)
    monkeypatch.setattr(
        core_store,
        "current",
        lambda d, g=None, p=None: (core_store.Plan([], {}, Path(), Path(), p), None),
    )
    profile = tmp_path / "p.profdata"
    core_store.library_env(
        ["dn2", "dt2"], tmp_path, environ={}, profiles={"dt2": profile}
    )
    assert seen == {"dn2": None, "dt2": profile}
