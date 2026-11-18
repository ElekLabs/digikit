"""Regions kept by hand (`[[firmware]] cf_flash_cave`, `cf_expansion`) are checked against the image, and a port of a device
without a DSP names no DSP stream."""

import tomllib
from types import SimpleNamespace

import pytest

from tools.patchkit import port_resolve as R
from tools.patchkit.port_resolve import (
    ROOT,
    ResolveError,
    manual_cave,
    manual_expansion,
)

CLEAR = 0x4000


def facts(main: bytes):
    """A CfFacts stand-in: `main` at base 0x40000000, the reset clear starting at CLEAR, the bss ending at 0x1000 above."""
    return SimpleNamespace(
        reset={"clear_start": 0x40000000 + CLEAR, "clear_end": 0x41000000},
        bytes_at=lambda a, n: main[a - 0x40000000 : a - 0x40000000 + n],
        analysis={"expansion": {"end": 0x47FE0000}},
    )


def test_a_zero_cave_that_ends_at_the_clear_start_is_kept():
    main = bytes([1]) * 0x100 + bytes(CLEAR - 0x100)
    got = manual_cave(facts(main), [0x40000000 + 0x200, 0x40000000 + CLEAR])
    assert (got["start"], got["size"]) == (0x40000200, CLEAR - 0x200)


@pytest.mark.parametrize(
    "span",
    [
        [0x40000200, 0x40003000],  # does not end at the clear start
        [0x40000000, 0x40000000 + CLEAR],  # holds non-zero bytes
        [0x40000000 + CLEAR, 0x40000000 + CLEAR],  # empty
    ],
)
def test_a_cave_that_is_not_zero_or_does_not_end_at_the_clear_start_is_refused(span):
    main = bytes([1]) * 0x100 + bytes(CLEAR - 0x100)
    with pytest.raises(ResolveError):
        manual_cave(facts(main), span)


def test_an_expansion_must_lie_above_the_bss_and_below_the_stack_reserve():
    f = facts(b"")
    assert manual_expansion(f, [0x47E8B000, 0x47FE0000]) == {
        "start": 0x47E8B000,
        "end": 0x47FE0000,
    }
    for bad in (
        [0x40100000, 0x47FE0000],
        [0x47E8B000, 0x47FF0000],
        [0x47FE0000, 0x47E8B000],
    ):
        with pytest.raises(ResolveError):
            manual_expansion(f, bad)


def test_the_port_of_a_device_without_a_dsp_has_no_dsp_region_stream_limits_or_anchors():
    bare = [
        p for p in R.load_profiles().values() if p["port"] and R.dsp_section(p) is None
    ]
    assert bare, "no ported firmware without a DSP"
    for prof in bare:
        port = tomllib.loads(
            (
                ROOT / "devices" / "ports" / f"{prof['device']}-{prof['version']}.toml"
            ).read_text()
        )
        assert {r["name"] for r in port["region"]} == {"cf.flash_cave", "cf.expansion"}
        assert "dsp_sha256" not in port["port"]
        assert set(port["limits"]) == {"container_max", "main_staged_max"}
        assert all(a["cpu"] == "cf" for a in port["anchor"])


def test_every_ported_firmware_with_a_dsp_names_its_stream():
    for prof in R.load_profiles().values():
        path = ROOT / "devices" / "ports" / f"{prof['device']}-{prof['version']}.toml"
        if not prof["port"] or not path.exists():
            continue
        port = tomllib.loads(path.read_text())["port"]
        assert ("dsp_sha256" in port) == (R.dsp_section(prof) is not None), path.name
