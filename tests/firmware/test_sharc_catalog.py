"""Score and path helpers of tools/sharc_dev/sharc_catalog.py, on synthetic inputs."""

import sqlite3
import unittest

from tools.sharc_dev import sharc_catalog as sc
from tools.sharc_dev.sharc_cost import cost_evidence, fn_cost


class DspScoreTest(unittest.TestCase):
    def test_score_adds_terms(self) -> None:
        s = sc.dsp_score(1, "strict", ["voice_path", "hot", "helper"], [])
        self.assertAlmostEqual(s, 4 + 2 + 1.5 + 1 + 0.5)

    def test_path_tags_count_once(self) -> None:
        s = sc.dsp_score(0, None, ["voice_path", "master_path", "frame_path"], [])
        self.assertAlmostEqual(s, 1.5)

    def test_weak_pair_and_anchored(self) -> None:
        self.assertAlmostEqual(sc.dsp_score(0, "shape", ["near"], []), 0)
        self.assertAlmostEqual(
            sc.dsp_score(0, None, ["anchor"], ["dsp.fx.fn.send"]), -3
        )


class AnchorSwTest(unittest.TestCase):
    def test_pm_wins(self) -> None:
        a = {"addr": 0x283840F6, "pm": 0x1C207B, "kind": "code"}
        self.assertEqual(sc.anchor_sw(a), 0x1C207B)

    def test_linked_address_and_value_source(self) -> None:
        self.assertEqual(sc.anchor_sw({"addr": 0x283840F6, "kind": "code"}), 0x1C207B)
        v = {"addr": 0x254778, "src": 0x28384356, "kind": "value"}
        self.assertEqual(sc.anchor_sw(v), 0x1C21AB)
        self.assertIsNone(sc.anchor_sw({"addr": 0x254778, "kind": "code"}))


class RoleTest(unittest.TestCase):
    def test_roles(self) -> None:
        self.assertEqual(
            sc.dsp_role("driver/peripheral", True, True, True, False), "io"
        )
        self.assertEqual(sc.dsp_role("", True, False, False, False), "voice")
        self.assertEqual(sc.dsp_role("", False, True, False, False), "master/fx")
        self.assertEqual(sc.dsp_role("", True, True, False, False), "util")
        self.assertEqual(sc.dsp_role("", False, False, True, False), "frame")
        self.assertEqual(sc.dsp_role("", False, False, False, False), "other")
        self.assertEqual(sc.dsp_role("", True, True, False, True), "voice")


def test_recursive_cost_exposes_unresolved_callee_edges() -> None:
    db = sqlite3.connect(":memory:")
    db.executescript("""
        CREATE TABLE insn (image, function_sw, aligned, sw, form, fields, width);
        CREATE TABLE mem_access (image, sw, direction, space);
        CREATE TABLE edges (image, from_function, from_sw, to_sw, kind, to_function, cond);
        INSERT INTO insn VALUES ('fixture', 16, 1, 16, 'none', '{}', 6);
        INSERT INTO insn VALUES ('fixture', 32, 1, 32, 'none', '{}', 6);
        INSERT INTO edges VALUES ('fixture', 16, 16, 32, 'call', 32, NULL);
        INSERT INTO edges VALUES ('fixture', 32, 32, NULL, 'indirect', NULL, NULL);
    """)
    try:
        evidence = cost_evidence(fn_cost(db, "fixture", 16), follow_calls=True)
        assert evidence["self"] == "1" and evidence["total"] == "2"
        assert evidence["unknown_calls"] == 0
        assert "0x20: 1 unresolved calls or jumps excluded" in evidence["notes"]
    finally:
        db.close()


if __name__ == "__main__":
    unittest.main()
