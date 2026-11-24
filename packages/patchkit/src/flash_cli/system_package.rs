//! The CLI system package installer, over the same MIDI and emulator links as flash.

use super::{Emu, EmuRpcLink, INSTR_PER_MS, MidiLink, find_port, is_patched, open, out_names};
use dkrt_sdk::logk::BOOT_START;
use dkrt_sdk::proto::{
    CAP_FOLDERS, CAP_LOG, CAP_MODULES, CMD_MODULES, CMD_RESTART, CMD_RETRY, SVC_LOG,
};
use dkupload::{Error as UploadError, Info, Session, Status, Transport, file_hash};
use patchkit::error::{Error, Result};
use patchkit::flash::Prepared;
use patchkit::syspkg::{self, Artifact, InstallAction, InstallRefusal};
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

const STORE_PATH: &str = "digikit/system.dksp";
const BOOT_TIMEOUT: Duration = Duration::from_secs(90);
const USAGE: &str = "usage: patchkit syspkg install [FILE.dksp] (--port NAME | --emulator SOCKET)
  Recover module support on a core-only runtime from the same build.
  Omit FILE.dksp to use the package linked from this checkout.";

pub(crate) fn recover(root: &Path, args: &[String]) -> Result<()> {
    if args == ["--help"] {
        println!("{USAGE}");
        return Ok(());
    }
    let mut file = None;
    let mut target = None;
    let mut it = args.iter().map(String::as_str);
    while let Some(arg) = it.next() {
        match arg {
            "--port" | "--emulator" if target.is_none() => {
                let name = it
                    .next()
                    .filter(|name| !name.is_empty() && !name.starts_with("--"))
                    .ok_or_else(|| Error(USAGE.into()))?;
                target = Some((arg, name));
            }
            arg if !arg.starts_with('-') && file.is_none() => file = Some(arg),
            _ => return Err(Error(USAGE.into())),
        }
    }
    let (kind, name) = target.ok_or_else(|| Error(USAGE.into()))?;
    let package = if let Some(path) = file {
        explicit_package(root, Path::new(path))?
    } else {
        syspkg::build_from_repo(root)?
    };
    if kind == "--port" {
        let name = find_port(&out_names()?, name)?;
        install(Session::new(Midi::open(&name)?), &package, None)
    } else {
        let mut emu = Emu::connect(name)?;
        install(Session::new(Emulated::new(&mut emu)?), &package, None)
    }
}

fn explicit_package(root: &Path, path: &Path) -> Result<Artifact> {
    let bytes = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    check_explicit_package(root, path, bytes)
}

fn check_explicit_package(root: &Path, path: &Path, bytes: Vec<u8>) -> Result<Artifact> {
    let file_error = |e: Error| Error(format!("{}: {e}", path.display()));
    let package = Artifact::from_bytes(bytes).map_err(&file_error)?;
    // The DKSP header checks the resident ABI. Only the canonical bytes tie it to this build.
    let canonical = syspkg::build_from_repo(root).map_err(|e| {
        file_error(Error(format!(
            "could not link the system package from this checkout: {e}"
        )))
    })?;
    if package.bytes() != canonical.bytes() {
        return Err(file_error(Error(
            "does not match the system package linked from this checkout. Omit FILE.dksp to use this checkout's package.".into(),
        )));
    }
    Ok(package)
}

pub(super) fn after_midi_flash(
    package: &Artifact,
    name: &str,
    link: MidiLink,
    prepared: &Prepared,
) -> Result<()> {
    after_flash(install(
        Session::new(Midi {
            name,
            link,
            boot: None,
        }),
        package,
        Some(prepared),
    ))
}

pub(super) fn after_emu_flash(
    package: &Artifact,
    emu: &mut Emu,
    prepared: &Prepared,
) -> Result<()> {
    after_flash(
        Emulated::new(emu).and_then(|link| install(Session::new(link), package, Some(prepared))),
    )
}

fn after_flash(result: Result<()>) -> Result<()> {
    result.map_err(|e| Error(format!(
        "firmware flash verified, but system package setup failed: {e}\n         If the target boots in core-only mode, use patchkit syspkg install to recover module support."
    )))
}

fn upload<T>(result: std::result::Result<T, UploadError>, action: &str) -> Result<T> {
    result.map_err(|e| Error(format!("{action}: {e}")))
}

struct Identity {
    device: String,
    image_id: [u8; 8],
    runtime: u32,
}

impl Identity {
    fn confirm(&self, info: &Info) -> Result<()> {
        if info.port != self.device || info.image_id != Some(self.image_id) {
            return Err(Error("the target device or firmware changed".into()));
        }
        if info.runtime() != self.runtime {
            return Err(Error(format!(
                "the system package needs runtime {}, the target reports {}",
                patchkit::dkm::fmt_runtime(self.runtime),
                patchkit::dkm::fmt_runtime(info.runtime())
            )));
        }
        if !info.has_cap(CAP_MODULES) {
            return Err(Error(
                "the runtime does not advertise module support".into(),
            ));
        }
        Ok(())
    }
}

fn writable(info: &Info) -> Result<()> {
    // INFO flags (the resident admin command): bit 2 marks a read-only card.
    if info.flags & 4 != 0 {
        return Err(Error("the target card store is read-only".into()));
    }
    Ok(())
}

fn confirm_writable_target(
    session: &mut Session<impl Transport>,
    identity: &Identity,
) -> Result<Info> {
    let current = upload(
        session.info(),
        "checking the target before updating the card",
    )?;
    identity.confirm(&current)?;
    writable(&current)?;
    if current.upload_open() {
        return Err(Error("the target already has an upload in progress".into()));
    }
    Ok(current)
}

fn module_support(
    session: &mut Session<impl Transport>,
    info: &Info,
) -> std::result::Result<bool, UploadError> {
    match session.request(CMD_MODULES, &[0]) {
        Ok(body) if body.len() >= 18 => Ok(true),
        Ok(_) => Err(UploadError::Protocol(
            "MODULES reply is missing its page header".into(),
        )),
        Err(UploadError::Device {
            status: Status::Proto,
            ..
        }) if info.safe_mode() => Ok(false),
        Err(e) => Err(e),
    }
}

fn installed_bytes_match(
    session: &mut Session<impl Transport>,
    size: usize,
    hash: [u8; 32],
    info: &Info,
) -> Result<bool> {
    let files = if info.has_cap(CAP_FOLDERS) {
        session.list_prefix(STORE_PATH)
    } else {
        session.list()
    };
    let exact = upload(files, "listing the system package")?
        .iter()
        .any(|entry| entry.path == STORE_PATH && entry.size as usize == size);
    if !exact {
        return Ok(false);
    }
    match session.verify(STORE_PATH) {
        Ok(actual) => Ok(actual == hash),
        Err(UploadError::Device {
            status: Status::Read | Status::NoFile,
            ..
        }) => Ok(false),
        Err(e) => upload(Err(e), "verifying the system package"),
    }
}

fn install<T: PackageTransport>(
    mut session: Session<T>,
    package: &Artifact,
    prepared: Option<&Prepared>,
) -> Result<()> {
    let initial = upload(session.info(), "reading the system package target's INFO")?;
    if prepared.is_some_and(|p| !is_patched(p, &initial)) {
        return Err(Error(
            "the target no longer runs the firmware that was flashed".into(),
        ));
    }
    let identity = Identity {
        device: initial.port.clone(),
        image_id: initial
            .image_id
            .filter(|id| *id != [0; 8])
            .ok_or_else(|| Error("the target INFO has no stamped image id".into()))?,
        runtime: package.metadata().runtime_version(),
    };
    identity.confirm(&initial)?;
    let repair = prepared.is_none();
    let module_support = upload(module_support(&mut session, &initial), "checking MODULES")?;
    // Manual recovery must refuse an already healthy target before reading its card.
    syspkg::install_plan(repair, module_support, false).map_err(refusal)?;
    writable(&initial)?;
    println!("checking {STORE_PATH} on {}", identity.device);
    let bytes = package.bytes();
    let hash = file_hash(bytes);
    let same = installed_bytes_match(&mut session, bytes.len(), hash, &initial)?;
    let actions = syspkg::install_plan(repair, module_support, same).map_err(refusal)?;
    if actions.is_empty() {
        println!("OK: the system package is already verified and active");
        return Ok(());
    }
    for action in actions {
        match action {
            InstallAction::Put => {
                let current = confirm_writable_target(&mut session, &identity)?;
                session.chunk = session.chunk.min(current.max_chunk as usize);
                let mut step = 0;
                upload(
                    session.put(STORE_PATH, bytes.to_vec(), &mut |p| {
                        let pct = p.done * 20 / p.total.max(1);
                        if pct > step {
                            step = pct;
                            println!("uploaded {} of {} system package bytes", p.done, p.total);
                        }
                        true
                    }),
                    "uploading the system package",
                )?;
            }
            InstallAction::Verify => {
                if upload(
                    session.verify(STORE_PATH),
                    "verifying the uploaded system package",
                )? != hash
                {
                    return Err(Error(format!("verification failed for {STORE_PATH}")));
                }
                println!("system package verified on the card");
            }
            InstallAction::Retry => {
                confirm_writable_target(&mut session, &identity)?;
                upload(
                    session.request(CMD_RETRY, &[]),
                    "clearing the package boot counter",
                )?;
            }
            InstallAction::Restart => {
                println!("restarting for the system package");
                T::before_restart(&mut session, &initial, module_support)?;
                upload(session.request(CMD_RESTART, &[]), "restarting the target")?;
            }
            InstallAction::WaitForBoot => {
                T::wait_for_boot(&mut session, &identity).map_err(|e| {
                    Error(format!(
                        "system package installed but activation unconfirmed: {e}"
                    ))
                })?
            }
            InstallAction::ConfirmSupport => T::confirm_support(&mut session, &identity)?,
        }
    }
    println!("OK: the system package is active");
    Ok(())
}

fn refusal(refusal: InstallRefusal) -> Error {
    Error(refusal.to_string())
}

trait PackageTransport: Transport + Sized {
    fn before_restart(_session: &mut Session<Self>, _initial: &Info, _proof: bool) -> Result<()> {
        Ok(())
    }
    fn wait_for_boot(session: &mut Session<Self>, identity: &Identity) -> Result<()>;
    fn confirm_support(session: &mut Session<Self>, identity: &Identity) -> Result<()>;
}

struct Midi<'a> {
    name: &'a str,
    link: MidiLink,
    boot: Option<MidiBoot>,
}

struct MidiBoot {
    endpoint: Option<(midir::MidiOutput, String)>,
    port_disappeared: bool,
    log: Option<[u8; 32]>,
}

impl MidiBoot {
    fn ready(&self, port_present: Option<bool>, current_log: Option<[u8; 32]>) -> bool {
        (self.port_disappeared && port_present == Some(true))
            || matches!((self.log, current_log), (Some(old), Some(new)) if old != new)
    }
}

fn log_boot_start(session: &mut Session<impl Transport>) -> Option<[u8; 32]> {
    let page = session.request(SVC_LOG, &0u32.to_be_bytes()).ok()?;
    let count = usize::from(*page.get(8)?);
    if count == 0 || page.len() != 9 + 32 * count {
        return None;
    }
    let first = &page[9..41];
    // Sequence 1 is the boot event. Its arguments include the card's boot record
    // sequence and counter, as used by dk-session's LogCursor.
    (first[..4] == 1u32.to_be_bytes() && first[8..10] == BOOT_START.to_be_bytes())
        .then(|| first.try_into().expect("32-byte event"))
}

impl<'a> Midi<'a> {
    fn open(name: &'a str) -> Result<Self> {
        Ok(Self {
            name,
            link: open(name)?,
            boot: None,
        })
    }

    fn wait(
        session: &mut Session<Self>,
        identity: &Identity,
        mut ready: impl FnMut(
            &mut Session<Self>,
            &Info,
            Option<bool>,
        ) -> std::result::Result<bool, UploadError>,
        expectation: &str,
    ) -> Result<()> {
        quick_probes(session);
        let end = Instant::now() + BOOT_TIMEOUT;
        let mut reconnect = false;
        while Instant::now() < end {
            let port_present = session.transport.boot.as_mut().and_then(|boot| {
                let (output, port_id) = boot.endpoint.as_ref()?;
                let present = output.ports().iter().any(|port| port.id() == *port_id);
                boot.port_disappeared |= !present;
                Some(present)
            });
            if port_present == Some(false) {
                reconnect = true;
            }
            if reconnect {
                match open(session.transport.name) {
                    Ok(link) => session.transport.link = link,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(400));
                        continue;
                    }
                }
            }
            match session.info() {
                Ok(info) => {
                    identity.confirm(&info)?;
                    match ready(session, &info, port_present) {
                        Ok(true) => return Ok(()),
                        Ok(false) => reconnect = false,
                        Err(e) if unanswered(&e) => reconnect = true,
                        Err(e) => return upload(Err(e), expectation),
                    }
                }
                Err(e) if unanswered(&e) => reconnect = true,
                Err(e) => return upload(Err(e), "reading INFO after the package restart"),
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        Err(Error(format!(
            "{expectation} was not confirmed on MIDI port {:?}. Check the device and reselect its port if the name changed.",
            session.transport.name
        )))
    }
}

impl Transport for Midi<'_> {
    fn send(&mut self, msg: &[u8]) -> std::result::Result<(), String> {
        self.link.send(msg)
    }

    fn recv(&mut self, timeout: Duration) -> std::result::Result<Option<Vec<u8>>, String> {
        self.link.recv(timeout)
    }
}

impl PackageTransport for Midi<'_> {
    fn before_restart(session: &mut Session<Self>, initial: &Info, proof: bool) -> Result<()> {
        if proof {
            let name = session.transport.name;
            let endpoint = midir::MidiOutput::new("patchkit").ok().and_then(|output| {
                let ports = output.ports();
                let mut matching = ports.iter().filter(|port| {
                    output
                        .port_name(port)
                        .is_ok_and(|port_name| port_name == name)
                });
                let port = matching.next()?;
                matching.next().is_none().then(|| (output, port.id()))
            });
            let (timeout, retries) = (session.timeout, session.retries);
            quick_probes(session);
            let log = initial
                .has_cap(CAP_LOG)
                .then(|| log_boot_start(session))
                .flatten();
            session.timeout = timeout;
            session.retries = retries;
            session.transport.boot = Some(MidiBoot {
                endpoint,
                port_disappeared: false,
                log,
            });
        }
        Ok(())
    }

    fn wait_for_boot(session: &mut Session<Self>, identity: &Identity) -> Result<()> {
        let result = Self::wait(
            session,
            identity,
            |s, info, port_present| {
                let should_read_log = s.transport.boot.as_ref().is_some_and(|boot| {
                    boot.log.is_some() && !boot.ready(port_present, None) && info.has_cap(CAP_LOG)
                });
                let log = should_read_log.then(|| log_boot_start(s)).flatten();
                Ok(s.transport
                    .boot
                    .as_ref()
                    .is_some_and(|boot| boot.ready(port_present, log)))
            },
            "a new boot",
        );
        session.transport.boot = None;
        result.map_err(|e| {
            Error(format!(
                "{e} Restart the device and check module support to confirm activation."
            ))
        })
    }

    fn confirm_support(session: &mut Session<Self>, identity: &Identity) -> Result<()> {
        Self::wait(
            session,
            identity,
            |s, info, _| module_support(s, info),
            "module support",
        )
    }
}

fn quick_probes(session: &mut Session<impl Transport>) {
    session.timeout = Duration::from_secs(1);
    session.retries = 0;
}

fn unanswered(error: &UploadError) -> bool {
    matches!(error, UploadError::Timeout | UploadError::Transport(_))
}

struct Emulated<'a> {
    link: EmuRpcLink<'a>,
    restart_epoch: Option<u64>,
}

impl<'a> Emulated<'a> {
    fn new(emu: &'a mut Emu) -> Result<Self> {
        emu.usb_out()?; // start the USB capture before sending any requests
        Ok(Self {
            link: EmuRpcLink { emu, buf: vec![] },
            restart_epoch: None,
        })
    }

    fn boot_if_pending(&mut self) -> Result<()> {
        let Some(epoch) = self.restart_epoch.take() else {
            return Ok(());
        };
        let emu = &mut self.link.emu;
        let end = Instant::now() + BOOT_TIMEOUT;
        while Instant::now() < end {
            for event in std::mem::take(&mut emu.events) {
                match event["kind"].as_str() {
                    Some("firmware.loaded") if event["ok"].as_bool() == Some(false) => {
                        return Err(Error(
                            "the emulator failed to boot after the package restart".into(),
                        ));
                    }
                    Some("firmware.loaded")
                        if event["ok"].as_bool() == Some(true)
                            && event["epoch"].as_u64().is_some_and(|e| e > epoch) =>
                    {
                        // The restarted emulator has a new USB output capture.
                        emu.usb_out()?;
                        self.link.buf.clear();
                        return Ok(());
                    }
                    Some("cpu.reset") if event["restart"].as_str() == Some("unsupported") => {
                        return Err(Error(format!(
                            "the emulator refused the package restart: {event}"
                        )));
                    }
                    _ => {}
                }
            }
            if emu.step(20_000_000).is_err() {
                std::thread::sleep(Duration::from_millis(500)); // the cold boot runs on its worker
            }
        }
        Err(Error(
            "the emulator did not report a new boot after the package restart".into(),
        ))
    }
}

impl Transport for Emulated<'_> {
    fn send(&mut self, msg: &[u8]) -> std::result::Result<(), String> {
        self.link.send(msg)
    }

    fn recv(&mut self, timeout: Duration) -> std::result::Result<Option<Vec<u8>>, String> {
        self.link.recv(timeout)
    }
}

impl PackageTransport for Emulated<'_> {
    fn before_restart(session: &mut Session<Self>, _initial: &Info, _proof: bool) -> Result<()> {
        let transport = &mut session.transport;
        let emu = &mut transport.link.emu;
        let status = emu.call("status", json!({}))?;
        transport.restart_epoch = Some(
            status["epoch"]
                .as_u64()
                .ok_or_else(|| Error("the emulator status has no boot epoch".into()))?,
        );
        emu.events.clear();
        Ok(())
    }

    fn wait_for_boot(session: &mut Session<Self>, _identity: &Identity) -> Result<()> {
        session.transport.boot_if_pending()
    }

    fn confirm_support(session: &mut Session<Self>, identity: &Identity) -> Result<()> {
        // The loader event also keeps USB probes out of the emulator's cold boot job.
        session.transport.boot_if_pending()?;
        quick_probes(session);
        let end = session.transport.link.emu.used + BOOT_TIMEOUT.as_millis() as u64 * INSTR_PER_MS;
        while session.transport.link.emu.used < end {
            session.transport.link.emu.step(50 * INSTR_PER_MS)?;
            match session.info() {
                Ok(info) => {
                    identity.confirm(&info)?;
                    match module_support(session, &info) {
                        Ok(true) => return Ok(()),
                        Ok(false) => {}
                        Err(e) if unanswered(&e) => {}
                        Err(e) => {
                            return upload(Err(e), "confirming module support after the restart");
                        }
                    }
                }
                Err(e) if unanswered(&e) => {}
                Err(e) => return upload(Err(e), "reading INFO after the package restart"),
            }
        }
        Err(Error(
            "no module support after the emulator's package restart".into(),
        ))
    }
}

#[cfg(test)]
mod tests;
