"""Panel key scripts and probe runs for the arena-safety proofs (finding 25).

A `Script` is a timeline of panel taps, encoder turns, MIDI/USB input and frame dumps, in instructions after the
firmware is ready. `args()` turns it into `regions_probe` arguments (packages/emulator/boot/examples/regions_probe.rs).
`SCRIPTS` names the reusable ones; `tools/coldfire/regions.py scenario_args` falls back to them.
"""

from __future__ import annotations

import pathlib

FUNC = 17
YES = 10
NO = 12
UP, DOWN, LEFT, RIGHT = 11, 14, 13, 15
PLAY, STOP, REC = 20, 21, 19


class Script:
    def __init__(self, t0: int = 1_000_000) -> None:
        self.t = t0
        self.events: list[tuple[int, str]] = []  # (time, probe args as "--flag value")
        self.frames: list[tuple[str, int]] = []  # (name, time)
        self.extra: list[str] = []  # more probe arguments (--pc ADDR, --peek ADDR)

    # time
    def wait(self, n: int) -> Script:
        self.t += n
        return self

    # panel
    def down(self, code: int) -> Script:
        self.events.append((self.t, "--event %d:%d:down" % (self.t, code)))
        return self

    def up(self, code: int) -> Script:
        self.events.append((self.t, "--event %d:%d:up" % (self.t, code)))
        return self

    def tap(self, code: int, hold: int = 2_000_000, gap: int = 25_000_000) -> Script:
        self.down(code).wait(hold).up(code).wait(gap)
        return self

    def chord(
        self, held: int, code: int, hold: int = 2_000_000, gap: int = 25_000_000
    ) -> Script:
        """Hold `held`, tap `code`, release `held`."""
        self.down(held).wait(5_000_000).tap(code, hold, 5_000_000).up(held).wait(gap)
        return self

    def turn(self, enc: int, detents: int, gap: int = 12_000_000) -> Script:
        self.events.append((self.t, "--turn %d:%d:%d" % (self.t, enc, detents)))
        self.t += gap
        return self

    # host write (test scaffolding)
    def poke(self, addr: int, value: int) -> Script:
        self.events.append((self.t, "--poke %d:%#x:%#x" % (self.t, addr, value)))
        return self

    # host read at a time (`regions_probe --peek-at`): the longword at `addr`, or with `via` at (longword at `addr`) + via
    def peek_at(self, addr: int, via: int | None = None) -> Script:
        spec = "%d:%#x" % (self.t, addr) + ("" if via is None else ":%d" % via)
        self.events.append((self.t, "--peek-at " + spec))
        return self

    # MIDI
    def midi(self, data: bytes) -> Script:
        """MIDI DIN bytes at wire rate."""
        self.events.append((self.t, "--midi %d:%s" % (self.t, data.hex())))
        return self

    def usb(self, data: bytes) -> Script:
        """USB MIDI bytes through the firmware's USB receive callback."""
        self.events.append((self.t, "--usb %d:%s" % (self.t, data.hex())))
        return self

    # observation
    def frame(self, name: str) -> Script:
        self.frames.append((name, self.t))
        return self

    def args(
        self, frame_dir: pathlib.Path | str | None = None, tail: int = 50_000_000
    ) -> list[str]:
        out = ["--extra", str(self.t + tail), *self.extra]
        for _, spec in self.events:
            flag, _, value = spec.partition(" ")
            out += [flag, value]
        for name, t in self.frames:
            if frame_dir is None:
                continue
            out += ["--frame", "%d:%s" % (t, pathlib.Path(frame_dir) / (name + ".raw"))]
        return out


def boot() -> Script:
    return Script()


def _prelude(s: Script) -> Script:
    """NO once: gets rid of a dialog the ready state may show (the nav script does the same)."""
    return s.tap(NO, gap=30_000_000)


SETTINGS = 8
G = 30_000_000  # a dialog or a screen change settles within this many instructions


SCRIPTS: dict = {}
# What a script must have reached, checked by `arena_safety.check_reached`: project slots the card overlay touched
# (MmcFs pool, slot n = sectors 0x58000 + 0x8000 * n), and OLED frames (by name) whose SHA-256 is in
# tests/golden/arena_safety_frames.json.
EXPECT: dict[str, dict] = {}


def script(expect: dict | None = None):
    def register(fn):
        SCRIPTS[fn.__name__] = fn
        EXPECT[fn.__name__] = expect or {}
        return fn

    return register


def _settings(s: Script) -> Script:
    """The SETTINGS menu, from the main screen (NO first: clears a dialog the ready state may show)."""
    return s.tap(NO, gap=G).tap(SETTINGS, gap=G)


def _project_menu(s: Script) -> Script:
    return _settings(s).tap(
        YES, gap=G
    )  # PROJECT: LOAD PROJECT, SAVE PROJECT AS, MANAGE PROJECTS


def _save_as_001(s: Script, tag: str = "") -> Script:
    """SAVE PROJECT AS, slot 001 (the cursor starts there), keep the proposed name."""
    s.tap(DOWN, gap=G).tap(YES, gap=40_000_000).frame(tag + "slots")
    s.tap(YES, gap=60_000_000).frame(tag + "naming")
    s.tap(YES, gap=100_000_000).frame(tag + "save_ok")
    return s.tap(NO, gap=G)  # back to the PROJECT menu with SAVE PROJECT AS highlighted


def _to_system(s: Script) -> Script:
    _settings(s)
    for _ in range(7):
        s.tap(DOWN, gap=15_000_000)
    return s.tap(
        YES, gap=40_000_000
    )  # SYSTEM: USB CONFIG, OS UPGRADE, FORMAT +DRIVE, CHECK FILESYSTEM, STORAGE


@script({"project_slots": [0], "frames": ["save_ok"]})
def save_project() -> Script:
    """SETTINGS > PROJECT > SAVE PROJECT AS > slot 001 > YES (SAVE OK)."""
    return _save_as_001(_project_menu(boot()))


@script({"project_slots": [0], "frames": ["save_ok", "quick_dialog", "quick_saved"]})
def save_quick() -> Script:
    """Save as 001, back to the main screen, then FUNC+SETTINGS (save and overwrite) and YES."""
    s = _save_as_001(_project_menu(boot()))
    s.tap(NO, gap=G).tap(NO, gap=G)
    s.chord(FUNC, SETTINGS, gap=G).frame("quick_dialog")
    return s.tap(YES, gap=100_000_000).frame("quick_saved")


@script({"project_slots": [0], "frames": ["save_ok", "load_list", "loaded_done"]})
def load_project() -> Script:
    """Save as 001, then LOAD PROJECT > slot 001 > YES."""
    s = _save_as_001(_project_menu(boot()))
    s.tap(UP, gap=G).tap(YES, gap=40_000_000).frame("load_list")
    s.tap(YES, gap=200_000_000).frame("loading")
    return s.wait(400_000_000).frame("loaded_done")


@script({"project_slots": [], "frames": ["new_init", "new_done"]})
def new_project() -> Script:
    """LOAD PROJECT > CREATE NEW (the list holds only that entry on an empty card)."""
    s = _project_menu(boot())
    s.tap(YES, gap=40_000_000).tap(YES, gap=30_000_000).frame("new_init")
    return s.wait(200_000_000).frame("new_done")


def _manage_command(s: Script, downs: int, tag: str) -> Script:
    """Save as 001, MANAGE PROJECTS, RIGHT (command list), `downs` times DOWN, YES."""
    _save_as_001(_project_menu(s), tag="")
    s.tap(DOWN, gap=G).tap(YES, gap=40_000_000).frame(tag + "manage")
    s.tap(RIGHT, gap=40_000_000).frame(tag + "commands")
    for _ in range(downs):
        s.tap(DOWN, gap=20_000_000)
    return s.tap(YES, gap=60_000_000).frame(tag + "asked")


@script({"project_slots": [0], "frames": ["delete_asked", "delete_done"]})
def manage_delete() -> Script:
    """Save as 001, MANAGE PROJECTS > slot 001 > RIGHT > DELETE > YES (and its confirmation)."""
    s = _manage_command(boot(), 1, "delete_")
    return s.tap(YES, gap=150_000_000).frame("delete_done")


@script({"project_slots": [0], "frames": ["rename_asked", "rename_done"]})
def manage_rename() -> Script:
    """Save as 001, MANAGE PROJECTS > slot 001 > RIGHT > RENAME > YES (naming screen) > YES."""
    s = _manage_command(boot(), 2, "rename_")
    return s.tap(YES, gap=150_000_000).frame("rename_done")


@script({"frames": ["format_checked", "format_prompt", "format_done"]})
def format_pool() -> Script:
    """SYSTEM > FORMAT +DRIVE > PROJ+PST+KIT (RIGHT ticks it) > ERASE SELECTED DATA > YES > YES."""
    s = _to_system(boot())
    s.tap(DOWN, gap=20_000_000).tap(DOWN, gap=20_000_000).tap(YES, gap=60_000_000)
    s.tap(RIGHT, gap=30_000_000).frame("format_checked")
    s.tap(DOWN, gap=20_000_000).tap(DOWN, gap=20_000_000).tap(
        YES, gap=60_000_000
    ).frame("format_prompt")
    s.tap(YES, gap=300_000_000).frame("format_done")
    return s.wait(300_000_000)


@script({"frames": ["os_waiting", "os_cancelled"]})
def os_update() -> Script:
    """SYSTEM > OS UPGRADE (waits for SysEx; nothing is sent) > NO."""
    s = _to_system(boot())
    s.tap(DOWN, gap=20_000_000).tap(YES, gap=60_000_000).frame("os_waiting")
    return s.wait(100_000_000).tap(NO, gap=G).frame("os_cancelled")


@script({"frames": ["check_fs", "check_done"]})
def check_filesystem() -> Script:
    """SYSTEM > CHECK FILESYSTEM > YES."""
    s = _to_system(boot())
    for _ in range(3):
        s.tap(DOWN, gap=20_000_000)
    s.tap(YES, gap=60_000_000).frame("check_fs")
    return s.wait(300_000_000).frame("check_done")


@script({"frames": ["pattern_saved"]})
def pattern_temp() -> Script:
    """FUNC+YES: pattern saved to the temp area."""
    s = boot().tap(NO, gap=G)
    return s.chord(FUNC, YES, gap=20_000_000).frame("pattern_saved")


# --- SETTINGS > DIGIKIT (modules/core/apps/digikit-menu, finding 28) ---------------------------------------------------
# Only meaningful on an image patched with core/digikit-menu: DIGIKIT is the 9th SETTINGS entry (8 DOWN from PROJECT;
# DN2 and DT2 alike while OUTBOX 8 is hidden).


def _to_digikit(s: Script) -> Script:
    """SETTINGS, 8 x DOWN to DIGIKIT, YES: the DIGIKIT view (ABOUT, HEALTH, MEMORY, MODULES, DEBUG)."""
    _settings(s)
    for _ in range(8):
        s.tap(DOWN, gap=15_000_000)
    s.frame("settings_digikit")
    return s.tap(YES, gap=40_000_000).frame("digikit_main")


DG = 60_000_000  # a DIGIKIT row action rebuilds the list; the OLED shows it after about this many instructions
KEY = 15_000_000  # a cursor move
RUNTIME_LOG_VALUE = (
    456 + 24
)  # DKRT_STATE + this: the value of log entry 0 = the address of test/dyn-demo's DYN_COUNTER


def _to_modules(s: Script) -> Script:
    """From the DIGIKIT view with the cursor on MODULES (3 x DOWN from the top): open the list (rescans the store)."""
    return s.tap(YES, gap=DG)


@script({"frames": ["settings_digikit", "digikit_main"]})
def digikit_main() -> Script:
    """SETTINGS list scrolled to DIGIKIT, then DIGIKIT opened."""
    return _to_digikit(boot())


def digikit_about_script() -> Script:
    """DIGIKIT > ABOUT, then NO back to the DIGIKIT view."""
    s = boot()
    _to_digikit(s)
    s.tap(YES, gap=DG).frame("about")  # ABOUT is the first row
    return s.tap(NO, gap=G).frame("back_main")


@script({"frames": ["settings_digikit", "digikit_main", "about", "back_main"]})
def digikit_about() -> Script:
    """DIGIKIT > ABOUT (runtime ABI, loaded count, free KB), then NO back to the DIGIKIT view."""
    return digikit_about_script()


# --- MIDI -------------------------------------------------------------------

# Read-only MidiRpc requests (docs/MIDI-SYSEX-RPC.md section 11): (type, payload). The Data object calls
# have no documented payload; these two are guesses (a path string) and only show that the firmware parses and
# answers (or ignores) them.
RPC_INFO = [
    (0x01, b""),  # Ping
    (0x02, b""),  # SoftwareVersion
    (0x03, b""),  # DeviceUID
    (0x05, b"\x01"),  # StorageSpace, drive 1
    (0x06, b""),  # TempoRead
    (0x09, b"sample_file.interleaved_stereo_support\x00"),  # Query
    (
        0x04,
        b"",
    ),  # Screenshot (1024 bytes of the OLED shadow); last: on DT2 it is never answered, and nothing after it is
]
RPC_FILES = [
    (0x10, b"/\x00"),  # ReadDir /
    (0x10, b"/factory\x00"),
    (0x53, b"/projects\x00"),  # DataList (payload layout not documented: a path)
    (0x53, b"/soundbanks\x00"),
    (
        0x54,
        b"/projects/001\x00",
    ),  # DataReadOpen (payload layout not documented: a path)
]


def _u32s(*v: int) -> bytes:
    return b"".join(x.to_bytes(4, "big") for x in v)


# The file the FsSample write check creates: a 2048-byte 16-bit mono WAV (the firmware stores a .wav as a native sample, so the
# bytes must parse), sent as two 1024-byte requests. Its samples are a ramp that has bit 7 set in about half of the bytes (they
# travel through the 8-in-7 packing of the device-0x10 framing, finding 42 [C]); checked byte for byte after the run.
WRITE_NAME = "arenaprobe.wav"  # in the root of the path filesystem
WRITE_CHUNK = 1024


def _write_wav() -> bytes:
    n = 2048 - 44
    pcm = b"".join(
        ((i * 331 + 0x4081) & 0xFFFF).to_bytes(2, "little") for i in range(n // 2)
    )
    fmt = (
        b"fmt "
        + (16).to_bytes(4, "little")
        + _le16(1, 1)
        + (44100).to_bytes(4, "little")
        + (88200).to_bytes(4, "little")
        + _le16(2, 16)
    )
    return (
        b"RIFF"
        + (36 + len(pcm)).to_bytes(4, "little")
        + b"WAVE"
        + fmt
        + b"data"
        + len(pcm).to_bytes(4, "little")
        + pcm
    )


def _le16(*v: int) -> bytes:
    return b"".join(x.to_bytes(2, "little") for x in v)


WRITE_DATA = _write_wav()


def _write_v1(handle: int, offset: int, data: bytes) -> bytes:
    """WriteFileV1 body (finding 42 section 3): u32 handle, u32 length, u32 offset (= the file position), data."""
    return _u32s(handle, len(data), offset) + data


# Writing RPC (open: a u32 the firmware takes as 0 and as any other value, then the file name without a slash; then WriteFileV1 {handle, length, offset, data} in file order, then close; the framing of every
# request is packed, devrpc.build_request). The handle of the first open file is 0 (the replies confirm: zero status).
# The Data object writes (0x57, 0x58) are refused: "Unable to handle path", "File transfer is inactive".
RPC_WRITE = [
    (
        0x40,
        _u32s(0) + WRITE_NAME.encode() + b"\x00",
    ),  # FsSampleOpenFileForWrite: u32 (0), then the name, no slash
    (0x42, _write_v1(0, 0, WRITE_DATA[:WRITE_CHUNK])),  # FsSampleWriteFileV1, offset 0
    (
        0x42,
        _write_v1(0, WRITE_CHUNK, WRITE_DATA[WRITE_CHUNK:]),
    ),  # FsSampleWriteFileV1, offset 1024
    (
        0x46,
        _u32s(0, 2 * WRITE_CHUNK, 8) + b"RIFFWAVE",
    ),  # FsSampleWriteFileV2 (layout unread)
    (0x41, _u32s(0)),  # close
    (0x57, b"/projects/002\x00"),  # DataWriteOpen
    (0x58, b"\x00" * 8),  # DataWritePartial
    (0x10, b"/\x00"),  # ReadDir: the new file is listed
]


def _rpc(reqs, gap: int = 40_000_000) -> Script:
    from tools.device import devrpc

    s = boot().tap(NO, gap=G)
    for k, (type_byte, payload) in enumerate(reqs, 1):
        s.usb(devrpc.build_request(type_byte, seq=k, payload=payload)).wait(gap)
    return s


def split_sysex(stream: bytes) -> list[bytes]:
    """The F0 ... F7 messages in a captured byte stream."""
    out, cur = [], None
    for b in stream:
        if b == 0xF0:
            cur = bytearray([b])
        elif cur is not None:
            cur.append(b)
            if b == 0xF7:
                out.append(bytes(cur))
                cur = None
    return out


@script({"usb_replies": {"dt2": [1, 2, 3, 5, 6, 9], "dn2": [1, 2, 3, 6, 9, 4]}})
def midirpc_info() -> Script:
    """MidiRpc over the USB callback: Ping, SoftwareVersion, DeviceUID, StorageSpace, TempoRead, Query, Screenshot.
    DT2 answers all but Screenshot (no reply at all, here); DN2 has no StorageSpace handler and answers Screenshot."""
    return _rpc(RPC_INFO)


@script({"usb_replies": {"dt2": [16, 16, 83, 83, 84], "dn2": [83, 83, 84]}})
def midirpc_files() -> Script:
    """MidiRpc ReadDir (and the undocumented Data calls as path guesses) over the USB callback."""
    return _rpc(RPC_FILES)


@script({"usb_replies": {"dt2": [64, 66, 66, 70, 65, 87, 88, 16], "dn2": [87, 88]}})
def midirpc_write() -> Script:
    """MidiRpc writes over the USB callback: a sample file is created and written by path (FsSample), the Data
    object writes are refused."""
    return _rpc(RPC_WRITE)


@script({"din_bytes": 28, "pcs": {"0x4011d1f6": 28}})
def din_midi() -> Script:
    """MIDI DIN at wire rate: note on/off and a CC on channel 1, a Sample Dump inquiry (no reply on DIN out), then a
    MidiRpc Ping (not accepted from DIN: the firmware's device 0x10 handler takes only the USB sources 2 and 4)."""
    from tools.device import devrpc

    s = boot().tap(NO, gap=G)
    # DT2 1.16: the DIN byte parser, the device 0x10 handler, the MidiRpc receive chain
    s.extra += ["--pc", "0x4011d1f6", "--pc", "0x4011efb8", "--pc", "0x40125184"]
    s.midi(bytes([0x90, 0x3C, 0x64])).wait(5_000_000)
    s.midi(bytes([0x80, 0x3C, 0x40])).wait(5_000_000)
    s.midi(bytes([0xB0, 0x07, 0x50])).wait(5_000_000)
    s.midi(bytes.fromhex("f07e7f0601f7")).wait(30_000_000)
    s.midi(devrpc.build_request(0x01, seq=1)).wait(60_000_000)
    return s
