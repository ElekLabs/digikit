"""tools/patchkit/pk_isacheck.py: the preflight scan of section 7 for stores the SHARC+ ISA does not have (finding 49 section 10).

The stock-image cases skip when the firmware is absent."""

import json

import pytest

from tools.firmware import config
from tools.patchkit import pk_isacheck


def block(index, target, count, code=0xAD000001):
    return {
        "index": index,
        "target_address": target,
        "byte_count": count,
        "block_code": code,
        "fill": bool(code & (1 << 8)),
    }


def test_the_scan_takes_stock_code_blocks_by_target_and_the_appended_run_before_the_final_block():
    blocks = [
        block(0, 0x20000000, 100),  # L2 main code
        block(
            1, 0x282D7158, 388
        ),  # a data table the decoder reads as code: not scanned
        block(2, 0x283827CC, 100),  # the main code region
        block(3, 0x80467CD0, 36),  # SDRAM data
        block(4, 0x2001AB80, 1108),  # appended by a patch
        block(5, 0x2009AFE8, 1832),  # appended by a patch
        block(6, 0x1C1338, 0, 0xAD1B8001),  # the final block
    ]
    assert pk_isacheck.code_blocks(blocks) == [0, 2, 4, 5]


@pytest.mark.parametrize("name", ["Digitakt_II_OS1.17.syx", "Digitone_II_OS1.12.syx"])
def test_a_stock_image_has_no_excluded_store(name):
    syx = config.stock(name)
    if not syx.exists():
        pytest.skip(f"{name} not present")
    assert pk_isacheck.scan(pk_isacheck.section7(syx)) == []


def test_the_plan_gives_the_code_ranges_of_the_placed_sharc_objects(tmp_path):
    syx = tmp_path / "img.syx"
    assert pk_isacheck.plan_code_ranges(syx) is None
    plan = {
        "writes": [
            {"cpu": "sharc", "kind": "place", "code": [[0x2001AB80, 0x2001B83E]]},
            {"cpu": "sharc", "kind": "patch"},
            {"cpu": "coldfire", "kind": "place", "code": [[0x47E80000, 0x47E80100]]},
        ]
    }
    (tmp_path / "img.plan.json").write_text(json.dumps(plan))
    assert pk_isacheck.plan_code_ranges(syx) == [(0x2001AB80, 0x2001B83E)]
