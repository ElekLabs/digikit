//! The route of an effect: record flags, saved setting and the describe words.

use super::*;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp;
use dkrt_abi::provider::WordSink;
use dkrt_abi::shared::Shared;

/// The routes of a record's flags word: (allowed `ROUTE_*` bits, default `ROUTE_*`). The loader has checked the word.
pub(super) fn routes_of(flags: u32) -> (u8, u8) {
    let mut allow = 0;
    if flags & FXF_NOMASTER == 0 {
        allow |= ROUTE_MASTER;
    }
    if flags & FXF_SEND != 0 {
        allow |= ROUTE_SEND;
    }
    (
        allow,
        if flags & FXF_DEFAULT_SEND != 0 {
            ROUTE_SEND
        } else {
            ROUTE_MASTER
        },
    )
}

/// The route effect row `k` has in the settings, as `ROUTE_*`: the stored one when the effect allows it, else its default. UI task.
pub(super) unsafe fn saved_route<C: CoreApi>(core: &C, k: usize) -> u8 {
    let s = ss();
    let mut key = [0u8; KEY_MAX];
    let kn = key_of(&mut key, b"fx/route/", &s.id[k], s.idlen[k] as usize);
    let mut v = [0u8; 4];
    if core.settings_get(&key[..kn], &mut v) == 1 {
        let r = if v[0] == 1 {
            ROUTE_SEND
        } else if v[0] == 0 {
            ROUTE_MASTER
        } else {
            0
        };
        if r != 0 && s.allow[k] & r != 0 {
            return r;
        }
    }
    s.dflt[k]
}

// `describe` gives one word for each effect row: the core refuses a section with more than `DESCRIBE_WORDS` words, so every row must fit
const _: () = assert!(dsp::MAX_FX <= dkrt_abi::DESCRIBE_WORDS);

/// What `describe` reads for each effect row: the module record + 1 that owns its page (0: none) and its word (the running route, the saved route, the allowed routes and the
/// default). It is published as one table (`publish`), so the MODULES handler's task, which may preempt the task that registers, removes or re-routes an effect, copies a
/// whole state in one masked section and never reads the owner, the route and the settings one after another.
struct View {
    owner: [u8; dsp::MAX_FX],
    word: [u32; dsp::MAX_FX],
}

static VIEW: Shared<View> = Shared::new(View {
    owner: [0; dsp::MAX_FX],
    word: [0; dsp::MAX_FX],
});

/// Publish the describe words of every effect row: the state and the saved route are read outside a section (the settings are a call out), then copied in one (`VIEW`). Call it
/// when a change of the page owners, the routes or the saved routes is complete. UI task.
pub unsafe fn publish<C: CoreApi>(core: &C) {
    let s = ss();
    let mut now = View {
        owner: [0; dsp::MAX_FX],
        word: [0; dsp::MAX_FX],
    };
    let mut k = 0;
    while k < dsp::MAX_FX {
        let owner = fxpage::page_owner(k);
        if owner != 0 {
            let running = if s.route[k] != 0 {
                ROUTE_SEND
            } else {
                ROUTE_MASTER
            };
            now.owner[k] = owner;
            now.word[k] = k as u32
                | (running as u32) << 8
                | (saved_route(core, k) as u32) << 12
                | (s.allow[k] as u32) << 16
                | (s.dflt[k] as u32) << 20;
        }
        k += 1;
    }
    VIEW.with(core, |v| {
        v.owner = now.owner;
        v.word = now.word;
    });
}

/// Forget what `publish` made known (the reset of a host test). No task runs meanwhile.
pub fn reset_published() {
    // SAFETY: a host test calls it between runs
    unsafe {
        VIEW.set_unmasked(View {
            owner: [0; dsp::MAX_FX],
            word: [0; dsp::MAX_FX],
        })
    };
}

/// The words of the MODULES reply for module record `owner` (1 ..): one for each effect row of the module that has a page, in row order (the `Describe` of the service). Bits 0 to 7
/// the row, then four bits each: the route it runs on, the route the settings give it now (differs from the running one after a pick that waits for its reload), the routes its
/// record allows and its default (`ROUTE_*`). It runs on the MODULES command handler's task, outside the owner: it copies the published table (`publish`) in one section.
pub fn describe<C: CoreApi>(core: &C, owner: u8, out: &mut WordSink) {
    if owner == 0 {
        return;
    }
    let (owners, words) = VIEW.with(core, |v| (v.owner, v.word));
    let mut k = 0;
    while k < dsp::MAX_FX {
        if owners[k] == owner {
            out.push(words[k]);
        }
        k += 1;
    }
}
