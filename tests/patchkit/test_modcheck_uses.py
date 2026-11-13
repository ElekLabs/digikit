"""`pk_modcheck.write_uses`: the `[uses]` lock lines of a module.toml."""

from tools.patchkit import pk_modcheck

COUNTS = {"machine_rows": 1, "param_rows": 4, "page_rows": 1}
BASE = '[module]\nid = "a/b"\n\n# the machine\n[[machine]]\nid = "b"\n'


def test_appends_a_missing_table(tmp_path):
    p = tmp_path / "module.toml"
    p.write_text(BASE)
    assert pk_modcheck.write_uses(p, COUNTS)
    assert p.read_text() == (
        BASE + "\n[uses]\nmachine_rows = 1\nparam_rows = 4\npage_rows = 1\n"
    )
    assert not pk_modcheck.write_uses(p, COUNTS)


def test_changes_only_the_counts_and_keeps_comments(tmp_path):
    p = tmp_path / "module.toml"
    old = (
        BASE
        + "\n[uses]\n# written by module:check\nmachine_rows = 1\nparam_rows = 2\n\n"
        + '[targets]\n"dt2-1.17" = "experimental"\n'
    )
    p.write_text(old)
    assert pk_modcheck.write_uses(p, COUNTS)
    assert p.read_text() == old.replace("param_rows = 2", "param_rows = 4").replace(
        "param_rows = 4\n\n", "param_rows = 4\npage_rows = 1\n\n"
    )
    assert not pk_modcheck.write_uses(p, COUNTS)
