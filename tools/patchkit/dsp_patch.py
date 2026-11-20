"""DSP patch files: edits addressed in SHARC+ memory terms (see
packages/emulator/sharc-peer/src/dsp_patch.rs for the format, which the Rust side also parses).

    BASE <sha256 of the unpatched .syx>
    PM <pc> <hex> [was <hex>]      4/8/12 hex digits = 16/32/48-bit instruction
    SW <pc> <hex16> [<hex16> ...]  raw short words
    DM <addr> <hex32> [was <hex32>]
    BYTES <addr> <hex>
    APPEND <addr> <hex>            a new boot-stream block (memory the stream does not load)

Commands (the image is the packed loader image, `tools/sharc_dev/sharc_pack_image.py`):

    dsp_patch.py check IMAGE.bin PATCH [--syx SYX]   guards and BASE only
    dsp_patch.py apply IMAGE.bin PATCH OUT.bin       write a patched image
    dsp_patch.py show  IMAGE.bin PATCH               before/after bytes
    dsp_patch.py syx   BASE.syx PATCH OUT.syx        write a patched .syx

`coupled-run --dsp-patch PATCH` overlays the same file onto an imported DSP
state (a snapshot holds its own copy of code and data, so patching the image
alone does not change a snapshot run). Output is firmware-derived: keep it
outside the repository. Writing a patched .syx would need a packer for the
ELZ stream (tools/firmware/elz.py only depacks), so the patch file plus the base .syx is
the input.

`syx` is that writer, built on tools/firmware/build.py (store-only aPLib packing, so the
file is about 3x larger than the original): it applies the patch to the DSP boot
stream (section 7) of BASE.syx and rebuilds the container with correct
checksums and HMAC trailer. Every other section keeps its decoded bytes, so the
MAIN OS digest is unchanged. The emulator's `firmware.load` (and
`tools/control/digi_client.py inject OUT.syx`) takes such a file and hot-injects the
changed DSP words into the running emulator. A patch can only change bytes the
boot stream stores (not FILL blocks); `APPEND` adds a block before the stream's FINAL block
(`Stream.append_block`: free L2 only, depacked section 7 at most 0xfffff bytes, see
docs/findings/06-sharc-engine-and-startup.md "Appending to the boot stream").
"""

from __future__ import annotations

import argparse
import functools
import hashlib
import operator
import re
import struct
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

LOW = 0x2800_0000
ALIAS = 0x2000_0000


@dataclass
class Op:
    addr: int  # byte address
    new: bytes
    was: bytes | None
    text: str
    append: bool = False


@dataclass
class Patch:
    base_sha256: str | None = None
    ops: list[Op] = field(default_factory=list)


def _hex(s: str) -> int:
    return int(s.removeprefix("0x"), 16)


def _words(s: str) -> bytes:
    s = s.removeprefix("0x")
    if not s or len(s) % 4 or not re.fullmatch(r"[0-9a-fA-F]+", s):
        raise ValueError(f"bad word list {s!r}")
    return b"".join(
        int(s[i : i + 4], 16).to_bytes(2, "little") for i in range(0, len(s), 4)
    )


def pc_address(pc: int, present=lambda a: True) -> int:
    """Byte address of short-word pc (the 0x20000000 alias for the
    0xb80000 window when the low alias is empty)."""
    low = LOW + pc * 2
    if 0xB80000 <= pc < 0xC00000 and not present(low):
        return ALIAS + (pc - 0xB80000) * 2
    return low


def parse(text: str, present=lambda a: True) -> Patch:
    patch = Patch()
    for n, raw in enumerate(text.splitlines(), 1):
        line = re.split(r"[#;]|//", raw)[0].strip()
        if not line:
            continue
        t = line.split()
        key = t[0].upper()
        try:
            was = None
            if key in ("PM", "DM") and len(t) > 3:
                if t[3] != "was" or len(t) != 5:
                    raise ValueError("expected 'was <hex>'")
                was = t[4]
            if key == "BASE" and len(t) == 2:
                if not re.fullmatch(r"[0-9a-fA-F]{64}", t[1]):
                    raise ValueError("BASE needs a 64-digit sha256")
                patch.base_sha256 = t[1].lower()
            elif key == "PM" and len(t) >= 3:
                new = _words(t[2])
                if len(new) > 6:
                    raise ValueError("PM takes 4, 8 or 12 hex digits")
                old = _words(was) if was else None
                if old is not None and len(old) != len(new):
                    raise ValueError("'was' width differs from the new value")
                patch.ops.append(Op(pc_address(_hex(t[1]), present), new, old, line))
            elif key == "SW" and len(t) >= 3:
                patch.ops.append(
                    Op(
                        pc_address(_hex(t[1]), present),
                        b"".join(_words(w) for w in t[2:]),
                        None,
                        line,
                    )
                )
            elif key == "DM" and len(t) >= 3:
                patch.ops.append(
                    Op(
                        _hex(t[1]),
                        struct.pack("<I", _hex(t[2])),
                        struct.pack("<I", _hex(was)) if was else None,
                        line,
                    )
                )
            elif key in ("BYTES", "APPEND") and len(t) == 3:
                s = t[2].removeprefix("0x")
                patch.ops.append(
                    Op(_hex(t[1]), bytes.fromhex(s), None, line, key == "APPEND")
                )
            else:
                raise ValueError("cannot parse")
        except ValueError as e:
            raise SystemExit(f"patch line {n}: {e}: {line!r}") from e
    return patch


class Image:
    """The SHIM loader image blob: (address, bytes) segments."""

    def __init__(self, blob: bytes):
        if blob[:4] != b"SHIM":
            raise SystemExit("not a packed DSP image (no SHIM magic)")
        self.blob = bytearray(blob)
        (count,) = struct.unpack_from("<I", blob, 8)
        pos, self.segs = 12, []
        for _ in range(count):
            lo, n = struct.unpack_from("<II", blob, pos)
            self.segs.append((lo, pos + 8, n))
            pos += 8 + n

    def _find(self, addr: int, n: int) -> int | None:
        for lo, off, size in self.segs:
            if lo <= addr and addr + n <= lo + size:
                return off + addr - lo
        return None

    def present(self, addr: int) -> bool:
        return self._find(addr, 1) is not None

    def read(self, addr: int, n: int) -> bytes:
        off = self._find(addr, n)
        if off is None:
            raise SystemExit(f"{addr:#x}+{n}: not in one image segment")
        return bytes(self.blob[off : off + n])

    def write(self, addr: int, data: bytes) -> None:
        off = self._find(addr, len(data))
        if off is None:
            raise SystemExit(f"{addr:#x}+{len(data)}: not in one image segment")
        self.blob[off : off + len(data)] = data


def check(img: Image, patch: Patch, syx: Path | None) -> None:
    if syx is not None and patch.base_sha256:
        have = hashlib.sha256(syx.read_bytes()).hexdigest()
        if have != patch.base_sha256:
            raise SystemExit(f"BASE {patch.base_sha256} but {syx} is {have}")
    for op in patch.ops:
        if op.append:
            continue
        if op.was is not None and img.read(op.addr, len(op.was)) != op.was:
            raise SystemExit(
                f"{op.text!r}: image holds {img.read(op.addr, len(op.was)).hex()}"
            )


# Where an appended block may go: the L2 byte window past the stock loader (an append never overlaps loaded
# bytes, so this only keeps blocks out of L1, the SDRAM window and the MMR space).
L2_BYTE_BASE = 0x2000_0000
L2_BYTE_LIMIT = 0x2010_0000
# The boot ROM refuses a block aimed at its 8 KB workspace 0x200FE000..0x20100000 (HWR 40-2 Table 40-1 and
# note; tools/sharc_dev/sharc_bootcheck.py rule H9). That range is free at run time, but not bootable.
ROM_WORKSPACE_BASE = 0x200F_E000
APPEND_WINDOWS = ((L2_BYTE_BASE, ROM_WORKSPACE_BASE),)
# The ColdFire boot routine (FUN_400ccf74 on DT2 1.16) refuses a depacked section 7 over 0xfffff bytes.
MAX_STREAM = 0xFFFFF


def block_header(target: int, count: int, flags: int = 1, argument: int = 0) -> bytes:
    """A boot-stream block header for core 0: little-endian block code, target address, byte count
    and argument; byte 2 of the block code makes the XOR of all 16 bytes zero."""
    raw = bytearray(
        struct.pack("<IIII", 0xAD00_0000 | (flags & 0xFFFF), target, count, argument)
    )
    check = 0
    for b in raw:
        check ^= b
    raw[2] = check
    return bytes(raw)


def _loaded_memory(blob: bytes):
    from tools.sharc_dev import sharcldr

    return sharcldr.LoadedMemory.from_stream(blob)


class Stream:
    """The DSP boot stream (section 7) with writes mapped back into the file.

    Addresses are loader byte addresses, as in `Image`; a byte can be written
    only where a block with a payload (not a FILL block) last wrote it.
    """

    def __init__(self, blob: bytes):
        self.blob = bytearray(blob)
        self.mem = _loaded_memory(blob)

    def _offset(self, addr: int) -> int:
        found = self.mem._segment(addr)
        if found is None:
            raise SystemExit(f"{addr:#x}: the boot stream does not load this address")
        block = self.mem.blocks[found[1]]
        if block["fill"]:
            raise SystemExit(
                f"{addr:#x}: a FILL block loads this address (no stored bytes)"
            )
        return int(block["payload_offset"] + addr - block["target_address"])

    def present(self, addr: int) -> bool:
        return self.mem._segment(addr) is not None

    def read(self, addr: int, n: int) -> bytes:
        data = self.mem.read(addr, n)
        if data is None:
            raise SystemExit(f"{addr:#x}+{n}: not loaded by the boot stream")
        return bytes(data)

    def write(self, addr: int, data: bytes) -> None:
        for k, byte in enumerate(data):
            self.blob[self._offset(addr + k)] = byte

    def append_block(
        self,
        addr: int,
        data: bytes,
        windows: tuple[tuple[int, int], ...] = APPEND_WINDOWS,
    ) -> int:
        """Insert a data block of DATA at byte address ADDR before the FINAL block.

        The block is plain (flag word 1, argument 0, like the stream's own data blocks) and its
        payload is padded with zeros to a multiple of 4. Refused: a range outside WINDOWS, a range
        any block already writes (an append never changes loaded memory), an ADDR that is not a
        multiple of 4, and a stream whose size would pass MAX_STREAM (the ColdFire boot routine
        sends at most that many depacked bytes). Returns the new stream length."""
        if not data:
            raise SystemExit("APPEND: empty block")
        if addr % 4:
            raise SystemExit(f"APPEND {addr:#x}: the address is not a multiple of 4")
        payload = data + b"\0" * (-len(data) % 4)
        end = addr + len(payload)
        if not any(lo <= addr and end <= hi for lo, hi in windows):
            raise SystemExit(
                f"APPEND {addr:#x}..{end:#x}: outside the allowed windows "
                + ", ".join(f"{lo:#x}..{hi:#x}" for lo, hi in windows)
            )
        for a in range(addr, end, 4):
            if self.present(a) or self.present(a + 3):
                raise SystemExit(
                    f"APPEND {a:#x}: the boot stream already loads this address"
                )
        final = next(
            (b for b in reversed(self.mem.blocks) if "FINAL" in b["flags"]), None
        )
        if final is None:
            raise SystemExit("the boot stream has no FINAL block")
        block = block_header(addr, len(payload)) + payload
        if len(self.blob) + len(block) > MAX_STREAM:
            raise SystemExit(
                f"APPEND: the stream would be {len(self.blob) + len(block):#x} bytes, "
                f"more than the {MAX_STREAM:#x} the ColdFire boot routine sends"
            )
        at = int(final["offset"])
        self.blob[at:at] = block
        # Each FIRST block's ARGUMENT is the byte count to the next application (or the end of the stream,
        # HWR 40-85 pNextDxe; sharc_bootcheck H12): grow the ones that span the insertion point.
        for b in self.mem.blocks:
            if (
                "FIRST" in b["flags"]
                and b["offset"] < at <= b["offset"] + 16 + b["argument"]
            ):
                head = bytearray(self.blob[b["offset"] : b["offset"] + 16])
                struct.pack_into("<I", head, 12, b["argument"] + len(block))
                head[2] = 0
                head[2] = functools.reduce(operator.xor, bytes(head))
                self.blob[b["offset"] : b["offset"] + 16] = head
        self.mem = _loaded_memory(bytes(self.blob))
        return len(self.blob)

    def free_ranges(
        self, window: tuple[int, int] = (L2_BYTE_BASE, L2_BYTE_LIMIT), align: int = 4
    ) -> list[tuple[int, int]]:
        """Maximal ranges of WINDOW that no block of the stream writes (ALIGN-granular)."""
        out: list[tuple[int, int]] = []
        start = None
        for a in range(window[0], window[1], align):
            free = not self.present(a)
            if free and start is None:
                start = a
            elif not free and start is not None:
                out.append((start, a))
                start = None
        if start is not None:
            out.append((start, window[1]))
        return out


def section_blob(syx: Path, section: int) -> bytes:
    """The decoded bytes of one section of a .syx."""
    from tools.firmware.container import sections
    from tools.firmware.elz import depack_section
    from tools.firmware.extract import classify

    container, table = sections(str(syx))
    for sid, off, clen, _dest in table:
        if sid == section:
            kind, payload = classify(bytes(container[off : off + clen]))
            return depack_section(payload) if kind == "packed" else bytes(payload)
    raise SystemExit(f"{syx}: no section {section}")


def write_syx(base: Path, patch: Patch, out: Path) -> tuple[int, int]:
    """Apply PATCH to BASE's DSP stream and write the rebuilt .syx to OUT.

    Returns (edits, changed bytes)."""
    have = hashlib.sha256(base.read_bytes()).hexdigest()
    if (
        patch.base_sha256
        and patch.base_sha256 != "0" * 64
        and have != patch.base_sha256
    ):
        raise SystemExit(f"BASE {patch.base_sha256} but {base} is {have}")
    stream = Stream(section_blob(base, 7))
    changed = 0
    # In-place edits first, then the new blocks (an append never touches loaded bytes).
    for op in sorted(patch.ops, key=lambda o: o.append):
        if op.append:
            stream.append_block(op.addr, op.new)
            changed += len(op.new)
            continue
        if op.was is not None and stream.read(op.addr, len(op.was)) != op.was:
            raise SystemExit(
                f"{op.text!r}: the stream holds {stream.read(op.addr, len(op.was)).hex()}"
            )
        before = stream.read(op.addr, len(op.new))
        changed += sum(a != b for a, b in zip(before, op.new, strict=True))
        stream.write(op.addr, op.new)
    from tools.firmware.build import FLASH_CONTAINER_MAX, rebuild

    out.write_bytes(
        rebuild(
            str(base),
            {7: bytes(stream.blob)},
            pack="aplib",
            max_container=FLASH_CONTAINER_MAX,
        )
    )
    return len(patch.ops), changed


def parse_for_stream(text: str, base: Path) -> Patch:
    return parse(text, Stream(section_blob(base, 7)).present)


def main(argv: list[str]) -> int:
    if argv[:1] == ["syx"]:
        s = argparse.ArgumentParser(prog="dsp_patch.py syx")
        s.add_argument("command")
        s.add_argument("base", type=Path)
        s.add_argument("patch", type=Path)
        s.add_argument("out", type=Path)
        sa = s.parse_args(argv)
        edits, changed = write_syx(
            sa.base, parse_for_stream(sa.patch.read_text(), sa.base), sa.out
        )
        print(f"{sa.out}: {edits} edits, {changed} changed bytes")
        return 0
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("command", choices=["check", "apply", "show"])
    p.add_argument("image", type=Path)
    p.add_argument("patch", type=Path)
    p.add_argument("out", type=Path, nargs="?")
    p.add_argument("--syx", type=Path, help="verify the patch's BASE against this")
    a = p.parse_args(argv)
    img = Image(a.image.read_bytes())
    patch = parse(a.patch.read_text(), img.present)
    check(img, patch, a.syx)
    if any(op.append for op in patch.ops):
        raise SystemExit("APPEND adds boot-stream blocks: use the `syx` command")
    for op in patch.ops:
        before = img.read(op.addr, len(op.new))
        if a.command == "show":
            print(f"{op.addr:#010x}: {before.hex()} -> {op.new.hex()}   ({op.text})")
        if a.command == "apply":
            img.write(op.addr, op.new)
    if a.command == "apply":
        if a.out is None:
            raise SystemExit("apply needs OUT.bin")
        a.out.write_bytes(bytes(img.blob))
        print(f"{a.out}: {len(img.blob)} bytes, {len(patch.ops)} edits")
    elif a.command == "check":
        print(f"ok: {len(patch.ops)} edits")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
