"""The frequency shifter on a Digitone II (scenario fx-freqshift-dn2, in the module's tests/scenarios/): the page and its knobs, the sine moves by the shift in both directions with the image 40 dB down,
the send default falls back to the master (no send cells), MIX 0 leaves the PCM as it was, Save Kit and Load Kit give SHIFT back. One file per group, so that verify runs them in parallel."""

import pytest
from scen_cases import cases, run


@pytest.mark.parametrize(("scen", "image"), cases(("fxshift",), "dn2"))
def test_scenario(scen, image, tmp_path_factory, request):
    run(scen, image, tmp_path_factory, request)
