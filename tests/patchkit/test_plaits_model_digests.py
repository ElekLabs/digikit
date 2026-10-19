"""Recipe (g) of the all-engines plan: every enabled PLAITS model after VA renders bit for bit as when its digests were
last pinned (model 0 has its own file and test, tests/patchkit/test_plaits_model0_h2.py).

tests/plaits/vectors/model<N>_<image>.json (tests/support/plaits_vectors.py `vectors`) holds SHA-256 digests of
model N's output words for the fixed scenarios of tests/support/plaits_vectors.py: the compiled DSP object of each
variant on the concrete core and the host build of the same C. Model 1 was first recorded from the H3 sources
(5931e47), models 2..4 from H3b (Task 8), 5 and 6 from H3c (Task 9), 7 from H3e (Task 10), 12 and 21 from H3f
(Task 10b). History kept in each file: `per_note_init` (before Task 9b, a note-on re-initialised the engine) and
`pre_output_stage` (before Task 10b's output stage and accent).

The host tests also prove what the pinned digests are: every scenario equals upstream rendered continuously with its
output stage (the oracle), and the host build without the output stage (PL_OUTPUT_STAGE_NEUTRAL) still gives the
digests pinned before Task 10b (plaits_vectors.check). PLAITS_RECORD_MODEL=N records model N's file for a newly
enabled model (only from the sources of the task that first enables it); PLAITS_RECORD_10B=1 re-records the pinned
digests of the models already recorded (Task 10b)."""

from __future__ import annotations

import ctypes
import json
import os

import pytest
from support import plaits_vectors as pv
from support.plaits_host import FP, Port, build_port, oracle_missing, stmlib_root
from support.plaits_oracle import Oracle, build

from tools.patchkit import pk_toolchains

MODELS = sorted(pv.RECORDED_AT)
RECORD = os.environ.get("PLAITS_RECORD_MODEL", "")


def _check(
    model: int,
    key: str,
    got: dict[str, str],
    neutral: dict[str, str] | None = None,
    reference: dict[str, str] | None = None,
) -> None:
    path = pv.vectors(model)
    if str(model) == RECORD:
        # a newly enabled model: its first digests (they must still equal upstream with its output stage)
        if reference is not None:
            assert got == reference, (model, key)
        data = json.loads(path.read_text()) if path.exists() else {}
        data.setdefault("recorded_from", pv._rev())
        data[key] = got
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
        return
    pv.check(path, key, got, neutral if model in range(8) else None, reference)


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    p = build_port(tmp_path_factory.mktemp("plaits-digests"))
    p.lib.hb_init.argtypes = [ctypes.c_void_p]
    p.lib.hb_up2.argtypes = [ctypes.c_void_p, FP, FP, ctypes.c_int]
    return p


@pytest.fixture(scope="module")
def neutral(tmp_path_factory: pytest.TempPathFactory) -> Port:
    """The host build without Task 10b's output stage and accent (PL_OUTPUT_STAGE_NEUTRAL)."""
    return build_port(
        tmp_path_factory.mktemp("plaits-neutral"), ("PL_OUTPUT_STAGE_NEUTRAL",)
    )


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    if stmlib_root() is None:
        oracle_missing("upstream stmlib not found")
    return build(tmp_path_factory.mktemp("run-oracle"))


@pytest.mark.parametrize("model", MODELS)
@pytest.mark.parametrize("mode", [1, 0])
def test_model_host_wrapper_unchanged(
    port: Port, neutral: Port, oracle: Oracle, model: int, mode: int
) -> None:
    _check(
        model,
        f"host-mode{mode}",
        pv.render(mode, pv.host_block(port, mode, model)),
        pv.render(mode, pv.host_block(neutral, mode, model)),
        pv.reference_digests(oracle, port, mode, model),
    )


@pytest.mark.slow
@pytest.mark.skipif(pk_toolchains.selache_dir() is None, reason="selache not built")
@pytest.mark.parametrize("model", MODELS)
@pytest.mark.parametrize("mode", [1, 0])
def test_model_compiled_core_unchanged(model: int, mode: int) -> None:
    from support.plaits_rig import PlaitsRig

    _check(
        model, f"core-mode{mode}", pv.render(mode, pv.rig_block(PlaitsRig(mode), model))
    )
