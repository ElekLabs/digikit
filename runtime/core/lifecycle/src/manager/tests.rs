//! Unit tests of the package manager over the recording fakes: the order of calls across seams, the unwind of every bring-up step, fit and `can_stop`
//! refusals, orphans and pins, busy restores and reload.

extern crate std;

use crate::fake::{manager, Call, FakeManager, Pkg, Probe};
use crate::{LifecycleError, PackageId};
use std::vec::Vec;

mod bring_up;
mod reload;
mod resources;
mod take_down;
mod unwind;

const V1: u32 = 0x010000;

fn id(n: u8) -> PackageId {
    PackageId(n)
}

fn ids(v: &[u8]) -> Vec<PackageId> {
    v.iter().map(|&n| id(n)).collect()
}

/// The service `ref` (package 1), pinned by nothing, with a pool `slots` of 4.
fn reference() -> Pkg {
    Pkg::service(1, "ref", V1).resource("slots", 4)
}

/// A module that needs `ref`.
fn user(n: u8) -> Pkg {
    Pkg::module(n).needs("ref", V1)
}

fn world(pkgs: &[Pkg]) -> (FakeManager, Probe) {
    manager(pkgs)
}

fn live(pm: &FakeManager, candidates: &[u8]) -> Vec<u8> {
    candidates
        .iter()
        .copied()
        .filter(|&n| pm.live().contains(id(n)))
        .collect()
}

fn taken(r: &crate::Report) -> Vec<PackageId> {
    r.taken_down().as_slice().to_vec()
}

fn brought(r: &crate::Report) -> Vec<PackageId> {
    r.brought_up().as_slice().to_vec()
}

fn position(calls: &[Call], c: Call) -> usize {
    calls
        .iter()
        .position(|&x| x == c)
        .unwrap_or_else(|| panic!("{c:?} not called in {calls:?}"))
}

/// `a` happened before `b`.
fn before(calls: &[Call], a: Call, b: Call) -> bool {
    position(calls, a) < position(calls, b)
}

fn err(r: Result<crate::Report, LifecycleError>) -> LifecycleError {
    r.expect_err("the operation should be refused")
}
