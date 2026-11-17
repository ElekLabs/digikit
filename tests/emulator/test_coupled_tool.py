"""coupled: pure helpers of tools/emulator/coupled.py (no cargo, no emulator)."""

import tools.emulator.coupled as coupled


def test_merge_reports_keeps_requested_order():
    r1 = {"schema_version": 1, "scenarios": [{"name": "c"}, {"name": "a"}]}
    r2 = {"schema_version": 1, "scenarios": [{"name": "b"}]}
    merged = coupled.merge_reports([r1, r2], ["a", "b", "c"])
    assert [s["name"] for s in merged["scenarios"]] == ["a", "b", "c"]
    assert merged["schema_version"] == 1
    try:
        coupled.merge_reports([r2], ["a", "b"])
    except RuntimeError as exc:
        assert "a" in str(exc)
    else:
        raise AssertionError("missing scenario not reported")
