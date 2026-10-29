"""Isolated DDR diagnostic: actual assembled receiver and ColdFire wrapper checks.

No daemon build or coupled run. The CF harness's normal firmware state is merely
CPU setup; every firmware function/data pointer used here is a stand-in.
"""

import os
import struct
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

from tools.firmware import config
from tools.patchkit import patchkit, pk_sharc, pk_toolchains
from tools.patchkit.pk_cfcall import CfMachine

MOD = REPO_ROOT / "modules/core/adapters/dsp-ddr-proof"
MAGIC = 0xDDF4A601
TOKEN = 0x81234567
RING, RECORD, FRAME = 0x20040000, 0x20050000, 0x20060000
DEST = 0x10200000
BACK = 0xB9F000


def check(words):
    s1 = s2 = 0
    for word in words:
        s1 = (s1 + word) & 0xFFFFFFFF
        s2 = (s2 + s1) & 0xFFFFFFFF
    return s1 ^ s2


def packet(seq, op, offset=0, payload=(), token=TOKEN, count=None):
    count = len(payload) if count is None else count
    head = [4, MAGIC, token, seq, op, offset, count]
    return head + [check([*head, *payload])] + [0] * 24 + list(payload)


@pytest.fixture(scope="module")
def dsp():
    if pk_toolchains.selache_dir() is None:
        pytest.skip("assembler absent")
    obj = pk_sharc.assemble((MOD / "src/dsp/ddr-proof.s").read_text())
    syms = {
        name: 0x20080000 + offset for name, (_, offset, _) in obj.data_symbols.items()
    }
    linked = pk_sharc.link(obj, 0xB90000, {**syms, "_dd_back": BACK})
    return linked, syms


def receiver(dsp):
    linked, syms = dsp
    sess = pk_sharc.CallSession([linked], BACK)
    for address in syms.values():
        sess.poke(address // 4, 0)
    sess.poke(FRAME // 4 - 3, RING)
    return sess, syms


def deliver(sess, words):
    for i, word in enumerate(words):
        sess.poke(RING // 4 + i, word)
    regs = {
        "I4": RECORD,
        "I6": FRAME,
        "I7": FRAME - 0x100,
        "R1": 0x1234,
        "R2": 0x5678,
        "M5": 7,
        "M6": 9,
        "L2": 17,
        "L3": 19,
        **{f"R{i}": 0x7000 + i for i in (0, 3, 4, 5, 6)},
    }
    r = sess.call("_dd_hook", regs, max_steps=30000, read_regs=tuple(regs))
    assert r["halt"]["reason"] == "breakpoint", r["halt"]
    assert all(r["regs"][name] == value for name, value in regs.items())
    return r


def value(sess, syms, name):
    return sess.peek(syms[f"_dd_{name}"] // 4)


def test_init_data_duplicate_and_stale_token(dsp):
    sess, syms = receiver(dsp)
    deliver(sess, packet(0, 1))
    assert value(sess, syms, "token") == TOKEN
    assert value(sess, syms, "ready") == 0
    for lo in (DEST - 1024, DEST + 240000):
        assert [sess.peek(lo + i) for i in range(1024)] == [0xA55A3CC3] * 1024
    data = [(i * 0x12571 + 0x8000FFFF) & 0xFFFFFFFF for i in range(640)]
    deliver(sess, packet(1, 2, payload=data))
    assert [sess.peek(DEST + i) for i in range(640)] == data
    # Poison the destination after acceptance. A duplicate must ACK, never rewrite.
    sess.poke(DEST, 0xEEDDCCBB)
    deliver(sess, packet(1, 2, payload=data))
    assert sess.peek(DEST) == 0xEEDDCCBB
    assert value(sess, syms, "words") == 640
    deliver(sess, packet(2, 2, 640, [99], token=TOKEN + 1))
    assert value(sess, syms, "token") == TOKEN
    assert value(sess, syms, "seq") == 1
    assert value(sess, syms, "words") == 640
    assert sess.peek(RECORD // 4 + 0x64 // 4) == TOKEN
    # INIT cannot replace the existing token or reset the canaries.
    sess.poke(DEST - 1, 77)
    deliver(sess, packet(0, 1, token=TOKEN + 1))
    assert sess.peek(DEST - 1) == 77


@pytest.mark.parametrize("bad", ["checksum", "range", "gap", "partial", "commit"])
def test_failed_transfer_stays_unready_without_payload_write(dsp, bad):
    sess, syms = receiver(dsp)
    deliver(sess, packet(0, 1))
    sess.poke(DEST, 0x44556677)
    words = packet(1, 2, payload=[123])
    if bad == "checksum":
        words[7] ^= 1
    elif bad == "range":
        words = packet(1, 2, 240000, [123])
    elif bad == "gap":
        words = packet(2, 2, payload=[123])
    elif bad == "partial":
        deliver(sess, packet(1, 2, payload=[0x44556677]))
        words = packet(2, 3, 1)
    else:
        words = packet(1, 3)
    deliver(sess, words)
    assert value(sess, syms, "status") >= 2
    assert value(sess, syms, "ready") == 0
    assert sess.peek(DEST) == 0x44556677
    deliver(sess, packet(1, 2, payload=[0xABCD]))
    assert sess.peek(DEST) == 0x44556677
    assert value(sess, syms, "ready") == 0


def test_commit_requires_exact_total_and_publishes_ready(dsp):
    sess, syms = receiver(dsp)
    deliver(sess, packet(0, 1))
    # Boundary setup isolates the final block and COMMIT; the coupled test owns
    # the complete 960000-byte transport and readback proof.
    sess.poke(syms["_dd_seq"] // 4, 374)
    sess.poke(syms["_dd_words"] // 4, 239360)
    data = [0x87654321] * 640
    deliver(sess, packet(375, 2, 239360, data))
    assert value(sess, syms, "words") == 240000
    assert value(sess, syms, "ready") == 0
    assert sess.peek(DEST + 239999) == data[-1]
    deliver(sess, packet(376, 3, 240000))
    assert value(sess, syms, "ready") == 1
    assert value(sess, syms, "status") == 1
    assert sess.peek(RECORD // 4 + 24) == 0xA6DD | (376 << 16) | (1 << 25)


@pytest.fixture(scope="module")
def cf(tmp_path_factory):
    if pk_toolchains.m68k_bin("gcc") is None:
        pytest.skip("ColdFire compiler absent")
    stub = tmp_path_factory.mktemp("ddr-proof") / "stock.c"
    stub.write_text("""unsigned int dd_capture[4], dd_clear_mask;
void dd_stock_send(int n, const unsigned short *s, int r, const volatile unsigned short *p)
{ dd_capture[0]=n; dd_capture[1]=(unsigned int)s;
  dd_capture[2]=r; dd_capture[3]=(unsigned int)p;
  if (dd_clear_mask) for (unsigned int i=17;i<=20;i++) ((unsigned short *)s)[i]=0;
}
""")
    return patchkit.cf_link([MOD / "src/cf/ddr-proof.c", stub], 0x47E80000, {})


def test_cf_quiet_gate_ack_and_watchdog(cf):
    syx = config.stock("Digitakt_II_OS1.17.syx")
    if not syx.exists():
        pytest.skip("CF test setup firmware absent")
    state = os.environ.get("DIGI_DDR_PROOF_CF_STATE")
    machine = CfMachine(syx, state=Path(state) if state else None)
    src, rec = 0x47EC0000, 0x47ED0000
    frame = bytearray(0xA80)
    struct.pack_into(">H", frame, 0, 3)
    staging = b"".join(struct.pack(">HH", w & 65535, w >> 16) for w in packet(0, 1))
    mail, stage, capture = (
        cf.symbols[n] for n in ("dd_mail", "dd_frame", "dd_capture")
    )
    reads = [(mail, 16), (capture, 16)]

    def call(p=0):
        return {
            "pc": cf.symbols["dd_frame_hook"],
            "args": [0xA80, src, 0xABC, p],
            "reads": reads,
        }

    pokes = {
        cf.base: cf.image,
        cf.base + len(cf.image): bytes(cf.bss_size),
        src: bytes(frame),
        stage: staging,
        mail: struct.pack(">4I", 4, 0, 0, 0),
    }
    r = machine.run(pokes, [call(), call()])
    assert all(x.ok and not x.clobbered for x in r)
    assert struct.unpack(">4I", r[0].reads[1])[1] == src
    assert struct.unpack(">4I", r[1].reads[1])[1] == stage
    ack = bytearray(0xABC)
    struct.pack_into(">4H", ack, 0x60, 0xA6DD, 0, TOKEN & 65535, TOKEN >> 16)
    r = machine.run({**pokes, rec: bytes(ack)}, [call(), call(), call(rec)])
    assert struct.unpack(">4I", r[-1].reads[0])[0] == 2
    assert struct.unpack(">4I", r[-1].reads[1])[1] == src
    stale = bytearray(ack)
    struct.pack_into(">H", stale, 0x64, (TOKEN + 1) & 65535)
    r = machine.run({**pokes, rec: bytes(stale)}, [call(), call(), call(rec)])
    assert struct.unpack(">4I", r[-1].reads[0])[0] == 1
    # Each mask independently blocks substitution, including a one-frame pulse.
    for index in (17, 18, 19, 20):
        noisy = bytearray(frame)
        struct.pack_into(">H", noisy, index * 2, 1)
        pulse = machine.run(
            {
                **pokes,
                src: bytes(noisy),
                cf.symbols["dd_clear_mask"]: struct.pack(">I", 1),
            },
            [call(), call(), call()],
        )
        assert [struct.unpack(">4I", x.reads[1])[1] for x in pulse] == [src, src, stage]
        r = machine.run({**pokes, src: bytes(noisy)}, [call()] * 902)
        assert all(x.ok and not x.clobbered for x in r)
        assert struct.unpack(">4I", r[-1].reads[0])[:2] == (3, 100)
        assert all(struct.unpack(">4I", x.reads[1])[1] == src for x in r)


@pytest.mark.parametrize("kind", ["checksum", "range", "token", "operation"])
def test_malformed_init_poison_cannot_be_reset(dsp, kind):
    sess, syms = receiver(dsp)
    sess.poke(DEST - 1, 0x11223344)
    words = packet(0, 1)
    if kind == "checksum":
        words[7] ^= 1
    elif kind == "range":
        words = packet(0, 1, 240001)
    elif kind == "token":
        words = packet(0, 1, token=0)
    else:
        words = packet(0, 2)
    deliver(sess, words)
    assert value(sess, syms, "status") >= 2
    deliver(sess, packet(0, 1))
    assert value(sess, syms, "token") == 0
    assert value(sess, syms, "ready") == 0
    assert sess.peek(DEST - 1) == 0x11223344


def test_normal_observer_masks_pitch_but_proof_gate_requires_exact_word(dsp):
    sess, syms = receiver(dsp)
    sess.poke(DEST - 1, 0x11223344)
    for i, pitch in enumerate((0x3C00, 0x7F00), 1):
        deliver(sess, [(pitch << 16) | 3])
        assert value(sess, syms, "normal_frames") == i
    deliver(sess, [0x3C000001])
    assert value(sess, syms, "normal_frames") == 2
    forged = packet(0, 1)
    forged[0] |= 0x3C000000
    deliver(sess, forged)
    assert value(sess, syms, "token") == 0
    assert value(sess, syms, "ready") == 0
    assert sess.peek(DEST - 1) == 0x11223344
    deliver(sess, packet(0, 1))
    assert value(sess, syms, "token") == TOKEN
