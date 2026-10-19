"""Output of each PLAITS model of the DSP object, as SHA-256 digests of its output words (regression vectors).

H2 (out/patch/plaits-va-48k-1.17.syx, sources at 40ea03d) went to the owner with model VA only. Every later image
must render model 0 bit for bit as H2 did, given the same inputs. `render(mode, compiled)` plays fixed scenarios
through the compiled object on the concrete core (tests/support/plaits_rig.py) or through the host build of the same
C (tests/support/plaits_host.py) and returns one digest per scenario; tests/plaits/vectors/model0_h2.json holds the
digests recorded from the H2 sources, and tests/patchkit/test_plaits_model0_h2.py compares. The later models use the
same scenarios with their MODEL value: `vectors(model)` is the file of each, recorded from the sources of the task that
first enabled it (tests/patchkit/test_plaits_model_digests.py).

Task 9b changed what a note-on does: the engine runs on from note to note and is initialised only at the voice's first
note-on and at a model change (modules/machines/plaits/src/dsp/plaits.c note_on). The scenarios play one voice from
scenario to scenario and retrigger it, so every model's digests were re-recorded (PLAITS_RECORD_9B=1, since retired);
the digests of the per-note initialisation before it are kept in each file under PER_NOTE_INIT. Two checks prove that
only the retrigger behaviour changed: scenario set0 (the voice's first note after init) is unchanged, and the
per-note initialisation replayed on the current sources (a voice cleared at every note-on: `host_block_fresh`,
`rig_block_fresh`) still gives every digest of PER_NOTE_INIT.

Task 10b added Plaits' output stage (per-model OUT and AUX gains, the limiter, the DAC step's clip; the output gain
0.601 / 0.8) and the accent from the note-on's velocity (plaits.c), which changes every model's output; the digests
pinned before it are kept under PRE_OUTPUT_STAGE (PLAITS_RECORD_10B=1, `record_10b`). Two checks (host) prove what
changed: every scenario equals upstream rendered continuously with its output stage (the oracle,
support.plaits_oracle.reference: `reference_digests`), and the host build without the output stage
(PL_OUTPUT_STAGE_NEUTRAL: no post, gain 0.601, accent 0.8) still gives every PRE_OUTPUT_STAGE digest."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from collections.abc import Callable, Iterator
from pathlib import Path
from typing import Any

import numpy as np

from support.plaits_host import Port, frame

ROOT = Path(__file__).resolve().parents[2]
VECTORS = ROOT / "tests" / "plaits" / "vectors" / "model0_h2.json"
# model -> the image whose sources recorded its digests: 1 at H3 (5931e47), 2..4 at H3b (Task 8 of the all-engines plan),
# 5 and 6 at H3c (Task 9), 7 at H3e (Task 10), 12 and 21 at H3f (Task 10b)
RECORDED_AT = {
    1: "h3",
    2: "h3b",
    3: "h3b",
    4: "h3b",
    5: "h3c",
    6: "h3c",
    7: "h3e",
    12: "h3f",
    21: "h3f",
}
# the key of each file's digests from before Task 9b (a note-on re-initialised the engine)
PER_NOTE_INIT = "per_note_init"
# the key of the digests pinned before Task 10b's output stage
PRE_OUTPUT_STAGE = "pre_output_stage"
RECORD_10B = os.environ.get("PLAITS_RECORD_10B") == "1"


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


def host_block_fresh(
    port: Port, mode: int, model: int = 0
) -> Callable[[int, dict[str, int], int], Any]:
    """`host_block` with the voice cleared at every note-on: the wrapper's behaviour before Task 9b."""
    v = [port.voice()]

    def block(note: int, slots: dict[str, int], trig: int) -> Any:
        if trig:
            v[0] = port.voice()
        return port.render(v[0], frame(note, model=model, **slots), trig, mode)

    return block


def rig_block_fresh(
    rig: Any, model: int = 0
) -> Callable[[int, dict[str, int], int], Any]:
    """`rig_block` with track 0's voice state (plaits_voices[0] in the object's L2 data) zeroed at every note-on, as
    at load: the wrapper's behaviour before Task 9b."""
    inner = rig_block(rig, model)

    def block(note: int, slots: dict[str, int], trig: int) -> Any:
        if trig:
            rig.clear_voice(0)
        return inner(note, slots, trig)

    return block


def check(
    path: Path,
    key: str,
    got: dict[str, str],
    neutral: dict[str, str] | None = None,
    reference: dict[str, str] | None = None,
) -> None:
    """Compare (or with PLAITS_RECORD_10B=1, re-record) the digests `got` under `key` of the file at `path`.
    `reference`: the digests of upstream rendered continuously with its output stage, which must equal `got`;
    `neutral`: the scenarios on the host build without the output stage, which must give the digests pinned before
    Task 10b (the file's own before the re-record, PRE_OUTPUT_STAGE after it)."""
    if reference is not None:
        assert got == reference, (path.name, key, "upstream with its output stage")
    data = json.loads(path.read_text())
    if neutral is not None:
        old = data.get(PRE_OUTPUT_STAGE, data)
        assert neutral == old[key], (path.name, key, "without the output stage")
    if RECORD_10B:
        record_10b(path, data, key, got)
        data = json.loads(path.read_text())
    assert key in data, f"{path.name} has no {key}"
    assert data[key] == got, (path.name, key)


def _rev() -> str:
    return subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        capture_output=True,
        text=True,
        cwd=ROOT,
    ).stdout.strip()


def record_10b(path: Path, data: dict[str, Any], key: str, got: dict[str, str]) -> None:
    """Task 10b's re-record: the pinned digests from before move under PRE_OUTPUT_STAGE once (with their
    recorded_from), the new ones replace them, recorded_from is the current commit."""
    if PRE_OUTPUT_STAGE not in data:
        old = {k: v for k, v in data.items() if k.startswith(("host-", "core-"))}
        old["recorded_from"] = data.get("recorded_from", "")
        data[PRE_OUTPUT_STAGE] = old
    data["recorded_from"] = _rev()
    data[key] = got
    path.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")


def scenario_blocks(
    mode: int, model: int
) -> tuple[list[dict[str, int]], list[tuple[str, int]]]:
    """All scenarios of `scenarios(mode)` as one control stream of `model` (support.plaits_streams blocks, velocity
    100 as frame()) and (name, block count) per scenario."""
    blocks: list[dict[str, int]] = []
    names: list[tuple[str, int]] = []
    for name, steps in scenarios(mode):
        for note, slots, trig in steps:
            blocks.append(
                {
                    "note": note,
                    "tune": slots.get("tune", 0x4000),
                    "model": model,
                    "trig": trig,
                    "harmonics": slots.get("harmonics", 0x4000),
                    "timbre": slots.get("timbre", 0x4000),
                    "morph": slots.get("morph", 0x4000),
                    "mix": slots.get("mix", 0),
                    "velocity": 100,
                }
            )
        names.append((name, len(steps)))
    return blocks, names


def reference_digests(oracle: Any, port: Port, mode: int, model: int) -> dict[str, str]:
    """The digests of the scenarios rendered by upstream (support.plaits_oracle.reference, one voice through all of
    them, as `render` plays them), at 96 kHz through the wrapper's half-band (mode 0)."""
    from support.plaits_oracle import reference, up2

    blocks, names = scenario_blocks(mode, model)
    x = reference(oracle, blocks)
    if not mode:
        x = up2(port, x, blocks)
    n = 32 if mode else 64
    out, at = {}, 0
    for name, count in names:
        out[name] = digest(x[at : at + n * count])
        at += n * count
    return out


def rig_block(rig: Any, model: int = 0) -> Callable[[int, dict[str, int], int], Any]:
    rig.active(0)

    def block(note: int, slots: dict[str, int], trig: int) -> Any:
        s = dict(slots)
        tune = s.pop("tune", 0x4000)
        rig.controls(0, note, tune=tune, model=model << 8, **s)
        rig.trig(0, bool(trig))
        return np.array(rig.block(0)["samples"], dtype=np.float32)

    return block
