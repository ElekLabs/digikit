"""core/rows support tools: the card overlay applier and the anchor rules of the framework."""

import os
import tomllib

from support.paths import REPO_ROOT

ROOT = str(REPO_ROOT)

from tools.emulator import card_overlay  # noqa: E402
from tools.patchkit import pk_device, port_resolve  # noqa: E402


def record(sector: int, mask_bytes: dict[int, int]) -> bytes:
    mask = bytearray(64)
    for i in mask_bytes:
        mask[i >> 3] |= 1 << (i & 7)
    body = bytearray(512)
    for i, v in mask_bytes.items():
        body[i] = v
    return sector.to_bytes(8, "little") + bytes(mask) + bytes(body)


def test_overlay_writes_only_the_masked_bytes(tmp_path):
    base = tmp_path / "base.img"
    base.write_bytes(bytes([0xAA]) * 512 * 4)
    ov = tmp_path / "o"
    # sector 2: bytes 0 and 511 written (511 as zero: a written zero must mask the base byte)
    ov.write_bytes(record(2, {0: 0x11, 511: 0x00}))
    out = tmp_path / "out.img"
    assert card_overlay.apply(base, ov, out) == 1
    img = out.read_bytes()
    assert img[2 * 512] == 0x11 and img[2 * 512 + 511] == 0x00
    assert img[2 * 512 + 1] == 0xAA and img[0] == 0xAA and img[3 * 512] == 0xAA
    assert base.read_bytes() == bytes([0xAA]) * 512 * 4, "the base image is untouched"


def test_sound_types_joins_adjacent_sectors():
    head = card_overlay.SOUND_HEAD
    rec = bytearray(0x455)
    rec[: len(head)] = head
    rec[card_overlay.SOUND_TYPE_AT] = 7
    # the record starts 100 bytes before the end of sector 5, so the type byte is in sector 6
    blob = bytearray(1024)
    blob[412:512] = rec[:100]
    blob[512:] = (bytes(rec) + bytes(1024))[100:612]
    full = b"".join(
        (s).to_bytes(8, "little")
        + bytes([0xFF] * 64)
        + bytes(blob[(s - 5) * 512 : (s - 4) * 512])
        for s in (5, 6)
    )
    assert card_overlay.sound_types(full) == {7: 1}
    # a gap between the sectors breaks the run: the type byte is not found
    gap = full[:584] + (8).to_bytes(8, "little") + full[592:]
    assert card_overlay.sound_types(gap) == {}


def test_framework_rules_are_declared_once_per_site():
    with open(
        os.path.join(ROOT, "image-modules", "points", "machines", "module.toml"),
        "rb",
    ) as f:
        m = tomllib.load(f)
    # A rule in ports/fw/ may be shared, but its site must occur only once per device.
    machine_rules = [
        r
        for r in port_resolve.fw_rules(REPO_ROOT)
        if r["name"].startswith(("cf.machine.", "dsp.machine."))
    ]
    devices = pk_device.load_all()
    rules = set()
    for r in machine_rules:
        for dev in r.get("devices") or devices:
            key = (r["name"], dev)
            assert key not in rules, key
            rules.add(key)
    # every anchor a hook, write, section or extern of the manifest names has a rule for each device it is active on
    targets = tuple(fw.split("-", 1)[0] for fw in m["targets"])
    named = set()
    for op in m["op"]:
        named.add((op.get("anchor"), tuple(op.get("devices", targets))))
    for sec in m["section"]:
        named.add((sec["from"]["anchor"], tuple(sec.get("devices", targets))))
    for obj in m["object"]:
        for anchor in obj.get("externs", {}).values():
            if anchor.startswith(("cf.", "dsp.")):
                named.add((anchor, tuple(obj.get("devices", targets))))
    for name, devs in named:
        if name:
            for dev in devs:
                assert (name, dev) in rules, (name, dev)
    for r in machine_rules:
        if r["kind"] == "cf_func":
            assert len(r["hash"]) == 32 and r["len"] > 0
