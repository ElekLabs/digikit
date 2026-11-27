//! One request the UI task is about to run, with copies of what it needs from the registry so the work runs unlocked: the settings
//! key, the stored length and the bounded slice of the field's kind body that `Kind::accepts` reads.

use super::descriptor::Field;
use super::kind::{Kind, ACCEPT_BODY_MAX};
use super::Request;
use dkrt_sdk::settings::KEY_MAX;

pub(super) struct Job {
    pub(super) rec: usize,
    pub(super) request: Request,
    pub(super) key: [u8; KEY_MAX + 1],
    pub(super) max: u8,
    pub(super) kind: Kind,
    body: [u8; ACCEPT_BODY_MAX],
    body_len: u8,
    pub(super) revision: u32,
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

    pub(super) fn body(&self) -> &[u8] {
        &self.body[..self.body_len as usize]
    }

    /// Is `value` a stored value this job's field takes? The same rule the write path checks.
    pub(super) fn accepts(&self, value: &[u8]) -> bool {
        self.kind.accepts(self.body(), self.max, value)
    }
}
