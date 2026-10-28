"""The frequency shifter on a Digitakt II (scenario fx-freqshift-dt2, in the module's tests/scenarios/): the page and its knobs, the sine moves by the shift in both directions with the image 40 dB down,
MIX 0 leaves the PCM as it was, Save Kit and Load Kit give SHIFT back (master route); the send route (amount on track 1, return = amount^2 x MIX x the shifted track, compared with the float model) and an amount on another track. One file per group, so that verify runs them in parallel."""

import pytest
from scen_cases import cases, run


@pytest.mark.parametrize(("scen", "image"), cases(("fxshift",), "dt2"))
def test_scenario(scen, image, tmp_path_factory, request):
    run(scen, image, tmp_path_factory, request)
