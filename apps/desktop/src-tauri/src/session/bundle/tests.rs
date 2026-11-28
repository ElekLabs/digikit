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
fn dsp_report_survives_a_move_and_failed_copies_keep_the_last_save() {
    use crate::session::{document::MANIFEST, dsp};
    use serde_json::json;

    let origin = fixture();
    let card = Card {
        image: "1".repeat(64),
        bytes: vec![1],
    };
    let session = save(&card, origin.path(), Some(Path::new("live")), "Example").unwrap();
    let checkpoint = session.document.checkpoint;
    let selected = dsp::save(origin.path(), "some/fx".into(), None).unwrap();
    assert_eq!(selected.document.dsp.unwrap().module, "some/fx");
    assert_eq!(selected.document.checkpoint, checkpoint);
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("quality-media")).unwrap();
    let media = [
        ("comparison", "json", &b"{}"[..]),
        ("audio", "wav", &b"RIFF"[..]),
        ("plot", "png", &b"PNG"[..]),
    ]
    .map(|(name, ext, bytes)| {
        let relative = format!("quality-media/{name}-{}.{ext}", sha256_hex(bytes));
        fs::write(source.path().join(&relative), bytes).unwrap();
        (relative, bytes)
    });
    let unreferenced = format!("quality-media/audio-{}.wav", "0".repeat(64));
    fs::write(source.path().join(&unreferenced), b"unreferenced").unwrap();
    let report = json!({
        "schema": "dk.module-quality/1",
        "module": "some/fx",
        "scope": "declarative SHARC rig checks",
        "sampleRate": 48000,
        "outcome": "fail",
        "diagnostics": [],
        "files": [],
        "comparisons": [{
            "file": media[0].0,
            "baseline": { "file": "outside.wav", "wav": media[1].0 },
            "candidate": { "file": "../../outside.wav", "wav": "../../outside.wav" },
            "plots": [media[2].0]
        }]
    });
    let report_bytes = serde_json::to_vec_pretty(&report).unwrap();
    let report_file = source.path().join("quality.json");
    fs::write(&report_file, &report_bytes).unwrap();
    let report_source = || {
        Some(dsp::ReportSource {
            folder: source.path().to_owned(),
            sha256: sha256_hex(&report_bytes),
        })
    };
    let with_report = dsp::save(origin.path(), "some/fx".into(), report_source()).unwrap();
    let report_folder = with_report.document.dsp.unwrap().report_folder.unwrap();
    assert_eq!(report_folder, format!("dsp/{}", sha256_hex(&report_bytes)));
    let same_module = dsp::save(origin.path(), "some/fx".into(), None).unwrap();
    assert_eq!(
        same_module.document.dsp.unwrap().report_folder.as_deref(),
        Some(report_folder.as_str())
    );

    let manifest = fs::read(origin.path().join(MANIFEST)).unwrap();
    assert!(dsp::save(origin.path(), "another/fx".into(), report_source()).is_err());
    let mut replacement = report;
    replacement["outcome"] = json!("pass");
    fs::write(&report_file, serde_json::to_vec(&replacement).unwrap()).unwrap();
    let replaced = dsp::save(origin.path(), "some/fx".into(), report_source())
        .err()
        .expect("stale report must be rejected");
    assert!(replaced.contains("changed since it was opened"));
    assert_eq!(fs::read(origin.path().join(MANIFEST)).unwrap(), manifest);
    fs::write(report_file, &report_bytes).unwrap();
    fs::write(source.path().join(&media[1].0), b"changed").unwrap();
    assert!(dsp::save(origin.path(), "some/fx".into(), report_source()).is_err());
    #[cfg(unix)]
    {
        let path = source.path().join(&media[1].0);
        let external = tempfile::NamedTempFile::new().unwrap();
        fs::write(external.path(), media[1].1).unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(external.path(), path).unwrap();
        assert!(dsp::save(origin.path(), "some/fx".into(), report_source()).is_err());
    }
    assert_eq!(fs::read(origin.path().join(MANIFEST)).unwrap(), manifest);
    drop(source);
    let dest = tempfile::tempdir().unwrap();
    let moved = dest.path().join("moved");
    fs::rename(origin.path(), &moved).unwrap();
    let reopened = read(&moved).unwrap();
    assert_eq!(reopened.document.checkpoint, checkpoint);
    assert_eq!(
        reopened.document.dsp.unwrap().report_folder.as_deref(),
        Some(report_folder.as_str())
    );
    let bundle = moved.join(report_folder);
    assert_eq!(fs::read(bundle.join("quality.json")).unwrap(), report_bytes);
    for (relative, bytes) in media {
        assert_eq!(fs::read(bundle.join(relative)).unwrap(), bytes);
    }
    assert!(!bundle.join(unreferenced).exists());
}

#[test]
fn fork_keeps_base_and_checkpoint_but_does_not_copy_old_results() {
    use crate::session::dsp;

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
    let report =
        br#"{"schema":"dk.module-quality/1","module":"some/fx","outcome":"pass","comparisons":[]}"#;
    fs::write(output.path().join("quality.json"), report).unwrap();
    let source = dsp::ReportSource {
        folder: output.path().to_owned(),
        sha256: sha256_hex(report),
    };
    let original = dsp::save(&original.root, "some/fx".into(), Some(source)).unwrap();
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
    assert!(original.document.dsp.unwrap().report_folder.is_some());
    let dsp = copy.document.dsp.unwrap();
    assert_eq!(dsp.module, "some/fx");
    assert!(dsp.report_folder.is_none());
    assert!(!copy.root.join("dsp").exists());
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
