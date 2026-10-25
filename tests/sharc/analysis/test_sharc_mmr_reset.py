"""Reset-table parsing stays explicit and rejects contradictory inputs."""

import pytest

from tools.sharc_dev.sharc_mmr_reset import parse_reset_tables


def test_reads_concrete_rows_and_deduplicates_identical_facts():
    text = """Register Address Reset
0x3108D004 CGU0_PLLCTL PLL Control Register 0x00000000
0x3108D008 CGU0_STAT Status Register 0x0000000F
0x3108D004 CGU0_PLLCTL PLL Control Register 0x00000000
0x10000000 NOT_A_REGISTER Synthetic address 0x00000000
0x3108D000 CGU0_CTL Unspecified reset varies
"""
    assert parse_reset_tables(text) == {
        0x3108D004: (0, "CGU0_PLLCTL"),
        0x3108D008: (15, "CGU0_STAT"),
    }


def test_conflicting_reset_fact_is_an_error():
    with pytest.raises(ValueError, match="conflicting reset values"):
        parse_reset_tables(
            "0x3108D008 CGU0_STAT Status 0x0000000F\n"
            "0x3108D008 CGU0_STAT Status 0x00000005\n"
        )


def test_a_second_register_at_one_address_keeps_the_first():
    # HWR Rev 1.0 lists the write-side PKTE0_OUTBUF_DECR at the address of the readable
    # PKTE0_OUTBUF_CNT; pdftotext from xpdf keeps both rows on one line each.
    text = (
        "0x310CD114 PKTE0_OUTBUF_CNT PKTE0 Packet Engine Output Buffer Count Register    0x00000000\n"
        "0x310CD114 PKTE0_OUTBUF_DECR PKTE0 Packet Engine Output Buffer Count Decrement  0x0000001F\n"
    )
    assert parse_reset_tables(text) == {0x310CD114: (0, "PKTE0_OUTBUF_CNT")}


def test_missing_reset_table_is_an_error():
    with pytest.raises(ValueError, match="no ADSP-2156x"):
        parse_reset_tables("not a reset register extraction")
