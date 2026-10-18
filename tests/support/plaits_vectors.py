"""Output of each PLAITS model of the DSP object, as SHA-256 digests of its output words (regression vectors).

H2 (out/patch/plaits-va-48k-1.17.syx, sources at 40ea03d) went to the owner with model VA only. Every later image
must render model 0 bit for bit as H2 did, given the same inputs. `render(mode, compiled)` plays fixed scenarios
through the compiled object on the concrete core (tests/support/plaits_rig.py) or through the host build of the same
C (tests/support/plaits_host.py) and returns one digest per scenario; tests/plaits/vectors/model0_h2.json holds the
digests recorded from the H2 sources, and tests/patchkit/test_plaits_model0_h2.py compares. The later models use the
same scenarios with their MODEL value: `vectors(model)` is the file of each, recorded from the sources of the task that
first enabled it (tests/patchkit/test_plaits_model_digests.py)."""

from __future__ import annotations

import hashlib
from collections.abc import Callable, Iterator
from pathlib import Path
from typing import Any

import numpy as np

from support.plaits_host import Port, frame

ROOT = Path(__file__).resolve().parents[2]
VECTORS = ROOT / "tests" / "plaits" / "vectors" / "model0_h2.json"
# model -> the image whose sources recorded its digests: 1 at H3 (5931e47), 2..4 at H3b (Task 8 of the all-engines plan),
# 5 and 6 at H3c (Task 9)
RECORDED_AT = {1: "h3", 2: "h3b", 3: "h3b", 4: "h3b", 5: "h3c", 6: "h3c"}


def vectors(model: int) -> Path:
    """The digest file of `model` (model 0: VECTORS)."""
    if model == 0:
        return VECTORS
    return (
        ROOT
        / "tests"
        / "plaits"
        / "vectors"
        / f"model{model}_{RECORDED_AT[model]}.json"
    )


# The settings of test_plaits_module.py's equivalence test (row values); each changes some controls half-way.
SETTINGS: list[dict[str, Any]] = [
    {"note": 60, "first": {}, "second": {"timbre": 0x6000}},
    {
        "note": 48,
        "tune": 0x4180,
        "first": {"harmonics": 0, "timbre": 0x7F00, "morph": 0x2000, "mix": 0x7F00},
        "second": {"harmonics": 0x1000, "morph": 0x3000},
    },
    {
        "note": 72,
        "first": {
            "harmonics": 0x7F00,
            "timbre": 0x1000,
            "morph": 0x7F00,
            "mix": 0x4000,
        },
        "second": {"timbre": 0x7F00, "mix": 0},
    },
    {
        "note": 36,
        "tune": 0x3E00,
        "first": {
            "harmonics": 0x2A00,
            "timbre": 0x6000,
            "morph": 0x5400,
            "mix": 0x2000,
        },
        "second": {"morph": 0x0800, "harmonics": 0x5000},
    },
]


def scenarios(mode: int) -> Iterator[tuple[str, list[tuple[int, dict[str, int], int]]]]:
    """(name, blocks): each block is (note, row values incl. tune, trig) for track 0's even voice (any one model)."""
    blocks = 120 if mode else 24
    for k, s in enumerate(SETTINGS):
        steps = []
        for b in range(blocks):
            slots = {
                "tune": s.get("tune", 0x4000),
                **s["first"],
                **(s["second"] if b >= blocks // 2 else {}),
            }
            steps.append((s["note"], slots, int(b == 0)))
        yield f"set{k}", steps
    ext = []
    for note in (0, 127):
        for tune in (0, 0x4000, 0x7F00):
            for h in (0, 0x7F00):
                for t in (0, 0x7F00):
                    for m in (0, 0x7F00):
                        for b in range(3):
                            ext.append(
                                (
                                    note,
                                    {
                                        "tune": tune,
                                        "harmonics": h,
                                        "timbre": t,
                                        "morph": m,
                                    },
                                    int(b == 0),
                                )
                            )
    yield "extreme", ext
    # a retrigger after the engine ran on, and a note change at a note-on
    rt = [
        (55, {"harmonics": 0x6000, "timbre": 0x3000}, int(b in (0, 11)))
        for b in range(20)
    ]
    rt += [
        (67, {"harmonics": 0x6000, "timbre": 0x3000, "mix": 0x7F00}, int(b == 0))
        for b in range(10)
    ]
    yield "retrigger", rt


def digest(samples: Any) -> str:
    return hashlib.sha256(np.asarray(samples, dtype="<f4").tobytes()).hexdigest()


def render(
    mode: int, block: Callable[[int, dict[str, int], int], Any]
) -> dict[str, str]:
    """One digest per scenario; `block(note, slots, trig)` renders one block (32 or 64 float32 samples)."""
    out: dict[str, str] = {}
    for name, steps in scenarios(mode):
        out[name] = digest(
            np.concatenate([np.asarray(block(*s), dtype=np.float32) for s in steps])
        )
    return out


def host_block(
    port: Port, mode: int, model: int = 0
) -> Callable[[int, dict[str, int], int], Any]:
    v = port.voice()

    def block(note: int, slots: dict[str, int], trig: int) -> Any:
        return port.render(v, frame(note, model=model, **slots), trig, mode)

    return block


def rig_block(rig: Any, model: int = 0) -> Callable[[int, dict[str, int], int], Any]:
    rig.active(0)

    def block(note: int, slots: dict[str, int], trig: int) -> Any:
        s = dict(slots)
        tune = s.pop("tune", 0x4000)
        rig.controls(0, note, tune=tune, model=model << 8, **s)
        rig.trig(0, bool(trig))
        return np.array(rig.block(0)["samples"], dtype=np.float32)

    return block
