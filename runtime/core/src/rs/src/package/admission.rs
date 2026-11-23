//! The rules the core applies to a package on top of the package graph (docs/plans/runtime-contract-and-lifecycle.md, RC2b-3a). The graph (`dkrt-lifecycle`) decides the
//! order, the unmet or incompatible needs and the cycles; `Manifest::admit` (`lifecycle/manifest.rs`) asks this file about each package in bring-up order, before anything of
//! it is placed, and a refusal is the package's verdict (`dkrt_abi::plan`, the codes and detail words the boot record and `LOADS` carry).
//!
//! - Exclusive: one owner for each kind (a service of the table, or a module's `KINDS`), slot and hook site; the keys of every kind (machines, effects, a service's own
//!   kinds) are unique across the packages admitted and inside each table (`key_refusal`, `E_PLAN_KEY`).
//! - Fit: the table's `USES` (machine, parameter and page rows, the record block, the DSP cost) against the capacity the packages admitted before left, with the same function
//!   the menu uses (`dkrt_fit::fits_now` and `take`; the ledger's pools replace it in RC3). The DSP load is `max(per-voice machine cost) * voices + sum(effect costs)` against the
//!   headroom of the device profile; the capacity's DSP entry keeps the running maximum and the sum. A package that takes the load over the headroom is refused with
//!   `Reason::Fit(CAPK_DSP_COST)`.
//! - A provider takes no module record (`take_fit` with `record` false), and no service of the runtime's is a provider's to take.
//!
//! The packages are judged in the order the manager brings them up (services first, then the modules by selection index), so what is admitted first wins a clash. A module
//! table that declares kinds is not ordered before the modules with keys for them any more: a service owns a kind, and patchkit packs the need that orders a module after it.
use crate::package::handlers::ServiceTable;
use crate::package::loader::discover::is_runtime_service;
use dkneeds::keys::{tag, Keys};
use dkrt_abi::plan::{Reason, Verdict};
use dkrt_fit as fit;
use dkrt_sdk::{DkrtCap, CAP_MAX, PROVIDER_MAX, SEL_MAX};

/// Whether item `(first word == w)` is in entry `t` (an index loop: the iterator forms are code of their own for each closure).
fn has_word(t: &Keys, tg: u16, w: u32) -> bool {
    let mut i = 0;
    while let Some(x) = t.words(tg, i) {
        if x.0 == w {
            return true;
        }
        i += 1;
    }
    false
}

fn provides_kind(t: &Keys, kind: u32) -> bool {
    has_word(t, tag::KINDS, kind)
}

/// The first word that entry `a` and entry `b` of the two tables share (slots, hook sites).
fn shared(a: &Keys, b: &Keys, tg: u16) -> Option<u32> {
    let mut i = 0;
    while let Some(x) = a.words(tg, i) {
        if has_word(b, tg, x.0) {
            return Some(x.0);
        }
        i += 1;
    }
    None
}

/// The needs of a module, as the fit function counts them.
fn needs_of_uses(t: &Keys) -> fit::Needs {
    let rb = t.record_bytes();
    fit::Needs {
        machine_rows: t.machine_rows(),
        param_rows: t.param_rows(),
        page_rows: t.page_rows(),
        dsp_cost_voice: t.dsp_cost_voice(),
        dsp_cost_fx: t.dsp_cost_fx(),
        arena: if rb != 0 { dkneeds::arena_block(rb).unwrap_or(u32::MAX) } else { 0 },
        ..fit::Needs::default()
    }
}

/// Fit `t`'s uses against `caps` and take them. A provider takes no module record: the module and selection entries are left as they were.
fn take_fit(t: &Keys, caps: &mut [DkrtCap], record: bool) -> Option<u8> {
    let mut saved = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; 2];
    let mut at = [usize::MAX; 2];
    if !record {
        let mut s = 0;
        let mut c = 0;
        while c < caps.len() {
            let k = caps[c].kind;
            if (k == dkrt_sdk::proto::CAPK_MODULES as u32 || k == dkrt_sdk::proto::CAPK_SELECTION as u32) && s < 2 {
                saved[s] = caps[c];
                at[s] = c;
                s += 1;
                // room for the record the fit function counts
                caps[c].limit = caps[c].limit.saturating_add(1);
            }
            c += 1;
        }
    }
    let n = needs_of_uses(t);
    let short = fit::fits_now(&n, caps).map(|s| s.kind);
    if short.is_none() {
        fit::take(&n, caps);
    }
    let mut s = 0;
    while s < 2 {
        if at[s] != usize::MAX {
            caps[at[s]] = saved[s];
        }
        s += 1;
    }
    short
}

/// The first kind, slot or hook site of `t` that another owner has: a kind of the runtime's table, or a kind, slot or hook site of an accepted module (`done`, not
/// index `skip_m`) or of an accepted provider (`pdone`, not index `skip_p`). One rule for modules and providers (a service module's KINDS, SLOTS and HOOKS).
#[allow(clippy::too_many_arguments)]
fn owner_clash(
    services: &ServiceTable,
    t: &Keys,
    mods: &[Option<Keys>],
    done: &[bool; SEL_MAX],
    skip_m: usize,
    provs: &[Option<Keys>],
    pdone: &[bool; PROVIDER_MAX],
    skip_p: usize,
) -> Option<u32> {
    let mut ki = 0;
    while let Some(d) = t.words(tag::KINDS, ki) {
        ki += 1;
        let kind = d.0;
        if services.owner(kind).is_some() {
            return Some(kind);
        }
        let mut i = 0;
        while i < mods.len() {
            if i != skip_m && done[i] {
                if let Some(ti) = &mods[i] {
                    if provides_kind(ti, kind) {
                        return Some(kind);
                    }
                }
            }
            i += 1;
        }
        let mut i = 0;
        while i < provs.len() {
            if i != skip_p && pdone[i] {
                if let Some(ti) = &provs[i] {
                    if provides_kind(ti, kind) {
                        return Some(kind);
                    }
                }
            }
            i += 1;
        }
    }
    let mut i = 0;
    while i < mods.len() {
        if i != skip_m && done[i] {
            if let Some(ti) = &mods[i] {
                if let Some(site) = shared(t, ti, tag::SLOTS).or_else(|| shared(t, ti, tag::HOOKS)) {
                    return Some(site);
                }
            }
        }
        i += 1;
    }
    let mut i = 0;
    while i < provs.len() {
        if i != skip_p && pdone[i] {
            if let Some(ti) = &provs[i] {
                if let Some(site) = shared(t, ti, tag::SLOTS).or_else(|| shared(t, ti, tag::HOOKS)) {
                    return Some(site);
                }
            }
        }
        i += 1;
    }
    None
}

/// The refusal for the keys of `t` against `others` (the accepted modules and providers, or the live set of a run-time load), or `None`: a key `t` provides twice
/// or a key another table provides too, for any kind. One rule for every kind, for modules and providers (rule 8 of docs/plans/extension-model.md); a `None` entry
/// is skipped.
pub fn key_refusal(t: &Keys, others: &[Option<&Keys>]) -> Option<Reason> {
    if let Some((kind, key)) = t.first_repeat() {
        return Some(Reason::DuplicateKey(kind, key));
    }
    let mut i = 0;
    while i < others.len() {
        if let Some(o) = others[i] {
            if let Some((kind, key)) = t.first_shared_key(o) {
                return Some(Reason::DuplicateKey(kind, key));
            }
        }
        i += 1;
    }
    None
}

/// The refusal for the keys of `t`, if any: a group of keys for a kind nobody owns (the runtime's table, `t` itself, an accepted module or provider), or a key
/// that an accepted module (`done`, not index `skip_m`) or provider (`pdone`, not index `skip_p`) has too, or one that `t` has twice (`key_refusal`).
#[allow(clippy::too_many_arguments)]
fn keys_clash(
    services: &ServiceTable,
    t: &Keys,
    mods: &[Option<Keys>],
    done: &[bool; SEL_MAX],
    skip_m: usize,
    provs: &[Option<Keys>],
    pdone: &[bool; PROVIDER_MAX],
    skip_p: usize,
) -> Option<Reason> {
    let mut gi = 0;
    while let Some((kind, _)) = t.group(gi) {
        gi += 1;
        let mut owned = services.owner(kind).is_some();
        let mut i = 0;
        while i < mods.len() {
            if i != skip_m && done[i] {
                if let Some(ti) = &mods[i] {
                    owned |= provides_kind(ti, kind);
                }
            }
            i += 1;
        }
        owned |= provides_kind(t, kind);
        let mut p = 0;
        while p < provs.len() {
            if p != skip_p && pdone[p] {
                if let Some(tp) = &provs[p] {
                    owned |= provides_kind(tp, kind);
                }
            }
            p += 1;
        }
        if !owned {
            return Some(Reason::UnownedKind(kind));
        }
    }
    let mut others: [Option<&Keys>; SEL_MAX + PROVIDER_MAX] = [None; SEL_MAX + PROVIDER_MAX];
    let mut i = 0;
    while i < mods.len() && i < SEL_MAX {
        if i != skip_m && done[i] {
            others[i] = mods[i].as_ref();
        }
        i += 1;
    }
    let mut p = 0;
    while p < provs.len() && p < PROVIDER_MAX {
        if p != skip_p && pdone[p] {
            others[SEL_MAX + p] = provs[p].as_ref();
        }
        p += 1;
    }
    key_refusal(t, &others)
}

/// The running state of an admission: the capacity the packages admitted so far left, and which ones they are. The modules and providers are given by the caller, which keeps
/// their tables (`mods[k]` is the table of selection index `k`, `provs[j]` of provider `j`; `None`: no table the shared parser accepts).
pub struct Admission {
    caps: [DkrtCap; CAP_MAX],
    ncaps: usize,
    done: [bool; SEL_MAX],
    pdone: [bool; PROVIDER_MAX],
}

impl Admission {
    /// An admission against `caps` (`dkrt_capacity`, as the caller adjusted it), with nothing admitted.
    pub fn new(caps: &[DkrtCap]) -> Admission {
        let mut a = Admission {
            caps: [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX],
            ncaps: caps.len().min(CAP_MAX),
            done: [false; SEL_MAX],
            pdone: [false; PROVIDER_MAX],
        };
        a.caps[..a.ncaps].copy_from_slice(&caps[..a.ncaps]);
        a
    }

    /// Provider `j` counts as admitted without being judged or taking capacity: it is loaded already (a live admission).
    pub fn provider_is_there(&mut self, j: usize) {
        if j < PROVIDER_MAX {
            self.pdone[j] = true;
        }
    }

    /// The DSP load of the admitted packages and the largest per-voice machine cost among them: the DSP entry of the capacity.
    pub fn dsp_load(&self) -> (u32, u32) {
        let mut c = 0;
        while c < self.ncaps {
            if self.caps[c].kind == dkrt_sdk::proto::CAPK_DSP_COST as u32 {
                return (self.caps[c].used, self.caps[c].largest);
            }
            c += 1;
        }
        (0, 0)
    }

    /// The verdict of provider `j` against the providers admitted before it. An admitted provider takes its share of the capacity.
    pub fn judge_provider(&mut self, services: &ServiceTable, provs: &[Option<Keys>], j: usize) -> Verdict {
        let v = self.provider_verdict(services, provs, j);
        self.pdone[j] = v.is_accepted();
        v
    }

    fn provider_verdict(&mut self, services: &ServiceTable, provs: &[Option<Keys>], j: usize) -> Verdict {
        let t = match &provs[j] {
            Some(t) => t,
            None => return Verdict::Accepted,
        };
        // no service of the runtime's is a provider's to take (a second provider of one name is the graph's: `Refusal::DuplicateService`)
        let mut k = 0;
        while let Some(x) = t.words(tag::PROVIDES, k) {
            k += 1;
            if is_runtime_service(x.0, services) {
                return Verdict::Refused(Reason::DuplicateOwner(x.0));
            }
        }
        if let Some(site) = owner_clash(services, t, &[], &[false; SEL_MAX], usize::MAX, provs, &self.pdone, j) {
            return Verdict::Refused(Reason::DuplicateOwner(site));
        }
        if let Some(r) = keys_clash(services, t, &[], &[false; SEL_MAX], usize::MAX, provs, &self.pdone, j) {
            return Verdict::Refused(r);
        }
        match take_fit(t, &mut self.caps[..self.ncaps], false) {
            Some(kind) => Verdict::Refused(Reason::Fit(kind)),
            None => Verdict::Accepted,
        }
    }

    /// The verdict of module `j` against the providers and the modules admitted before it. An admitted module takes its share of the capacity.
    pub fn judge_module(&mut self, services: &ServiceTable, mods: &[Option<Keys>], provs: &[Option<Keys>], j: usize) -> Verdict {
        let v = self.module_verdict(services, mods, provs, j);
        self.done[j] = v.is_accepted();
        v
    }

    fn module_verdict(&mut self, services: &ServiceTable, mods: &[Option<Keys>], provs: &[Option<Keys>], j: usize) -> Verdict {
        let t = match &mods[j] {
            Some(t) => t,
            None => return Verdict::Accepted,
        };
        // the kinds the module owns, the slots and hook sites it takes: one owner each
        if let Some(site) = owner_clash(services, t, mods, &self.done, j, provs, &self.pdone, usize::MAX) {
            return Verdict::Refused(Reason::DuplicateOwner(site));
        }
        // the keys: an owner for the kind, and no key twice for a generic kind
        if let Some(r) = keys_clash(services, t, mods, &self.done, j, provs, &self.pdone, usize::MAX) {
            return Verdict::Refused(r);
        }
        match take_fit(t, &mut self.caps[..self.ncaps], true) {
            Some(kind) => Verdict::Refused(Reason::Fit(kind)),
            None => Verdict::Accepted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dkneeds::keys::{name_hash, write_table, KeyGroup, KindDecl, Pair, Slot, Table, Use};
    use dkneeds::{KIND_EFFECT, KIND_MACHINE, RES_DSP_COST_FX, RES_DSP_COST_VOICE, RES_MACHINE_ROWS};
    use crate::package::core_api::{CoreApi, REAL_CORE};
    use dkrt_abi::service::{Service, ServiceRow};
    use dkrt_sdk::proto::*;
    use dkrt_sdk::{E_PLAN_FIT, E_PLAN_KIND, E_PLAN_OWNER};
    use std::vec;
    use std::vec::Vec;

    struct S;
    impl<C: CoreApi> Service<C> for S {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();
    }
    static ROWS: [ServiceRow; 1] = [ServiceRow::of::<S, crate::package::core_api::RealCore>("machines", &[(KIND_MACHINE, 0x6c), (KIND_EFFECT, 0x1a4)])];
    static ROWS_P: dkrt_abi::service::RowPtrs<1> = dkrt_abi::service::RowPtrs::of(&ROWS);

    fn bytes(t: &Table) -> Vec<u8> {
        let mut b = vec![0u8; dkneeds::keys::table_size(t)];
        write_table(t, &mut b).unwrap();
        b
    }

    fn caps() -> Vec<DkrtCap> {
        let c = |kind: u8, limit: u32| DkrtCap { kind: kind as u32, limit, used: 0, largest: limit };
        vec![c(CAPK_MODULES, 8), c(CAPK_SELECTION, 8), c(CAPK_MACHINE_ROWS, 4), c(CAPK_PARAM_ROWS, 16), c(CAPK_PAGE_ROWS, 8), c(CAPK_STAGE, 65536), c(CAPK_ARENA, 1 << 20)]
    }

    /// The capacity of a device with 4 voices and a DSP headroom of 1000 instructions per block.
    fn dsp_caps() -> Vec<DkrtCap> {
        let mut c = caps();
        c.push(DkrtCap { kind: CAPK_DSP_COST as u32, limit: 1000, used: 0, largest: 0 });
        c.push(DkrtCap { kind: CAPK_DSP_VOICES as u32, limit: 4, used: 0, largest: 0 });
        c
    }

    /// What the admission said, by selection index and provider index.
    struct Plan {
        verdicts: [Verdict; SEL_MAX],
        provider_verdicts: [Verdict; PROVIDER_MAX],
        dsp_load: (u32, u32),
    }

    /// The providers in index order, then the modules in index order: the order of a bring-up of a graph with no needs.
    fn admit(mods: &[Option<Vec<u8>>], provs: &[Option<Vec<u8>>], c: Vec<DkrtCap>) -> Plan {
        let services = ServiceTable::new(&ROWS_P.0, &REAL_CORE);
        fn parse(t: &[Option<Vec<u8>>]) -> Vec<Option<Keys<'_>>> {
            t.iter().map(|t| t.as_ref().map(|b| Keys::parse(b).unwrap())).collect()
        }
        let (mods, provs) = (parse(mods), parse(provs));
        let mut a = Admission::new(&c);
        let mut p = Plan { verdicts: [Verdict::Accepted; SEL_MAX], provider_verdicts: [Verdict::Accepted; PROVIDER_MAX], dsp_load: (0, 0) };
        for j in 0..provs.len() {
            p.provider_verdicts[j] = a.judge_provider(&services, &provs, j);
        }
        for j in 0..mods.len() {
            p.verdicts[j] = a.judge_module(&services, &mods, &provs, j);
        }
        p.dsp_load = a.dsp_load();
        p
    }

    fn run(tables: &[Option<Vec<u8>>]) -> Plan {
        admit(tables, &[], caps())
    }

    fn run_with(tables: &[Option<Vec<u8>>], c: Vec<DkrtCap>) -> Plan {
        admit(tables, &[], c)
    }

    fn run_p(mods: &[Option<Vec<u8>>], provs: &[Option<Vec<u8>>]) -> Plan {
        admit(mods, provs, caps())
    }

    const KEEP: Table<'static> =
        Table { needs: &[], uses: &[], keys: &[], kinds: &[], slots: &[], hooks: &[], exports: &[], resources: &[], provides: &[], bind: &[], fw: &[], names: &[], sites: &[], callback_order: None, service_imports: &[] };

    #[test]
    fn a_module_with_no_table_is_accepted_and_takes_nothing() {
        let p = run(&[None, Some(bytes(&KEEP)), None]);
        assert_eq!(p.verdicts[..3], [Verdict::Accepted; 3]);
        assert_eq!(p.dsp_load, (0, 0));
    }

    #[test]
    fn no_service_of_the_runtime_is_a_providers_to_take() {
        let rt = Table { provides: &[Pair { a: name_hash("runtime"), b: 0 }], ..KEEP };
        let p = run_p(&[], &[Some(bytes(&rt))]);
        assert_eq!(p.provider_verdicts[0], Verdict::Refused(Reason::DuplicateOwner(name_hash("runtime"))));
        assert_eq!(p.provider_verdicts[0].result(), E_PLAN_OWNER);
        // a service that is a service file's (it has handle types in the SDK, but the core does not provide it) is
        let fx = Table { provides: &[Pair { a: name_hash("fx"), b: 0 }], ..KEEP };
        assert_eq!(run_p(&[], &[Some(bytes(&fx))]).provider_verdicts[0], Verdict::Accepted);
    }

    #[test]
    fn a_provider_that_is_there_is_not_judged_again_and_takes_no_capacity() {
        // a provider whose kind the service table owns already (its own row, loaded): a boot's judgment refuses it as a second owner, a live admission keeps it
        let provider = bytes(&Table { provides: &[Pair { a: 6, b: 0 }], kinds: &[KindDecl { kind: KIND_MACHINE, len: 0x6c, entry: 0 }], uses: &[Use { resource: RES_MACHINE_ROWS, count: 3 }], ..KEEP });
        let two = bytes(&Table { uses: &[Use { resource: RES_MACHINE_ROWS, count: 2 }], ..KEEP });
        let services = ServiceTable::new(&ROWS_P.0, &REAL_CORE);
        let (p, m) = ([Some(Keys::parse(&provider).unwrap())], [Some(Keys::parse(&two).unwrap())]);
        let c = caps();
        let mut boot = Admission::new(&c);
        assert_eq!(boot.judge_provider(&services, &p, 0), Verdict::Refused(Reason::DuplicateOwner(KIND_MACHINE)));
        let mut live = Admission::new(&c);
        live.provider_is_there(0);
        assert_eq!(live.judge_module(&services, &m, &p, 0), Verdict::Accepted, "the provider's 3 of the 4 rows are in the capacity already, not taken again");
    }

    #[test]
    fn keys_for_a_kind_nobody_owns_are_refused_and_a_provider_makes_the_kind_owned() {
        let ks = [5u32];
        let unowned = Table { keys: &[KeyGroup { kind: 90, keys: &ks }], ..KEEP };
        let p = run(&[Some(bytes(&unowned))]);
        assert_eq!(p.verdicts[0], Verdict::Refused(Reason::UnownedKind(90)));
        assert_eq!(p.verdicts[0].result(), E_PLAN_KIND);
        // a service of the table owns the machine kind
        let ms = [0x100u32];
        let owned = Table { keys: &[KeyGroup { kind: KIND_MACHINE, keys: &ms }], ..KEEP };
        assert_eq!(run(&[Some(bytes(&owned))]).verdicts[0], Verdict::Accepted);
        // a module that declares the kind owns it for the modules after it: the order of the manager (services first, then the modules by index) decides, and a module that
        // declares a kind sits before the module with keys for it
        let provider = Table { kinds: &[KindDecl { kind: 90, len: 16, entry: 8 }], ..KEEP };
        let p = run(&[Some(bytes(&provider)), Some(bytes(&unowned))]);
        assert_eq!(p.verdicts[..2], [Verdict::Accepted; 2]);
        let p = run(&[Some(bytes(&unowned)), Some(bytes(&provider))]);
        assert_eq!(p.verdicts[..2], [Verdict::Refused(Reason::UnownedKind(90)), Verdict::Accepted]);
    }

    #[test]
    fn modules_that_own_a_kind_the_other_has_keys_for_are_both_refused() {
        let ks = [5u32];
        // A owns kind 90 and has keys for 91; B owns 91 and has keys for 90: neither is admitted before the other
        let a = Table { kinds: &[KindDecl { kind: 90, len: 16, entry: 8 }], keys: &[KeyGroup { kind: 91, keys: &ks }], ..KEEP };
        let b = Table { kinds: &[KindDecl { kind: 91, len: 16, entry: 8 }], keys: &[KeyGroup { kind: 90, keys: &ks }], ..KEEP };
        let p = run(&[Some(bytes(&a)), Some(bytes(&b))]);
        assert_eq!(p.verdicts[..2], [Verdict::Refused(Reason::UnownedKind(91)), Verdict::Refused(Reason::UnownedKind(90))]);
    }

    #[test]
    fn the_uses_of_a_provider_count_against_fit_without_a_module_record() {
        let three = Table { uses: &[Use { resource: RES_MACHINE_ROWS, count: 3 }], ..KEEP };
        let two = Table { uses: &[Use { resource: RES_MACHINE_ROWS, count: 2 }], ..KEEP };
        let three_p = Table { provides: &[Pair { a: 6, b: 0 }], ..three };
        // 4 machine rows: the provider takes 3, so the module of 2 does not fit
        let p = run_p(&[Some(bytes(&two))], &[Some(bytes(&three_p))]);
        assert_eq!(p.provider_verdicts[0], Verdict::Accepted);
        assert_eq!(p.verdicts[0], Verdict::Refused(Reason::Fit(CAPK_MACHINE_ROWS)));
        // a provider over the capacity is refused
        let five = Table { uses: &[Use { resource: RES_MACHINE_ROWS, count: 5 }], provides: &[Pair { a: 7, b: 0 }], ..KEEP };
        let p = run_p(&[], &[Some(bytes(&five))]);
        assert_eq!(p.provider_verdicts[0], Verdict::Refused(Reason::Fit(CAPK_MACHINE_ROWS)));
        // the providers take no module record: six providers and eight modules fit the eight records
        let provs: Vec<Option<Vec<u8>>> = (0..6u32).map(|k| Some(bytes(&Table { provides: &[Pair { a: 100 + k, b: 0 }], ..KEEP }))).collect();
        let mods: Vec<Option<Vec<u8>>> = (0..8).map(|_| Some(bytes(&KEEP))).collect();
        let p = run_p(&mods, &provs);
        assert_eq!(p.provider_verdicts[..6], [Verdict::Accepted; 6]);
        assert_eq!(p.verdicts[..8], [Verdict::Accepted; 8]);
    }

    #[test]
    fn the_keys_of_a_provider_are_judged_like_the_keys_of_a_module() {
        let g = dkrt_sdk::records::GENERIC_KINDS[0];
        let ks = [5u32];
        let svc = |n: &str| [Pair { a: name_hash(n), b: 0 }];
        let (pa, pb) = (svc("a"), svc("b"));
        // a provider with keys for a kind nobody owns
        let unowned = Table { provides: &pa, keys: &[KeyGroup { kind: 90, keys: &ks }], ..KEEP };
        let p = run_p(&[], &[Some(bytes(&unowned))]);
        assert_eq!(p.provider_verdicts[0], Verdict::Refused(Reason::UnownedKind(90)));
        assert_eq!(p.provider_verdicts[0].result(), E_PLAN_KIND);
        // a generic key twice: the later provider is refused
        let first = Table { provides: &pa, kinds: &[KindDecl { kind: g, len: 16, entry: 8 }], keys: &[KeyGroup { kind: g, keys: &ks }], ..KEEP };
        let second = Table { provides: &pb, keys: &[KeyGroup { kind: g, keys: &ks }], ..KEEP };
        let p = run_p(&[], &[Some(bytes(&first)), Some(bytes(&second))]);
        assert_eq!(p.provider_verdicts[..2], [Verdict::Accepted, Verdict::Refused(Reason::DuplicateKey(g, 5))]);
        // a module with a key of a provider is refused (the providers are judged first)
        let user = Table { keys: &[KeyGroup { kind: g, keys: &ks }], ..KEEP };
        let p = run_p(&[Some(bytes(&user))], &[Some(bytes(&first))]);
        assert_eq!(p.provider_verdicts[0], Verdict::Accepted);
        assert_eq!(p.verdicts[0], Verdict::Refused(Reason::DuplicateKey(g, 5)));
    }

    #[test]
    fn a_kind_a_slot_or_a_hook_site_has_one_owner() {
        let dup_kind = Table { kinds: &[KindDecl { kind: KIND_MACHINE, len: 8, entry: 0 }], ..KEEP };
        let p = run(&[Some(bytes(&dup_kind))]);
        assert_eq!(p.verdicts[0], Verdict::Refused(Reason::DuplicateOwner(KIND_MACHINE)));
        assert_eq!(p.verdicts[0].result(), E_PLAN_OWNER);
        let k = Table { kinds: &[KindDecl { kind: 90, len: 16, entry: 8 }], ..KEEP };
        let p = run(&[Some(bytes(&k)), Some(bytes(&k))]);
        assert_eq!(p.verdicts[..2], [Verdict::Accepted, Verdict::Refused(Reason::DuplicateOwner(90))]);
        let slot = Table { slots: &[Slot { id: 7, flags: 0, entry: 0 }], ..KEEP };
        let hook = Table { hooks: &[Pair { a: 9, b: 4 }], ..KEEP };
        let p = run(&[Some(bytes(&slot)), Some(bytes(&slot)), Some(bytes(&hook)), Some(bytes(&hook))]);
        assert_eq!(
            p.verdicts[..4],
            [Verdict::Accepted, Verdict::Refused(Reason::DuplicateOwner(7)), Verdict::Accepted, Verdict::Refused(Reason::DuplicateOwner(9))]
        );
    }

    #[test]
    fn a_providers_kinds_slots_and_hooks_have_one_owner_with_the_modules_and_the_other_providers() {
        let svc = |n: &str| Pair { a: name_hash(n), b: 0 };
        let (pa, pb) = ([svc("a")], [svc("b")]);
        let k = [KindDecl { kind: 90, len: 16, entry: 8 }];
        let slot = [Slot { id: 7, flags: 0, entry: 0 }];
        let hook = [Pair { a: 9, b: 4 }];
        let owner = |n| Verdict::Refused(Reason::DuplicateOwner(n));
        // the runtime's kind
        let dup = Table { provides: &pa, kinds: &[KindDecl { kind: KIND_MACHINE, len: 8, entry: 0 }], ..KEEP };
        assert_eq!(run_p(&[], &[Some(bytes(&dup))]).provider_verdicts[0], owner(KIND_MACHINE));
        // another provider's kind, slot and hook site
        for (what, first, second, site) in [
            ("kind", Table { provides: &pa, kinds: &k, ..KEEP }, Table { provides: &pb, kinds: &k, ..KEEP }, 90),
            ("slot", Table { provides: &pa, slots: &slot, ..KEEP }, Table { provides: &pb, slots: &slot, ..KEEP }, 7),
            ("hook", Table { provides: &pa, hooks: &hook, ..KEEP }, Table { provides: &pb, hooks: &hook, ..KEEP }, 9),
        ] {
            let p = run_p(&[], &[Some(bytes(&first)), Some(bytes(&second))]);
            assert_eq!(p.provider_verdicts[..2], [Verdict::Accepted, owner(site)], "{what}");
            assert_eq!(p.provider_verdicts[1].result(), E_PLAN_OWNER);
        }
        // a module that takes what an accepted provider owns is refused (the providers are judged first)
        let prov = Table { provides: &pa, kinds: &k, slots: &slot, ..KEEP };
        let by_kind = Table { kinds: &k, ..KEEP };
        let by_slot = Table { slots: &slot, ..KEEP };
        let p = run_p(&[Some(bytes(&by_kind)), Some(bytes(&by_slot))], &[Some(bytes(&prov))]);
        assert_eq!(p.provider_verdicts[0], Verdict::Accepted);
        assert_eq!(p.verdicts[..2], [owner(90), owner(7)]);
        // a module with keys for a kind a provider owns has an owner for it
        let keys = [0x100u32];
        let user = Table { keys: &[KeyGroup { kind: 90, keys: &keys }], ..KEEP };
        let p = run_p(&[Some(bytes(&user))], &[Some(bytes(&prov))]);
        assert_eq!(p.verdicts[0], Verdict::Accepted);
    }

    #[test]
    fn a_machine_key_or_an_effect_id_in_two_modules_refuses_the_later_one() {
        use dkneeds::KIND_EFFECT;
        let (ms, fs) = ([0x100u32, 0x200], [7u32]);
        let m = Table { keys: &[KeyGroup { kind: KIND_MACHINE, keys: &ms }], ..KEEP };
        let f = Table { keys: &[KeyGroup { kind: KIND_EFFECT, keys: &fs }], ..KEEP };
        // the same module twice, and another module with an effect id the first has
        let p = run(&[Some(bytes(&m)), Some(bytes(&m)), Some(bytes(&f)), Some(bytes(&f))]);
        assert_eq!(
            p.verdicts[..4],
            [
                Verdict::Accepted,
                Verdict::Refused(Reason::DuplicateKey(KIND_MACHINE, 0x100)),
                Verdict::Accepted,
                Verdict::Refused(Reason::DuplicateKey(KIND_EFFECT, 7))
            ]
        );
        assert_eq!(p.verdicts[1].result(), dkrt_sdk::E_PLAN_KEY);
        // one machine in common is enough, and the refused module keeps nothing for the next
        let share = [0x200u32, 0x300];
        let m2 = Table { keys: &[KeyGroup { kind: KIND_MACHINE, keys: &share }], ..KEEP };
        let p = run(&[Some(bytes(&m)), Some(bytes(&m2))]);
        assert_eq!(p.verdicts[1], Verdict::Refused(Reason::DuplicateKey(KIND_MACHINE, 0x200)));
        // the same number for two kinds is no clash, and neither is the same local id in two modules (the keys derive from `<module>:<id>`)
        let same_number = Table { keys: &[KeyGroup { kind: KIND_EFFECT, keys: &ms }], ..KEEP };
        let p = run(&[Some(bytes(&m)), Some(bytes(&same_number))]);
        assert_eq!(p.verdicts[..2], [Verdict::Accepted; 2]);
    }

    #[test]
    fn a_key_twice_inside_one_module_is_refused() {
        let twice = [0x100u32, 0x100];
        let m = Table { keys: &[KeyGroup { kind: KIND_MACHINE, keys: &twice }], ..KEEP };
        let p = run(&[Some(bytes(&m))]);
        assert_eq!(p.verdicts[0], Verdict::Refused(Reason::DuplicateKey(KIND_MACHINE, 0x100)));
        // two groups of one kind count as one table
        let one = [0x100u32];
        let split = Table { keys: &[KeyGroup { kind: KIND_MACHINE, keys: &one }, KeyGroup { kind: KIND_MACHINE, keys: &one }], ..KEEP };
        assert_eq!(run(&[Some(bytes(&split))]).verdicts[0], Verdict::Refused(Reason::DuplicateKey(KIND_MACHINE, 0x100)));
    }

    #[test]
    fn key_refusal_checks_a_candidate_against_a_live_set() {
        use dkneeds::KIND_EFFECT;
        let (a, b) = ([1u32, 2], [3u32]);
        let ta = bytes(&Table { keys: &[KeyGroup { kind: KIND_MACHINE, keys: &a }], ..KEEP });
        let tb = bytes(&Table { keys: &[KeyGroup { kind: KIND_EFFECT, keys: &b }], ..KEEP });
        let cand = bytes(&Table { keys: &[KeyGroup { kind: KIND_MACHINE, keys: &[2] }], ..KEEP });
        let (ka, kb, kc) = (Keys::parse(&ta).unwrap(), Keys::parse(&tb).unwrap(), Keys::parse(&cand).unwrap());
        assert_eq!(key_refusal(&kc, &[Some(&kb), None]), None);
        assert_eq!(key_refusal(&kc, &[Some(&kb), Some(&ka)]), Some(Reason::DuplicateKey(KIND_MACHINE, 2)));
    }

    #[test]
    fn an_effect_id_a_live_module_has_is_refused_like_any_key() {
        use dkneeds::KIND_EFFECT;
        let effect = |keys: &[u32]| bytes(&Table { keys: &[KeyGroup { kind: KIND_EFFECT, keys }], ..KEEP });
        let (live, same, other, twice) = (effect(&[0x1001]), effect(&[0x1001]), effect(&[0x1002]), effect(&[0x1003, 0x1003]));
        let (kl, ks, ko, kt) = (Keys::parse(&live).unwrap(), Keys::parse(&same).unwrap(), Keys::parse(&other).unwrap(), Keys::parse(&twice).unwrap());
        assert_eq!(key_refusal(&ks, &[Some(&kl)]), Some(Reason::DuplicateKey(KIND_EFFECT, 0x1001)));
        assert_eq!(Verdict::Refused(Reason::DuplicateKey(KIND_EFFECT, 0x1001)).result(), dkrt_sdk::E_PLAN_KEY);
        assert_eq!(key_refusal(&ko, &[Some(&kl)]), None);
        assert_eq!(key_refusal(&kt, &[]), Some(Reason::DuplicateKey(KIND_EFFECT, 0x1003)));
    }

    #[test]
    fn a_full_selection_of_modules_that_use_nothing_is_accepted() {
        // eight modules, eight selection paths and eight module records: the plan counts the paths as it takes the modules, so all fit
        let tables: Vec<Option<Vec<u8>>> = (0..8).map(|_| Some(bytes(&KEEP))).collect();
        let p = run(&tables);
        assert_eq!(p.verdicts[..8], [Verdict::Accepted; 8]);
    }

    #[test]
    fn the_dsp_load_is_the_largest_machine_for_every_voice_plus_the_effects() {
        let voice = |n| bytes(&Table { uses: &[Use { resource: RES_DSP_COST_VOICE, count: n }], ..KEEP });
        let fx = |n| bytes(&Table { uses: &[Use { resource: RES_DSP_COST_FX, count: n }], ..KEEP });
        // 4 voices, headroom 1000: machines of 100 and 250 share the voices (250 * 4 = 1000), a third of 60 adds nothing
        let (m1, m2, m3) = (voice(100), voice(250), voice(60));
        let p = run_with(&[Some(m1.clone()), Some(m2.clone()), Some(m3.clone())], dsp_caps());
        assert_eq!(p.verdicts[..3], [Verdict::Accepted; 3]);
        assert_eq!(p.dsp_load, (1000, 250));
        // a machine of 251 takes 1004: refused with the DSP load as the reason, and it takes nothing
        let big = voice(251);
        let p = run_with(&[Some(m1.clone()), Some(big.clone())], dsp_caps());
        assert_eq!(p.verdicts[..2], [Verdict::Accepted, Verdict::Refused(Reason::Fit(CAPK_DSP_COST))]);
        assert_eq!(p.verdicts[1].result(), E_PLAN_FIT);
        assert_eq!(p.dsp_load, (400, 100));
        // effects add up on top of the voices: 800 for the voices, then 100 and 100 fit and one more instruction does not
        let (e1, e2, e3) = (fx(100), fx(100), fx(1));
        let m = voice(200);
        let p = run_with(&[Some(m.clone()), Some(e1.clone()), Some(e2.clone()), Some(e3.clone())], dsp_caps());
        assert_eq!(
            p.verdicts[..4],
            [Verdict::Accepted, Verdict::Accepted, Verdict::Accepted, Verdict::Refused(Reason::Fit(CAPK_DSP_COST))]
        );
        // a selection of effects only
        let p = run_with(&[Some(fx(1000)), Some(fx(1))], dsp_caps());
        assert_eq!(p.verdicts[..2], [Verdict::Accepted, Verdict::Refused(Reason::Fit(CAPK_DSP_COST))]);
        // a capacity without the DSP entries (an older runtime) is not judged
        assert_eq!(run(&[Some(voice(u32::MAX))]).verdicts[0], Verdict::Accepted);
    }

    #[test]
    fn the_uses_are_summed_against_the_capacity_the_earlier_modules_left() {
        let three = Table { uses: &[Use { resource: RES_MACHINE_ROWS, count: 3 }], ..KEEP };
        let two = Table { uses: &[Use { resource: RES_MACHINE_ROWS, count: 2 }], ..KEEP };
        let one = Table { uses: &[Use { resource: RES_MACHINE_ROWS, count: 1 }], ..KEEP };
        // 4 machine rows: 3 fit, 2 more do not, 1 more does; a refused module takes nothing
        let p = run(&[Some(bytes(&three)), Some(bytes(&two)), Some(bytes(&one))]);
        assert_eq!(
            p.verdicts[..3],
            [Verdict::Accepted, Verdict::Refused(Reason::Fit(CAPK_MACHINE_ROWS)), Verdict::Accepted]
        );
        assert_eq!(p.verdicts[1].result(), E_PLAN_FIT);
    }

    #[test]
    fn a_generic_key_in_two_modules_refuses_the_later_one() {
        let g = dkrt_sdk::records::GENERIC_KINDS[0];
        let (k5, k6) = ([5u32], [6u32]);
        let first = Table { kinds: &[KindDecl { kind: g, len: 16, entry: 8 }], keys: &[KeyGroup { kind: g, keys: &k5 }], ..KEEP };
        let same = Table { keys: &[KeyGroup { kind: g, keys: &k5 }], ..KEEP };
        let other = Table { keys: &[KeyGroup { kind: g, keys: &k6 }], ..KEEP };
        let p = run(&[Some(bytes(&first)), Some(bytes(&same)), Some(bytes(&other))]);
        assert_eq!(p.verdicts[..3], [Verdict::Accepted, Verdict::Refused(Reason::DuplicateKey(g, 5)), Verdict::Accepted]);
        match p.verdicts[1] {
            Verdict::Refused(why) => assert_eq!(why.detail(), [g, 5]),
            Verdict::Accepted => unreachable!(),
        }
    }
}
