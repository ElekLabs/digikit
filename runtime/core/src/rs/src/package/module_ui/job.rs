//! One request the UI task is about to run, with copies of what it needs from the registry so the work runs unlocked: the settings
//! key, the stored length and the bounded slice of the field's kind body that `Kind::accepts` reads.

use super::descriptor::Field;
use super::kind::{Kind, ACCEPT_BODY_MAX};
use super::Request;
use dkrt_sdk::settings::KEY_MAX;

pub(super) struct Job {
    rec: usize,
    request: Request,
    key: [u8; KEY_MAX + 1],
    max: u8,
    kind: Kind,
    body: [u8; ACCEPT_BODY_MAX],
    body_len: u8,
    revision: u32,
}

impl Job {
    pub(super) fn new(
        rec: usize,
        request: Request,
        field: Field,
        descriptor: &[u8],
        revision: u32,
    ) -> Self {
        let raw_key = field.key(descriptor);
        let mut key = [0; KEY_MAX + 1];
        key[..raw_key.len()].copy_from_slice(raw_key);
        let accepted = field.accepted_body(descriptor);
        let mut body = [0; ACCEPT_BODY_MAX];
        body[..accepted.len()].copy_from_slice(accepted);
        Self {
            rec,
            request,
            key,
            max: field.max,
            kind: field.kind(),
            body,
            body_len: accepted.len() as u8,
            revision,
        }
    }

    pub(super) fn rec(&self) -> usize {
        self.rec
    }

    pub(super) fn request(&self) -> Request {
        self.request
    }

    pub(super) fn key(&self) -> *const u8 {
        self.key.as_ptr()
    }

    pub(super) fn max(&self) -> u8 {
        self.max
    }

    pub(super) fn kind(&self) -> Kind {
        self.kind
    }

    /// The revision of the field when the job was made or last refreshed.
    pub(super) fn revision(&self) -> u32 {
        self.revision
    }

    /// Re-read the field's revision from the registry, after this job changed the setting.
    pub(super) unsafe fn refresh_revision(&mut self) {
        let (rec, index) = (self.rec, self.request.index);
        self.revision = super::locked(|registry| registry.modules[rec].revision(index));
    }

    fn body(&self) -> &[u8] {
        &self.body[..self.body_len as usize]
    }

    /// Is `value` a stored value this job's field takes? The same rule the write path checks.
    pub(super) fn accepts(&self, value: &[u8]) -> bool {
        self.kind.accepts(self.body(), self.max, value)
    }
}
