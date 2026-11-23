//! The Workbench settings registered by loaded content modules. MIDI submits bounded work and reads completions.
//! Only the UI task accesses the real module settings. The shared queue is touched with interrupts masked.

mod descriptor;

use self::descriptor::{
    file_name_ok, Descriptor, DESCRIPTOR_MAX, EXTENSION_MAX, FIELDS_MAX, TEXT_MAX,
};
use super::loader::records;
use super::resident::{platform, settings, store, transport};
use super::{entry, files, module_cmds, settings_overlay};
use core::ptr::addr_of_mut;
use dkrt_abi::{mem::cstr_len, MAX_MODULES};
use dkrt_sdk::proto::{
    E_MODULE_UI_CONFLICT, E_MODULE_UI_SCOPE, E_MSG, E_SET_KEY, E_SET_NOKEY, E_SET_VALUE, E_STATE,
};
use dkrt_sdk::settings::KEY_MAX;
use dkrt_sdk::{E_BUSY, E_CONTEXT, E_FORMAT, E_NOT_FOUND, OK};

const DESCRIBE: u8 = 0;
const GET: u8 = 1;
const PUT: u8 = 2;
const RESET: u8 = 3;
const POLL: u8 = 4;
const RETAINED: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Request {
    ticket: u32,
    verb: u8,
    index: u8,
    revision: u32,
    len: u8,
    value: [u8; TEXT_MAX],
}

impl Request {
    const NONE: Self = Self {
        ticket: 0,
        verb: 0,
        index: 0,
        revision: 0,
        len: 0,
        value: [0; TEXT_MAX],
    };

    fn value(&self) -> &[u8] {
        &self.value[..self.len as usize]
    }
}

#[derive(Clone, Copy)]
struct Outcome {
    result: i32,
    revision: u32,
    present: bool,
    len: u8,
    value: [u8; TEXT_MAX],
}

impl Outcome {
    const EMPTY: Self = Self {
        result: OK,
        revision: 0,
        present: false,
        len: 0,
        value: [0; TEXT_MAX],
    };

    fn encode(&self, out: &mut [u8]) -> usize {
        out[0] = 0;
        out[1..5].copy_from_slice(&self.result.to_be_bytes());
        out[5..9].copy_from_slice(&self.revision.to_be_bytes());
        out[9] = self.present as u8;
        let len = self.len as usize;
        out[10..10 + len].copy_from_slice(&self.value[..len]);
        10 + len
    }
}

#[derive(Clone, Copy)]
struct Completion {
    request: Request,
    outcome: Outcome,
}

impl Completion {
    const EMPTY: Self = Self {
        request: Request::NONE,
        outcome: Outcome::EMPTY,
    };
}

struct Enrollment {
    descriptor: Descriptor,
    instance: u32,
    scope_revision: u32,
    field_revisions: [u32; FIELDS_MAX],
    pending: Request,
    completions: [Completion; RETAINED],
    next: usize,
}

impl Enrollment {
    const EMPTY: Self = Self {
        descriptor: Descriptor::EMPTY,
        instance: 0,
        scope_revision: 0,
        field_revisions: [0; FIELDS_MAX],
        pending: Request::NONE,
        completions: [Completion::EMPTY; RETAINED],
        next: 0,
    };

    fn rescope(&mut self, revision: u32) {
        self.scope_revision = revision;
        self.field_revisions.fill(0);
    }

    fn matches(&self, instance: u32) -> bool {
        self.instance != 0 && self.instance == instance
    }

    fn revision(&self, index: u8) -> u32 {
        let rev = self.field_revisions[index as usize];
        if rev == 0 {
            self.scope_revision
        } else {
            rev
        }
    }

    fn completed(&self, ticket: u32) -> Option<&Completion> {
        self.completions
            .iter()
            .find(|entry| entry.request.ticket == ticket)
    }

    fn submit(&mut self, request: Request, draining: bool) -> Result<usize, i32> {
        if let Some(previous) = self
            .completed(request.ticket)
            .map(|entry| entry.request)
            .or_else(|| (self.pending.ticket == request.ticket).then_some(self.pending))
        {
            return if previous == request {
                Ok(0)
            } else {
                Err(E_MSG)
            };
        }
        if draining || self.pending.ticket != 0 {
            return Err(E_BUSY);
        }
        let field = self.descriptor.field(request.index).ok_or(E_SET_KEY)?;
        let value = request.value();
        if request.verb == PUT {
            let extension = field.extension(self.descriptor.bytes());
            if value.len() > field.max as usize
                || core::str::from_utf8(value).is_err()
                || (!extension.is_empty() && !file_name_ok(value, extension))
            {
                return Err(E_SET_VALUE);
            }
        }
        self.pending = request;
        Ok(0)
    }

    fn complete(&mut self, outcome: Outcome) {
        self.completions[self.next] = Completion {
            request: self.pending,
            outcome,
        };
        self.next = (self.next + 1) % RETAINED;
        self.pending = Request::NONE;
    }

    fn poll(&self, ticket: u32, out: &mut [u8]) -> Result<usize, i32> {
        if let Some(entry) = self.completed(ticket) {
            Ok(entry.outcome.encode(out))
        } else if self.pending.ticket == ticket {
            out[0] = 1;
            Ok(1)
        } else {
            Err(E_NOT_FOUND)
        }
    }

    fn handle(
        &mut self,
        verb: u8,
        tail: &[u8],
        out: &mut [u8],
        draining: bool,
    ) -> Result<usize, i32> {
        if verb == DESCRIBE && tail.is_empty() {
            let bytes = self.descriptor.bytes();
            out[..bytes.len()].copy_from_slice(bytes);
            return Ok(bytes.len());
        }
        let mut rest = tail;
        let ticket = take_word(&mut rest)?;
        if ticket == 0 {
            return Err(E_MSG);
        }
        if verb == POLL && rest.is_empty() {
            return self.poll(ticket, out);
        }
        if !(GET..=RESET).contains(&verb) {
            return Err(E_MSG);
        }
        let (&index, mut rest) = rest.split_first().ok_or(E_MSG)?;
        let revision = take_word(&mut rest)?;
        if verb != PUT && !rest.is_empty() {
            return Err(E_MSG);
        }
        if rest.len() > TEXT_MAX {
            return Err(E_SET_VALUE);
        }
        let mut request = Request {
            ticket,
            verb,
            index,
            revision,
            len: rest.len() as u8,
            value: [0; TEXT_MAX],
        };
        request.value[..rest.len()].copy_from_slice(rest);
        self.submit(request, draining)
    }
}

fn take_word(input: &mut &[u8]) -> Result<u32, i32> {
    let (word, rest) = input.split_first_chunk::<4>().ok_or(E_MSG)?;
    *input = rest;
    Ok(u32::from_be_bytes(*word))
}

/// Allocate nonzero generation and revision tokens. The selection sequence varies across normal boots.
struct Registry {
    modules: [Enrollment; MAX_MODULES],
    serial: u32,
    next_rec: usize,
}

impl Registry {
    const EMPTY: Self = Self {
        modules: [const { Enrollment::EMPTY }; MAX_MODULES],
        serial: 0,
        next_rec: 0,
    };

    fn next(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }

    fn publish(&mut self, rec: usize) {
        if self.modules[rec].descriptor.count() == 0 {
            return;
        }
        let instance = self.next();
        let slot = &mut self.modules[rec];
        slot.instance = instance;
        slot.scope_revision = instance;
    }

    fn changed(&mut self, rec: usize, key: &[u8]) {
        if let Some(index) = self.modules[rec].descriptor.key_index(key) {
            self.modules[rec].field_revisions[index] = self.next();
        }
    }

    fn scope_changed(&mut self) {
        let rev = self.next();
        for module in &mut self.modules {
            module.rescope(rev);
        }
    }

    fn handle(&mut self, body: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let (&verb, rest) = body.split_first().ok_or(E_MSG)?;
        let (&rec, mut rest) = rest.split_first().ok_or(E_MSG)?;
        let instance = take_word(&mut rest)?;
        let slot = self
            .modules
            .get_mut(rec as usize)
            .filter(|slot| slot.matches(instance))
            .ok_or(E_NOT_FOUND)?;
        let draining = unsafe { module_cmds::is_draining(rec as usize) };
        slot.handle(verb, rest, out, draining)
    }
}

static mut REGISTRY: Registry = Registry::EMPTY;

unsafe fn locked<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
    let mask = platform::irq_mask();
    let result = f(&mut *addr_of_mut!(REGISTRY));
    platform::irq_restore(mask);
    result
}

/// Called before the boot's first content module loads.
pub(crate) unsafe fn set_boot_sequence(seq: u32) {
    locked(|registry| registry.serial = seq.wrapping_mul(0x9e37_79b9));
}

pub(crate) unsafe extern "C" fn dk_workbench_register(
    ctx: *mut u8,
    descriptor: *const u8,
    size: u32,
) -> i32 {
    let Some(rec) = entry::own_rec(ctx).filter(|&rec| rec < MAX_MODULES) else {
        return E_CONTEXT;
    };
    if !files::in_init(rec) {
        return E_CONTEXT;
    }
    if descriptor.is_null() || size as usize > DESCRIPTOR_MAX {
        return E_FORMAT;
    }
    let module = records::module(rec);
    let name =
        core::slice::from_raw_parts(module.name.as_ptr(), cstr_len(module.name.as_ptr(), 31));
    let Some(descriptor) =
        Descriptor::parse(name, core::slice::from_raw_parts(descriptor, size as usize))
    else {
        return E_FORMAT;
    };
    locked(|registry| {
        let slot = &mut registry.modules[rec];
        if slot.descriptor.count() != 0 {
            return E_STATE;
        }
        slot.descriptor = descriptor;
        OK
    })
}

pub(crate) unsafe fn publish(rec: usize) {
    locked(|registry| registry.publish(rec))
}

pub(crate) unsafe fn withdraw(rec: usize) {
    if rec < MAX_MODULES {
        locked(|registry| registry.modules[rec] = Enrollment::EMPTY);
    }
}

pub(crate) unsafe fn instance(rec: usize) -> u32 {
    locked(|registry| registry.modules[rec].instance)
}

pub(crate) unsafe fn setting_changed(rec: usize, key: &[u8]) {
    locked(|registry| registry.changed(rec, key))
}

pub(crate) unsafe fn scope_changed() {
    locked(Registry::scope_changed)
}

pub(crate) unsafe fn command(seq: u32, _buf: *mut u8, body: *const u8, len: usize) -> i32 {
    use dkrt_sdk::proto::CMD_MODULE_UI;
    let out = core::slice::from_raw_parts_mut(transport::rbody().add(1), DESCRIPTOR_MAX);
    match locked(|registry| registry.handle(core::slice::from_raw_parts(body, len), out)) {
        Ok(size) => transport::reply(CMD_MODULE_UI, seq, OK, size),
        Err(status) => transport::reply(CMD_MODULE_UI, seq, status, 0),
    }
}

unsafe fn read_setting(rec: usize, key: &[u8], cap: u8, revision: u32) -> Outcome {
    let mut outcome = Outcome {
        revision,
        ..Outcome::EMPTY
    };
    let n = settings::module_get(
        rec as u32 + 1,
        key.as_ptr(),
        outcome.value.as_mut_ptr(),
        cap as u32,
    );
    if n == E_SET_NOKEY {
        return outcome;
    }
    if n < 0 {
        outcome.result = n;
    } else if let Some(text) = outcome
        .value
        .get(..n as usize)
        .filter(|text| core::str::from_utf8(text).is_ok())
    {
        outcome.len = text.len() as u8;
        outcome.present = true;
    } else {
        outcome.result = E_SET_VALUE;
    }
    outcome
}

/// Process at most one request per UI pass. A pending ticket remains visible during settings I/O.
pub(crate) unsafe fn tick() {
    let job = locked(|registry| {
        let rec = (0..MAX_MODULES)
            .map(|i| (registry.next_rec + i) % MAX_MODULES)
            .find(|&rec| {
                registry.modules[rec].pending.ticket != 0 && !module_cmds::is_draining(rec)
            })?;
        registry.next_rec = (rec + 1) % MAX_MODULES;
        let slot = &registry.modules[rec];
        let request = slot.pending;
        let field = slot.descriptor.field(request.index)?;
        let bytes = slot.descriptor.bytes();
        let raw_key = field.key(bytes);
        let mut key = [0; KEY_MAX + 1];
        key[..raw_key.len()].copy_from_slice(raw_key);
        let raw_extension = field.extension(bytes);
        let mut extension = [0; EXTENSION_MAX];
        extension[..raw_extension.len()].copy_from_slice(raw_extension);
        Some((
            rec,
            request,
            key,
            field.max,
            slot.revision(request.index),
            extension,
            raw_extension.len(),
        ))
    });
    let Some((rec, request, key, cap, revision, extension, extension_len)) = job else {
        return;
    };
    let outcome = apply(
        rec,
        request,
        &key,
        cap,
        revision,
        &extension[..extension_len],
    );
    locked(|registry| registry.modules[rec].complete(outcome));
}

/// Check the file by its exact store path. The module id comes from the enrolled record.
unsafe fn check_file(rec: usize, leaf: &[u8]) -> Result<(), i32> {
    use dkrt_sdk::store::{path_ok, PATH_BYTES};
    let name = &records::module(rec).name;
    let id = &name[..cstr_len(name.as_ptr(), 31)];
    let mut path = [0; PATH_BYTES];
    let prefix = b"data/";
    let slash = prefix.len() + id.len();
    let end = slash + 1 + leaf.len();
    if end >= path.len() {
        return Err(E_SET_VALUE);
    }
    path[..prefix.len()].copy_from_slice(prefix);
    path[prefix.len()..slash].copy_from_slice(id);
    path[slash] = b'/';
    path[slash + 1..end].copy_from_slice(leaf);
    if path_ok(path.as_ptr(), path.len()) != Some(end) {
        return Err(E_SET_VALUE);
    }
    let file = store::find(path.as_ptr())?;
    if file.size == 0 {
        return Err(E_SET_VALUE);
    }
    Ok(())
}

unsafe fn apply(
    rec: usize,
    request: Request,
    key: &[u8],
    cap: u8,
    revision: u32,
    extension: &[u8],
) -> Outcome {
    let mut before = read_setting(rec, key, cap, revision);
    if request.verb == GET {
        return before;
    }
    if settings_overlay::loaded() {
        before.result = E_MODULE_UI_SCOPE;
        return before;
    }
    if request.revision != revision {
        before.result = E_MODULE_UI_CONFLICT;
        return before;
    }
    if before.result != OK && before.result != E_SET_VALUE {
        return before;
    }
    let client = rec as u32 + 1;
    let result = if request.verb == PUT {
        if !extension.is_empty() {
            if let Err(status) = check_file(rec, request.value()) {
                before.result = status;
                return before;
            }
        }
        if before.present && &before.value[..before.len as usize] == request.value() {
            return before;
        }
        entry::module_put(
            client,
            key.as_ptr(),
            request.value.as_ptr(),
            request.len as u32,
        )
    } else {
        if !before.present && before.result == OK {
            return before;
        }
        entry::module_remove(client, key.as_ptr())
    };
    if result != OK {
        before.result = result;
        return before;
    }
    let revision = locked(|registry| registry.modules[rec].revision(request.index));
    read_setting(rec, key, cap, revision)
}

#[cfg(test)]
mod tests;
