"""The patched 1.17 image's own DSP code on the concrete core: the stock renderer (SW 0x1c4ecf) called for one voice
the way the voice loop of FUN_1c642a calls it, run to the caller's return point. Used by the in-situ tests of
core/dsp-inline (tests/patchkit/test_dsp_inline.py) and test/inline-tone (tests/patchkit/test_inline_tone.py)."""

from __future__ import annotations

from pathlib import Path

from tools.patchkit import pk_sharc

REC = 0x1D8
CALLER_RET = 0x1C6B06  # after `CJUMP 0x1c4ecf` in the voice loop of FUN_1c642a
IMG_RECORDS, IMG_OUT, IMG_TYPES = 0x2412CC, 0x252DF8, 0x255970
IMG_STACK = 0x2A0000
IMG_ROUTINES, IMG_PROBE = (
    0xBF0000,
    0x200E8000,
)  # free L2, above everything the image loads
CALLEE_SAVED: dict[str, int] = {
    **{f"R{n}": 0x7000 + n for n in (3, 5, 6, 7, 9, 10, 11, 13, 15)},
    **{f"I{n}": 0x20000010 * (n + 1) for n in (0, 1, 2, 3, 5)},
    **{"M1": 11, "M2": 12, "M3": 13, "B3": 0x200, "I8": 0x20000080, "M8": 8},
}


def image_memory(syx: Path, extra=None):
    from tools.patchkit import port_resolve
    from tools.sharc_dev.sharcldr import LoadedMemory

    mem = LoadedMemory.from_stream(port_resolve.sections_of(syx)[7])
    if extra is None:
        return mem
    blocks = [dict(b) for b in mem.blocks]
    blocks.append(
        {
            "target_address": pk_sharc.SW_ALIAS_BASE + 2 * extra.base_sw,
            "byte_count": len(extra.code_boot),
            "fill": False,
            "payload_offset": len(mem.data),
            "payload_len": len(extra.code_boot),
            "index": len(blocks),
        }
    )
    return LoadedMemory(mem.data + extra.code_boot, blocks)


def render_voice(
    mem,
    v: int,
    pokes: dict[int, int],
    active: int = 1,
    reads: dict[str, tuple[int, int]] | None = None,
) -> dict:
    """Call the renderer for voice v the way the voice loop does; return the output, the probe, the record, the
    registers and `reads` (name -> (byte address, word count)) as the run left them."""
    from tools.sharc_dev import sharc_run as sr
    from tools.sharc_dev import sharc_trace as st

    rec, outp = IMG_RECORDS + v * REC, IMG_OUT + v * 0x80
    regs: dict[str | int, int | str] = {
        "R4": rec,
        "R8": outp,
        "R12": 32,
        "R14": 32 - v,
        "M5": 0,
        "M6": 1,
        "M7": -1,
        "M13": 0,
        "M14": 1,
        "M15": -1,
        "I6": IMG_STACK,
        "I7": IMG_STACK - 8,
    }
    for name, value in CALLEE_SAVED.items():
        regs[name] = value
    runner = sr.Runner(mem, 0x1C4ECF, regs=regs, breakpoints=[CALLER_RET])
    runner.state.explicit_memory_model = True
    words = {
        IMG_STACK - 4: CALLER_RET - 1,  # the return address the CJUMP pushed
        IMG_STACK: IMG_STACK + 0x10000,  # the caller's frame pointer
        rec: 0x300000,  # word +0, the sample pointer
        **{outp + 4 * k: 0x7F7F7F7F for k in range(32)},
        **pokes,
    }
    for a, w in words.items():
        st._dm_write(runner.state, a, 4, st.Const(w & 0xFFFFFFFF))
    st._dm_write(runner.state, rec + 0x1B8, 1, st.Const(active))
    runner.state.call_stack.append(CALLER_RET)
    res = runner.run(20000)

    def word(a: int) -> int:
        x = st._dm_read(runner.state, a, 4)
        return 0 if x is None else x.value & 0xFFFFFFFF

    return {
        **{
            name: [word(a + 4 * k) for k in range(n)]
            for name, (a, n) in (reads or {}).items()
        },
        "halt": res.halt.to_json(),
        "final_pc_sw": res.final_pc_sw,
        "out": [word(outp + 4 * k) for k in range(32)],
        "probe": [word(IMG_PROBE + 4 * k) for k in range(6)],
        "record": [word(rec + 4 * k) for k in range(REC // 4)],
        "regs": {
            n: getattr(runner.state.uregs.get(st.UREG_CODES[n]), "value", None)
            for n in (*CALLEE_SAVED, "I6", "I7")
        },
    }
