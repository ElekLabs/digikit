//! The emulator transport adapter. Bundles also accept a fake working card.
use super::{
    bundle::{CardCheckpoint, WorkingCard},
    document::problem,
};
use digi_control::Handle;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

impl WorkingCard for Handle {
    fn path(&self) -> Result<PathBuf, String> {
        let status = self.call("status", json!({})).map_err(problem)?;
        status["firmware"]["card"]
            .as_str()
            .map(PathBuf::from)
            .ok_or("boot an emulator with a card first".into())
    }

    fn read(&self, card: &Path) -> Result<CardCheckpoint, String> {
        let tmp = tempfile::NamedTempFile::new().map_err(problem)?;
        let result = self
            .call(
                "card.overlay",
                json!({
                    "path": tmp.path(), "card": card, "require_ready": true,
                }),
            )
            .map_err(problem)?;
        Ok(CardCheckpoint::new(
            fs::read(tmp.path()).map_err(problem)?,
            result["syx_sha256"]
                .as_str()
                .ok_or("no mounted firmware")?
                .into(),
            result["base_sha256"]
                .as_str()
                .ok_or("no mounted firmware base")?
                .into(),
            result["dsp_enabled"]
                .as_bool()
                .ok_or("no mounted DSP mode")?,
        ))
    }
}
