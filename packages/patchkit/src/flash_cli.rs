//! `patchkit flash`: the command line around `patchkit::flash`. The checks, the message list and the pacing live in the library; this file
//! adds the people (instructions, the word "flash"), a MIDI output (midir) or the emulator's control socket, and the look at the device after
//! the restart (INFO: runtime version and image id).

use dkupload::{Info, Session, Transport};
use patchkit::error::{Error, Result};
use patchkit::flash::{self, Prepared, Sender, Step};
use patchkit::module::Library;
use patchkit::port::Registry;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub const USAGE: &str = "usage: patchkit flash STOCK.syx (--port NAME | --emulator SOCKET) [--restore] [--set NAME] [--dry-run] [--save OUT.syx] [--yes]
  STOCK.syx   the stock firmware file of the device (it is identified by its SHA-256; a patched file is refused)
  --port      the MIDI port of the device (part of its name; USB MIDI)
  --emulator  the control socket of a running emulator that shows OS UPGRADE (tools/emulator/os_upgrade_rig.py)
  --restore   send the stock file itself (the way back from a patched image)
  --set       the device file's patch set to apply (default: device)
  --dry-run   patch and check; print the checks and stop
  --save F    also write the image that is checked and sent to F (for `mise run patch:preflight`, the section 7 ISA scan)
  --yes       do not wait for the word \"flash\"";

fn flag(rest: &[String], name: &str) -> Option<String> {
    rest.iter()
        .position(|a| a == name)
        .and_then(|i| rest.get(i + 1).cloned())
}
fn has(rest: &[String], name: &str) -> bool {
    rest.iter().any(|a| a == name)
}

pub fn run(reg: &Registry, lib: &Library, rest: &[String]) -> Result<()> {
    let path = rest
        .first()
        .filter(|a| !a.starts_with("--"))
        .ok_or_else(|| Error(USAGE.into()))?;
    let stock = std::fs::read(path).map_err(|e| Error(format!("{path}: {e}")))?;
    let (port, emu) = (flag(rest, "--port"), flag(rest, "--emulator"));
    let (restore, dry, yes) = (
        has(rest, "--restore"),
        has(rest, "--dry-run"),
        has(rest, "--yes"),
    );
    if !dry && port.is_some() == emu.is_some() {
        return Err(Error(format!(
            "give exactly one of --port and --emulator (or --dry-run)\n{USAGE}"
        )));
    }
    let t0 = Instant::now();
    let p = if restore {
        flash::restore(&stock, reg)?
    } else {
        flash::prepare(
            &stock,
            reg,
            lib,
            &flag(rest, "--set").unwrap_or_else(|| "device".into()),
        )?
    };
    if let Some(f) = flag(rest, "--save") {
        std::fs::write(&f, &p.image).map_err(|e| Error(format!("{f}: {e}")))?;
        println!("image written to {f}");
    }
    print_checks(&p);
    println!("prepared in {:.1} s", t0.elapsed().as_secs_f64());
    if !p.ok() {
        return Err(Error(format!(
            "{} check(s) failed: nothing is sent",
            p.failed().len()
        )));
    }
    let secs = p.messages.len() as f64 * flash::PACE_MS as f64 / 1000.0;
    println!(
        "{} {} {}: {} messages, {} bytes, about {:.0} s to send{}",
        if p.restore { "restore" } else { "flash" },
        p.device,
        p.os,
        p.messages.len(),
        p.image.len(),
        secs,
        p.image_id_hex()
            .map_or(String::new(), |i| format!(", image id {i}"))
    );
    if dry {
        return Ok(());
    }
    if let Some(sock) = emu {
        return emulator(&p, &sock);
    }
    let name = port.unwrap();
    println!("{}", instructions(&p, secs));
    if !yes {
        print!("Type \"flash\" and press Return to send (anything else stops): ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).ok();
        if line.trim() != "flash" {
            return Err(Error("stopped: nothing was sent".into()));
        }
    }
    device(&p, &name)
}

fn print_checks(p: &Prepared) {
    for c in &p.checks {
        let d: String = c
            .detail
            .chars()
            .take(if c.ok { 150 } else { 600 })
            .collect();
        println!(
            "{:<6} {:<12} {d}",
            if c.ok { "ok" } else { "FAILED" },
            c.name
        );
    }
}

fn instructions(p: &Prepared, secs: f64) -> String {
    let what = if p.restore {
        format!("the stock {} {} firmware", p.device, p.os)
    } else {
        format!("a patched {} {} firmware", p.device, p.os)
    };
    format!(
        "
About to send {what} to the device's OS UPGRADE screen.
  1. Connect the power adapter. Keep the USB cable in until the device has restarted.
  2. On the device open SETTINGS > SYSTEM > OS UPGRADE and wait for the line WAITING FOR SYSEX.
  3. Sending takes about {secs:.0} s. The device then checks the image, erases and writes its flash and restarts by itself (no YES key).
     Do not touch the device, the cable or the power until it has restarted.
If the image does not pass the device's own check it shows an error and erases nothing.
If a flash is cut or the device does not start again: power off, hold FUNC while powering on, press TRIG 4 (OS UPGRADE), and send the
stock file over a DIN MIDI interface (docs/PATCHING.md, Recovery). Keep the stock file of the installed version."
    )
}

// ---- a MIDI port ------------------------------------------------------------------------------------------------------------

/// SysEx messages out of the pieces a MIDI callback delivers.
#[derive(Default)]
struct Sysex(Vec<u8>);

impl Sysex {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut done = vec![];
        for &b in bytes {
            if b == 0xf0 {
                self.0.clear();
                self.0.push(b);
            } else if self.0.is_empty() || b >= 0xf8 {
                continue;
            } else if b == 0xf7 {
                self.0.push(b);
                done.push(std::mem::take(&mut self.0));
            } else if b < 0x80 {
                self.0.push(b);
            } else {
                self.0.clear();
            }
        }
        done
    }
}

fn find_port(names: &[String], want: &str) -> Result<String> {
    let w = want.to_lowercase();
    let hits: Vec<&String> = names
        .iter()
        .filter(|n| n.to_lowercase().contains(&w))
        .collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(Error(format!(
            "no MIDI port with {want:?} in its name (ports: {})",
            if names.is_empty() {
                "none".into()
            } else {
                names.join(", ")
            }
        ))),
        many => Err(Error(format!(
            "{want:?} names several ports: {}",
            many.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn out_names() -> Result<Vec<String>> {
    let o = midir::MidiOutput::new("patchkit").map_err(|e| Error(e.to_string()))?;
    Ok(o.ports()
        .iter()
        .filter_map(|p| o.port_name(p).ok())
        .collect())
}

struct MidiLink {
    out: midir::MidiOutputConnection,
    _input: midir::MidiInputConnection<()>,
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
}

fn open(name: &str) -> Result<MidiLink> {
    let e = |e: &dyn std::fmt::Display| Error(e.to_string());
    let mut input = midir::MidiInput::new("patchkit").map_err(|x| e(&x))?;
    input.ignore(midir::Ignore::None);
    let output = midir::MidiOutput::new("patchkit").map_err(|x| e(&x))?;
    let ip = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).is_ok_and(|n| n == name))
        .ok_or_else(|| Error(format!("no MIDI input port {name:?}")))?;
    let op = output
        .ports()
        .into_iter()
        .find(|p| output.port_name(p).is_ok_and(|n| n == name))
        .ok_or_else(|| Error(format!("no MIDI output port {name:?}")))?;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut asm = Sysex::default();
    let conn = input
        .connect(
            &ip,
            "patchkit in",
            move |_, bytes, _| {
                for m in asm.feed(bytes) {
                    let _ = tx.send(m);
                }
            },
            (),
        )
        .map_err(|x| e(&x))?;
    let out = output.connect(&op, "patchkit out").map_err(|x| e(&x))?;
    Ok(MidiLink {
        out,
        _input: conn,
        rx,
    })
}

impl Transport for MidiLink {
    fn send(&mut self, msg: &[u8]) -> std::result::Result<(), String> {
        self.out.send(msg).map_err(|e| e.to_string())
    }
    fn recv(&mut self, timeout: Duration) -> std::result::Result<Option<Vec<u8>>, String> {
        match self.rx.recv_timeout(timeout) {
            Ok(m) => Ok(Some(m)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The messages out over the sender's clock; stops at the first error.
fn send_all(
    p: &Prepared,
    mut out: impl FnMut(&[u8]) -> std::result::Result<(), String>,
) -> Result<()> {
    let start = Instant::now();
    let mut s = Sender::new(p.messages.len());
    loop {
        let now = start.elapsed().as_millis() as u64;
        match s.next(now) {
            Step::Send(i) => {
                if let Err(e) = out(p.message(i)) {
                    s.fail();
                    return Err(Error(format!(
                        "message {i} of {} failed: {e}. The device keeps its old image (a cut stream is never used); start again from the header",
                        s.total()
                    )));
                }
                if i % 2000 == 0 || i + 1 == s.total() {
                    println!(
                        "sent {} of {} ({:.0} s)",
                        i + 1,
                        s.total(),
                        start.elapsed().as_secs_f64()
                    );
                }
            }
            Step::Wait(ms) => std::thread::sleep(Duration::from_millis(ms)),
            Step::Done => return Ok(()),
            Step::Aborted => return Err(Error("aborted".into())),
        }
    }
}

fn device(p: &Prepared, want: &str) -> Result<()> {
    let name = find_port(&out_names()?, want)?;
    println!("port {name:?}");
    let mut link = open(&name)?;
    let t0 = Instant::now();
    send_all(p, |m| link.send(m))?;
    println!(
        "all {} messages sent in {:.1} s; the device checks and flashes now",
        p.messages.len(),
        t0.elapsed().as_secs_f64()
    );
    drop(link);
    // the device restarts: its port goes away and comes back
    wait_port(&name, false, Duration::from_secs(240))
        .map_err(|e| Error(format!("{e}: look at the device screen; it still runs the old image unless it shows the flash running")))?;
    println!("port gone after {:.0} s", t0.elapsed().as_secs_f64());
    wait_port(&name, true, Duration::from_secs(240))?;
    println!("port back after {:.0} s", t0.elapsed().as_secs_f64());
    if p.restore {
        println!("stock firmware restored (it has no digikit runtime to ask)");
        return Ok(());
    }
    let mut last = String::from("no answer");
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(2));
        let r = open(&name).and_then(|l| {
            let mut s = Session::new(l);
            s.timeout = Duration::from_secs(2);
            s.retries = 1;
            s.info().map_err(|e| Error(e.to_string()))
        });
        match r {
            Ok(info) => return check_info(p, &info),
            Err(e) => last = e.0,
        }
    }
    Err(Error(format!(
        "the port is back but INFO does not answer ({last}): the new image may not have reached the runtime; look at the screen"
    )))
}

fn wait_port(name: &str, present: bool, within: Duration) -> Result<()> {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if out_names()?.iter().any(|n| n == name) == present {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Err(Error(format!(
        "the port {name:?} did not {} within {} s",
        if present { "come back" } else { "go away" },
        within.as_secs()
    )))
}

/// INFO after the restart: the runtime version and the image id must be what the patch made.
fn check_info(p: &Prepared, info: &Info) -> Result<()> {
    let want = patchkit::dkm::runtime_version();
    let got = info.runtime();
    println!(
        "INFO: device {}, runtime {} (expected {}), image id {}",
        info.port,
        patchkit::dkm::fmt_runtime(got),
        patchkit::dkm::fmt_runtime(want),
        info.image_id
            .map_or("none".into(), |i| patchkit::error::hex(&i))
    );
    if got != want {
        return Err(Error(
            "the runtime version differs from the one patched in".into(),
        ));
    }
    if info.image_id != p.image_id {
        return Err(Error(format!(
            "the image id differs: the device reports {:?}, the patch made {:?}",
            info.image_id.map(|i| patchkit::error::hex(&i)),
            p.image_id_hex()
        )));
    }
    println!("OK: the device runs the image that was sent");
    Ok(())
}

// ---- the emulator -----------------------------------------------------------------------------------------------------------

/// The emulator's control protocol: JSON-RPC lines over a Unix socket (docs/CONTROL.md).
struct Emu {
    w: UnixStream,
    r: BufReader<UnixStream>,
    id: u64,
    /// The guest instruction count the virtual clock is built on: 132,000 instructions are a millisecond (the oracle clock).
    used: u64,
    /// Events that arrived while a call waited for its reply (after `events.subscribe`).
    events: Vec<Value>,
}

const INSTR_PER_MS: u64 = 132_000;

impl Emu {
    fn connect(path: &str) -> Result<Emu> {
        let w = UnixStream::connect(path).map_err(|e| Error(format!("{path}: {e}")))?;
        let r = BufReader::new(w.try_clone().map_err(|e| Error(e.to_string()))?);
        let mut e = Emu {
            w,
            r,
            id: 0,
            used: 0,
            events: vec![],
        };
        e.call("hello", json!({"min_schema": 1}))?;
        e.call("events.subscribe", json!({}))?;
        Ok(e)
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.id += 1;
        let req = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        writeln!(self.w, "{req}").map_err(|e| Error(e.to_string()))?;
        loop {
            let mut line = String::new();
            if self
                .r
                .read_line(&mut line)
                .map_err(|e| Error(e.to_string()))?
                == 0
            {
                return Err(Error("the emulator closed the connection".into()));
            }
            let v: Value = serde_json::from_str(&line)?;
            if v.get("id").and_then(Value::as_u64) != Some(self.id) {
                if v.get("method").and_then(Value::as_str) == Some("event") {
                    self.events.push(v["params"].clone());
                }
                continue;
            }
            if let Some(e) = v.get("error") {
                return Err(Error(format!("{method}: {e}")));
            }
            return Ok(v["result"].clone());
        }
    }
    fn step(&mut self, n: u64) -> Result<Value> {
        let r = self.call("step", json!({"instructions": n}))?;
        self.used += n;
        Ok(r)
    }
    fn usb_in(&mut self, bytes: &[u8]) -> Result<()> {
        // one call per message: all of it in one chunk (a chunk is at most 384 bytes)
        self.call(
            "midi.usb.in",
            json!({"bytes": patchkit::error::hex(bytes), "chunk": bytes.len().clamp(1, 384)}),
        )?;
        Ok(())
    }
    fn usb_out(&mut self) -> Result<Vec<u8>> {
        let r = self.call("midi.usb.out", json!({}))?;
        patchkit::error::unhex(r["hex"].as_str().unwrap_or(""))
    }
    fn flash_status(&mut self) -> Result<(u64, u64)> {
        let r = self.call("flash.status", json!({}))?;
        Ok((
            r["erases"].as_u64().unwrap_or(0),
            r["programs"].as_u64().unwrap_or(0),
        ))
    }
}

/// INFO through the emulated USB: the request goes in, the emulator runs until a whole reply is out.
struct EmuLink<'a> {
    emu: &'a mut Emu,
    buf: Vec<u8>,
}

impl Transport for EmuLink<'_> {
    fn send(&mut self, msg: &[u8]) -> std::result::Result<(), String> {
        self.buf.clear();
        self.emu.usb_out().map_err(|e| e.0)?;
        self.emu.usb_in(msg).map_err(|e| e.0)
    }
    fn recv(&mut self, _timeout: Duration) -> std::result::Result<Option<Vec<u8>>, String> {
        // up to 600 M instructions (4.5 emulated seconds) in slices
        for _ in 0..30 {
            self.emu.step(20_000_000).map_err(|e| e.0)?;
            self.buf.extend(self.emu.usb_out().map_err(|e| e.0)?);
            if let Some(end) = self.buf.iter().position(|&b| b == 0xf7)
                && let Some(start) = self.buf[..end].iter().rposition(|&b| b == 0xf0)
            {
                let m = self.buf[start..=end].to_vec();
                self.buf.drain(..=end);
                return Ok(Some(m));
            }
        }
        Ok(None)
    }
}

fn emulator(p: &Prepared, sock: &str) -> Result<()> {
    let mut emu = Emu::connect(sock)?;
    let t0 = Instant::now();
    emu.call("midi.usb.out", json!({}))?; // starts the USB capture
    // The sender runs on the emulated clock: a step of the guest is a pause of the sender.
    let mut s = Sender::new(p.messages.len());
    loop {
        let now = emu.used / INSTR_PER_MS;
        match s.next(now) {
            Step::Send(i) => {
                emu.usb_in(p.message(i))?;
                if i % 2000 == 0 || i + 1 == s.total() {
                    println!(
                        "sent {} of {} (guest {} ms, wall {:.0} s)",
                        i + 1,
                        s.total(),
                        now,
                        t0.elapsed().as_secs_f64()
                    );
                }
            }
            Step::Wait(ms) => {
                emu.step(ms * INSTR_PER_MS)?;
            }
            Step::Done => break,
            Step::Aborted => return Err(Error("aborted".into())),
        }
    }
    println!(
        "all {} messages sent in {:.1} s wall ({} ms guest)",
        p.messages.len(),
        t0.elapsed().as_secs_f64(),
        emu.used / INSTR_PER_MS
    );
    // The device validates, then erases and programs the container area. Run until the programming stops, then compare.
    let want = flash::container_of(&p.image)?;
    let t1 = Instant::now();
    let mut last = (0, 0);
    let mut stable = 0;
    for _ in 0..300 {
        emu.step(50_000_000)?;
        let st = emu.flash_status()?;
        if st.1 > 0 && st == last {
            stable += 1;
            if stable >= 2 {
                break;
            }
        } else {
            stable = 0;
        }
        last = st;
    }
    println!(
        "flash writes: {} erases, {} programs after {:.1} s wall",
        last.0,
        last.1,
        t1.elapsed().as_secs_f64()
    );
    if last.1 == 0 {
        return Err(Error("the device wrote nothing to its flash (the check failed on the device side, or the OS UPGRADE screen was not open)".into()));
    }
    let mut got = Vec::with_capacity(want.len());
    while got.len() < want.len() {
        let n = (want.len() - got.len()).min(1 << 20);
        let r = emu.call(
            "flash.read",
            json!({"offset": 0x80000 + got.len(), "length": n}),
        )?;
        got.extend(patchkit::error::unhex(r["hex"].as_str().unwrap_or(""))?);
    }
    if got != want {
        let at = got.iter().zip(&want).position(|(a, b)| a != b).unwrap_or(0);
        return Err(Error(format!(
            "the flash container differs from the image at byte {at}"
        )));
    }
    println!(
        "OK: the flash container ({} bytes at 0x80000) equals the image's container",
        want.len()
    );
    // The device restarts by itself. The emulator boots the container the guest wrote (the soft reset). The cold boot is a job of its
    // own that refuses steps until it is done; its end is the `firmware.loaded` event.
    let mut booted = false;
    let end = Instant::now() + Duration::from_secs(1800);
    while !booted && Instant::now() < end {
        if emu.step(20_000_000).is_err() {
            std::thread::sleep(Duration::from_millis(500)); // the boot runs on its worker
        }
        for ev in std::mem::take(&mut emu.events) {
            match ev["kind"].as_str() {
                Some("firmware.loaded") if ev["ok"].as_bool() == Some(true) => booted = true,
                Some("firmware.loaded") => {
                    return Err(Error(format!(
                        "the cold boot of the flashed image failed: {ev}"
                    )));
                }
                Some("flash.invalid") => {
                    return Err(Error(format!(
                        "the emulator could not boot the flash container: {ev}"
                    )));
                }
                _ => {}
            }
        }
    }
    if !booted {
        return Err(Error("the device did not restart after the flash".into()));
    }
    println!(
        "restart seen after {:.0} s wall",
        t0.elapsed().as_secs_f64()
    );
    if p.restore {
        return Ok(());
    }
    // let the new image boot, then ask INFO over the emulated USB
    emu.call("midi.usb.out", json!({}))?;
    let mut last_err = String::new();
    for _ in 0..40 {
        emu.step(200_000_000)?;
        let mut s = Session::new(EmuLink {
            emu: &mut emu,
            buf: vec![],
        });
        s.timeout = Duration::from_secs(1);
        s.retries = 0;
        match s.info() {
            Ok(info) => return check_info(p, &info),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(Error(format!(
        "INFO did not answer after the restart: {last_err}"
    )))
}
