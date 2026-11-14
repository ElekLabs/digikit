from __future__ import annotations

from pathlib import Path

from tools.devtools import fast_roots


def test_budget_evaluation() -> None:
    budget = fast_roots.Budget("s", "f.json", 3, 1000)
    assert fast_roots.over_budget(999.0, budget) == []
    assert fast_roots.over_budget(1001.0, budget) == ["interpreted"]
    report = {
        "scenarios": [
            {
                "name": "s",
                "audio_seconds": 2.0,
                "dsp_instructions": {"interpreter": 3000, "block_entries": 8000},
            }
        ]
    }
    assert fast_roots.interpreted_per_audio_second(report, "s") == 1500.0
    assert fast_roots.block_entries_per_audio_second(report, "s") == 4000.0


def test_block_entries_are_checked_only_when_the_budget_has_a_limit() -> None:
    open_budget = fast_roots.Budget("s", "f.json", 3, 1000)
    assert fast_roots.over_budget(1.0, open_budget, 1e12) == []
    limited = fast_roots.Budget("s", "f.json", 3, 1000, 5000)
    assert fast_roots.over_budget(1.0, limited, 4999.0) == []
    assert fast_roots.over_budget(1.0, limited, 5001.0) == ["block entries"]
    assert fast_roots.over_budget(2000.0, limited, 5001.0) == [
        "interpreted",
        "block entries",
    ]


def test_budget_file_lists_cases_per_state_and_the_reset_case_fails_when_over(
    tmp_path: Path,
) -> None:
    path = tmp_path / "budgets.json"
    path.write_text(
        '{"_comment": "x", "dn2": {'
        '"first-boot": {"scenario": "a", "scenarios_file": "f", "seconds": 3, "max_interpreted_per_audio_s": 100},'
        '"reset": {"scenario": "a", "scenarios_file": "f", "seconds": 3, "max_interpreted_per_audio_s": 200,'
        ' "max_block_entries_per_audio_s": 900}}}'
    )
    cases = fast_roots.load_budgets(path)["dn2"]
    assert list(cases) == ["first-boot", "reset"]
    assert cases["first-boot"].max_block_entries_per_audio_s is None
    # the same rates pass the first-boot case and fail the reset case
    assert fast_roots.over_budget(150.0, cases["reset"], 1000.0) == ["block entries"]
    assert fast_roots.over_budget(250.0, cases["reset"], 10.0) == ["interpreted"]


def test_fixture_key_changes_with_each_input(tmp_path: Path) -> None:
    tracked = tmp_path / "tracked.rs"
    gen = tmp_path / "image.rs"
    tracked.write_text("a")
    gen.write_text("g")

    def key(stock: str = "s") -> str:
        return fast_roots.fixture_key(stock, {"tracked.rs": tracked}, {"image.rs": gen})

    base = key()
    assert key() == base
    tracked.write_text("a2")
    assert key() != base
    tracked.write_text("a")
    gen.write_text("g2")
    assert key() != base
    gen.write_text("g")
    assert key("other") != base

    reset = fast_roots.fixture_key(
        "s", {"tracked.rs": tracked}, {"image.rs": gen}, "reset"
    )
    assert reset != base
    assert (
        fast_roots.fixture_key(
            "s", {"tracked.rs": tracked}, {"image.rs": gen}, "first-boot"
        )
        == base
    )
