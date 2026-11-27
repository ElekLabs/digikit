"""`pk_modcheck.write_uses`: the `[uses.<device>]` lock lines of a module.toml."""

from tools.patchkit import pk_modcheck

DT2 = {"machine_rows": 1, "param_rows": 4, "page_rows": 0, "record_bytes": 176}
DN2 = {"machine_rows": 1, "param_rows": 4, "page_rows": 1, "record_bytes": 336}
BOTH = {"dt2": DT2, "dn2": DN2}
BASE = '[module]\nid = "a/b"\n\n# the machine\n[[machine]]\nid = "b"\n'
TABLES = (
    "\n[uses.dt2]\nmachine_rows = 1\nparam_rows = 4\npage_rows = 0\nrecord_bytes = 176\n"
    "\n[uses.dn2]\nmachine_rows = 1\nparam_rows = 4\npage_rows = 1\nrecord_bytes = 336\n"
)


def test_appends_missing_tables_per_device(tmp_path):
    p = tmp_path / "module.toml"
    p.write_text(BASE)
    assert pk_modcheck.write_uses(p, BOTH)
    assert p.read_text() == BASE + TABLES
    assert not pk_modcheck.write_uses(p, BOTH)


def test_changes_only_the_changed_device_and_keeps_comments(tmp_path):
    p = tmp_path / "module.toml"
    old = (
        BASE
        + "\n[uses.dt2]\n# written by module:check\nmachine_rows = 1\nparam_rows = 2\n\n"
        + "[uses.dn2]\nmachine_rows = 1\nparam_rows = 4\npage_rows = 1\nrecord_bytes = 336\n\n"
        + '[targets]\n"dt2-1.17" = "experimental"\n'
    )
    p.write_text(old)
    assert pk_modcheck.write_uses(p, BOTH)
    assert p.read_text() == old.replace("param_rows = 2", "param_rows = 4").replace(
        "param_rows = 4\n\n[uses.dn2]",
        "param_rows = 4\npage_rows = 0\nrecord_bytes = 176\n\n[uses.dn2]",
    )
    assert not pk_modcheck.write_uses(p, BOTH)


def test_a_module_built_for_one_device_gets_one_table(tmp_path):
    p = tmp_path / "module.toml"
    p.write_text(BASE)
    assert pk_modcheck.write_uses(p, {"dn2": DN2})
    assert "[uses.dt2]" not in p.read_text()
    assert pk_modcheck.write_uses(p, {"dt2": DT2, "dn2": DN2})
    assert p.read_text().count("[uses.") == 2
