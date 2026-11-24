"""The MODULES reply decoder reads the bytes the runtime's encoder test wrote (`tests/fixtures/protocol/modules-reply.json`): the fixed part of a record ends at
`dsp_len`, then the sections of the services that hold something for the record."""

import json
from pathlib import Path

import pytest

from tools.device import modules_reply

FIXTURE = (
    Path(__file__).resolve().parents[1] / "fixtures" / "protocol" / "modules-reply.json"
)


def test_the_decoder_reads_the_records_and_the_sections_of_the_fixture_reply():
    fixture = json.loads(FIXTURE.read_text())
    page = modules_reply.decode_page(
        bytes.fromhex(fixture["hex"]), 1 << modules_reply.CAP_MODULE_SECTIONS
    )
    assert page["total"] == fixture["total"]
    # a record with no Workbench token has no `uiInstance` (the fixture says null)
    want = [{k: v for k, v in m.items() if v is not None} for m in fixture["modules"]]
    assert page["modules"] == want


def test_a_service_name_hashes_as_the_runtime_does():
    assert modules_reply.service_hash("") == 0x811C9DC5
    assert modules_reply.service_hash("foobar") == 0xBF9CF968


def test_a_reply_of_a_device_without_the_sections_capability_is_refused_as_an_older_layout():
    old = bytearray(18 + 71 + 2)
    old[16], old[17] = 1, 1
    old[18 + 61 + 4] = 2  # n_types of the older record
    for caps in (None, 1 << 5):
        with pytest.raises(modules_reply.OldModulesLayout, match="Update the firmware"):
            modules_reply.decode_page(bytes(old), caps)
    fixture = json.loads(FIXTURE.read_text())
    with pytest.raises(modules_reply.OldModulesLayout):
        modules_reply.decode_page(bytes.fromhex(fixture["hex"]), 1 << 5)


def test_the_capability_bit_is_the_generated_one():
    from tools.device import dkproto

    assert dkproto.CAP["MODULE_SECTIONS"] == modules_reply.CAP_MODULE_SECTIONS
