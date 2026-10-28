#!/usr/bin/env python3
"""Scan the DSP image (container section 7) of a firmware .syx for stores the SHARC+ ISA does not have.

    uv run python -m tools.patchkit.pk_isacheck IMAGE.syx

A store through an index register (Types 3a, 3b, 3d, 15a, 15b) cannot take a source register of the same DAG: `DM(x, Ia) = Ureg` not a
DAG1 register, `PM(x, Ic) = Ureg` not a DAG2 register (finding 49 section 10; the rule is `pk_sharc.ureg_class_problems`, the same one
`pk_build` fails a module build on). The stock images have none. In patched code it also refuses an indirect jump or call with (CI)
(`pk_sharc.ci_problem`): selas emits one for a 48-bit `JUMP (Md, Ic)`. `mise run patch:preflight` runs this scan after the Rust checks.

The scan decodes the code blocks of the boot stream with the aligned decoder that built the sharcdb databases (`sharcflow.aligned`):
the stock code blocks (`sharcinv.CODE_BLOCKS`, the layout of the DT2 images; the Digitone II list of `sharcdb.schema.KNOWN_CODE_BLOCKS`)
and the blocks a patch appended at the end of the stream (the run of blocks below 0x80000000 before the final block). Data blocks are not
scanned: decoded as code they give false stores (finding 49 section 10.2, 105 on DN2 1.12). With the image's plan
(`IMAGE.plan.json`, written by `patchkit patch`) an appended block is decoded only inside the code ranges it places: a module's tables
share blocks with code and give false stores too.
Exit status 1 when a store is found.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from tools.firmware.container import sections
from tools.firmware.elz import depack_section
from tools.firmware.extract import classify
from tools.patchkit import pk_sharc
from tools.sharc_dev import sharcinv, sharcldr

ROOT = Path(__file__).resolve().parents[2]
FINAL_BIT = 1 << 15
# The stock code blocks, by the target address they load to (the indices differ between images, the targets do not). Left out: the DT2
# block at 0x282d7158 (388 bytes; its one hit, PM store of B10 at pc 0x16b8fe, is inside a data table) and the DN2 block at 0x2001e888
# (data decoded as code: 231 hits outside every function; finding 49 section 10.2).
DT2_CODE_TARGETS = {
    0x20000000,
    0x2838004C,
    0x283801A4,
    0x28380328,
    0x28380548,
    0x283826C8,
    0x283827CC,
}
DN2_CODE_TARGETS = DT2_CODE_TARGETS | {0x2838261C, 0x28382720}


def section7(syx: Path) -> bytes:
    """The depacked DSP boot stream of a firmware file."""
    c, secs = sections(syx)
    for sid, off, clen, _dest in secs:
        if sid == 7:
            kind, payload = classify(bytes(c[off : off + clen]))
            return depack_section(payload) if kind == "packed" else payload
    raise SystemExit(f"{syx}: no section 7")


def plan_code_ranges(syx: Path) -> list[tuple[int, int]] | None:
    """Byte ranges of the code the image's plan placed on the SHARC; None without a plan."""
    plan = syx.with_suffix(".plan.json")
    if not plan.exists():
        return None
    writes = json.loads(plan.read_text())["writes"]
    return [
        (lo, hi)
        for w in writes
        if w["cpu"] == "sharc"
        for lo, hi in w.get("code") or ()
    ]


def code_blocks(blocks: list[dict]) -> list[int]:
    """Indices of the blocks to scan: stock code by target address, then the appended run before the final block."""
    out = [
        b["index"]
        for b in blocks
        if not b["fill"] and b["byte_count"] and b["target_address"] in DN2_CODE_TARGETS
    ]
    k = len(blocks) - 1
    if blocks and blocks[k]["block_code"] & FINAL_BIT:
        k -= 1
    while k >= 0:
        b = blocks[k]
        if b["fill"] or not b["byte_count"] or b["target_address"] >= 0x80000000:
            break
        if b["index"] not in out:
            out.append(b["index"])
        k -= 1
    return sorted(out)


def scan(blob: bytes, code: list[tuple[int, int]] | None = None) -> list[str]:
    """One message per excluded store in the code blocks of the boot stream `blob`. With `code` (the plan's code ranges), an
    appended block is decoded only inside them: a module's data in the same block, decoded as code, gives false stores."""
    blocks = sharcldr.parse_blocks(blob)
    mem = sharcldr.LoadedMemory.from_stream(blob, blocks)
    by = {b["index"]: b for b in blocks}
    found = []
    for idx in code_blocks(blocks):
        r = sharcinv.analyze_block(mem, by, idx, 8)
        if r is None:
            continue
        at = by[idx]["target_address"]
        stock = at in DN2_CODE_TARGETS
        for off, ins in r["insns"]:
            if (
                code is not None
                and not stock
                and not any(lo <= at + off < hi for lo, hi in code)
            ):
                continue
            for msg in pk_sharc.ureg_class_problems(ins.fields, "", ins.type_name):
                sw = r["base_sw"] + off // 2
                found.append(f"block {idx} pc {sw:#x}{msg}")
            ci = pk_sharc.ci_problem(ins.fields, ins.type_name)
            if ci and not stock:
                found.append(f"block {idx} pc {r['base_sw'] + off // 2:#x}: {ci}")
    return found


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print(__doc__)
        return 2
    found = scan(section7(Path(argv[0])), plan_code_ranges(Path(argv[0])))
    if found:
        print(
            f"ISA check: FAIL {len(found)} instruction(s) the ISA does not allow in section 7 (finding 49 section 10)"
        )
        for f in found:
            print("  " + f)
        return 1
    print(
        "ISA check: ok, no store through an index register of a same-DAG register in section 7"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
