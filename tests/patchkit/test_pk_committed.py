"""`pk_committed`: the gallery and `[uses]` counts from committed rows (the output of `patchkit records --json`)."""

from tools.patchkit import pk_committed

PARAM = 0x80000000


def device(knobs, pages):
    return {
        "commit": {"size": 224},
        "records": {
            "counts": {"machine_rows": 1, "param_rows": 2, "page_rows": len(pages)},
            "machines": [
                {
                    "type": 7,
                    "clone_of": 0,
                    "own_dsp": 1,
                    "level": 10,
                    "knobs": knobs,
                    "name": "Tone",
                    "short": "TN",
                }
            ],
            "params": [
                {
                    "slot": 1,
                    "template": 204,
                    "min": 0,
                    "max": 100,
                    "default": 5,
                    "name": "Cut off",
                    "short": "CUT",
                },
                {
                    "slot": 2,
                    "template": 204,
                    "min": 0,
                    "max": 9,
                    "default": 1,
                    "name": "Cut off",
                    "short": "CT2",
                },
            ],
            "pages": pages,
            "effects": [],
        },
    }


def test_knobs_follow_the_codes_and_a_page_device_reads_its_page():
    machine_row = device([0xCA, PARAM, PARAM | 1, 0, 0, 0, 0, 0], [])
    page_row = device(
        [0] * 8,
        [
            {
                "type": 7,
                "knobs": [0, PARAM, 0, 0, 0, 0, 0, 0],
                "name": "T",
                "title": "Tone",
            }
        ],
    )
    models = {
        "dt2": {"slot_base": 0, "count": 8, "to": "machine"},
        "dn2": {"slot_base": 25, "count": 8, "to": "page"},
    }
    g = pk_committed.gallery(
        "a/tone", ["tone"], {"dt2": machine_row, "dn2": page_row}, models
    )
    assert [p["id"] for p in g["params"]["dt2"]] == [
        "cut_off_1",
        "cut_off_2",
    ]  # equal names get the slot
    dt2 = g["machines"][0]["devices"]["dt2"]
    assert dt2["knobs"][:3] == [
        {"slot": 0, "stock": 0xCA},
        {"slot": 1, "param": "cut_off_1"},
        {"slot": 2, "param": "cut_off_2"},
    ]
    assert len(dt2["knobs"]) == 8 and dt2["page"] is None
    dn2 = g["machines"][0]["devices"]["dn2"]
    assert dn2["knobs"] == [
        None,
        {"slot": 26, "param": "cut_off_1"},
    ]  # trailing empty knobs dropped
    assert dn2["page"] == {"name": "T", "title": "Tone"}


def test_uses_is_one_table_per_device():
    a, b = device([0] * 8, []), device([0] * 8, [])
    b["records"]["counts"] = {"machine_rows": 1, "param_rows": 9, "page_rows": 2}
    assert pk_committed.uses({"dt2": a, "dn2": b}) == {
        "dt2": {
            "machine_rows": a["records"]["counts"]["machine_rows"],
            "param_rows": a["records"]["counts"]["param_rows"],
            "page_rows": a["records"]["counts"]["page_rows"],
            "record_bytes": 224,
        },
        "dn2": {
            "machine_rows": 1,
            "param_rows": 9,
            "page_rows": 2,
            "record_bytes": 224,
        },
    }
