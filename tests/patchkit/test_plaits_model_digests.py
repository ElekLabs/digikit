"""Recipe (g) of the all-engines plan: every enabled PLAITS model after VA renders bit for bit as when it was first
enabled (model 0 has its own file and test, tests/patchkit/test_plaits_model0_h2.py).

tests/plaits/vectors/model<N>_<image>.json (tests/support/plaits_vectors.py `vectors`) holds SHA-256 digests of
model N's output words for the fixed scenarios of tests/support/plaits_vectors.py: the compiled DSP object of each
variant on the concrete core and the host build of the same C. Model 1 was recorded from the H3 sources (5931e47),
models 2..4 from the H3b sources (Task 8), models 5 and 6 from the H3c sources (Task 9). These tests render the same scenarios from the current sources and
require equal digests. PLAITS_RECORD_MODEL=N rewrites model N's file, and only that one: run it only from the
sources of the task that first enables model N."""

from __future__ import annotations

import json
import os
import subprocess

import pytest
from support import plaits_vectors as pv
from support.plaits_host import Port, build_port

from tools.patchkit import pk_toolchains

MODELS = sorted(pv.RECORDED_AT)
RECORD = os.environ.get("PLAITS_RECORD_MODEL", "")


def _check(model: int, key: str, got: dict[str, str]) -> None:
    path = pv.vectors(model)
    data = json.loads(path.read_text()) if path.exists() else {}
    if str(model) == RECORD:
        rev = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            capture_output=True,
            text=True,
            cwd=pv.ROOT,
        ).stdout.strip()
        data.setdefault("recorded_from", rev)
        data[key] = got
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
        return
    assert key in data, (
        f"{path.name} has no {key}: record it with PLAITS_RECORD_MODEL={model}"
    )
    assert data[key] == got, (model, key)


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(tmp_path_factory.mktemp("plaits-digests"))


@pytest.mark.parametrize("model", MODELS)
@pytest.mark.parametrize("mode", [1, 0])
def test_model_host_wrapper_unchanged(port: Port, model: int, mode: int) -> None:
    _check(model, f"host-mode{mode}", pv.render(mode, pv.host_block(port, mode, model)))


@pytest.mark.slow
@pytest.mark.skipif(pk_toolchains.selache_dir() is None, reason="selache not built")
@pytest.mark.parametrize("model", MODELS)
@pytest.mark.parametrize("mode", [1, 0])
def test_model_compiled_core_unchanged(model: int, mode: int) -> None:
    from support.plaits_rig import PlaitsRig

    _check(
        model, f"core-mode{mode}", pv.render(mode, pv.rig_block(PlaitsRig(mode), model))
    )
