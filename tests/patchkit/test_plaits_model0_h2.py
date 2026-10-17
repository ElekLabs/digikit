"""H3 is a strict superset of H2: model 0 (VA) renders bit for bit as in the H2 image (sources at 40ea03d).

tests/plaits/vectors/model0_h2.json holds SHA-256 digests of model 0's output words for fixed scenarios
(tests/support/plaits_vectors.py), recorded from the H2 sources: the compiled DSP object of each variant on the
concrete core, and the host build of the same C. These tests render the same scenarios from the current sources and
require equal digests. PLAITS_RECORD_MODEL0_H2=1 rewrites the file (only ever from the H2 sources)."""

from __future__ import annotations

import json
import os
import subprocess

import pytest
from support import plaits_vectors as pv
from support.plaits_host import Port, build_port

from tools.patchkit import pk_toolchains

RECORD = os.environ.get("PLAITS_RECORD_MODEL0_H2") == "1"


def _stored() -> dict:
    return json.loads(pv.VECTORS.read_text()) if pv.VECTORS.exists() else {}


def _check(key: str, got: dict[str, str]) -> None:
    data = _stored()
    if RECORD:
        rev = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            capture_output=True,
            text=True,
            cwd=pv.ROOT,
        ).stdout.strip()
        data.setdefault("recorded_from", rev)
        data[key] = got
        pv.VECTORS.parent.mkdir(parents=True, exist_ok=True)
        pv.VECTORS.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
        return
    assert data.get(key) == got, key


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(tmp_path_factory.mktemp("plaits-h2"))


@pytest.mark.parametrize("mode", [1, 0])
def test_model0_host_wrapper_unchanged_since_h2(port: Port, mode: int) -> None:
    _check(f"host-mode{mode}", pv.render(mode, pv.host_block(port, mode)))


@pytest.mark.skipif(pk_toolchains.selache_dir() is None, reason="selache not built")
@pytest.mark.parametrize("mode", [1, 0])
def test_model0_compiled_core_unchanged_since_h2(mode: int) -> None:
    from support.plaits_rig import PlaitsRig

    _check(f"core-mode{mode}", pv.render(mode, pv.rig_block(PlaitsRig(mode))))
