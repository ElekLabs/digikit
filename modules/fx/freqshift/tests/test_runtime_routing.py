"""The module's device acceptance retains its artifact and oracle ownership."""

import pytest

from tools.devtools import verify, verify_route
from tools.patchkit import modroots


@pytest.mark.parametrize(
    "path, owners",
    [
        (
            "modules/fx/freqshift/module.toml",
            ["../../modules/fx/freqshift/tests/freqshift.device.test.ts"],
        ),
        (
            "tools/patchkit/pk_digikit_fxshift.py",
            ["../../modules/fx/freqshift/tests/freqshift.device.test.ts"],
        ),
        (
            "modules/fx/freqshift/tests/freqshift_ref.py",
            ["../../modules/fx/freqshift/tests/freqshift.device.test.ts"],
        ),
    ],
)
def test_remaining_fx_sources_retain_runtime_owners(path, owners):
    assert verify_route.acceptance_files("runtime-fixture", [path]) == owners
    assert "runtime-fixture" in verify.steps_for([path])


def test_remaining_fx_dynamic_model_keeps_preparation_and_production_owners():
    model = "modules/fx/freqshift/tests/freqshift_ref.py"
    dependencies = verify_route.acceptance_python("runtime-fixture")
    assert model in dependencies
    assert "tools/patchkit/pk_digikit_fxshift.py" in dependencies
    assert verify_route.acceptance_files("module-load", [model]) == [
        "modules/freqshift.test.ts"
    ]
    assert (
        verify.runtime_pack_aliases(
            [
                "runtime/fx-kit-persist.test.ts",
                "../../modules/fx/freqshift/tests/freqshift.device.test.ts",
            ]
        )
        == {}
    )


def test_module_device_cli_packs_and_normalizes_registered_file():
    file = "modules/fx/freqshift/tests/freqshift.device.test.ts"
    _, units = verify.named_units([], [file])
    selected = units["runtime-fixture"]
    assert selected == ["../../" + file]
    assert verify.runtime_pack_modules(selected) == ["test/sine", "digikit/freqshift"]
    assert set(selected) <= verify_route.runtime_plusdrive_files()
    assert next(
        record
        for record in modroots.runtime_acceptance()
        if record.owner == "digikit/freqshift"
    ).files == (file,)
