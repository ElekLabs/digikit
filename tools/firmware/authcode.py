# fmt: off
"""The container's 32-byte trailer: HMAC-SHA256, keyed from section 2.

    CLI: tools/firmware/content_hmac.py

Traced from the depacked bootstrap (`out/sections/section_2_DSP.bin`, base
0x80000400 once its 4-byte length prefix is stripped) via Ghidra, and
confirmed byte-exact against both real firmwares below.

LAYOUT. `tools.firmware.container.decode_syx()` returns [u32 total_len][u32 checksum]
then the ELE3 container. The container's last 32 bytes, once its length has
been padded to a 16-byte boundary, are this trailer; `total_len` covers the
container *including* the trailer (confirmed: 1347728 = 1347692 + 4 pad + 32
for 1.15C; 1743120 = 1743084 + 4 + 32 for Digitone 1.10E).

CONSTRUCTION (`FUN_80005e2a` at 0x80005e2a in the bootstrap). Textbook
HMAC-SHA256 (`FUN_80005c4c`: ipad 0x36 / opad 0x5c over a 64-byte, zero-padded
key block; `FUN_80005bec` is the SHA-256 compressor). Covers the container
from its start (ELE3 magic, decoded-stream offset 8) through `total_len - 32`
-- i.e. everything except the trailer itself, which the routine slices off
before hashing and then compares byte-for-byte against.

KEY DERIVATION (`FUN_80005d90` at 0x80005d90 -- same address, same code, in
both devices' bootstraps; only the data it reads differs). For a per-device
STRING and 32-byte CONST stored back-to-back in section 2 (CONST starts
immediately after STRING's NUL):

    key[i] = CONST[i] ^ sha256(STRING)[i] ^ sha256(STRING[::-1])[i]      i in 0..31

Traced by disassembling the reversal loop: it copies STRING+NUL into a stack
buffer, writes it out reversed, then hashes the first len(STRING) bytes of
the *forward* copy and, separately, len(STRING) bytes of the reversed copy
starting one byte in (skipping the reversed copy's leading NUL) -- i.e.
sha256(STRING) and sha256(STRING reversed with no NUL either side).

Per-device STRING/CONST (found at the addresses below in each device's own
depacked section 2; the surrounding code is byte-identical between devices --
only these constants and the per-packet checksum's K byte, 0x0F vs 0x10,
differ):

    Digitakt II   STRING="Master Overdrive"  @0x80006ff8   CONST @0x80007009
    Digitone II   STRING="Multiplier"        @0x8000706c   CONST @0x80007077

Confirmed: recomputing the trailer this way reproduces, byte for byte --
    Digitakt_II_OS1.15C.syx  6050317eb616669caf11ed442f0d9df6f4a70bdcb85ee47474df2b0848d6c257
    Digitone_II_OS1.10E.syx  4f8184e7b42a82802e67a81f3ff43e02e67d8e3a6ef046e3f4468e77ad89feb5

FAILURE BEHAVIOR. Unlike the per-packet checksum's error flag (set but never
read -- see docs), this one is a real gate: `FUN_80003c9c` at 0x80003c9c
calls FUN_80005e2a at 0x80003cdc, `tst.b D0` / `beq` at 0x80003ce4/0x80003ce6
branches on the result, and on failure jumps to `FUN_80003bfc` (0x80003bfc),
which prints "UPGRADE ABORTED" / "PLEASE REBOOT" and hangs in an empty
infinite loop -- it never returns, so the flash erase/write loop that follows
in FUN_80003c9c is unreachable on a bad HMAC (or a bad content checksum,
which gates the same way one call earlier, at 0x80003ca6 -- not 0x40003ca6
as earlier docs have it; that appears to be a transcription slip, since
0x80003ca6 is the exact instruction that reads the length word this checksum
covers, `move.l (0x40000000).l,D2`, and the docs' digits after the '4' match
it exactly).
"""
import hashlib
import hmac
import pathlib
import struct

from tools.firmware import device as device_files
from tools.firmware.container import decode_syx

REPO = pathlib.Path(__file__).resolve().parents[2]

DEVICES = {
    'dt2': dict(string=b'Master Overdrive',
                const=bytes.fromhex(
                    '695d82bca2f03f2c44570420eff11178'
                    '31ea1aa03829c55e09065d1b790d63ff')),
    'dn2': dict(string=b'Multiplier',
                const=bytes.fromhex(
                    'fc83032c7b2781c970dd306263156e8e'
                    '07ff6fa0f312d2393cb2f5849f9670e5')),
}

# syx_id -> short name, from devices/*.toml; a device with no seal key in DEVICES cannot be sealed here
DEVICE_BY_ID = {
    d.syx_id: d.short
    for d in device_files.load_all(str(REPO / 'devices'))
    if d.syx_id is not None
}


def derive_key(string, const):
    """CONST xor sha256(STRING) xor sha256(STRING reversed) -> 32-byte key."""
    if len(const) != 32:
        raise ValueError('const must be 32 bytes, got %d' % len(const))
    h1 = hashlib.sha256(string).digest()
    h2 = hashlib.sha256(string[::-1]).digest()
    return bytes(a ^ b ^ c for a, b, c in zip(const, h1, h2, strict=False))


def split_container(decoded):
    """Decoded .syx stream -> (data_to_hash, trailer, total_len)."""
    total_len = struct.unpack_from('>I', decoded, 0)[0]
    container = decoded[8:8 + total_len]
    if len(container) != total_len:
        raise ValueError('decoded stream shorter than its own total_len field')
    return container[:total_len - 32], container[total_len - 32:], total_len


def compute_trailer(decoded, key):
    data, _trailer, _total_len = split_container(decoded)
    return hmac.new(key, data, hashlib.sha256).digest()


def seal(container, device):
    """Container bytes (ending in a 32-byte zeroed trailer slot) -> the same
    container with its trailer filled with the real HMAC-SHA256, keyed for
    `device` (a DEVICES key)."""
    key = derive_key(**DEVICES[device])
    data = container[:-32]
    trailer = hmac.new(key, data, hashlib.sha256).digest()
    return data + trailer


def verify(path, device):
    key = derive_key(**DEVICES[device])
    decoded = decode_syx(path)
    _data, trailer, total_len = split_container(decoded)
    computed = compute_trailer(decoded, key)
    return total_len, trailer, computed
