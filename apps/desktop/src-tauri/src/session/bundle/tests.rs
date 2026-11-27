use super::*;

struct Card {
    image: String,
    bytes: Vec<u8>,
}

impl WorkingCard for Card {
    fn path(&self) -> Result<PathBuf, String> {
        Err("no default card".into())
    }

    fn read(&self, _: &Path) -> Result<CardCheckpoint, String> {
        Ok(CardCheckpoint::new(
            self.bytes.clone(),
            self.image.clone(),
            "0".repeat(64),
            true,
        ))
    }
}

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("card")).unwrap();
    fs::write(dir.path().join(BASE), vec![7; 512]).unwrap();
    let fw = Firmware {
        path: "/missing/firmware.syx".into(),
        sha256: "0".repeat(64),
        device: "fixture".into(),
        runtime: true,
        dsp: false,
        image_sha256: None,
    };
    Document::new("fixture".into(), "Untitled Session".into(), fw)
        .publish(dir.path())
        .unwrap();
    dir
}

#[test]
fn checkpoint_mount_is_disposable_and_failed_save_keeps_previous_manifest() {
    let folder = fixture();
    let card = Card {
        image: "1".repeat(64),
        bytes: vec![1, 2, 3],
    };
    let session = save(&card, folder.path(), Some(Path::new("live")), "Example").unwrap();
    let first = session.document.checkpoint.clone().unwrap();
    let (mount_dir, working_image) = mount(&session).unwrap();
    assert_eq!(fs::read(&working_image).unwrap(), vec![7; 512]);
    assert_eq!(
        fs::read(mount_dir.path().join("overlay.bin")).unwrap(),
        card.bytes
    );
    fs::write(mount_dir.path().join("overlay.bin"), [42]).unwrap();
    assert_eq!(
        fs::read(session.document.overlay(&session.root).unwrap().unwrap()).unwrap(),
        card.bytes,
    );
    let changed = Card {
        image: "2".repeat(64),
        bytes: vec![42],
    };
    assert!(save(&changed, folder.path(), Some(&working_image), "Wrong").is_err());
    let again = read(folder.path()).unwrap();
    assert_eq!(again.document.name, "Example");
    assert_eq!(again.document.checkpoint.as_deref(), Some(first.as_str()));
}

#[test]
fn fork_keeps_base_and_checkpoint_but_does_not_copy_old_results() {
    let original_dir = fixture();
    let card = Card {
        image: "1".repeat(64),
        bytes: vec![1],
    };
    let original = save(
        &card,
        original_dir.path(),
        Some(Path::new("live")),
        "Recorded",
    )
    .unwrap();
    let output = tempfile::tempdir().unwrap();
    let source = output.path().join("old.profile.json");
    fs::write(
        &source,
        br#"{"schema":"dk.profile/1","run":{"id":"first"}}"#,
    )
    .unwrap();
    let original = attach(original.root, source).unwrap();
    let copy = create(
        &card,
        Create {
            destination: output.path().join("Copy"),
            name: "Recorded Copy".into(),
            firmware: PathBuf::new(),
            source: "live".into(),
            card: None,
            fork: Some(original.root.clone()),
            runtime: false,
            dsp: false,
        },
    )
    .unwrap();
    assert_ne!(original.document.id, copy.document.id);
    assert_eq!(original.document.checkpoint, copy.document.checkpoint);
    assert_eq!(original.document.attachments.len(), 1);
    assert!(copy.document.attachments.is_empty());
    let next = Card {
        image: card.image,
        bytes: vec![2],
    };
    let copy = save(&next, &copy.root, Some(Path::new("live")), "Copy").unwrap();
    assert_ne!(
        read(&original.root).unwrap().document.checkpoint,
        copy.document.checkpoint
    );
}

#[test]
fn offline_session_accepts_a_legacy_capture_and_rejects_an_escaping_asset() {
    let folder = fixture();
    let foreign = tempfile::tempdir().unwrap();
    let source = foreign.path().join("legacy.profile.json");
    let data = br#"{"schema":"dk.calcaptures/1","cases":[]}"#;
    fs::write(&source, data).unwrap();
    let session = attach(folder.path().into(), source).unwrap();
    let capture = &session.document.attachments[0];
    assert_eq!(capture.kind, "capture");
    assert_eq!(
        fs::read(asset(&session.root, &capture.path).unwrap()).unwrap(),
        data
    );
    assert!(asset(&session.root, "../session.json").is_err());
}
