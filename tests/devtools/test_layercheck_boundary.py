"""The resident/package boundary rules of tools/devtools/layercheck.py, on small fixture trees."""

from __future__ import annotations

from pathlib import Path

import pytest

from tools.devtools import layercheck


def tree(tmp_path: Path, files: dict[str, str]) -> Path:
    src = tmp_path / layercheck.RT_SRC
    for name, text in files.items():
        f = src / name
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text, encoding="utf-8")
    return tmp_path


def rules(root: Path) -> list[tuple[str, str]]:
    return [
        (v.rule, v.where.split("src/")[-1].split(":")[0])
        for v in layercheck.check_resident_boundary(root)
    ]


def test_resident_reaching_package_is_flagged_except_through_hooks(
    tmp_path: Path,
) -> None:
    root = tree(
        tmp_path,
        {
            "lib.rs": "mod package;\nmod tick;\nmod hooks;\n",
            "tick.rs": "fn t() { crate::package::boot_steps::run(); }\n",
            "hooks.rs": "use crate::package::entry;\n",
            "package/entry.rs": "",
        },
    )
    assert rules(root) == [("boundary-hook", "tick.rs")]


def test_package_reaching_resident_only_through_resident_rs(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "dlog.rs": "",
            "settings.rs": "",
            "package/files.rs": "use crate::{dlog, settings::rows};\nuse dkrt_abi::MAX_MODULES;\n",
            "package/resident.rs": "use crate::dlog;\nuse crate::settings::rows;\n",
            "package/loader.rs": "use crate::package::files;\nuse super::files;\n",
        },
    )
    assert rules(root) == [("boundary-table", "package/files.rs")] * 2


def test_a_type_that_crosses_belongs_to_the_abi_crate(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "state.rs": "pub struct Rec;\npub const REC_MAX: u8 = 1;\npub fn reset() {}\n",
            "api.rs": "use crate::package::loader::Plan;\nfn f() { let _ = crate::package::loader::run(); }\n",
            "package/loader.rs": "pub struct Plan;\npub fn run() {}\nuse crate::state::{Rec, REC_MAX};\n",
            "package/resident.rs": "use crate::state::Rec;\nuse crate::state::reset;\n",
        },
    )
    assert sorted(rules(root)) == sorted(
        [
            ("boundary-abi", "api.rs"),
            ("boundary-hook", "api.rs"),
            ("boundary-abi", "package/loader.rs"),
            ("boundary-table", "package/loader.rs"),
            ("boundary-abi", "package/resident.rs"),
        ]
    )


def test_host_tests_and_inline_test_modules_are_not_read(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "dlog.rs": "",
            "tick.rs": "#[cfg(test)]\nmod tests {\n    use crate::package::x::y;\n}\n",
            "store_tests.rs": "use crate::package::files;\n",
            "package/files_tests.rs": "use crate::dlog;\n",
        },
    )
    assert rules(root) == []


def test_super_chains_resolve_against_the_module_path(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "dlog.rs": "",
            "package/mod.rs": "",
            "package/loader/mod.rs": "",
            "package/loader/plan.rs": "use super::super::super::dlog;\nfn f() { super::super::super::dlog::w(); }\n",
            "package/loader/ok.rs": "use super::plan;\nuse super::super::loader::plan as p;\nfn f() { super::plan::run(); }\n",
            "tick.rs": "fn f() { self::x(); }\n",
            "ui/mod.rs": "use super::package::files;\n",
            "package/files.rs": "",
        },
    )
    assert sorted(rules(root)) == [
        ("boundary-hook", "ui/mod.rs"),
        ("boundary-table", "package/loader/plan.rs"),
        ("boundary-table", "package/loader/plan.rs"),
    ]


def test_inline_crate_paths_inside_package_are_flagged(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "dlog.rs": "",
            "package/files.rs": "fn f() { crate::dlog::write(); crate::package::scan::run(); }\n",
            "package/resident.rs": "fn f() { crate::dlog::write(); }\n",
        },
    )
    assert rules(root) == [("boundary-table", "package/files.rs")]


def test_a_use_tree_mixing_both_sides_flags_each_crossing(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "dlog.rs": "",
            "tick.rs": "use crate::{dlog, package::files};\n",
            "package/files.rs": "use crate::{dlog, package::scan};\nuse dkrt_abi::X;\n",
        },
    )
    assert sorted(rules(root)) == [
        ("boundary-hook", "tick.rs"),
        ("boundary-table", "package/files.rs"),
    ]


def test_type_and_item_come_from_declarations_not_the_case(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "state.rs": "pub struct Rec;\nimpl Rec { pub fn new() {} }\npub enum Kind { A }\npub const MAX: u8 = 1;\npub fn Odd() {}\npub type Alias = u8;\n",
            "package/a.rs": "fn f() { crate::state::Rec::new(); }\n",
            "package/b.rs": "fn f() { let _ = crate::state::Kind::A; }\n",
            "package/c.rs": "fn f() { crate::state::Odd(); let _ = crate::state::MAX; }\n",
            "package/d.rs": "use crate::state::Alias;\n",
        },
    )
    assert sorted(rules(root)) == [
        ("boundary-abi", "package/a.rs"),
        ("boundary-abi", "package/b.rs"),
        ("boundary-abi", "package/d.rs"),
        ("boundary-table", "package/c.rs"),
        ("boundary-table", "package/c.rs"),
    ]


def test_exemptions_cover_functions_only_and_lib_rs_is_read(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "lib.rs": "mod package;\npub use package::loader::Plan;\nfn init() { package::boot::run(); }\n",
            "hooks.rs": "use crate::package::boot;\nfn f() { crate::package::boot::run(); let _: crate::package::loader::Plan; }\n",
            "state.rs": "pub struct Rec;\npub fn reset() {}\n",
            "package/loader.rs": "pub struct Plan;\n",
            "package/resident.rs": "use crate::state::reset;\nfn f(_: crate::state::Rec) {}\n",
        },
    )
    assert sorted(rules(root)) == [
        ("boundary-abi", "hooks.rs"),
        ("boundary-abi", "lib.rs"),
        ("boundary-abi", "package/resident.rs"),
        ("boundary-hook", "lib.rs"),
    ]


def only_boundary(monkeypatch: pytest.MonkeyPatch) -> None:
    """`check()` with every other rule silenced: the fixture trees hold only the runtime sources."""
    monkeypatch.setattr(layercheck, "load_modules", lambda root: [])
    for name in dir(layercheck):
        if name.startswith("check_") and name != "check_resident_boundary":
            monkeypatch.setattr(layercheck, name, lambda *a, **k: [])


def test_the_rules_are_off_in_check_until_enforced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    only_boundary(monkeypatch)
    assert layercheck.BOUNDARY_ENFORCED is False
    root = tree(tmp_path, {"dlog.rs": "", "package/files.rs": "use crate::dlog;\n"})
    assert not [v for v in layercheck.check(root) if v.rule.startswith("boundary")]


def test_check_reports_crossings_as_failures_when_enforced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    only_boundary(monkeypatch)
    root = tree(tmp_path, {"dlog.rs": "", "package/files.rs": "use crate::dlog;\n"})
    found = layercheck.check(root, boundary_enforced=True)
    assert [v.rule for v in found if v.rule.startswith("boundary")] == [
        "boundary-table"
    ]
