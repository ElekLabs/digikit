#!/usr/bin/env python3
"""Read memory/registers/task state out of an emu snapshot without restoring
a Unicorn machine.

Investigation of a snapshot is mostly read-only: "what does track 3's row
look like in this capture", "which tasks exist and where is each one's
stack". This reads the Rust DT2SNP01 format and the old pickled Python
format with a plain read/u32/u16/u8/regs/tasks API, no machine.

    from tools.emulator.snapread import Snapshot
    s = Snapshot("out/snapshots/dt2-1.16-drive3/loaded.snap")
    s.u32(0x80004704)                  # -> int or None (unmapped)
    s.read(0x80003cd0, 0x9a)           # -> bytes or None
    s.regs["pc"]                       # -> int
    s.tasks()                          # -> [{tcb, entry, entry_name, prio, ...}, ...]

CLI:
    uv run python -m tools.emulator.snapread SNAP read ADDR N   # hex dump, N bytes
    uv run python -m tools.emulator.snapread SNAP u32 ADDR
    uv run python -m tools.emulator.snapread SNAP u16 ADDR
    uv run python -m tools.emulator.snapread SNAP u8 ADDR
    uv run python -m tools.emulator.snapread SNAP regs
    uv run python -m tools.emulator.snapread SNAP tasks

A firmware function name (the address map, tools/devtools/addrnames.py) is shown next to `regs`' pc/a-registers and `tasks`' entry/tcb
when the CLI prints them. The image comes from the snapshot's path (dt2-1.17, dn2-1.12, ...) or `--image IMAGE`; an image without a
port (devices/ports/) has no names.
"""

from __future__ import annotations

import pickle
import re
import struct
import sys
from typing import Any

from tools.devtools.addrnames import AddrNames, AddrNamesError


class _SnapshotUnpickler(pickle.Unpickler):
    """Python snapshots contain only primitive state, never executable classes."""

    def find_class(self, module, name):
        raise pickle.UnpicklingError(
            "checkpoint may not contain %s.%s" % (module, name)
        )


def _load_blob(path):
    """A pickled Python-machine snapshot (the Unicorn-era format)."""
    with open(path, "rb") as f:
        blob = _SnapshotUnpickler(f).load()
    if not isinstance(blob, dict) or any(
        k not in blob
        for k in ("regs", "pages", "all_mapped", "mmio", "ctlregs", "extra")
    ):
        raise RuntimeError("invalid checkpoint blob")
    return blob


PAGE = 0x100000
RUST_MAGIC = b"DT2SNP01"


def _load_rust_pages(path: str) -> dict[int, bytearray]:
    """Memory pages of a Rust DT2SNP01 snapshot (packages/emulator/machine board.rs
    `BORD` section): u64 LE page count, then per page a u32 LE index (address
    >> 20) and 256 chunks of 4 KiB, each preceded by a presence byte. Regs and
    the rest of the file are not parsed."""
    with open(path, "rb") as fh:
        d = fh.read()
    i = d.find(b"BORD")
    if i < 0:
        raise ValueError("%s: no BORD section" % path)
    (n,) = struct.unpack_from("<Q", d, i + 4)
    p = i + 12
    pages: dict[int, bytearray] = {}
    for _ in range(n):
        (idx,) = struct.unpack_from("<I", d, p)
        p += 4
        pg = bytearray(PAGE)
        for b in range(256):
            f = d[p]
            p += 1
            if f:
                pg[b * 4096 : (b + 1) * 4096] = d[p : p + 4096]
                p += 4096
        pages[idx] = pg
    return pages


_names: AddrNames | None = None


def use_image(image: str | None) -> None:
    """Name addresses with the firmware names of IMAGE (`dt2-1.17`); None, or an image without a port: no names."""
    global _names
    _names = None
    if image is None:
        return
    try:
        _names = AddrNames.for_stock(image)
    except AddrNamesError as e:
        print("snapread: no names (%s)" % e, file=sys.stderr)


def image_of(path: str) -> str | None:
    """The image a snapshot path names: out/snapshots/dt2-1.17-x/loaded.snap -> dt2-1.17."""
    m = re.search(r"(?:dt2|dn2)-\d+\.\d+[A-Z]?", path)
    return m.group(0) if m else None


def name_for(addr: int) -> str | None:
    """The firmware function name of addr (name+0xoff inside a function), or None."""
    if _names is None or addr is None:
        return None
    return _names.name(addr)


class Snapshot:
    """A read-only view of one unicorn-reference-final:emu/snapshot.py .snap file: registers, mapped
    memory (pages decompressed lazily and cached, since a capture like
    out/snapshots/dt2-1.16-drive3/loaded.snap can hold well over a hundred MB
    once decompressed), MMIO/control-register shadow state, and whatever
    `extra` the capturing tool stored (task_create hits, instruction count,
    ...). Reads the pickled blob with _load_blob
    rather than re-implementing the pickle/zlib format here."""

    def __init__(self, path: str):
        self.path = path
        self._page_cache: dict[int, bytes] = {}
        with open(path, "rb") as fh:
            magic = fh.read(8)
        if magic == RUST_MAGIC:
            # Rust DT2SNP01 file (packages/emulator/boot runtime.rs): memory only.
            pages = _load_rust_pages(path)
            self._blob: dict[str, Any] = {
                "regs": {},
                "mmio": {},
                "ctlregs": {},
                "extra": {},
            }
            self._mapped = {i << 20 for i in pages}
            self._page_cache = {i << 20: bytes(pg) for i, pg in pages.items()}
            return
        self._blob = _load_blob(path)
        self._mapped = set(self._blob["all_mapped"])

    # --- raw blob bits, exposed as-is -----------------------------------

    @property
    def regs(self) -> dict[str, int]:
        return dict(self._blob["regs"])

    @property
    def mmio(self) -> dict[int, int]:
        return dict(self._blob["mmio"])

    @property
    def ctlregs(self) -> dict[int, int]:
        return dict(self._blob["ctlregs"])

    @property
    def extra(self) -> dict:
        return self._blob["extra"]

    @property
    def components(self) -> list[str]:
        return sorted(self._blob.get("components", {}))

    @property
    def mapped_bases(self) -> list[int]:
        return sorted(self._mapped)

    # --- memory ------------------------------------------------------------

    def _page(self, base: int) -> bytes | None:
        """The decompressed PAGE-sized page at `base`, or None if `base` is
        mapped but was an all-zero page at capture time (unicorn-reference-final:emu/snapshot.py's save()
        only stores a page whose bytes aren't all zero -- see its own `if
        data.strip(b"\\x00")` check). Caller must confirm `base` is mapped
        first; this returns None for that case too, so a bare call can't
        tell "zero page" from "not mapped" -- read()/u32() etc. use
        self._mapped for that distinction."""
        if base in self._page_cache:
            return self._page_cache[base]
        comp = self._blob["pages"].get(base)
        if comp is None:
            return None
        import zlib

        data = zlib.decompress(comp)
        self._page_cache[base] = data
        return data

    def read(self, addr: int, n: int) -> bytes | None:
        """n bytes starting at addr, possibly spanning several pages, or
        None if any byte in the range is unmapped."""
        if n <= 0:
            return b""
        out = bytearray()
        a, remaining = addr, n
        while remaining > 0:
            base = a & ~(PAGE - 1)
            if base not in self._mapped:
                return None
            offset = a - base
            chunk = min(remaining, PAGE - offset)
            page = self._page(base)
            out += bytes(chunk) if page is None else page[offset : offset + chunk]
            a += chunk
            remaining -= chunk
        return bytes(out)

    def u32(self, addr: int) -> int | None:
        r = self.read(addr, 4)
        return struct.unpack(">I", r)[0] if r is not None else None

    def u16(self, addr: int) -> int | None:
        r = self.read(addr, 2)
        return struct.unpack(">H", r)[0] if r is not None else None

    def u8(self, addr: int) -> int | None:
        r = self.read(addr, 1)
        return r[0] if r is not None else None

    def s32(self, addr: int) -> int | None:
        r = self.read(addr, 4)
        return struct.unpack(">i", r)[0] if r is not None else None

    def s16(self, addr: int) -> int | None:
        r = self.read(addr, 2)
        return struct.unpack(">h", r)[0] if r is not None else None

    # --- tasks ---------------------------------------------------------------

    def tasks(self) -> list[dict]:
        """One dict per TASK_CREATE hit this capture recorded (see
        unicorn-reference-final:emu/dspboot.py's do_task_create): call_site, tcb, entry, prio, stack,
        stacksize, n (instruction count at capture), plus entry_name/
        call_site_name from the address map (`use_image`) when known. [] if this
        snapshot's `extra` has no 'tasks' key (most snapshots don't -- only
        ones taken with task-tracking state threaded through, see
        unicorn-reference-final:emu/snapshot.py's restore_into's own st['task_create_hits'] merge),
        sorted by prio like the scratch queries that came before this did."""
        raw = self.extra.get("tasks")
        if not raw:
            return []
        out = []
        for site_hex, info in raw.items():
            site = int(site_hex, 16) if isinstance(site_hex, str) else site_hex
            entry = info.get("entry")
            out.append(
                {
                    "call_site": site,
                    "call_site_name": name_for(site),
                    "tcb": info.get("tcb"),
                    "entry": entry,
                    "entry_name": name_for(entry) if entry is not None else None,
                    "prio": info.get("prio"),
                    "stack": info.get("stack"),
                    "stacksize": info.get("stacksize"),
                    "n": info.get("n"),
                }
            )
        out.sort(key=lambda d: (d["prio"] is None, d["prio"]))
        return out


# --- CLI ---------------------------------------------------------------------


def _fmt_addr(a):
    if a is None:
        return "None"
    nm = name_for(a)
    return "0x%08x (%s)" % (a, nm) if nm else "0x%08x" % a


def _cmd_read(s, argv):
    addr = int(argv[0], 16)
    n = int(argv[1], 0) if len(argv) > 1 else 16
    data = s.read(addr, n)
    if data is None:
        print("0x%08x: unmapped" % addr)
        return 1
    for i in range(0, len(data), 16):
        chunk = data[i : i + 16]
        hexpart = " ".join("%02x" % b for b in chunk)
        asciipart = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        print("0x%08x  %-47s  %s" % (addr + i, hexpart, asciipart))
    return 0


def _cmd_scalar(s, argv, reader, width):
    addr = int(argv[0], 16)
    v = reader(addr)
    if v is None:
        print("0x%08x: unmapped" % addr)
        return 1
    nm = name_for(addr)
    suffix = "  (%s)" % nm if nm else ""
    print("0x%08x = 0x%0*x%s" % (addr, width, v, suffix))
    return 0


def _cmd_regs(s, argv):
    for k, v in s.regs.items():
        nm = name_for(v) if k in ("pc",) or k.startswith("a") else None
        print("%-4s 0x%08x%s" % (k, v, "  (%s)" % nm if nm else ""))
    return 0


def _cmd_tasks(s, argv):
    tasks = s.tasks()
    if not tasks:
        print("no task-tracking state in this snapshot's extra")
        return 1
    for t in tasks:
        print(
            "prio=%-3s tcb=0x%08x entry=%s stack=0x%08x size=0x%x call_site=%s n=%s"
            % (
                t["prio"],
                t["tcb"] or 0,
                _fmt_addr(t["entry"]),
                t["stack"] or 0,
                t["stacksize"] or 0,
                _fmt_addr(t["call_site"]),
                t["n"],
            )
        )
    return 0


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) < 2:
        print(__doc__.split("CLI:")[1].strip("\n"), file=sys.stderr)
        return 2
    argv = list(argv)
    image = None
    if "--image" in argv:
        i = argv.index("--image")
        image = argv[i + 1]
        del argv[i : i + 2]
    path, cmd, rest = argv[0], argv[1], argv[2:]
    use_image(image or image_of(path))
    s = Snapshot(path)
    if cmd == "read":
        return _cmd_read(s, rest)
    if cmd == "u32":
        return _cmd_scalar(s, rest, s.u32, 8)
    if cmd == "u16":
        return _cmd_scalar(s, rest, s.u16, 4)
    if cmd == "u8":
        return _cmd_scalar(s, rest, s.u8, 2)
    if cmd == "regs":
        return _cmd_regs(s, rest)
    if cmd == "tasks":
        return _cmd_tasks(s, rest)
    print("unknown command %r" % cmd, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
