use super::*;
use dkrt_sdk::proto::{CAP_FILEHASH, CMD_BEGIN, CMD_END, CMD_INFO, CMD_LIST, CMD_VERIFY};
use dkupload::mock::MockDevice;
use dkupload::{decode, encode};
use std::collections::VecDeque;

const IMAGE_ID: [u8; 8] = [1; 8];

struct Target {
    store: MockDevice,
    module_status: Status,
    flags: u8,
    replies: VecDeque<Vec<u8>>,
    commands: Vec<u8>,
    broken_verify: bool,
    first_verify_error: Option<Status>,
    verify_error: Option<Status>,
    log_page: Vec<u8>,
    boot_seen: bool,
    proof_required: bool,
    confirm_called: bool,
}

impl Target {
    fn new(module_status: Status, flags: u8) -> Self {
        Self {
            store: MockDevice::new(1 << 20),
            module_status,
            flags,
            replies: VecDeque::new(),
            commands: vec![],
            broken_verify: false,
            first_verify_error: None,
            verify_error: None,
            log_page: vec![],
            boot_seen: false,
            proof_required: false,
            confirm_called: false,
        }
    }
}

impl Transport for &mut Target {
    fn send(&mut self, bytes: &[u8]) -> std::result::Result<(), String> {
        let request = decode(bytes).unwrap();
        self.commands.push(request.cmd);
        let reply = |status: Status, extra: &[u8]| {
            let mut body = vec![status.code() as u8];
            body.extend_from_slice(extra);
            encode(request.cmd, true, request.seq, &body)
        };
        let response = match request.cmd {
            CMD_INFO => {
                let runtime = patchkit::dkm::runtime_version();
                let mut body = vec![0, 1, 0, 0];
                body.extend_from_slice(b"dt2\0");
                body.extend_from_slice(&self.store.free_bytes().to_be_bytes());
                body.extend_from_slice(&512u32.to_be_bytes());
                body.push(self.flags);
                body.extend_from_slice(&[0; 8]);
                body.extend_from_slice(&runtime.to_be_bytes());
                body.extend_from_slice(&(CAP_FILEHASH | CAP_FOLDERS | CAP_MODULES).to_be_bytes());
                body.extend_from_slice(&IMAGE_ID);
                reply(Status::Ok, &body)
            }
            CMD_MODULES => reply(self.module_status, &[0; 18]),
            CMD_VERIFY => match self.first_verify_error.take().or(self.verify_error) {
                Some(status) => reply(status, &[]),
                None if self.broken_verify => reply(Status::Ok, &[0; 32]),
                None => self.store.handle(bytes).unwrap(),
            },
            SVC_LOG => reply(Status::Ok, &self.log_page),
            CMD_RETRY | CMD_RESTART => reply(Status::Ok, &[]),
            _ => self.store.handle(bytes).unwrap(),
        };
        self.replies.push_back(response);
        Ok(())
    }

    fn recv(&mut self, _timeout: Duration) -> std::result::Result<Option<Vec<u8>>, String> {
        Ok(self.replies.pop_front())
    }
}

impl PackageTransport for &mut Target {
    fn wait_for_boot(session: &mut Session<Self>, _identity: &Identity) -> Result<()> {
        session.transport.proof_required = true;
        if !session.transport.boot_seen {
            return Err(Error("no new boot".into()));
        }
        Ok(())
    }

    fn confirm_support(session: &mut Session<Self>, identity: &Identity) -> Result<()> {
        session.transport.confirm_called = true;
        let info = upload(session.info(), "INFO")?;
        identity.confirm(&info)?;
        session.transport.module_status = Status::Ok;
        assert!(upload(module_support(session, &info), "MODULES")?);
        Ok(())
    }
}

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn package() -> Artifact {
    syspkg::build_from_repo(&root()).unwrap()
}

fn prepared() -> Prepared {
    Prepared {
        image: vec![],
        messages: vec![],
        checks: vec![],
        device: "dt2".into(),
        os: "1.17".into(),
        image_id: Some(IMAGE_ID),
        sha_stock: String::new(),
        sha_image: String::new(),
        restore: false,
        ship_map: None,
    }
}

#[test]
fn a_failed_card_verification_cannot_restart() {
    let package = package();
    let mut target = Target::new(Status::Proto, 1);
    target.broken_verify = true;
    let error = install(Session::new(&mut target), &package, Some(&prepared())).unwrap_err();
    assert!(error.0.contains("verification failed"));
    assert!(target.commands.ends_with(&[CMD_END, CMD_VERIFY]));
    assert_eq!(
        target.store.files,
        [(STORE_PATH.into(), package.bytes().to_vec())]
    );
    assert!(!target.confirm_called);
}

#[test]
fn a_previously_active_package_requires_evidence_of_a_new_boot() {
    let package = package();
    let mut target = Target::new(Status::Ok, 0);
    target
        .store
        .files
        .push((STORE_PATH.into(), vec![0; package.bytes().len()]));
    let error = install(Session::new(&mut target), &package, Some(&prepared())).unwrap_err();
    assert_eq!(
        error.0,
        "system package installed but activation unconfirmed: no new boot"
    );
    assert!(
        target
            .commands
            .ends_with(&[CMD_END, CMD_VERIFY, CMD_RESTART])
    );
    assert!(target.proof_required && !target.confirm_called);
    assert_eq!(
        target.store.files,
        [(STORE_PATH.into(), package.bytes().to_vec())]
    );
}

#[test]
fn a_verified_package_is_reused_and_only_recovery_may_retry_it() {
    let package = package();
    let prepared = prepared();
    let mut target = Target::new(Status::Ok, 0);
    target
        .store
        .files
        .push((STORE_PATH.into(), package.bytes().to_vec()));

    let refusal = install(Session::new(&mut target), &package, None).unwrap_err();
    assert_eq!(refusal.0, InstallRefusal::AlreadyHealthy.to_string());
    assert_eq!(target.commands, [CMD_INFO, CMD_MODULES]);

    target.commands.clear();
    install(Session::new(&mut target), &package, Some(&prepared)).unwrap();
    assert!(target.commands.ends_with(&[CMD_LIST, CMD_VERIFY]));
    assert!(!target.commands.contains(&CMD_BEGIN));
    assert!(!target.commands.contains(&CMD_RESTART));

    target.commands.clear();
    target.module_status = Status::Proto;
    target.flags = 1; // The same E_PROTO indicates core-only mode only with the safe flag.
    let refusal = install(Session::new(&mut target), &package, Some(&prepared)).unwrap_err();
    assert_eq!(refusal.0, InstallRefusal::RecoveryRequired.to_string());
    assert!(target.commands.ends_with(&[CMD_LIST, CMD_VERIFY]));

    target.commands.clear();
    install(Session::new(&mut target), &package, None).unwrap();
    assert!(target.commands.ends_with(&[
        CMD_VERIFY,
        CMD_INFO,
        CMD_RETRY,
        CMD_RESTART,
        CMD_INFO,
        CMD_MODULES
    ]));
    assert!(!target.commands.contains(&CMD_BEGIN));
    assert!(!target.proof_required && target.confirm_called);
}

#[test]
fn refusing_a_read_only_or_changed_target_leaves_its_card_alone() {
    let package = package();
    let mut target = Target::new(Status::Proto, 1 | 4);
    let error = install(Session::new(&mut target), &package, None).unwrap_err();
    assert!(error.0.contains("read-only"));
    assert_eq!(target.commands, [CMD_INFO, CMD_MODULES]);

    target.commands.clear();
    let mut prepared = prepared();
    prepared.image_id = Some([2; 8]);
    let error = install(Session::new(&mut target), &package, Some(&prepared)).unwrap_err();
    assert!(error.0.contains("the firmware that was flashed"));
    assert_eq!(target.commands, [CMD_INFO]);

    target.commands.clear();
    target.flags = 0;
    let error = install(Session::new(&mut target), &package, None).unwrap_err();
    assert!(error.0.contains("E_PROTO"));
    assert_eq!(target.commands, [CMD_INFO, CMD_MODULES]);
}

#[test]
fn a_competing_upload_prevents_a_package_write_or_retry() {
    let package = package();
    let mut target = Target::new(Status::Proto, 1 | 2);
    let error = install(Session::new(&mut target), &package, None).unwrap_err();
    assert!(error.0.contains("upload in progress"));
    assert!(target.store.files.is_empty());
    assert!(!target.commands.contains(&CMD_BEGIN));

    target.commands.clear();
    target
        .store
        .files
        .push((STORE_PATH.into(), package.bytes().to_vec()));
    let error = install(Session::new(&mut target), &package, None).unwrap_err();
    assert!(error.0.contains("upload in progress"));
    assert!(target.commands.ends_with(&[CMD_VERIFY, CMD_INFO]));
    assert!(!target.commands.contains(&CMD_RETRY));
    assert!(!target.commands.contains(&CMD_RESTART));
}

#[test]
fn an_explicit_package_must_be_valid_and_match_the_checkout() {
    let canonical = package();
    let root = root();
    let path = Path::new("system.dksp");
    let bytes = canonical.bytes().to_vec();
    let matched = check_explicit_package(&root, path, bytes.clone()).unwrap();
    assert_eq!(matched.bytes(), bytes);

    let (major, minor) = canonical.metadata().table();
    let mut body = bytes[dkrt_sdk::syspkg::HEADER_BYTES..].to_vec();
    // Change a byte of the entry code without changing the placement metadata.
    body[dkrt_sdk::syspkg::body::HEADER_BYTES] ^= 1;
    let different = syspkg::pack(&body, major, minor).unwrap();
    assert!(Artifact::from_bytes(different.clone()).is_ok());
    let error = check_explicit_package(&root, path, different).unwrap_err();
    assert!(error.0.contains("system.dksp: does not match"));

    let mut corrupt = bytes;
    corrupt.pop();
    let error = check_explicit_package(&root, path, corrupt).unwrap_err();
    assert!(error.0.starts_with("system.dksp:"));
    assert!(!error.0.contains("does not match the system package linked"));
}

#[test]
fn an_unreadable_or_disappeared_package_is_replaceable_but_upload_verification_still_matters() {
    let package = package();
    for status in [Status::Read, Status::NoFile] {
        let mut target = Target::new(Status::Proto, 1);
        target
            .store
            .files
            .push((STORE_PATH.into(), vec![0; package.bytes().len()]));
        target.first_verify_error = Some(status);
        install(Session::new(&mut target), &package, None).unwrap();
        assert!(target.commands.contains(&CMD_BEGIN));
        assert!(target.confirm_called && !target.proof_required);
        assert_eq!(
            target.store.files,
            [(STORE_PATH.into(), package.bytes().to_vec())]
        );

        let mut target = Target::new(Status::Proto, 1);
        target.verify_error = Some(status);
        let error = install(Session::new(&mut target), &package, None).unwrap_err();
        assert!(error.0.contains("verifying the uploaded system package"));
        assert!(error.0.contains(status.name()));
        assert!(target.commands.ends_with(&[CMD_END, CMD_VERIFY]));
        assert!(!target.commands.contains(&CMD_RESTART));
    }
}

#[test]
fn midi_boot_proof_requires_the_endpoint_to_return_or_a_changed_boot_log() {
    let mut target = Target::new(Status::Ok, 0);
    let mut event = [0; 32];
    event[..4].copy_from_slice(&1u32.to_be_bytes());
    event[8..10].copy_from_slice(&BOOT_START.to_be_bytes());
    target.log_page = vec![0, 0, 0, 1, 0, 0, 0, 1, 1];
    target.log_page.extend_from_slice(&event);
    let mut session = Session::new(&mut target);
    let log = log_boot_start(&mut session);
    assert_eq!(log, Some(event));
    let mut boot = MidiBoot {
        endpoint: None,
        port_disappeared: false,
        log,
    };
    assert!(!boot.ready(None, None));
    assert!(!boot.ready(Some(true), log));

    session.transport.log_page[40] = 1;
    assert!(boot.ready(Some(true), log_boot_start(&mut session)));
    session.transport.log_page.pop();
    assert!(!boot.ready(Some(true), log_boot_start(&mut session)));

    boot.port_disappeared = true;
    assert!(!boot.ready(Some(false), None));
    assert!(!boot.ready(None, None));
    assert!(boot.ready(Some(true), None));
}
