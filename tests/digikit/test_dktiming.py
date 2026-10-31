"""The timing of a put (tools/device/dktiming.py), `put --chunk --depth --timing` of tools/device/dkupload.py, and the parts of
tools/device/winmidi.py that run without Windows. No emulator, no device."""

import json

import pytest

from tools.device import dktiming as T
from tools.device import dkupload as D
from tools.device import winmidi
from tools.firmware.build import encode_8in7


def _ok_reply(msg: bytes) -> bytes:
    return msg[:4] + bytes([1, msg[5] | 0x40, msg[6]]) + encode_8in7(b"\x00") + b"\xf7"


class Clock:
    def __init__(self):
        self.now = 100.0

    def __call__(self) -> float:
        return self.now


class Device:
    """Answers every request with ok. A send takes `send_s`; each reply comes `work_s` after its request was taken, one at a time."""

    def __init__(self, clock: Clock, send_s: float, work_s: float):
        self.clock, self.send_s, self.work_s = clock, send_s, work_s
        self.pending: list[tuple[float, bytes]] = []
        self.free_at = 0.0

    def send(self, msg: bytes) -> None:
        self.clock.now += self.send_s
        start = max(self.clock.now, self.free_at)
        self.free_at = start + self.work_s
        self.pending.append((self.free_at, _ok_reply(msg)))

    def recv(self) -> bytes:
        at, reply = self.pending.pop(0)
        self.clock.now = max(self.clock.now, at)
        return reply


def test_depth_one_put_times_each_request_and_reply():
    clock = Clock()
    t = T.TimedTransport(Device(clock, send_s=0.010, work_s=0.030), clock=clock)
    D.Uploader(t, chunk=1024, depth=1).put("a.bin", bytes(4096))
    cmds = [(e["dir"], e["cmd"]) for e in t.events]
    assert cmds == [("tx", "BEGIN"), ("rx", "BEGIN")] + [
        ("tx", "DATA"),
        ("rx", "DATA"),
    ] * 4 + [("tx", "END"), ("rx", "END")]
    assert t.events[0]["t"] == 0.0 and t.events[0]["t_done"] == pytest.approx(0.010)
    s = T.summary(t.events, 4096)
    assert s["chunks"] == 4
    assert s["send_mean_ms"] == pytest.approx(10.0)
    assert s["rtt_mean_ms"] == pytest.approx(40.0)  # 10 ms to send, 30 ms on the device
    assert s["data_s"] == pytest.approx(0.160)
    assert s["kbps"] == pytest.approx(4096 / 0.160 / 1000)
    assert s["wall_s"] == pytest.approx(0.240)  # BEGIN, four DATA, END: 40 ms each
    assert s["end_s"] == pytest.approx(0.040)
    assert "4 DATA" in T.summary_line(s) and "KB/s" in T.summary_line(s)


def test_depth_two_overlaps_the_send_with_the_device_work():
    def data_s(depth: int) -> float:
        clock = Clock()
        t = T.TimedTransport(Device(clock, send_s=0.020, work_s=0.020), clock=clock)
        D.Uploader(t, chunk=512, depth=depth).put("a.bin", bytes(512 * 8))
        return T.summary(t.events, 512 * 8)["data_s"]

    assert data_s(1) == pytest.approx(8 * 0.040)
    assert data_s(2) < 0.7 * data_s(1)


def test_replies_match_requests_by_seq_and_a_lost_reply_is_left_out():
    ev = [
        {"dir": "tx", "t": 0.0, "t_done": 0.1, "cmd": "DATA", "seq": 1},
        {"dir": "tx", "t": 0.1, "t_done": 0.2, "cmd": "DATA", "seq": 2},
        {"dir": "rx", "t": 0.5, "cmd": "DATA", "seq": 2, "status": 0},
        {"dir": "rx", "t": 0.3, "cmd": "INFO", "seq": 1, "status": 0},
    ]
    s = T.summary(ev, 512)
    assert s["chunks"] == 1 and s["rtt_mean_ms"] == pytest.approx(400.0)


def test_p95_is_the_nearest_rank():
    assert T.p95([]) == 0.0
    assert T.p95([3.0]) == 3.0
    assert T.p95([float(x) for x in range(1, 101)]) == 95.0
    assert T.p95([float(x) for x in range(1, 21)]) == 19.0


def test_a_transport_stamp_on_arrival_wins_over_the_return_time():
    clock = Clock()

    class Stamped(Device):
        last_rx_t = None

        def recv(self) -> bytes:
            r = super().recv()
            self.last_rx_t = clock.now
            clock.now += 0.5  # the reply waited in the queue before recv returned it
            return r

    t = T.TimedTransport(Stamped(clock, 0.0, 0.010), clock=clock)
    D.Uploader(t, chunk=512, depth=1).put("a.bin", bytes(512))
    assert T.summary(t.events, 512)["rtt_mean_ms"] == pytest.approx(10.0)


class FakeEmu:
    """`midi.usb.in` / `midi.usb.out` of the emulator: a reply shows after the next step."""

    def __init__(self):
        self.later: list[bytes] = []
        self.out = b""

    def usb_in(self, msg: bytes, chunk, period) -> None:
        self.later.append(_ok_reply(msg))

    def step(self, n: int) -> None:
        self.out += b"".join(self.later)
        self.later = []

    def usb_out(self) -> bytes:
        o, self.out = self.out, b""
        return o


def test_cli_put_with_chunk_depth_and_timing_over_the_emulator_transport(
    tmp_path, monkeypatch, capsys
):
    monkeypatch.setattr(D, "_emu", lambda run_dir=None: FakeEmu())
    src = tmp_path / "s.bin"
    src.write_bytes(bytes(range(256)) * 40)  # 10240 bytes: 5 chunks of 2048
    log = tmp_path / "put.jsonl"
    rc = D.main(
        [
            "--emu",
            "put",
            str(src),
            "samples/s.bin",
            "--chunk",
            "2048",
            "--depth",
            "1",
            "--timing",
            str(log),
        ]
    )
    assert rc == 0
    out = capsys.readouterr().out.splitlines()
    assert out[0].startswith("samples/s.bin: 10240 bytes")
    assert (
        "10240 bytes, 5 DATA" in out[1]
        and "DATA stepped 2000000 instructions" in out[1]
    )
    ev = [json.loads(x) for x in log.read_text().splitlines()]
    assert [e["cmd"] for e in ev if e["dir"] == "tx"] == ["BEGIN"] + ["DATA"] * 5 + [
        "END"
    ]
    assert all("stepped" in e for e in ev)
    bad = tmp_path / "bad.jsonl"
    with pytest.raises(SystemExit) as e:
        D.main(["--emu", "put", str(src), "--chunk", "100", "--timing", str(bad)])
    assert e.value.code == 2 and "multiple of 512" in capsys.readouterr().err
    assert not bad.exists()  # rejected before the timing file is opened


def test_pick_takes_one_port_and_names_the_others():
    names = ["Elektron Digitone", "Elektron Digitone II", "Other"]
    assert winmidi.pick(names, "digitone ii", "in") == 1
    with pytest.raises(OSError, match="more than one"):
        winmidi.pick(names, "Digitone", "in")
    with pytest.raises(OSError, match="no MIDI out port.*0 Elektron Digitone"):
        winmidi.pick(names, "Digitakt", "out")
