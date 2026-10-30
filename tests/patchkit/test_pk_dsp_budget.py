"""pk_dsp_budget: the arithmetic of the DSP budget gate (no emulator)."""

from tools.patchkit import pk_dsp_budget as b


def tiers(audio_s, instructions, idle):
    return {"audio_s": audio_s, "instructions": instructions, "idle_skip": idle}


def test_busy_per_block_is_executed_minus_idle_over_blocks():
    # 2 s of audio = 3000 blocks; 1,000,000 executed, 400,000 idle-skipped -> 200 busy per block
    assert (
        b.busy_per_block(tiers(1.0, 100, 50), tiers(3.0, 1_000_100, 400_050)) == 200.0
    )
    assert b.busy_per_block(tiers(1.0, 0, 0), tiers(1.0, 5, 0)) is None


def test_stock_load_reproduces_the_report():
    # DN2 stock held note: 334,529 busy per block -> 50 % emulator, 57 % nominal, 67 % high
    dn2, dt2 = b.FACTORS["dn2"]["stock_busy"], b.FACTORS["dt2"]["stock_busy"]
    assert round(b.load_pct(dn2, "dn2", "nominal")) == 57
    assert round(b.load_pct(dn2, "dn2", "high")) == 67
    assert round(b.load_pct(dt2, "dt2", "nominal")) == 35
    assert round(b.load_pct(dt2, "dt2", "high")) == 44


def test_added_load_and_gate():
    r = b.judge("dn2", 334_725.0, 334_800.0, 334_529.0)
    assert round(r["added_instructions_per_block"]) == 196
    assert abs(r["added_load_high_pct"] - 196 * 1.326075 / 6666.6667) < 1e-9
    assert r["ok"]
    assert not b.judge("dn2", 334_725.0, 334_800.0, 334_529.0, budget_pct=10)["ok"]
    assert not b.judge("dt2", 600_000.0, 600_000.0, 208_640.0)["ok"]  # 130 % high
