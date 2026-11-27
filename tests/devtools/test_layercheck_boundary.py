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


def test_check_runs_the_rules_by_default(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    only_boundary(monkeypatch)
    assert layercheck.BOUNDARY_ENFORCED is True
    root = tree(tmp_path, {"dlog.rs": "", "package/files.rs": "use crate::dlog;\n"})
    assert [
        v.rule for v in layercheck.check(root) if v.rule.startswith("boundary")
    ] == ["boundary-table"]
    assert not [
        v
        for v in layercheck.check(root, boundary_enforced=False)
        if v.rule.startswith("boundary")
    ]


def test_items_behind_cfg_test_are_not_read_on_one_line_or_several(
    tmp_path: Path,
) -> None:
    root = tree(
        tmp_path,
        {
            "dlog.rs": "",
            "package/a.rs": "#[cfg(test)] mod t { use crate::dlog; }\n#[cfg(test)]\nuse crate::dlog;\n"
            "#[cfg(test)]\npub(crate) use crate::dlog as d;\n"
            "#[cfg(test)]\n#[allow(dead_code)]\nfn f() {\n    crate::dlog::w();\n}\n"
            "#[cfg(all(test, unix))] fn g() { crate::dlog::w(); }\n",
            "package/b.rs": "#[cfg(not(test))]\nfn f() { crate::dlog::w(); }\n",
        },
    )
    assert rules(root) == [("boundary-table", "package/b.rs")]


def test_a_method_is_no_module_item(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "lib.rs": "mod package;\nmod api;\n",
            # `reset` and `Plan` are declared inside an impl block and an inline module: they are no module items of the resident side
            "api.rs": "pub struct Box;\nimpl Box { pub fn reset() {} }\nmod inner { pub struct Plan; }\n",
            "package/a.rs": "fn f() { crate::api::Box::reset(); }\nfn g() { crate::api::reset(); }\n",
        },
    )
    # `Box::reset` names a type's function (an abi crossing); `api::reset` is a function nobody declared at the top level, so it is a plain table crossing
    assert sorted(rules(root)) == [
        ("boundary-abi", "package/a.rs"),
        ("boundary-table", "package/a.rs"),
    ]


def test_lib_rs_alias_of_the_package_and_path_modules(tmp_path: Path) -> None:
    root = tree(
        tmp_path,
        {
            "lib.rs": '#[path = "aux_task.rs"]\npub mod aux;\nuse package::loader;\nmod package;\n',
            "aux_task.rs": "pub fn run() {}\n",
            "package/files.rs": "fn f() { crate::aux::run(); }\n",
            "package/loader.rs": "",
        },
    )
    assert rules(root) == [("boundary-table", "package/files.rs")]


def test_check_reports_crossings_as_failures_when_enforced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    only_boundary(monkeypatch)
    root = tree(tmp_path, {"dlog.rs": "", "package/files.rs": "use crate::dlog;\n"})
    found = layercheck.check(root, boundary_enforced=True)
    assert [v.rule for v in found if v.rule.startswith("boundary")] == [
        "boundary-table"
    ]


def de_naming(tmp_path: Path, files: dict[str, str]) -> list[str]:
    (tmp_path / "services" / "fx").mkdir(parents=True)
    for name, text in files.items():
        f = tmp_path / name
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text, encoding="utf-8")
    return [
        v.where.split(":")[0]
        for v in layercheck.check_de_naming(tmp_path)
        if v.rule == "de-naming"
    ]


def test_de_naming_sees_a_name_after_slashes_inside_a_string(tmp_path: Path) -> None:
    hits = de_naming(
        tmp_path,
        {
            "runtime/platform/x/src/a.rs": 'const U: &str = "http://x"; const P: &str = "services/fx/dks";\n',
            "runtime/platform/x/src/b.rs": "// services/fx/ in a comment\nconst Q: char = '\"'; /* services/fx/ */\n",
        },
    )
    assert hits == ["runtime/platform/x/src/a.rs"]


def test_de_naming_pipe_is_a_comment_in_coldfire_and_bit_or_in_sharc(
    tmp_path: Path,
) -> None:
    hits = de_naming(
        tmp_path,
        {
            "runtime/platform/x/src/cf/a.s": "| services/fx/ is a gas comment\n",
            "runtime/platform/x/src/dsp/b.s": "r0 = r1 | services/fx/;\n",
            "runtime/platform/x/src/dsp/c.inc": "// services/fx/ comment\n",
        },
    )
    assert hits == ["runtime/platform/x/src/dsp/b.s"]
