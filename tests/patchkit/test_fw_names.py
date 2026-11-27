"""Declaration names of ports/fw: <cpu>.<subsystem>.<kind>.<name> against the subsystem list."""

import pytest

from tools.patchkit import port_resolve as pr

VOCAB = {"cf": {"menu", "machine"}, "dsp": {"master"}}


def test_a_valid_name_passes():
    pr.check_name("cf.menu.fn.item_builder", "fn", VOCAB)
    pr.check_name("dsp.master.value.q31_converter.float_buf", "value", VOCAB)


def test_an_index_suffix_is_allowed():
    pr.check_name("cf.machine.site.clone.0", "site", VOCAB)
    pr.check_name("cf.machine.site.clone.0.alt", "site", VOCAB)


@pytest.mark.parametrize(
    "name, table, why",
    [
        ("arm.menu.fn.x", "fn", "cpu"),
        ("cf.audio.fn.x", "fn", "subsystem"),
        ("dsp.menu.fn.x", "fn", "subsystem"),
        ("cf.menu.fn.x", "site", "differs from its table"),
        ("cf.menu.fn", "fn", "form"),
        ("cf.menu.fn.Item", "fn", "lower case"),
        ("cf.menu.fn.item-builder", "fn", "lower case"),
        ("cf.menu.fn.x..y", "fn", "segment"),
    ],
)
def test_a_bad_name_is_refused(name, table, why):
    with pytest.raises(pr.ResolveError, match=why):
        pr.check_name(name, table, VOCAB)
