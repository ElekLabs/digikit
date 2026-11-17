"""tools/device/dkmodule.py without an emulator: the .dkm header groups."""

import struct

from tools.device import dkmodule as T
from tools.device import dkproto
from tools.device import dkupload as D


def header(name, machines=0, fx=0, size=500, dsp=0, table_version=1, role=0):
    """A .dkm header with the keys table behind it (when it lists machines or effects, or the role or table version is not the default)."""
    keys = b""
    if machines or fx or role or table_version != 1:
        # the header table: the head and one KEYS entry with a group for each kind that has keys (packages/dkneeds/src/keys.rs)
        groups = b""
        n_groups = 0
        for kind, n in (
            (dkproto.RECORD_KIND["MACHINE"], machines),
            (dkproto.RECORD_KIND["EFFECT"], fx),
        ):
            if n:
                groups += struct.pack(">II", kind, n) + bytes(4 * n)
                n_groups += 1
        keys = (
            struct.pack(">IHH", dkproto.TABLE_MAGIC, table_version, 1)
            + struct.pack(">HHI", T.KEYS_TAG, n_groups, len(groups))
            + groups
        )
    h = bytearray(D.HEADER_BYTES)
    h[:4] = b"DKMF"
    struct.pack_into(">II", h, 8, size, 0xA0)
    struct.pack_into(">I", h, 0x34, 0x000102)
    struct.pack_into(">I", h, dkproto.H_ROLE, role)
    h[0x40 : 0x40 + len(name)] = name.encode()
    struct.pack_into(">II", h, 0x64, 100, 20)
    struct.pack_into(">II", h, 0x90, D.HEADER_BYTES, len(keys))
    struct.pack_into(">I", h, 0x9C, dsp)
    return bytes(h) + keys


def test_header_groups():
    m = T.header_info(header("acme/synth", machines=1, dsp=64))
    assert (m["group"], m["name"], m["version"], m["arena"], m["dsp"]) == (
        "Machines",
        "acme/synth",
        "0.1.2",
        120,
        64,
    )
    assert T.header_info(header("a", fx=1))["group"] == "Effects"
    assert T.header_info(header("a"))["group"] == "Tools"
    assert T.header_info(b"nope" + bytes(200)) is None
    assert T.header_info(b"DKMF") is None


def test_header_table_versions_and_service_role():
    assert T.group_of(header("a", machines=1, table_version=2)) == "Machines"
    assert T.group_of(header("a", role=1, table_version=2)) == "Services"
    assert T.group_of(header("a", machines=1, role=1)) == "Services"
    # a version outside 1..TABLE_VERSION is the older-runtime case: no table is read
    for bad in (0, dkproto.TABLE_VERSION + 1):
        assert T.group_of(header("a", machines=1, role=1, table_version=bad)) == "Tools"
