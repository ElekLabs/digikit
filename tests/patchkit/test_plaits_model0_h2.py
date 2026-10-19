"""Model 0 (VA) renders as in the H2 image (sources at 40ea03d), apart from Task 9b's deliberate change at a retrigger
and Task 10b's output stage.

tests/plaits/vectors/model0_h2.json holds SHA-256 digests of model 0's output words for fixed scenarios
(tests/support/plaits_vectors.py): the compiled DSP object of each variant on the concrete core, and the host build of
the same C. The H2 digests (a note-on re-initialised the engine) are kept under `per_note_init`, the digests of Task
9b (the engine runs on from note to note) under `pre_output_stage`; the pinned digests were re-recorded at Task 10b
(Plaits' output stage: VA's gain 0.8 with the output gain 0.601 / 0.8, the clip). The host test also requires that
the scenarios equal upstream with its output stage and that the build without it gives the Task 9b digests
(plaits_vectors.check). PLAITS_RECORD_10B=1 re-records the pinned digests."""

from __future__ import annotations

import ctypes

import pytest
from support import plaits_vectors as pv
from support.plaits_host import FP, Port, build_port, oracle_missing, stmlib_root
from support.plaits_oracle import Oracle, build

from tools.patchkit import pk_toolchains


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    p = build_port(tmp_path_factory.mktemp("plaits-h2"))
    p.lib.hb_init.argtypes = [ctypes.c_void_p]
    p.lib.hb_up2.argtypes = [ctypes.c_void_p, FP, FP, ctypes.c_int]
    return p


@pytest.fixture(scope="module")
def neutral(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(
        tmp_path_factory.mktemp("plaits-h2-neutral"), ("PL_OUTPUT_STAGE_NEUTRAL",)
    )


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    if stmlib_root() is None:
        oracle_missing("upstream stmlib not found")
    return build(tmp_path_factory.mktemp("run-oracle"))


@pytest.mark.parametrize("mode", [1, 0])
def test_model0_host_wrapper_unchanged_since_h2(
    port: Port, neutral: Port, oracle: Oracle, mode: int
) -> None:
    pv.check(
        pv.VECTORS,
        f"host-mode{mode}",
        pv.render(mode, pv.host_block(port, mode)),
        pv.render(mode, pv.host_block(neutral, mode)),
        pv.reference_digests(oracle, port, mode, 0),
    )


@pytest.mark.skipif(pk_toolchains.selache_dir() is None, reason="selache not built")
@pytest.mark.parametrize("mode", [1, 0])
def test_model0_compiled_core_unchanged_since_h2(mode: int) -> None:
    from support.plaits_rig import PlaitsRig

    pv.check(
        pv.VECTORS, f"core-mode{mode}", pv.render(mode, pv.rig_block(PlaitsRig(mode)))
    )
