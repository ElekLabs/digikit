"""Private DN2 LP0 diagnostic: our assembled guards, call frames and mailbox.

Driver stand-ins inspect our ABI without asserting third-party driver behavior.
The CF runner reuses the existing executable; these tests never build a daemon.
"""

import struct
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

from tools.firmware import config
from tools.patchkit import patchkit, pk_cfcall, pk_sharc, pk_toolchains

MOD = REPO_ROOT / "modules/core/adapters/dsp-lp0-proof"
FIX = REPO_ROOT / "tests/fixtures/dsp-lp0-proof/driver.s"
BACK = 0xBB0000
PORT = 0x20070000
STACK = 0x20078000
DEST = 0x10200000
CANARY = 0xA55A3CC3


@pytest.fixture(scope="module", autouse=True)
def prebuilt_tools():
    patch = pytest.MonkeyPatch()
    patch.setattr(patchkit, "_BUILT", patchkit.binary(build=False))
    yield
    patch.undo()


@pytest.fixture(scope="module")
def dsp():
    if pk_toolchains.selache_dir() is None:
        pytest.skip("assembler absent")
    objects = [
        pk_sharc.assemble(
            pk_sharc.expand_includes(path.read_text(), [REPO_ROOT / "sdk"])
        )
        for path in (MOD / "src/dsp/lp0-proof.s", FIX)
    ]
    address, syms, inits = 0x20080000, {}, []
    for obj in objects:
        sections = {}
        for sec, (data, align, _) in obj.data.items():
            address = (address + align - 1) & -align
            sections[sec] = address
            inits.append((address, data))
            address += len(data)
        for name, (sec, offset, _) in obj.data_symbols.items():
            syms[name] = sections[sec] + offset
    externs = {**syms, "_lp_back": BACK, "_test_return": BACK, "_lp_port_record": PORT}
    bases = [0xB90000, 0xBA0000]
    for obj, base in zip(objects, bases, strict=True):
        externs.update({name: base + off for name, (off, _) in obj.symbols.items()})
    linked = [
        pk_sharc.link(obj, base, externs)
        for obj, base in zip(objects, bases, strict=True)
    ]
    return linked, syms, inits


def fresh(dsp):
    linked, syms, inits = dsp
    sess = pk_sharc.CallSession(linked, BACK)
    for address, data in inits:
        for off in range(0, len(data), 4):
            sess.poke(
                (address + off) // 4, int.from_bytes(data[off : off + 4], "little")
            )
    sess.poke((PORT + 20) // 4, 0)
    for address in (0x30FFE000, 0x30FFF008):
        sess.poke(address, 0)
    return sess, syms


def val(sess, syms, name):
    return sess.peek(syms[name] // 4)


def put(sess, syms, name, value):
    sess.poke(syms[name] // 4, value)


CONSTS = {"M5": 0, "M6": 1, "M7": 0xFFFFFFFF, "M13": 0, "M14": 1, "M15": 0xFFFFFFFF}
REGS = {**CONSTS, "I6": STACK, "I7": STACK, "MODE1": 0x10000, "IMASKP": 0}


def init(sess, regs=None):
    regs = {**REGS, **(regs or {})}
    result = sess.call("_lp_hook", regs, max_steps=30000, read_regs=tuple(regs))
    assert result["halt"]["reason"] == "breakpoint", result["halt"]
    assert {key: result["regs"][key] for key in regs} == regs
    return result


def complete(sess, syms, tag, *, event=4, address=None, context=None):
    rx = val(sess, syms, "_lp_rx")
    payload = [(0x12340000 + i + tag) & 0xFFFFFFFF for i in range(1024)]
    sess.poke(rx // 4, tag)
    for i, word in enumerate(payload):
        sess.poke(rx // 4 + 1 + i, word)
    regs = {
        **REGS,
        "R4": syms["_lp_status"] if context is None else context,
        "R8": event,
        "R12": rx if address is None else address,
    }
    result = sess.call("_test_callback", regs, max_steps=30000, read_regs=("I6", "I7"))
    assert result["halt"]["reason"] == "breakpoint", result["halt"]
    assert {key: result["regs"][key] for key in ("I6", "I7")} == {
        "I6": STACK,
        "I7": STACK,
    }
    return payload


def test_setup_once_uses_real_call_frames_and_preserves_hook_context(dsp):
    sess, syms = fresh(dsp)
    hostile = {f"R{i}": 0x7100 + i for i in range(16)}
    hostile.update({f"I{i}": 0x20060000 + 16 * i for i in range(16) if i not in (6, 7)})
    hostile.update({f"M{i}": 9 + i for i in range(16)})
    hostile.update({f"L{i}": 17 + i for i in range(8)})
    hostile.update({f"B{i}": 0x20050000 + 16 * i for i in range(8)})
    init(sess, hostile)
    assert val(sess, syms, "_lp_status") == 2
    assert [
        val(sess, syms, "_test_open_arg0"),
        val(sess, syms, "_test_open_arg1"),
        val(sess, syms, "_test_open_size"),
    ] == [0, 1, 128]
    assert val(sess, syms, "_test_open_state") & 15 == 0
    assert val(sess, syms, "_test_setcb_fn") == sess.symbols["_lp_callback"]
    assert val(sess, syms, "_test_setcb_context") == syms["_lp_status"]
    assert val(sess, syms, "_test_submit_config") == 0
    assert val(sess, syms, "_test_submit_arg2") == 1
    rx = val(sess, syms, "_lp_rx")
    assert rx & 15 == 0
    assert [val(sess, syms, f"_test_desc{i}") for i in range(7)] == [
        0,
        rx,
        0x100000,
        0x401,
        4,
        0,
        0,
    ]
    init(sess)
    assert (
        val(sess, syms, "_test_open_calls")
        == val(sess, syms, "_test_submit_calls")
        == 1
    )


@pytest.mark.parametrize("bad", ["context", "owner", "lpctl", "dmacfg"])
def test_live_preflight_refuses_before_any_driver_mutation(dsp, bad):
    sess, syms = fresh(dsp)
    regs = {}
    if bad == "context":
        regs["IMASKP"] = 1
    else:
        addr = {"owner": PORT + 20, "lpctl": 0x30FFE000, "dmacfg": 0x30FFF008}[bad]
        sess.poke(addr // 4 if bad == "owner" else addr, 1)
    init(sess, regs)
    assert val(sess, syms, "_lp_status") == 4
    assert (
        val(sess, syms, "_test_open_calls")
        == val(sess, syms, "_test_submit_calls")
        == 0
    )
    init(sess)
    assert val(sess, syms, "_test_open_calls") == 0


def test_one_page_rearms_reconstructed_descriptor_and_exact_payload(dsp):
    sess, syms = fresh(dsp)
    init(sess)
    payload = complete(sess, syms, 0)
    assert [sess.peek(DEST + i) for i in range(1024)] == payload
    assert (val(sess, syms, "_lp_pages"), val(sess, syms, "_lp_bytes")) == (1, 4096)
    assert val(sess, syms, "_test_submit_calls") == 2
    assert (
        val(sess, syms, "_test_desc2") == 0x100000
    )  # stand-in poisoned it after first submit
    assert sess.peek(DEST - 1) == sess.peek(DEST + 240000) == CANARY


@pytest.mark.parametrize(
    "bad", ["event", "address", "context", "tag", "gap", "pages", "bytes", "state"]
)
def test_callback_failures_are_sticky_without_copy_or_rearm(dsp, bad):
    sess, syms = fresh(dsp)
    init(sess)
    kwargs, tag = {}, 0
    if bad == "event":
        kwargs["event"] = 3
    elif bad == "address":
        kwargs["address"] = val(sess, syms, "_lp_rx") + 4
    elif bad == "context":
        kwargs["context"] = 0
    elif bad in ("tag", "gap"):
        tag = 235 if bad == "tag" else 1
    elif bad == "pages":
        put(sess, syms, "_lp_pages", 0xFFFFFFFF)
        tag = 0xFFFFFFFF
    elif bad == "bytes":
        put(sess, syms, "_lp_bytes", 4)
    elif bad == "state":
        put(sess, syms, "_lp_status", 1)
    sess.poke(DEST, 0xAABBCCDD)
    complete(sess, syms, tag, **kwargs)
    assert val(sess, syms, "_lp_status") == 4
    assert sess.peek(DEST) == 0xAABBCCDD
    assert val(sess, syms, "_test_submit_calls") == 1
    complete(sess, syms, 0)
    assert sess.peek(DEST) == 0xAABBCCDD
    assert val(sess, syms, "_test_submit_calls") == 1


def test_final_page_clamps_to_1536_bytes_and_verifies_full_canaries(dsp):
    sess, syms = fresh(dsp)
    init(sess)
    put(sess, syms, "_lp_pages", 234)
    put(sess, syms, "_lp_bytes", 234 * 4096)
    payload = complete(sess, syms, 234)
    assert (val(sess, syms, "_lp_pages"), val(sess, syms, "_lp_bytes")) == (235, 960000)
    assert val(sess, syms, "_lp_status") == 3
    assert [sess.peek(DEST + 234 * 1024 + i) for i in range(384)] == payload[:384]
    assert all(sess.peek(DEST + 240000 + i) == CANARY for i in range(1024))
    assert all(sess.peek(DEST - 1024 + i) == CANARY for i in range(1024))
    assert val(sess, syms, "_test_submit_calls") == 1


@pytest.mark.parametrize("phase", ["open", "first_submit", "rearm"])
def test_driver_failure_is_sticky_and_never_reopens(dsp, phase):
    sess, syms = fresh(dsp)
    if phase == "open":
        put(sess, syms, "_test_open_result", 19)
    if phase == "first_submit":
        put(sess, syms, "_test_submit_result", 4)
    init(sess)
    if phase == "rearm":
        put(sess, syms, "_test_submit_result", 4)
        complete(sess, syms, 0)
    assert val(sess, syms, "_lp_status") == 4
    calls = val(sess, syms, "_test_submit_calls")
    init(sess)
    complete(sess, syms, 0)
    assert val(sess, syms, "_test_open_calls") == 1
    assert val(sess, syms, "_test_submit_calls") == calls


def test_duplicate_page_stops_and_guard_damage_prevents_completion(dsp):
    sess, syms = fresh(dsp)
    init(sess)
    complete(sess, syms, 0)
    sess.poke(DEST, 0xABCD1234)
    complete(sess, syms, 0)
    assert val(sess, syms, "_lp_status") == 4
    assert sess.peek(DEST) == 0xABCD1234
    sess, syms = fresh(dsp)
    init(sess)
    put(sess, syms, "_lp_pages", 234)
    put(sess, syms, "_lp_bytes", 234 * 4096)
    sess.poke(DEST + 240000 + 1023, 0)
    complete(sess, syms, 234)
    assert val(sess, syms, "_lp_status") == 4


@pytest.fixture(scope="module")
def cf(tmp_path_factory):
    if pk_toolchains.m68k_bin("gcc") is None:
        pytest.skip("ColdFire compiler absent")
    runner = Path("/Users/em/src/digi/digitakt2/target/release/examples/cf_call")
    if not runner.is_file():
        pytest.skip("prebuilt cf_call absent; no implicit native build")
    patch = pytest.MonkeyPatch()
    patch.setattr(pk_cfcall, "EXAMPLE", runner)
    patch.setattr(pk_cfcall, "_built", True)
    stub = tmp_path_factory.mktemp("lp0-proof") / "sender.c"
    stub.write_text("""typedef unsigned int u32;
volatile u32 lp_capture[8];
extern volatile u32 lp_mail[], lp_staging[];
void lp_stock_send(u32 tag, const volatile u32 *source, u32 format) {
    ++lp_capture[0]; lp_capture[1]=tag; lp_capture[2]=(u32)source;
    lp_capture[3]=format; lp_capture[4]=lp_mail[0];
    lp_capture[5]=source[0]; lp_capture[6]=source[1023];
}
void lp_test_stage(u32 tag) {
    lp_mail[2]=tag; lp_staging[0]=0x12345678; lp_staging[1023]=0x89abcdef;
    lp_mail[0]=1;
}
""")
    linked = patchkit.cf_link([MOD / "src/cf/lp0-proof.c", stub], 0x47E80000, {})
    try:
        yield linked
    finally:
        patch.undo()


def test_cf_mailbox_retains_request_through_sender_and_rejects_nonsequential(cf):
    image = config.stock("Digitone_II_OS1.12.syx")
    if not image.exists():
        pytest.skip("firmware for CF CPU setup absent")
    machine = pk_cfcall.CfMachine(image)
    mail, capture, staging = [
        cf.symbols[n] for n in ("lp_mail", "lp_capture", "lp_staging")
    ]
    reads = [(mail, 16), (capture, 32), (staging, 4), (staging + 4092, 4)]

    def tick():
        return {"pc": cf.symbols["lp_tick"], "args": [], "reads": reads}

    def stage(tag):
        return {"pc": cf.symbols["lp_test_stage"], "args": [tag], "reads": []}

    pokes = {cf.base: cf.image, cf.base + len(cf.image): bytes(cf.bss_size)}
    calls = [
        tick(),
        stage(0),
        tick(),
        tick(),
        stage(1),
        tick(),
        stage(1),
        tick(),
        stage(2),
        tick(),
    ]
    results = machine.run(pokes, calls)
    assert all(r.ok and not r.clobbered for r in results)

    def decode(r, i):
        return struct.unpack(">" + "I" * (len(r.reads[i]) // 4), r.reads[i])

    assert decode(results[0], 1)[0] == 0  # idle never sends
    first = results[2]
    assert decode(first, 0) == (3, 0, 0, 1)
    assert decode(first, 1)[:7] == (1, 0, staging, 1, 2, 0x12345678, 0x89ABCDEF)
    assert decode(first, 2) == (0x12345678,)
    assert decode(first, 3) == (0x89ABCDEF,)
    assert decode(results[3], 1)[0] == 1  # DONE cannot resend retained staging
    assert decode(results[5], 0) == (3, 0, 1, 2)
    assert decode(results[7], 0) == (4, 1, 1, 2)  # duplicate poisons mailbox
    assert decode(results[9], 0)[0] == 4
    assert decode(results[9], 1)[0] == 2  # host cannot reset sticky error


def test_restore_bases_precedes_indices_for_real_hardware():
    # PRM circular-buffer side effect is absent from the interpreter (finding49).
    # Verify the ordering that prevents a B restore overwriting a restored I.
    source = (MOD / "src/dsp/lp0-proof.s").read_text()
    restore = source.split("_lp_restore:", 1)[1].split("JUMP _lp_back;", 1)[0]
    for i in range(8):
        assert restore.index(f"B{i} = R0;") < restore.index(f"I{i} = R0;")
