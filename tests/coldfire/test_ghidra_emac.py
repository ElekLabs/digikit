"""Decode the repository's EMAC language from hand-assembled CFPRM vectors."""

import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

import pypcode
import pytest

from tools.coldfire.cfisa.oracle import LANG_DIR


@pytest.fixture(scope="module")
def emac(tmp_path_factory):
    sla = tmp_path_factory.mktemp("emac") / "coldfire_emac.sla"
    compiler = (
        Path(pypcode.__file__).parent
        / "bin"
        / ("sleigh.exe" if sys.platform == "win32" else "sleigh")
    )
    subprocess.run(
        [str(compiler), str(LANG_DIR / "coldfire_emac.slaspec"), str(sla)],
        check=True,
        capture_output=True,
        text=True,
    )
    language = ET.parse(LANG_DIR / "coldfire_emac.ldefs").getroot().find("language")
    assert language is not None
    language.set("slafile", str(sla))
    return pypcode.Context(pypcode.ArchLanguage(str(LANG_DIR), language))


# CFPRM Rev. 3, MAC with load (6-6/6-7), MSAC with load (6-31/6-32).
# All four constructors share m_eal; the displacement follows the EMAC word.
@pytest.mark.parametrize("subtract", [False, True], ids=["mac", "msac"])
@pytest.mark.parametrize("long", [False, True], ids=["word", "long"])
@pytest.mark.parametrize("displacement", [8, -2])
def test_mac_load_displacement(emac, subtract, long, displacement):
    extension = 0x1045 | (int(long) << 11) | (int(subtract) << 8)
    code = bytes.fromhex("a22a") + extension.to_bytes(2, "big")
    code += (displacement & 0xFFFF).to_bytes(2, "big")
    insn = emac.disassemble(code, max_instructions=1).instructions[0]
    assert insn.length == 6
    assert insn.mnem == ("msac" if subtract else "mac") + (".l" if long else ".w")
    offset = "0x8" if displacement == 8 else "-0x2"
    assert f"({offset},A2), D1, ACC1" in insn.body

    # A listing must resume after the displacement, not decode it as an opcode.
    following = emac.disassemble(code + bytes.fromhex("4e71")).instructions[1]
    assert following.addr.offset == 6
    assert following.mnem == "nop"


@pytest.mark.parametrize(
    ("opword", "operand"),
    [(0xA212, "(A2)"), (0xA21A, "(A2)+"), (0xA222, "-(A2)")],
)
def test_mac_load_without_displacement(emac, opword, operand):
    code = opword.to_bytes(2, "big") + bytes.fromhex("1045")
    insn = emac.disassemble(code, max_instructions=1).instructions[0]
    assert insn.length == 4
    assert insn.mnem == "mac.w"
    assert f"{operand}, D1, ACC1" in insn.body
