//! What the FX ROUTING list asks of an effect row: its module, saved route and the routes it may be given now, and the write of a new route.

use super::*;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp;

/// A route of an effect, as the list shows it. The settings byte is `byte` (0 master, 1 send).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Send,
    Master,
}

impl Route {
    /// The byte of the setting `fx/route/<id>`.
    pub fn byte(self) -> u8 {
        match self {
            Route::Send => 1,
            Route::Master => 0,
        }
    }

    fn of(bits: u8) -> Route {
        if bits == ROUTE_SEND {
            Route::Send
        } else {
            Route::Master
        }
    }
}

/// An effect row that has a page: its module record + 1, its module id, the route in the settings and the routes it may be given now.
pub struct Effect {
    pub owner: u8,
    pub id: [u8; 32],
    pub id_len: usize,
    pub saved: Route,
    pub send_allowed: bool,
    pub master_allowed: bool,
}

impl Effect {
    /// The module id.
    pub fn id(&self) -> &[u8] {
        &self.id[..self.id_len]
    }
}

/// Effect row `k`, or `None` when it has no page. The routes allowed are the effect's own (`allowed_choices`): SEND is absent when every send cell is taken by another effect
/// (the saved sends of the other rows count, the row's own does not). UI task.
pub unsafe fn effect<C: CoreApi>(core: &C, k: usize) -> Option<Effect> {
    if k >= dsp::MAX_FX || fxpage::page_owner(k) == 0 {
        return None;
    }
    let s = ss();
    let mut saved_sends = 0u32;
    for j in 0..dsp::MAX_FX {
        if fxpage::page_owner(j) != 0 && saved_route(core, j) == ROUTE_SEND {
            saved_sends |= 1 << j;
        }
    }
    let allowed = allowed_choices(s.allow[k], k as u32, saved_sends, slots(core)) as u8;
    Some(Effect {
        owner: fxpage::page_owner(k),
        id: s.id[k],
        id_len: s.idlen[k] as usize,
        saved: Route::of(saved_route(core, k)),
        send_allowed: allowed & ROUTE_SEND != 0,
        master_allowed: allowed & ROUTE_MASTER != 0,
    })
}

/// Save `route` as the setting of effect row `k` (`fx/route/<module id>`): the status of the put. UI task.
pub unsafe fn save_route<C: CoreApi>(core: &C, k: usize, route: Route) -> i32 {
    let s = ss();
    let mut key = [0u8; KEY_MAX];
    let kn = key_of(&mut key, b"fx/route/", &s.id[k], s.idlen[k] as usize);
    core.settings_put(&key[..kn], &[route.byte()])
}
