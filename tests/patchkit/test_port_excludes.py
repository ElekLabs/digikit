"""cf.expansion excludes: the firmware entry's `cf_expansion_excludes`, checked by the resolver and copied into the port."""

import tomllib

import pytest

from tools.patchkit import port_resolve as R
from tools.patchkit.port_resolve import ROOT, ResolveError, expansion_excludes

LO, HI = 0x46712000, 0x47FE0000


def test_excludes_come_back_sorted():
    assert expansion_excludes(
        [[0x46800000, 0x46801000], [0x46740000, 0x46742000]], LO, HI
    ) == [
        [0x46740000, 0x46742000],
        [0x46800000, 0x46801000],
    ]


@pytest.mark.parametrize(
    "bad",
    [
        [[0x46700000, 0x46713000]],  # starts below the region
        [[0x47FDF000, 0x47FE1000]],  # ends above it
        [[0x46740000, 0x46740000]],  # empty
        [[0x46740000, 0x46742000], [0x46741000, 0x46743000]],  # overlapping
    ],
)
def test_excludes_outside_empty_or_overlapping_are_refused(bad):
    with pytest.raises(ResolveError):
        expansion_excludes(bad, LO, HI)


def test_each_port_holds_its_firmware_entrys_excludes():
    # The port files are generated; this keeps a hand-resolved port and its device entry from drifting apart.
    for prof in R.load_profiles().values():
        path = ROOT / "ports" / f"{prof['device']}-{prof['version']}.toml"
        if not prof["port"] or not path.exists():
            continue
        port = tomllib.loads(path.read_text())
        region = next(r for r in port["region"] if r["name"] == "cf.expansion")
        assert region["excludes"] == sorted(prof["cf_expansion_excludes"]), path.name
