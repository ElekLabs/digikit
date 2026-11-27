"""The .dkm header and the wait for MODULES that the preview upload of tools/patchkit/pk_devreload.py uses.

The install, list and delete of run-time modules are the workbench's code (packages/workbench-core/src/modules/dktransfer.ts), driven from the
command line by `mise run transfer` (tools/devtools/wb_transfer.mjs)."""

from __future__ import annotations

import struct
from collections.abc import Callable
from typing import Any

from tools.device import dkproto
from tools.device import dkupload as D

MAGIC = b"DKMF"
KEYS_TAG = 3  # the KEYS entry of the header table (packages/dkneeds/src/keys.rs)


class TransferError(RuntimeError):
    pass


def header_info(h: bytes) -> dict[str, Any] | None:
    """The fields of a .dkm header (docs/design/digikit-dkm.md) that `list` shows; None when `h` is not one."""
    if len(h) < D.HEADER_BYTES or h[:4] != MAGIC:
        return None
    size, _hdr = struct.unpack_from(">II", h, 8)
    version = struct.unpack_from(">I", h, 0x34)[0]
    image, bss = struct.unpack_from(">II", h, 0x64)
    dsp = struct.unpack_from(">I", h, 0x9C)[0]
    return {
        "name": h[0x40:0x60].split(b"\0")[0].decode("ascii", "replace"),
        "version": D.fmt_runtime(version),
        "group": group_of(h),
        "size": size,
        "arena": image + bss,
        "dsp": dsp,
    }


def group_of(f: bytes) -> str:
    """Services for a file whose role word (header 0x38) is 1, Machines for a file whose keys table lists a machine, Effects for one that lists an
    effect, else Tools. The table is at the header's 0x90 pair (offset, size); a header alone, without the table, gives Tools. Table versions
    1 to TABLE_VERSION are read."""
    off, size = struct.unpack_from(">II", f, 0x90)
    t = f[off : off + size]
    if size < 8 or len(t) != size:
        return "Tools"
    magic, version = struct.unpack_from(">IH", t, 0)
    if magic != dkproto.TABLE_MAGIC or not 1 <= version <= dkproto.TABLE_VERSION:
        return "Tools"
    if struct.unpack_from(">I", f, dkproto.H_ROLE)[0] == 1:
        return "Services"
    kinds: set[int] = set()
    at = 8
    for _ in range(struct.unpack_from(">H", t, 6)[0]):
        tag, count, length = struct.unpack_from(">HHI", t, at)
        body = at + 8
        if tag == KEYS_TAG:
            g = body
            for _ in range(count):
                kind, n = struct.unpack_from(">II", t, g)
                if n:
                    kinds.add(kind)
                g += 8 + 4 * n
        at = body + length
    if dkproto.RECORD_KIND["MACHINE"] in kinds:
        return "Machines"
    if dkproto.RECORD_KIND["EFFECT"] in kinds:
        return "Effects"
    return "Tools"


# ---- the repo's modules ---------------------------------------------------------------------------------------------------------


def wait_loaded(
    up: D.Uploader,
    names: list[str],
    sleep: Callable[[float], None],
    now: Callable[[], float],
    timeout_s: float = 60.0,
    paths: list[str] | None = None,
) -> list[str]:
    """Poll MODULES until every name is listed; returns the loaded names. Raises TransferError at the timeout, saying what is
    wrong when it can tell: files that are not on the card after the restart (the restart lost the card's writes), or on the card
    but not loaded (see the log)."""
    end = now() + timeout_s
    got: list[str] = []
    while True:
        try:
            got = [m["name"] for m in up.modules()["modules"]]
        except (TimeoutError, D.UploadError):
            got = []
        if all(n in got for n in names):
            return got
        if now() >= end:
            miss = [n for n in names if n not in got]
            why = "see the log: dk:log"
            try:
                on_card = {p for p, _, _ in up.ls()}
                lost = [p for p in paths or [] if p not in on_card]
                if lost:
                    why = (
                        f"the card no longer holds {', '.join(lost)}: the restart lost the card's writes "
                        "(an emulator started without a card file needs the soft-reset fix)"
                    )
                elif paths:
                    why = "the files are on the card but did not load: see the log, dk:log"
            except (TimeoutError, D.UploadError):
                pass
            raise TransferError(
                f"MODULES does not list {', '.join(miss)} after the restart ({why})"
            )
        sleep(0.5)
