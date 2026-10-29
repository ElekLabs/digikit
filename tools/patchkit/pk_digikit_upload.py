#!/usr/bin/env python3
"""The `upload` scenarios of the digikit runtime: module upload over USB SysEx in the emulator (docs/design/digikit-upload.md,
docs/findings/43-digikit-upload.md). Run by `tools.patchkit.pk_digikit_scen` (kind `upload`); the scenario's `[upload] part` picks one:

  ops       the commands on a ColdFire-only rig: INFO, LIST, put (several sizes), replace, DELETE, CLEAR, E_FULL, E_HASH, a DATA retry,
            bad offset / length / state / path / version. After each step the card, read by the plusdrive reader from the card file
            with the guest's writes laid over it, equals what the device's LIST says and the bytes that were sent.
  cut       a power cut after the Nth card write of an upload (the runtime's test knob `DKRT_UPLOAD_CUT` refuses every later write, so the
            card is the state a cut leaves): during staging, after the table, between header B and header A, after A. A new daemon boots
            on that card and reads a valid store (old or new, never mixed), and takes another upload.
  stock     a stock MidiRpc Ping, SoftwareVersion and DeviceUID answer byte for byte the same with and without the runtime, and a SysEx
            that starts F0 7D but is not ours still goes the stock way.
  playback  (coupled) a note plays while a module is uploaded: its PCM equals the same note without the upload, and the card writes of
            the run are store sectors only.
  rust      the Rust host `Session` (packages/dkupload example `emu_session`, the code of the desktop Modules panel and the web page)
            against the device side: INFO, LIST, put of a built module and of 70,000 bytes, replace, DELETE, E_HASH, lost replies, CLEAR; each
            step compared with the Python tool's LIST and the card read by plusdrive.
  quick     a short `ops` (one upload, replace, delete) for the verify gate.
  clearcut  two faults in a row (finding 43): a cut between header B and header A (of a CLEAR, of an upload), then a torn header B on
            the next upload; the store still reads as the committed store and takes another upload.
  screenshot  the stock MidiRpc Screenshot (0x04) through tools/device/dkscreen.py equals the emulator's frame capture, pixel for pixel.
  txfull    a host that never reads: the in-flight word of the firmware's TX function is set high; replies are dropped (counted), none
            reaches the output, a stock Ping still answers, and the reply is sent again once the ring has room.

The through-the-menu proof (an uploaded module appears in DIGIKIT > MODULES, loads after APPLY & RESTART? and plays like the
card-built module) is the `menu` kind with `upload = true` (tests/digikit/scenarios/menu-upload-*.toml).
"""

from __future__ import annotations

import hashlib
import random
import struct
from pathlib import Path
from typing import Any

from tools.device import dkupload as D
from tools.patchkit import pk_dkrt_boot as B
from tools.patchkit import pk_dkrt_machine as M

# The delivery model of the emulated USB input: one high-speed microframe (a 512-byte packet carries at most 384 MIDI bytes)
HS: dict[str, int] = {"chunk": 384, "period": 16500}


# The test knob `DKRT_STORE_CAP` of the runtime keeps every sector from here to header B out of the allocations, so the DN2's 20 GiB store can be filled
# (and its free bytes fit INFO's 32 bits).
CAP_SECTORS = 0x40000


def apply_cap(c: Any, rig: Any, img: Any) -> None:
    """Take a store of more than 4 GiB down to `CAP_SECTORS` (128 MiB) on this rig; `free_fresh` follows."""
    if c.store.limit * 512 > 0xFFFFFFFF:
        rig.emu.poke32(M.sym(img.plan, "core/runtime", "DKRT_STORE_CAP"), CAP_SECTORS)
        c.store_cap = CAP_SECTORS


def free_fresh(c: Any) -> int:
    """INFO's free bytes of a card with no store: used 1, room for a one-entry table (the DN2's is cut by the cap)."""
    limit = getattr(c, "store_cap", None) or c.store.limit
    return min((limit - 1 - 1) * 512, 0xFFFFFFFF)


def blob(n: int, seed: int) -> bytes:
    """Deterministic bytes with every value (the 8-in-7 packing needs the high bits)."""
    return random.Random(seed).randbytes(n)


def transport(rig: Any) -> D.EmuTransport:
    return D.EmuTransport(rig.emu, **HS)


def sha(b: bytes) -> bytes:
    return hashlib.sha256(b).digest()


# ---- the card as the plusdrive reader sees it -----------------------------------------------------------------------------------


def card_view(
    c: Any, rig: Any, base: Path, tag: str
) -> tuple[dict[str, bytes], str, set[int]]:
    """The module store of BASE with the guest's writes laid over it: ({path: bytes}, the reader's header line, the sectors the guest
    wrote anywhere). The files are read (and their sha256 checked) by `plusdrive store ls --cat`."""
    from tools.coldfire import arena_safety
    from tools.patchkit.pk_digikit_project import card_after

    ov = rig.work / f"{tag}.ov.bin"
    rig.emu.call("card.overlay", path=str(ov))
    after = card_after(c, base, ov)
    writes = set(arena_safety.parse_overlay(ov.read_bytes()))
    import subprocess

    tools = c.env.tools
    r = tools.run(tools.plusdrive, "store", "ls", after, "--device", c.device)
    if r.returncode:
        return {}, f"error: {r.stderr.strip()}", writes
    files: dict[str, bytes] = {}
    head = ""
    for line in r.stdout.splitlines():
        if line.startswith("#"):
            head = line
            continue
        path = line.split("\t")[0]
        if line.endswith("\tfolder"):  # a folder with nothing in it: no file to read
            continue
        out = rig.work / f"{tag}.cat.bin"  # binary: through a file, not a decoded pipe
        with out.open("wb") as f:
            p = subprocess.run(
                [str(tools.plusdrive), "store", "ls", str(after), "--device", c.device, "--cat", path],
                stdout=f,
                stderr=subprocess.PIPE,
            )  # fmt: skip
        files[path] = out.read_bytes() if p.returncode == 0 else b"<unreadable>"
    return files, head, writes


def device_ls(up: D.Uploader) -> dict[str, tuple[int, str]]:
    return {p: (s, h) for p, s, h in up.ls()}


def same(
    c: Any,
    label: str,
    up: D.Uploader,
    files: dict[str, bytes],
    view: dict[str, bytes],
    head: str,
    version: int = 3,
) -> None:
    """The device's LIST, the reader's view and the bytes that were sent all agree."""
    want = {p: (len(b), sha(b).hex()) for p, b in files.items()}
    got = device_ls(up)
    c.check(f"{label}: LIST equals the files sent", got == want, f"{got} want {want}")
    c.check(
        f"{label}: the card, read by plusdrive, holds the same bytes",
        view == files,
        f"{ {p: len(b) for p, b in view.items()} } {head}",
    )
    c.check(f"{label}: store version {version}", f"version={version}" in head, head)


def put_status(up: D.Uploader, path: str, data: bytes, **kw: Any) -> int:
    """`put` and the status it ended with (0 for success)."""
    try:
        up.put(path, data, **kw)
        return 0
    except D.UploadError as e:
        return e.status


# ---- ops ------------------------------------------------------------------------------------------------------------------------


def run_ops(c: Any, quick: bool = False) -> None:
    img = c.image_for(B.RT, "run")
    base = c.card(None)
    files: dict[str, bytes] = {}
    with c.rig(img, base, "ops", cf_only=True) as rig:
        apply_cap(c, rig, img)
        up = D.Uploader(transport(rig))
        info = up.info()
        c.check(
            "INFO: protocol 1, the runtime's version, the device, the free bytes of an empty store, 32 KiB chunks",
            info["protocol"] == 1
            and info["runtime"] == B.RUNTIME_VERSION
            and info["port"] == c.device
            and info["free_bytes"] == free_fresh(c)
            and info["max_chunk"] == 32768
            and not info["safe"]
            and not info["upload_open"],
            str(info),
        )
        c.check("LIST of a card with no store: empty", up.ls() == [])

        def check(label: str) -> None:
            view, head, writes = card_view(c, rig, base, f"ops-{label}")
            same(c, label, up, files, view, head)
            c.store_writes |= {w for w in writes if c.store.first <= w < c.store.end}
            c.all_writes |= writes

        a = blob(70_000, 1)  # three chunks, the last partial
        files["modules/a.dkm"] = a
        c.check("put a 70,000-byte file", put_status(up, "modules/a.dkm", a) == 0)
        check("first upload")
        read_checks(c, up, "modules/a.dkm", a)
        if quick:
            _ops_quick(c, up, files, check)
        else:
            _ops_full(c, up, files, check)
            roundtrip(c, rig, base, files)
        c.report["device_requests"] = len(up.log)
        c.report["emu_instructions_stepped"] = up.t.stepped
        c.done(rig)
    _selection_untouched(c)
    c.common_checks_upload()


def read_checks(c: Any, up: D.Uploader, path: str, data: bytes) -> None:
    """READ (runtime 0.2.4): the header, a range across a sector boundary, the 768-byte cap and the end of the file equal the bytes sent;
    a missing file, offset past the end and length 0 are refused."""
    c.check(
        "READ: the first 0xa0 bytes equal the file sent",
        up.header(path) == data[: D.HEADER_BYTES],
    )
    got = up.read(path, 500, 700)
    c.check(
        "READ: a range across a sector boundary, and the file size",
        got == (len(data), data[500:1200]),
        f"{got[0]} {len(got[1])}",
    )
    got = up.read(path, 1, 5000)
    c.check(
        "READ: a long request returns 768 bytes",
        got[1] == data[1:769],
        str(len(got[1])),
    )
    tail = up.read(path, len(data) - 10, 100)
    c.check("READ: a range past the end is cut at the end", tail[1] == data[-10:])
    c.check("READ: the whole file by 768-byte reads", up.read_file(path) == data)
    c.check(
        "VERIFY: the device's sha256 of the file on the card equals the local one",
        up.verify(path) == hashlib.sha256(data).hexdigest(),
    )
    try:
        up.verify("modules/none.dkm")
        got_status = 0
    except D.UploadError as e:
        got_status = e.status
    c.check("VERIFY: a missing file is refused", got_status == -59, str(got_status))
    for what, args, status in (
        ("a missing file", ("modules/none.dkm", 0, 4), -59),
        ("offset at the end", (path, len(data), 4), -54),
        ("length 0", (path, 0, 0), -55),
    ):
        try:
            up.read(*args)
            got_status = 0
        except D.UploadError as e:
            got_status = e.status
        c.check(f"READ: {what} is refused", got_status == status, str(got_status))


def _ops_quick(c: Any, up: D.Uploader, files: dict[str, bytes], check: Any) -> None:
    b = blob(512, 2)
    files["modules/b.dkm"] = b
    up.put("modules/b.dkm", b)
    a2 = blob(600, 3)
    files["modules/a.dkm"] = a2
    up.put("modules/a.dkm", a2)
    check("replace")
    up.rm("modules/b.dkm")
    del files["modules/b.dkm"]
    check("delete")


def _ops_full(c: Any, up: D.Uploader, files: dict[str, bytes], check: Any) -> None:
    # sizes around a sector, a one-byte file and an empty file, a nested path, sorted insert before and after
    for path, n, seed in [
        ("a/z.bin", 513, 4),
        ("modules/b.dkm", 512, 5),
        ("modules/c.dkm", 1, 6),
        ("empty", 0, 7),
        ("modules/big.dkm", 65_536, 8),
    ]:
        files[path] = blob(n, seed)
        st = put_status(up, path, files[path])
        c.check(f"put {path} ({n} bytes)", st == 0, str(st))
    check("six files")
    # replace
    files["modules/a.dkm"] = blob(1000, 9)
    up.put("modules/a.dkm", files["modules/a.dkm"])
    check("replace")
    # delete, and a delete of a file that is not there
    up.rm("modules/c.dkm")
    del files["modules/c.dkm"]
    st, _ = up.request(D.CMD_DELETE, b"modules/c.dkm\0")
    c.check("DELETE of a missing path: E_NOFILE", st == -59, str(st))
    check("delete")
    # E_FULL: BEGIN past the free bytes answers with them; BEGIN of exactly the free bytes is accepted
    free = up.info()["free_bytes"]
    st, body = up.request(D.CMD_BEGIN, b"x\0" + struct.pack(">I", free + 1) + bytes(32))
    c.check(
        "BEGIN past the free bytes: E_FULL with the free bytes",
        st == -52 and struct.unpack(">I", body[:4])[0] == free,
        f"{st} {body.hex()} free {free}",
    )
    # (free space is the sum of the gaps: a file as big as that is accepted only on a store with one gap, tried after CLEAR below)
    st, _ = up.request(D.CMD_BEGIN, b"x\0" + struct.pack(">I", 512) + bytes(32))
    c.check("BEGIN of one sector is accepted", st == 0, str(st))
    c.check("INFO: an upload is open", up.info()["upload_open"])
    up.abort()
    c.check("ABORT closes it", not up.info()["upload_open"])
    # E_HASH: nothing is committed
    bad = blob(40_000, 10)
    st = put_status(up, "modules/bad.dkm", bad, sha256=sha(b"not the file"))
    c.check("a wrong sha256: E_HASH", st == -56, str(st))
    c.check("an upload ended by END is closed", not up.info()["upload_open"])
    check("after E_HASH (nothing committed)")
    # DATA: retry, bad offset, bad length, no upload, too much
    d = blob(2048, 11)
    up.begin("retry.bin", len(d), sha(d))
    st, _ = up.request(D.CMD_DATA, struct.pack(">I", 0) + d[:1024])
    st2, _ = up.request(D.CMD_DATA, struct.pack(">I", 0) + d[:1024])
    c.check(
        "a DATA retry (same offset, same length) is ok", st == 0 == st2, f"{st} {st2}"
    )
    st, _ = up.request(D.CMD_DATA, struct.pack(">I", 512) + d[512:1024])
    c.check(
        "a DATA at an offset that is neither next nor a retry: E_OFFSET",
        st == -54,
        str(st),
    )
    st, _ = up.request(D.CMD_DATA, struct.pack(">I", 1024) + d[1024:1500])
    c.check(
        "a DATA that is not a multiple of 512 and not the last: E_LENGTH",
        st == -55,
        str(st),
    )
    st, _ = up.request(D.CMD_DATA, struct.pack(">I", 1024) + d[1024:] + b"x")
    c.check("a DATA past the file's size: E_LENGTH", st == -55, str(st))
    st, _ = up.request(D.CMD_DATA, struct.pack(">I", 1024) + b"")
    c.check("a DATA with no bytes: E_MSG or E_LENGTH", st in (-50, -55), str(st))
    st, _ = up.request(D.CMD_END)
    c.check("END before all bytes: E_LENGTH", st == -55, str(st))
    up.begin("retry.bin", len(d), sha(d))
    up.request(D.CMD_DATA, struct.pack(">I", 0) + d[:1024])
    up.request(D.CMD_DATA, struct.pack(">I", 0) + d[:1024])
    st, _ = up.request(D.CMD_DATA, struct.pack(">I", 1024) + d[1024:])
    c.check("the rest of the file after the retry", st == 0, str(st))
    up.end()
    files["retry.bin"] = d
    check("after the retry")
    st, _ = up.request(D.CMD_DATA, struct.pack(">I", 0) + d[:512])
    c.check("DATA with no upload open: E_STATE", st == -53, str(st))
    st, _ = up.request(D.CMD_END)
    c.check("END with no upload open: E_STATE", st == -53, str(st))
    # paths and messages
    for p in ["", "/a", "a//b", "a/", "a/../b", "./a", "x" * 160]:
        st, _ = up.request(
            D.CMD_BEGIN, p.encode() + b"\0" + struct.pack(">I", 1) + bytes(32)
        )
        c.check(f"BEGIN with the path {p[:12]!r}: E_PATH", st == -51, str(st))
    st, _ = up.request(D.CMD_BEGIN, b"x\0\0")
    c.check("a short BEGIN: E_MSG", st == -50, str(st))
    st, _ = up.request(D.CMD_CLEAR, struct.pack(">I", 1))
    c.check("CLEAR without its key: E_MSG", st == -50, str(st))
    st, _ = up.request(D.CMD_INFO, b"", version=2)
    c.check("another protocol version: E_PROTO", st == -60, str(st))
    st, _ = up.request(0x33)
    c.check("an unknown command: E_PROTO", st == -60, str(st))
    c.check(
        "nothing above changed the store",
        device_ls(up) == {p: (len(b), sha(b).hex()) for p, b in files.items()},
    )
    # CLEAR
    up.clear()
    files.clear()
    check("CLEAR")
    info = up.info()
    c.check(
        "CLEAR gives the space back", info["free_bytes"] == free_fresh(c), str(info)
    )
    # one gap: a file of exactly the free bytes is accepted (the table goes beside it), one byte more is not
    st, _ = up.request(
        D.CMD_BEGIN, b"x\0" + struct.pack(">I", info["free_bytes"]) + bytes(32)
    )
    c.check("BEGIN of exactly the free bytes is accepted", st == 0, str(st))
    up.abort()
    files["again.dkm"] = blob(5000, 12)
    up.put("again.dkm", files["again.dkm"])
    check("an upload after CLEAR")


def roundtrip(c: Any, rig: Any, base: Path, files: dict[str, bytes]) -> None:
    """The +Drive round trip: `plusdrive export` of the card with the guest's writes writes the store's files to `digikit/` of a folder,
    and `plusdrive build` of that folder gives a card whose store holds the same files."""
    import shutil

    tools = c.env.tools
    ov = rig.work / "roundtrip.ov.bin"
    rig.emu.call("card.overlay", path=str(ov))
    top = rig.work / "roundtrip"
    folder = top / "plus_drive" / c.device
    shutil.rmtree(top, ignore_errors=True)
    folder.mkdir(parents=True)
    export = [tools.plusdrive, "export", base, "--overlay", ov, "-o", folder,
              "--out", top / "out", "--device", c.device]  # fmt: skip
    r = tools.run(*export)
    got = {
        str(p.relative_to(folder / "digikit")): p.read_bytes()
        for p in sorted((folder / "digikit").rglob("*"))
        if p.is_file() and p.name != "selection.txt"
    }
    c.check(
        "+Drive round trip: the export wrote the store's files to digikit/",
        r.returncode == 0 and got == files,
        f"{sorted(got)} want {sorted(files)} {r.stderr[-200:]}",
    )
    r2 = tools.run(*export)
    c.check(
        "+Drive round trip: a second export adds, replaces and keeps nothing",
        r2.returncode == 0
        and "added 0 replaced 0" in r2.stdout
        and "kept 0" in r2.stdout,
        r2.stdout.splitlines()[0] if r2.stdout else r2.stderr[-200:],
    )
    built = top / "card.img"
    r3 = tools.run(tools.plusdrive, "build", folder, "-o", built, "--device", c.device,
                   "--syx", c.env.stock)  # fmt: skip
    ls = tools.run(tools.plusdrive, "store", "ls", built, "--device", c.device)
    names = sorted(
        ln.split("\t")[0] for ln in ls.stdout.splitlines() if not ln.startswith("#")
    )
    c.check(
        "+Drive round trip: the next card build holds the uploaded files",
        r3.returncode == 0 and names == sorted(files),
        f"{names} {r3.stderr[-200:]}",
    )


def _selection_untouched(c: Any) -> None:
    c.check(
        "the upload wrote no selection sector",
        not (c.store_writes & {c.store.sel_a, c.store.sel_b}),
        f"{sorted(hex(s) for s in c.store_writes)}",
    )


# ---- folders and large files ------------------------------------------------------------------------------------------------


def card_ls(
    c: Any, rig: Any, base: Path, tag: str
) -> tuple[dict[str, tuple[int, int, str]], set[str], str]:
    """The store of BASE with the guest's writes laid over it, as `plusdrive store ls` prints it: ({path: (first sector, size, sha256)}, the
    folders with nothing in them, the header line). The bytes of a file are `card_view`'s."""
    from tools.patchkit.pk_digikit_project import card_after

    ov = rig.work / f"{tag}.ov.bin"
    rig.emu.call("card.overlay", path=str(ov))
    after = card_after(c, base, ov)
    tools = c.env.tools
    r = tools.run(tools.plusdrive, "store", "ls", after, "--device", c.device)
    files: dict[str, tuple[int, int, str]] = {}
    dirs: set[str] = set()
    head = f"error: {r.stderr.strip()}" if r.returncode else ""
    for line in r.stdout.splitlines():
        if line.startswith("#"):
            head = line
        elif line.endswith("\tfolder"):
            dirs.add(line.split("\t")[0].rstrip("/"))
        else:
            path, first, size, sh = line.split("\t")
            files[path] = (int(first), int(size), sh)
    return files, dirs, head


def run_folders(c: Any) -> None:
    """Folders, LIST by prefix, MKDIR, CAPACITY's STORE entry, a streamed upload of several MiB (DATA goes straight to the card), a delete that frees
    its space for the next file, and LIST paging; each step compared with the card as the plusdrive reader sees it."""
    img = c.image_for(B.RT, "run")
    base = c.card(None)
    zero = "0" * 64
    files: dict[str, bytes] = {}
    with c.rig(img, base, "folders", cf_only=True) as rig:
        apply_cap(c, rig, img)
        up = D.Uploader(transport(rig))
        info = up.info()
        c.check(
            "INFO: capability FOLDERS", D.has_cap(info, "FOLDERS"), str(info["caps"])
        )
        cap = up.capacity().get("STORE")
        total_kib = c.store.limit // 2
        c.check(
            "CAPACITY: a STORE entry with the area's size in KiB, nothing used yet, largest 0",
            cap is not None
            and cap["limit"] == total_kib
            and cap["used"] == 0
            and cap["largest"] == 0,
            str(cap),
        )

        step = 0

        def check(label: str, dirs: set[str]) -> dict[str, tuple[int, int, str]]:
            nonlocal step
            step += 1
            rows, rdirs, head = card_ls(c, rig, base, f"fold-{step}")
            got = {p: (s, h) for p, s, h in up.ls() if h != zero or s != 0}
            want = {p: (len(b), sha(b).hex()) for p, b in files.items()}
            c.check(
                f"{label}: LIST equals the files sent",
                got == want,
                f"{got} want {want}",
            )
            c.check(
                f"{label}: the card, read by plusdrive, lists the same files and sizes",
                {p: (r[1], r[2]) for p, r in rows.items()} == want,
                head,
            )
            listed = {
                p for p, s, h in up.ls() if h == zero and s == 0 and p not in files
            }
            c.check(
                f"{label}: folders with nothing in them: LIST = the card",
                listed == dirs == rdirs,
                f"{listed} {dirs} {rdirs}",
            )
            return rows

        c.check(
            "LIST under samples/ of an empty store: nothing", up.ls("samples/") == []
        )
        up.mkdir("samples")
        up.mkdir("samples/drums")
        up.mkdir("samples/drums")  # exists: ok
        check(
            "two MKDIR (the outer folder has a folder in it: one entry)",
            {"samples/drums"},
        )
        for bad, want in [("a//b", -51), ("../x", -51), ("a/", -51)]:
            st, _ = up.request(D.CMD_MKDIR, bad.encode() + b"\0")
            c.check(f"MKDIR {bad!r}: E_PATH", st == want, str(st))
        st, _ = up.request(D.CMD_MKDIR, b"\0")
        c.check("MKDIR with an empty path: E_MSG", st == -50, str(st))
        st, _ = up.request(D.CMD_MKDIR, b"x")
        c.check("MKDIR with no NUL: E_MSG or E_PATH", st in (-50, -51), str(st))
        st, _ = up.request(D.CMD_LIST, struct.pack(">I", 0) + b"sam")
        c.check("LIST with a prefix and no NUL: E_MSG", st == -50, str(st))

        # what one MiB costs the device (instructions the emulator ran while the host waited): the DATA requests, and END (which does not read the file back)
        probe = blob(1 << 20, 20)
        t0 = up.t.stepped
        up.begin("probe.bin", len(probe), sha(probe))
        for off in range(0, len(probe), D.MAX_CHUNK):
            st, _ = up.request(
                D.CMD_DATA, struct.pack(">I", off) + probe[off : off + D.MAX_CHUNK]
            )
        t1 = up.t.stepped
        up.end()
        t2 = up.t.stepped
        c.report["instructions_per_MiB_of_DATA"] = t1 - t0
        c.report["instructions_for_END_after_1_MiB"] = t2 - t1
        up.rm("probe.bin")
        up.begin("probe.bin", len(probe), sha(probe))
        for off in range(0, len(probe), D.MAX_CHUNK):
            up.request(
                D.CMD_DATA, struct.pack(">I", off) + probe[off : off + D.MAX_CHUNK]
            )
        up.end()
        t4 = up.t.stepped
        up.verify("probe.bin")
        c.report["instructions_for_VERIFY_of_1_MiB"] = up.t.stepped - t4
        c.check(
            "cost of 1 MiB (instructions): DATA, END, VERIFY",
            True,
            f"{t1 - t0} {t2 - t1} {up.t.stepped - t4}",
        )
        up.rm("probe.bin")
        c.check(
            "END of a 1 MiB file takes far less than its DATA (the hash was made as the data came)",
            t2 - t1 < (t1 - t0) // 4,
            f"{t1 - t0} {t2 - t1}",
        )

        # a streamed upload: the file is several MiB, far over the 64 KiB staging buffer, and goes to the card as it arrives
        free0 = up.info()["free_bytes"]
        mib = int(c.table().get("big_mib", 6))
        big = blob(mib * 1024 * 1024 + 77, 21)
        files["samples/drums/big.wav"] = big
        st = put_status(up, "samples/drums/big.wav", big)
        c.check(
            f"put a {mib} MiB file under a folder entry (DATA chunks of 32 KiB straight to the card)",
            st == 0,
            str(st),
        )
        free1 = up.info()["free_bytes"]
        c.check(
            "INFO: the free bytes fell by the file's sectors (and the table room)",
            0 < free0 - free1 <= len(big) + 2 * 512 + 512,
            f"{free0} -> {free1}",
        )
        cap1 = up.capacity()["STORE"]
        c.check(
            "CAPACITY: STORE used grew by the file in KiB",
            cap1["limit"] == total_kib and mib * 1024 <= cap1["used"] <= mib * 1024 + 8,
            str(cap1),
        )
        rows = check("big upload (the folder entry is gone)", set())
        src = rig.work / "big.bin"
        src.write_bytes(big)
        c.check(
            f"VERIFY by ranges: the device hashes the {mib} MiB file from the card in 1 MiB pieces, each equal to the host's",
            up.verify_file("samples/drums/big.wav", src),
        )
        c.check(
            "VERIFY of the last, short range (not a whole sector)",
            up.verify("samples/drums/big.wav", len(big) // 512 * 512, len(big) % 512)
            == sha(big[len(big) // 512 * 512 :]).hex(),
        )
        for off, ln, want in [
            (100, 10, -54),
            (len(big) + 512, 1, -54),
            (512, 0, -55),
            (512, len(big), -55),
        ]:
            st, _ = up.request(
                D.CMD_VERIFY, b"samples/drums/big.wav\0" + struct.pack(">II", off, ln)
            )
            c.check(f"VERIFY range ({off}, {ln}): refused", st == want, str(st))
        big_first = rows["samples/drums/big.wav"][0]

        # a delete frees the space and the next file of the size takes it again (first fit)
        up.rm("samples/drums/big.wav")
        del files["samples/drums/big.wav"]
        check("delete", set())
        c.check(
            "the delete gave the free bytes back (the folder was implied: no entry is left)",
            free0
            <= up.info()["free_bytes"]
            <= free0 + 1024,  # the empty table needs no sector
            f"{up.info()['free_bytes']} vs {free0}",
        )
        cap2 = up.capacity()["STORE"]
        c.check("CAPACITY: STORE used fell again", cap2["used"] < 16, str(cap2))
        files["samples/second.wav"] = blob(mib * 1024 * 1024 + 77, 22)
        up.put("samples/second.wav", files["samples/second.wav"])
        rows = check("a file of the same size", set())
        c.check(
            "the new file took the first gap, which holds the space the deleted one left (it starts at or before it)",
            rows["samples/second.wav"][0] <= big_first,
            f"{rows['samples/second.wav'][0]} vs {big_first}",
        )

        # a hole in the middle: three files, delete the middle one, a smaller file goes into the hole
        for i, n in enumerate([100_000, 100_000, 100_000]):
            files[f"holes/f{i}.bin"] = blob(n, 30 + i)
            up.put(f"holes/f{i}.bin", files[f"holes/f{i}.bin"])
        rows = check("three files in a row", set())
        mid = rows["holes/f1.bin"][0]
        up.rm("holes/f1.bin")
        del files["holes/f1.bin"]
        files["holes/small.bin"] = blob(50_000, 33)
        up.put("holes/small.bin", files["holes/small.bin"])
        rows = check("a smaller file after a delete in the middle", set())
        small = rows["holes/small.bin"][0]
        c.check(
            "the smaller file went into the hole between f0 and f2",
            rows["holes/f0.bin"][0] < small
            and small + 98 <= rows["holes/f2.bin"][0]
            and small <= mid,
            f"{small} (f0 {rows['holes/f0.bin'][0]}, hole {mid}, f2 {rows['holes/f2.bin'][0]})",
        )

        # MKDIR against files, implied folders and folder entries; DELETE of folders
        for bad in ["holes/f0.bin", "holes/f0.bin/x"]:
            st, _ = up.request(D.CMD_MKDIR, bad.encode() + b"\0")
            c.check(
                f"MKDIR {bad!r} (a file, or inside one): E_PATH", st == -51, str(st)
            )
        up.mkdir("holes")  # implied by the files in it: ok, no entry
        check("MKDIR of an implied folder makes no entry", set())
        up.mkdir("tmp/x")
        check("MKDIR tmp/x", {"tmp/x"})
        st, _ = up.request(D.CMD_BEGIN, b"tmp/x\0" + struct.pack(">I", 1) + bytes(32))
        c.check("BEGIN over a folder entry: E_PATH", st == -51, str(st))
        st, _ = up.request(D.CMD_DELETE, b"holes\0")
        c.check(
            "DELETE of an implied folder: E_NOFILE (delete its files)",
            st == -59,
            str(st),
        )
        up.rm("tmp/x")
        check("DELETE of a folder entry", set())

        # LIST paging: many entries, several replies
        for i in range(40):
            files[f"many/s{i:02d}.wav"] = blob(1 + i, 100 + i)
            up.put(f"many/s{i:02d}.wav", files[f"many/s{i:02d}.wav"])
        check("forty small files", set())
        got = up.ls("many/")
        c.check(
            "LIST under many/: forty entries in several pages",
            sorted(p for p, _, _ in got)
            == sorted(p for p in files if p.startswith("many/"))
            and len(got) == 40,
            f"{len(got)}",
        )
        c.check("LIST under a prefix nobody has: nothing", up.ls("zzz/") == [])
        c.check(
            "LIST with the prefix `m` finds many/ and nothing else",
            {p.split("/")[0] for p, _, _ in up.ls("m")} == {"many"},
        )
        roundtrip(c, rig, base, files)
        c.report["device_requests"] = len(up.log)
        c.done(rig)
    c.common_checks_upload()


# ---- power cuts ------------------------------------------------------------------------------------------------------------------


def run_cut(c: Any) -> None:
    """The upload of a three-chunk file does six card writes: three DATA chunks, the table, header B, header A. A cut after the Nth."""
    old = blob(3000, 21)
    folder = c.env.folder / "digikit" / "modules"
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "old.dkm").write_bytes(old)
    base = c.card(None)
    (folder / "old.dkm").unlink()
    img = c.image_for(B.RT, "run")
    cut_sym = M.sym(img.plan, "core/runtime", "DKRT_UPLOAD_CUT")
    new = blob(70_000, 22)
    more = blob(700, 23)
    old_files = {"modules/old.dkm": old}
    both = {"modules/old.dkm": old, "modules/new.dkm": new}
    # (writes allowed, what the next boot must read, what the upload must answer)
    cases = [
        ("after the first chunk", 1, old_files),
        ("after all chunks", 3, old_files),
        ("after the table", 4, old_files),
        ("after header B, before A", 5, both),
        ("after header A", 6, both),
    ]

    def one(label: str, n: int, want: dict[str, bytes], k: int) -> Any:
        def run() -> None:
            with c.rig(img, base, f"cut{k}-1", cf_only=True) as rig:
                rig.emu.poke32(cut_sym, n)
                up = D.Uploader(transport(rig))
                st = put_status(up, "modules/new.dkm", new)
                c.check(
                    f"cut {label}: the upload {'ends ok' if n >= 6 else 'is refused its next write'}",
                    (st == 0) == (n >= 6),
                    str(st),
                )
                from tools.coldfire import arena_safety  # noqa: F401
                from tools.patchkit.pk_digikit_project import card_after

                ov = rig.work / f"cut{k}.ov.bin"
                rig.emu.call("card.overlay", path=str(ov))
                after = card_after(c, base, ov)
                with c.lock:
                    c.all_writes |= _writes(ov)
                c.done(rig)
            with c.rig(img, after, f"cut{k}-2", cf_only=True) as rig:
                up = D.Uploader(transport(rig))
                got = {p: (s, h) for p, s, h in up.ls()}
                w = {p: (len(b), sha(b).hex()) for p, b in want.items()}
                c.check(
                    f"cut {label}: the next boot reads the {'new' if len(want) > 1 else 'old'} store, whole",
                    got == w,
                    f"{got}",
                )
                view, head, _ = card_view(c, rig, after, f"cut{k}-v")
                c.check(
                    f"cut {label}: plusdrive reads the same files from the card",
                    view == want,
                    head,
                )
                files = dict(want)
                files["modules/more.dkm"] = more
                st = put_status(up, "modules/more.dkm", more)
                view, head, _ = card_view(c, rig, after, f"cut{k}-w")
                c.check(
                    f"cut {label}: the next upload works and commits",
                    st == 0 and view == files,
                    f"{st} {sorted(view)} {head}",
                )
                c.done(rig)

        return run

    c.parallel(*[one(label, n, want, k) for k, (label, n, want) in enumerate(cases)])
    c.common_checks_upload()


def _writes(ov: Path) -> set[int]:
    from tools.coldfire import arena_safety

    return set(arena_safety.parse_overlay(ov.read_bytes()))


# ---- the stock route --------------------------------------------------------------------------------------------------------------


def rpc_request(type_byte: int, seq: int = 1) -> bytes:
    """A device-0x10 MidiRpc request with no payload, in the packed framing (finding 42 section 2.5)."""
    from tools.firmware.build import encode_8in7

    body = encode_8in7(bytes((0, seq, 0, 0, type_byte)))
    return bytes((0xF0, 0x00, 0x20, 0x3C, 0x10, 0x00)) + body + b"\xf7"


def run_stock(c: Any) -> None:
    from tools.firmware import config
    from tools.patchkit.pk_dkrt import DEVICE_SYX

    img = c.image_for(B.RT, "run")
    stock = B.Img(config.stock(DEVICE_SYX[c.image]), {"symbols": {}}, "")
    base = c.card(None)
    requests = [rpc_request(0x01), rpc_request(0x02), rpc_request(0x03)]
    # a message that starts F0 7D but is not ours (no 64 6B): the router ignores it; the Ping after it still answers
    foreign = b"\xf0\x7d\x01\x02\x03\xf7"
    out: dict[str, list[bytes]] = {}

    def talk(key: str, image: B.Img) -> Any:
        def run() -> None:
            # the reference is the unpatched firmware: no plan, so no write-watch
            rig_cm = (
                c.rig(image, base, f"stock-{key}", cf_only=True)
                if image is img
                else c.env.boot(
                    image, base, f"stock-{key}", cf_only=True, settle_ms=300
                )
            )
            with rig_cm as rig:
                rig.emu.usb_out()  # start the capture
                got: list[bytes] = []
                tr = D.EmuTransport(rig.emu, **HS)
                for req in [*requests[:2], foreign, *requests]:
                    rig.emu.usb_in(req, **HS)
                    for _ in range(40):
                        rig.emu.step(400_000)
                    tr.buf += rig.emu.usb_out()
                    got.extend(D.split_messages(tr.buf))
                out[key] = got
                if image is img:
                    c.done(rig)

        return run

    c.parallel(talk("rt", img), talk("stock", stock))
    rt, st = out["rt"], out["stock"]
    c.check(
        "the stock firmware answered the five MidiRpc requests",
        len(st) == 5
        and all(m[:6] == bytes((0xF0, 0x00, 0x20, 0x3C, 0x10, 0x00)) for m in st),
        f"{[m[:8].hex() for m in st]}",
    )
    c.check(
        "Ping, SoftwareVersion and DeviceUID replies are byte-identical with and without the runtime",
        rt == st,
        f"{[m.hex() for m in rt]} vs {[m.hex() for m in st]}",
    )
    c.common_checks_upload()


# ---- the Rust host ---------------------------------------------------------------------------------------------------------------


def rust_binary(c: Any) -> Path:
    """The `emu_session` example of packages/dkupload (the Rust `Session` the desktop Modules panel and the web page use)."""
    import json
    import subprocess

    from tools import REPO_ROOT

    r = subprocess.run(
        ["cargo", "build", "-q", "-p", "dkupload", "--example", "emu_session", "--message-format=json"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )  # fmt: skip
    exe = None
    for line in r.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("executable") and m.get("target", {}).get("name") == "emu_session":
            exe = Path(m["executable"])
    if r.returncode or exe is None:
        raise RuntimeError(
            f"cargo build of the emu_session example failed: {r.stderr[-400:]}"
        )
    return exe


def run_rust(c: Any) -> None:
    """The Rust host `Session` (packages/dkupload) against the real device side in the emulator, one process per step through the
    control socket (`midi.usb.in` / `midi.usb.out` / `step`, as modules.rs does). After each step the Python tool's view (LIST) and
    the card, read by plusdrive, are compared with what the Rust host did."""
    import json
    import subprocess

    exe = rust_binary(c)
    mod = c.pack("test/sine", "rust.dkm")
    dkm_file = c.env.folder / "digikit" / mod
    dkm = dkm_file.read_bytes()
    dkm_file.unlink()
    img = c.image_for(B.RT, "run")
    base = c.card(None)
    work = c.work
    files: dict[str, bytes] = {}

    HSF = ("--chunk", "384", "--period", "16500")

    def host(rig: Any, *args: str, flags: tuple[str, ...] = HSF) -> dict[str, Any]:
        r = subprocess.run(
            [str(exe), rig.emu.path, *flags, *args],
            capture_output=True,
            text=True,
            timeout=900,
        )
        try:
            return dict(json.loads(r.stdout.strip().splitlines()[-1]))
        except (ValueError, IndexError):
            return {"ok": False, "failure": {"error": f"no output: {r.stderr[-300:]}"}}

    def put(
        rig: Any, data: bytes, path: str, tag: str, flags: tuple[str, ...] = HSF
    ) -> dict[str, Any]:
        f = work / f"rust-{tag}.bin"
        f.write_bytes(data)
        return host(rig, "put", str(f), path, flags=flags)

    with c.rig(img, base, "rust", cf_only=True) as rig:
        apply_cap(c, rig, img)
        up = D.Uploader(transport(rig))

        def check(label: str) -> None:
            view, head, writes = card_view(
                c, rig, base, f"rust-{label.replace(' ', '-')}"
            )
            same(c, label, up, files, view, head)
            c.store_writes |= {w for w in writes if c.store.first <= w < c.store.end}
            c.all_writes |= writes

        def stat(label: str, r: dict[str, Any]) -> None:
            c.check(f"Rust host: {label}", bool(r.get("ok")), str(r.get("failure")))

        py = up.info()
        r = host(rig, "info")
        i = r.get("result", {})
        c.check(
            f"Rust INFO: the device id is {c.device!r}, as the Python tool reads it, the free bytes of an empty store, 32 KiB chunks",
            bool(r.get("ok"))
            and i["port"] == c.device == py["port"]
            and i["protocol"] == py["protocol"] == 1
            and i["runtime_version"] == py["runtime"]
            and i["free_bytes"] == py["free_bytes"] == free_fresh(c)
            and i["max_chunk"] == 32768
            and not i["safe_mode"]
            and not i["upload_open"],
            f"{r} vs {py}",
        )
        r = host(rig, "ls")
        c.check(
            "Rust LIST of an empty store", r.get("ok") and r["result"] == [], str(r)
        )

        # a real module, one chunk
        f = work / "rust-dkm.bin"
        f.write_bytes(dkm)
        r = host(rig, "put", str(f), mod)
        stat(f"put {mod} ({len(dkm)} bytes, a built module)", r)
        files[mod] = dkm
        got = host(rig, "ls")
        c.check(
            "Rust LIST shows the module with the right size and sha256",
            got.get("ok")
            and got["result"]
            == [{"path": mod, "size": len(dkm), "sha256": sha(dkm).hex()}],
            str(got),
        )
        check("Rust put of a module")

        # three chunks, two DATA in flight
        a = blob(70_000, 41)
        files["modules/a.dkm"] = a
        r = put(rig, a, "modules/a.dkm", "a")
        stat("put 70,000 bytes (three chunks, two in flight)", r)
        check("Rust put, three chunks")
        c.report["rust_put_70000_stepped"] = r.get("stepped")

        # the Python tool and the Rust host agree on the same bytes
        up.put("modules/py.dkm", dkm)
        files["modules/py.dkm"] = dkm
        rl = {e["path"]: (e["size"], e["sha256"]) for e in host(rig, "ls")["result"]}
        c.check(
            "the Rust LIST equals the Python LIST (the same module put by either has the same sha256)",
            rl == device_ls(up) and rl[mod] == rl["modules/py.dkm"],
            str(rl),
        )
        check("Python put next to the Rust ones")

        # folders and ranges through the Rust host: LIST by prefix, MKDIR, VERIFY of a range
        r = host(rig, "mkdir", "samples/drums")
        c.check("the Rust host: MKDIR", r.get("ok") is True, str(r))
        c.check(
            "the Rust host: LIST by prefix lists the folder entry and nothing else; the Python tool sees the same",
            [e["path"] for e in host(rig, "ls", "samples/")["result"]]
            == ["samples/drums"]
            and [p for p, _, _ in up.ls("samples/")] == ["samples/drums"],
        )
        ln = min(1000, len(files[mod]) - 512)
        r = host(rig, "verify", mod, "512", str(ln))
        c.check(
            "the Rust host: VERIFY of a range equals the Python tool's and the bytes sent",
            r.get("ok") is True
            and r["result"]
            == up.verify(mod, 512, ln)
            == sha(files[mod][512 : 512 + ln]).hex(),
            str(r),
        )
        up.rm("samples/drums")

        # replace
        a2 = blob(1000, 42)
        files["modules/a.dkm"] = a2
        stat("replace modules/a.dkm", put(rig, a2, "modules/a.dkm", "a2"))
        check("Rust replace")

        # delete (and a delete of a missing path)
        stat("delete modules/py.dkm", host(rig, "rm", "modules/py.dkm"))
        del files["modules/py.dkm"]
        r = host(rig, "rm", "modules/py.dkm")
        c.check(
            "Rust delete of a missing path: E_NOFILE (-59)",
            not r["ok"] and r["failure"].get("status") == -59,
            str(r),
        )
        check("Rust delete")

        # E_HASH: nothing committed, no upload left open
        bad = blob(40_000, 43)
        f = work / "rust-bad.bin"
        f.write_bytes(bad)
        r = host(rig, "badhash", str(f), "modules/bad.dkm")
        c.check(
            "Rust wrong sha256: END answers E_HASH (-56)",
            not r["ok"] and r["failure"].get("status") == -56,
            str(r),
        )
        c.check("no upload is left open after E_HASH", not up.info()["upload_open"])
        check("Rust E_HASH (nothing committed)")

        # a lost reply to INFO, to a DATA, to END (the END retry gets E_STATE; the host looks the file up)
        r = host(rig, "--drop", "1", "info", flags=(*HSF, "--budget", "150000000"))
        c.check(
            "a lost INFO reply: the host retries and gets it", bool(r.get("ok")), str(r)
        )
        d = blob(1024, 44)
        files["modules/drop-data.dkm"] = d
        f = work / "rust-dd.bin"
        f.write_bytes(d)
        r = host(rig, "--drop", "2", "put", str(f), "modules/drop-data.dkm",
                 flags=(*HSF, "--budget", "150000000"))  # fmt: skip
        c.check(
            "a lost DATA reply: the chunk is sent again (retry by offset) and the put ends ok",
            bool(r.get("ok")),
            str(r),
        )
        check("Rust lost DATA reply")
        e = blob(1024, 45)
        files["modules/drop-end.dkm"] = e
        f = work / "rust-de.bin"
        f.write_bytes(e)
        r = host(rig, "--drop", "3", "put", str(f), "modules/drop-end.dkm",
                 flags=(*HSF, "--budget", "150000000"))  # fmt: skip
        c.check(
            "a lost END reply: the second END gets E_STATE, the host finds the file in LIST and ends ok",
            bool(r.get("ok")),
            str(r),
        )
        check("Rust lost END reply")

        # the timing of one 32 KiB chunk, default USB delivery (64 bytes / 132,000 instructions) against the high-speed model
        big = blob(32768, 46)
        files["modules/big.dkm"] = big
        slow = put(rig, big, "modules/big.dkm", "big-slow", flags=())
        stat("put 32 KiB with the default delivery", slow)
        fast = put(rig, big, "modules/big.dkm", "big-fast")
        stat("put the same 32 KiB with chunk 384 / period 16,500", fast)
        c.report["rust_32k_stepped_default"] = slow.get("stepped")
        c.report["rust_32k_stepped_hs"] = fast.get("stepped")
        c.report["rust_32k_wall_default_s"] = slow.get("wall_s")
        c.report["rust_32k_wall_hs_s"] = fast.get("wall_s")
        check("Rust 32 KiB puts")

        # CLEAR
        stat("clear", host(rig, "clear"))
        files.clear()
        check("Rust clear")
        c.check("CLEAR gives the space back", up.info()["free_bytes"] == free_fresh(c))
        c.done(rig)
    _selection_untouched(c)
    c.common_checks_upload()


# ---- playback ----------------------------------------------------------------------------------------------------------------------


def run_playback(c: Any) -> None:
    """A note of a card-built module plays twice on the same boot: alone, and with a module uploaded while it sounds."""

    t = c.table()
    mods = [c.pack(m, f"m{i}.dkm") for i, m in enumerate(t["modules"])]
    img = c.image_for(B.RT, "run")
    card = c.card(mods)
    trig = t.get("trig_frame", M.TRIG_FRAME if c.dt2 else M.TRIG_FRAME_DN2)
    payload = blob(t.get("bytes", 100_000), 31)
    out: dict[str, Any] = {}

    def play(key: str, upload: bool) -> Any:
        def run() -> None:
            with c.rig(img, card, f"play-{key}") as rig:
                if c.dt2:
                    rig.select_type(rig.dyn_type(M.MODULE, M.MACHINE), t.get("hint", 7))
                else:
                    rig.select(t.get("hint_run", t["hint"]))
                up = D.Uploader(transport(rig))

                def during() -> None:
                    up.put("modules/up.dkm", payload)
                    out["listed"] = {p for p, _, _ in up.ls()}

                out[key] = rig.note(
                    c.work / f"{key}.wav",
                    max(trig, rig.frames() + 300),
                    during if upload else None,
                )
                ov = rig.work / f"play-{key}.ov.bin"
                rig.emu.call("card.overlay", path=str(ov))
                out[f"writes-{key}"] = _writes(ov)
                c.done(rig)

        return run

    c.parallel(play("alone", False), play("during", True))
    c.check(
        "the module was uploaded during the note",
        out.get("listed") == {"modules/up.dkm"} | {m for m in mods},
        str(out.get("listed")),
    )

    peak = float(abs(out["alone"]).max())
    c.check("the note is audible", peak > 0.01, f"peak {peak:.3f}")
    rig_checks: list[tuple[str, bool, str]] = []
    M.pcm_check(
        _Collect(rig_checks),
        out["alone"],
        out["during"],
        c.report,
        "note during an upload",
        # The onset may differ by a few blocks: the MIDI task's work (header reads, the SHA-256 of the file) keeps the UI task from
        # handling the TRIG key for as long as it runs, so the shift follows where the upload is when the key arrives (64 samples in the
        # first run of this scenario, 128 after the header-agreement flag, 256 with two extra card reads). From the onset on, not one sample may differ.
        max_shift=256,
    )
    c.checks += rig_checks
    extra = out["writes-during"] - out["writes-alone"]
    lo, hi = c.store.first, c.store.first + c.store.limit + 1
    stray = sorted(w for w in extra if not lo <= w < hi)
    c.check(
        "the card writes the upload added are store sectors only (header copies, data, table; no selection sector)",
        bool(extra) and not stray and not extra & {c.store.sel_a, c.store.sel_b},
        f"{len(extra)} sectors added, outside the store: {[hex(x) for x in stray[:6]]}",
    )
    c.all_writes |= out["writes-during"]
    c.common_checks_upload()


class _Collect:
    """`pcm_check` reports through `.check`."""

    def __init__(self, sink: list[tuple[str, bool, str]]):
        self.sink = sink

    def check(self, name: str, ok: bool, detail: str = "") -> None:
        self.sink.append((name, bool(ok), detail))
        print(f"{'ok  ' if ok else 'FAIL'} {name} {detail}", flush=True)


# ---- CLEAR: two faults in a row -------------------------------------------------------------------------------------------------


def _poke_u32(rig: Any, img: Any, name: str, value: int) -> None:
    rig.emu.poke32(M.sym(img.plan, "core/runtime", name), value)


def _after(c: Any, rig: Any, base: Path, tag: str) -> Path:
    """The card BASE with the guest's writes laid over it (the state a power cut at this moment leaves)."""
    from tools.patchkit.pk_digikit_project import card_after

    ov = rig.work / f"{tag}.ov.bin"
    rig.emu.call("card.overlay", path=str(ov))
    with c.lock:
        c.all_writes |= _writes(ov)
    return card_after(c, base, ov)


def run_clearcut(c: Any) -> None:
    """Finding 43, "CLEAR double fault". Fault 1 cuts an operation between header B and header A; fault 2 tears the next upload's header B.
    The store must still be a valid store (the committed one) afterwards, not damaged.

    clear   CLEAR is cut after header B: B is the empty store, A still the old one, pointing at a table in sector 1. The next upload
            stages its file over sector 1 (the store starts over) and its header B write tears. Without the rule that BEGIN first makes
            the copies equal, A would be the only usable copy and point at overwritten sectors: the store reads as damaged.
    upload  An upload is cut after header B (the new store in B, the old one in A) and the next upload's header B tears: the first
            upload's files must still be there (A was made equal to B by the second BEGIN)."""
    old = blob(3000, 61)
    one = blob(700, 62)
    two = blob(700, 63)
    folder = c.env.folder / "digikit" / "modules"
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "old.dkm").write_bytes(old)
    base = c.card(None)
    (folder / "old.dkm").unlink()
    img = c.image_for(B.RT, "run")
    old_files = {"modules/old.dkm": old}

    def session(
        label: str,
        card: Path,
        knobs: dict[str, int],
        action: Any,
    ) -> tuple[Path, int]:
        with c.rig(img, card, label, cf_only=True) as rig:
            for k, v in knobs.items():
                _poke_u32(rig, img, k, v)
            up = D.Uploader(transport(rig))
            st = action(up)
            nxt = _after(c, rig, card, label)
            c.done(rig)
        return nxt, st

    def verify(label: str, card: Path, want: dict[str, bytes], then: bytes) -> None:
        with c.rig(img, card, label, cf_only=True) as rig:
            up = D.Uploader(transport(rig))
            got = {p: (s, h) for p, s, h in up.ls()}
            w = {p: (len(b), sha(b).hex()) for p, b in want.items()}
            c.check(
                f"{label}: the store reads (not damaged) and holds {sorted(want) or 'nothing'}",
                got == w,
                f"{got}",
            )
            view, head, _ = card_view(c, rig, card, f"{label}-v")
            c.check(
                f"{label}: plusdrive reads the same from the card", view == want, head
            )
            st = put_status(up, "modules/after.dkm", then)
            files = dict(want)
            files["modules/after.dkm"] = then
            view, head, _ = card_view(c, rig, card, f"{label}-w")
            c.check(
                f"{label}: the next upload works and commits",
                st == 0 and view == files,
                f"{st} {sorted(view)} {head}",
            )
            c.done(rig)

    def clear_case() -> None:
        # fault 1: CLEAR writes header B (write 1) and is cut before header A
        card1, st1 = session(
            "cc-1", base, {"DKRT_UPLOAD_CUT": 1}, lambda up: put_status_clear(up)
        )
        c.check(
            "clear cut after header B: CLEAR is refused its second write",
            st1 == -57,
            str(st1),
        )
        with c.rig(img, card1, "cc-1v", cf_only=True) as rig:
            up = D.Uploader(transport(rig))
            c.check(
                "after the cut the next boot reads B, the empty store", up.ls() == []
            )
            c.done(rig)

        # fault 2: the next upload: write 1 = the repair of header A, 2 = the data, 3 = the table, 4 = header B, torn
        def torn_upload(up: D.Uploader) -> int:
            return put_status(up, "modules/one.dkm", one)

        card2, st2 = session("cc-2", card1, {"DKRT_UPLOAD_TEAR": 4}, torn_upload)
        c.check(
            "the upload whose header B tears reports a write error",
            st2 == -57,
            str(st2),
        )
        verify("clear-cut-then-tear", card2, {}, two)

    def upload_case() -> None:
        # fault 1: an upload of a small file does data (1), table (2), header B (3), header A (4): cut after B
        card1, st1 = session(
            "cu-1",
            base,
            {"DKRT_UPLOAD_CUT": 3},
            lambda up: put_status(up, "modules/one.dkm", one),
        )
        c.check(
            "upload cut after header B: refused its last write", st1 == -57, str(st1)
        )
        # fault 2: the next upload: 1 = repair of A, 2 = data, 3 = table, 4 = header B, torn
        card2, st2 = session(
            "cu-2",
            card1,
            {"DKRT_UPLOAD_TEAR": 4},
            lambda up: put_status(up, "modules/two.dkm", two),
        )
        c.check("the second upload's header B tears", st2 == -57, str(st2))
        verify(
            "upload-cut-then-tear", card2, {**old_files, "modules/one.dkm": one}, two
        )

    c.parallel(clear_case, upload_case)
    c.common_checks_upload()


def put_status_clear(up: D.Uploader) -> int:
    try:
        up.clear()
        return 0
    except D.UploadError as e:
        return e.status


# ---- a host that never reads -----------------------------------------------------------------------------------------------------


def _literal_value(rig: Any, image: str, anchor: str) -> int:
    """The 4-byte address literal of the code anchor ANCHOR of the port file (not word aligned: read as bytes)."""
    import tomllib

    from tools import REPO_ROOT

    port = tomllib.loads(
        (REPO_ROOT / "devices" / "ports" / f"{image}.toml").read_text()
    )
    at = next(a["addr"] for a in port["anchor"] if a["name"] == anchor)
    base = at & ~3
    raw = rig.w(base).to_bytes(4, "big") + rig.w(base + 4).to_bytes(4, "big")
    return int.from_bytes(raw[at - base : at - base + 4], "big")


def run_txfull(c: Any) -> None:
    """A host that never reads its MIDI input leaves the firmware's 4 KiB TX ring full, and the firmware's TX function waits in a loop for
    room. A reply sent into a full ring would hang the MIDI task. The runtime checks the bytes in flight before it sends and drops the reply.
    The emulator does not model the USB IN endpoint, so the test sets the in-flight word of the TX function (the anchor
    `cf.midi.site.tx_inflight_lit`) to what a host that reads nothing leaves, and checks: replies are dropped and counted, nothing reaches the
    output, the MIDI task still routes a stock message (the Ping answers), and a reply is sent again once the ring has room."""
    img = c.image_for(B.RT, "run")
    base = c.card(None)
    with c.rig(img, base, "txfull", cf_only=True) as rig:
        tr = transport(rig)
        up = D.Uploader(tr)
        sym = lambda n: M.sym(img.plan, "core/runtime", n)  # noqa: E731
        flight = _literal_value(rig, c.image, "cf.midi.site.tx_inflight_lit")
        c.check(
            "the in-flight word is a DDR address",
            0x40000000 <= flight < 0x48000000 and flight % 4 == 0,
            hex(flight),
        )
        c.check(
            "an INFO answers while the ring is empty", up.info()["port"] == c.device
        )
        dropped0 = rig.w(sym("DKRT_UPLOAD_DROPPED"))
        sent0 = rig.w(sym("DKRT_UPLOAD_COUNT"))

        def raw(msg: bytes) -> bytes:
            rig.emu.usb_out()  # empty the capture
            rig.emu.usb_in(msg, **HS)
            for _ in range(40):
                rig.emu.step(400_000)
            return bytes(rig.emu.usb_out())

        req = D.frame(D.CMD_INFO, 5)
        for level, name in ((0x0F00, "a full ring"), (0x0200, "512 bytes in flight")):
            rig.emu.poke32(flight, level)
            out = [raw(req) for _ in range(20)]
            c.check(
                f"{name}: 20 requests, no reply reaches the output",
                all(o == b"" for o in out),
                f"{[len(o) for o in out]}",
            )
        c.check(
            "the 40 replies were dropped and counted",
            rig.w(sym("DKRT_UPLOAD_DROPPED")) - dropped0 == 40
            and rig.w(sym("DKRT_UPLOAD_COUNT")) == sent0,
            f"dropped {rig.w(sym('DKRT_UPLOAD_DROPPED')) - dropped0}",
        )
        # the MIDI task is alive: a stock Ping goes the stock way and answers
        rig.emu.poke32(flight, 0x0F00)
        ping = raw(rpc_request(0x01))
        c.check(
            "the MIDI task still routes stock messages: the Ping answers while the ring is full",
            ping[:6] == bytes((0xF0, 0x00, 0x20, 0x3C, 0x10, 0x00)),
            ping[:12].hex(),
        )
        # room again: the host's retry gets its reply
        for level, name in ((0x0100, "256 bytes in flight"), (0, "an empty ring")):
            rig.emu.poke32(flight, level)
            out = raw(req)
            c.check(
                f"{name}: the reply is sent",
                D.parse_reply(next(iter(D.split_messages(bytearray(out))), b""))
                is not None,
                out.hex(),
            )
        # a long reply (LIST of a store with many files) needs more room than a short one
        c.done(rig)
    c.common_checks_upload()


def run_screenshot(c: Any) -> None:
    """The firmware's own MidiRpc Screenshot (0x04) over the emulated USB MIDI path (tools/device/dkscreen.py): the decoded 1024 bytes equal
    the emulator's frame capture of the same moment, pixel for pixel, on the main screen and on another view (SETTINGS)."""
    from tools.device import dkscreen as S

    img = c.image_for(B.RT, "run")
    with c.rig(img, c.card(None), "screen", cf_only=True) as rig:

        def grab(name: str) -> None:
            before = rig.emu.frame()[1]
            shot = S.emu_screenshot(rig.emu)
            after = rig.emu.frame()[1]
            (c.work / f"{name}.png").write_bytes(S.png_of(shot))
            c.check(
                f"Screenshot reply ({name}): 1024 bytes equal to the emulator's frame capture",
                len(shot) == 1024 and shot in (before, after),
                f"{sum(a != b for a, b in zip(shot, after, strict=False))} bytes differ from the frame after",
            )
            c.check(f"Screenshot reply ({name}): the screen is not blank", any(shot))

        grab("main")
        rig.s.do_tap("SETTINGS")
        rig.s.run_ms(600)
        grab("after-settings-key")
        c.done(rig)
    c.common_checks_upload()


PARTS = {
    "screenshot": run_screenshot,
    "ops": run_ops,
    "folders": run_folders,
    "quick": lambda c: run_ops(c, quick=True),
    "cut": run_cut,
    "stock": run_stock,
    "playback": run_playback,
    "rust": run_rust,
    "clearcut": run_clearcut,
    "txfull": run_txfull,
}


def run_upload(c: Any) -> None:
    PARTS[c.table()["part"]](c)
