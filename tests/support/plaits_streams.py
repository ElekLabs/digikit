"""Control streams for one PLAITS voice, block by block (Task 9b): note-ons at different pitches with knob changes and
no model change (`retrigger_stream`), and model changes at note-ons and between them (`switch_stream`). Each block is
the row values of the controls plus the trigger latch. Used by tests/plaits/test_continuous_host.py (against upstream
rendered continuously) and tests/patchkit/test_plaits_module.py (the compiled core against the host)."""

from __future__ import annotations

from typing import Any

import numpy as np

from support.plaits_host import frame

Block = dict[str, int]


def retrigger_stream(model: int, seed: int, blocks: int = 240) -> list[Block]:
    """Note-ons at different pitches, 1 to 23 blocks apart (repeated notes, 1/32-step bursts and held notes), each with
    its own knob p-locks, knobs ramping within some notes (as an LFO would) and TUNE changes; MODEL fixed."""
    rng = np.random.default_rng(seed)
    out: list[Block] = []
    gaps = [1, 1, 2, 3, 5, 8, 13, 23, 4, 1, 6, 10]
    notes = [60, 48, 67, 36, 72, 55, 84, 43, 60, 60, 96, 24]
    nxt, k = 0, 0
    knobs = {"harmonics": 0x4000, "timbre": 0x4000, "morph": 0x4000, "mix": 0}
    tune, note, ramp = 0x4000, 60, 0
    for b in range(blocks):
        trig = int(b == nxt)
        if trig:
            note = notes[k % len(notes)]
            for name in knobs:
                knobs[name] = int(rng.integers(0, 0x7F01))
            if k % 3 == 2:
                tune = int(rng.integers(0x3800, 0x4800))
            ramp = int(rng.integers(-0x300, 0x300)) if k % 2 else 0
            nxt = b + gaps[k % len(gaps)]
            k += 1
        elif ramp:
            for name in ("timbre", "morph"):
                knobs[name] = int(np.clip(knobs[name] + ramp, 0, 0x7F00))
        out.append({"note": note, "tune": tune, "model": model, "trig": trig, **knobs})
    return out


def switch_stream() -> list[Block]:
    """Model changes on one voice, 9 blocks per note: note-ons at blocks 0 and 5 of each (a retrigger of the same model
    at 5); at block 8 the MODEL row already shows the next note's model without a note-on (the running engine goes
    on); consecutive notes of one model (6 6, 5 5, 7 7, 0 0) do not re-initialise the engine at the note-on (SPEECH
    reads it as Plaits' trigger)."""
    seq = [0, 5, 6, 6, 4, 7, 6, 5, 5, 7, 7, 2, 3, 1, 6, 0, 0]
    out: list[Block] = []
    for k, model in enumerate(seq):
        for b in range(9):
            out.append(
                {
                    "note": 45 + 3 * k + (b >= 5),
                    "tune": 0x4000,
                    "model": seq[k + 1] if b == 8 and k + 1 < len(seq) else model,
                    "trig": int(b in (0, 5)),
                    "harmonics": 0x2000 + 0x500 * k,
                    "timbre": 0x7F00 - 0x400 * k,
                    "morph": 0x4400 if b < 5 else 0x3C00,
                    "mix": 0x2000 * (k % 4),
                }
            )
    return out


def as_frame(s: Block) -> Any:
    return frame(
        s["note"],
        tune=s["tune"],
        model=s["model"],
        harmonics=s["harmonics"],
        timbre=s["timbre"],
        morph=s["morph"],
        mix=s["mix"],
    )
