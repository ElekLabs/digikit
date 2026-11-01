"""Host adapters retain BASIC FX's independent fit, range and PCM oracles."""

import json
import struct

import pytest

from tools.patchkit import pk_digikit_fx as fx


@pytest.mark.parametrize("overlap", [False, True])
def test_chain_report_uses_overflow_need_and_complete_empty_inventory(
    monkeypatch, overlap
):
    caps = {"FX_SLOTS": {"limit": 1, "used": 1, "largest": 0}}
    needs = {b"gain": {"FX_SLOTS": 1}, b"overflow": {"FX_SLOTS": 1, "DSP_AREA": 8}}
    monkeypatch.setattr(fx.dkneeds, "needs_of", needs.__getitem__)
    requests = []

    def fit(capacity, requested, empty=False):
        requests.append((capacity, requested, empty))
        return {"empty": empty}

    monkeypatch.setattr(fx.dkneeds, "fit", fit)
    result = fx.chain_report(
        {
            "capacity": caps,
            "modules": [
                {"dsp_off": 0, "dsp_len": 8},
                {"dsp_off": 4 if overlap else 8, "dsp_len": 8},
                {"dsp_off": 0, "dsp_len": 0},
            ],
        },
        [b"gain", b"overflow"],
    )
    assert result == {
        "ranges_ok": not overlap,
        "live": {"empty": False},
        "empty": {"empty": True},
    }
    assert requests == [
        (caps, [needs[b"overflow"]], False),
        (caps, [needs[b"gain"], needs[b"overflow"]], True),
    ]


def test_pcm_peak_cli_reads_float_pcm(tmp_path, capsys):
    samples = struct.pack("<4f", 0, 0.125, -0.75, 0.5)
    body = b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 3, 2, 48000, 384000, 8, 32)
    body += b"data" + struct.pack("<I", len(samples)) + samples
    wav = tmp_path / "sine.wav"
    wav.write_bytes(b"RIFF" + struct.pack("<I", len(body)) + body)
    assert fx.main(["pcm-peak", str(wav)]) == 0
    assert json.loads(capsys.readouterr().out) == {"peak": 0.75}
