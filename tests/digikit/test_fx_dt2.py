"""Remaining run-time FX legs, Digitakt II (tests/digikit/scenarios/fx-dyn-dt2.toml): plain versus dry PCM, full-chain scaling, overflow and fit rules. The single-gain lifecycle and deselect/restart PCM are owned by tests/regression/runtime/fx-reuse.test.ts. Retained Python legs keep their launch-time safety checks."""

import pytest
from scen_cases import cases, run


@pytest.mark.parametrize(("scen", "image"), cases(("fx",), "dt2"))
def test_scenario(scen, image, tmp_path_factory, request):
    run(scen, image, tmp_path_factory, request)
