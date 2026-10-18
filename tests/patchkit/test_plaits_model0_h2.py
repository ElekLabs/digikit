"""Model 0 (VA) renders as in the H2 image (sources at 40ea03d), apart from Task 9b's deliberate change at a retrigger.

tests/plaits/vectors/model0_h2.json holds SHA-256 digests of model 0's output words for fixed scenarios
(tests/support/plaits_vectors.py): the compiled DSP object of each variant on the concrete core, and the host build of
the same C. The H2 digests (a note-on re-initialised the engine) are kept under `per_note_init`; the pinned digests
were re-recorded at Task 9b (the engine runs on from note to note). These tests require the pinned digests, the H2
digest of the first note after init (set0), and every H2 digest when the voice is cleared at each note-on
(plaits_vectors.check). PLAITS_RECORD_MODEL0_H2=1 rewrote the file from the H2 sources; PLAITS_RECORD_9B=1 re-records
the pinned digests."""

from __future__ import annotations

import pytest
from support import plaits_vectors as pv
from support.plaits_host import Port, build_port

from tools.patchkit import pk_toolchains


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(tmp_path_factory.mktemp("plaits-h2"))


@pytest.mark.parametrize("mode", [1, 0])
def test_model0_host_wrapper_unchanged_since_h2(port: Port, mode: int) -> None:
    pv.check(
        pv.VECTORS,
        f"host-mode{mode}",
        pv.render(mode, pv.host_block(port, mode)),
        pv.render(mode, pv.host_block_fresh(port, mode)),
    )


@pytest.mark.skipif(pk_toolchains.selache_dir() is None, reason="selache not built")
@pytest.mark.parametrize("mode", [1, 0])
def test_model0_compiled_core_unchanged_since_h2(mode: int) -> None:
    from support.plaits_rig import PlaitsRig

    pv.check(
        pv.VECTORS,
        f"core-mode{mode}",
        pv.render(mode, pv.rig_block(PlaitsRig(mode))),
        pv.render(mode, pv.rig_block_fresh(PlaitsRig(mode))),
    )
