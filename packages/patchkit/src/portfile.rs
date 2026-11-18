//! The port file (docs/plans/port-file.md): one `.dks` of role 2 ("port") per device and firmware version, built from committed data only: the anchors,
//! hooks and site facts of `devices/ports/<device>-<version>.toml` and the declarations of `devices/fw/`. It holds no firmware bytes: addresses, lengths and
//! a truncated sha256 of each site.
//!
//! The file has the container of a `.dkm` (header, sha256, table) with no code. It provides the service `port`, carries the identity of the firmware
//! (`FW`, the first 8 bytes of the stock `.syx` sha256) and two table entries that PR2 moves into `dkneeds` with the runtime:
//!
//! ```text
//! FWNAMES (tag 12), items of 12 bytes, sorted by (kind, hash, n):
//!     u32 hash   name_hash of the firmware name (kind 0) or of the hook (kind 1)
//!     u32 addr   the address on this firmware; 0 when absent
//!     u16 n      kind 1: the exit number (0: the end of the site, from 1: `exits[n-1]` of the hook); kind 0: 0
//!     u8  kind   0 `dk_fw_<hash>` name, 1 `dk_exit_<hash>_<n>` exit
//!     u8  flags  bit 0: absent (another device has the name, this firmware does not: a reference resolves to 0)
//! SITES (tag 13), items of 20 bytes, sorted by hash:
//!     u32 hash   name_hash of the hook name
//!     u32 addr   the site address; 0 when absent
//!     u16 len    bytes of the displaced whole instructions
//!     u8  kind   0 raw, 1 tail, 2 stock (the last two: the displaced bytes are copied from flash after the check)
//!     u8  flags  bit 0: absent; bits 4..7: number of fixups that follow (reserved, 0 today)
//!     [u8; 8]    the first 8 bytes of the sha256 of the displaced bytes
//! ```

use crate::bind::{BoundHook, FwRef, Place, bind_with_refs, eval_exit_with, parse_import};
use crate::dkm::{Dkm, EmitParts, ROLE_PORT, emit, max_file};
use crate::error::*;
use crate::firmware::Firmware;
use crate::fwgen::{HookDecl, service_decls};
use crate::port::{Hook, Port, Registry};
use crate::prim;
use dkneeds::keys::{Pair, Table, name_hash, table_size, write_table};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// The service a port file provides.
pub const PORT_SERVICE: &str = "port";
/// The version of the service `port` (major << 16 | minor << 8 | patch).
pub const PORT_VERSION: u32 = 0x000100;
/// The table entry tags of the port file.
pub const TAG_FWNAMES: u16 = 12;
pub const TAG_SITES: u16 = 13;
const NAME_ITEM: usize = 12;
const SITE_ITEM: usize = 20;
const FLAG_ABSENT: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    Raw = 0,
    Tail = 1,
    Stock = 2,
}

impl SiteKind {
    pub fn parse(s: &str) -> Result<SiteKind> {
        match s {
            "raw" => Ok(SiteKind::Raw),
            "tail" => Ok(SiteKind::Tail),
            "stock" => Ok(SiteKind::Stock),
            k => Err(err!("site kind {k:?} is not raw, tail or stock")),
        }
    }
    /// Does the runtime copy the displaced bytes (a `tail` or a `stock` site)?
    pub fn copies(self) -> bool {
        self != SiteKind::Raw
    }
}

/// One FWNAMES item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameItem {
    pub hash: u32,
    pub addr: u32,
    pub n: u16,
    /// 0 name, 1 exit.
    pub kind: u8,
    pub absent: bool,
}

/// One SITES item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteItem {
    pub hash: u32,
    pub addr: u32,
    pub len: u16,
    pub kind: SiteKind,
    pub absent: bool,
    pub check: [u8; 8],
}

/// What a port file holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortData {
    /// The first 8 bytes of the stock `.syx` sha256.
    pub fw_id: [u8; 8],
    pub names: Vec<NameItem>,
    pub sites: Vec<SiteItem>,
}

impl PortData {
    pub fn name(&self, hash: u32) -> Option<&NameItem> {
        self.names.iter().find(|x| x.kind == 0 && x.hash == hash)
    }
    pub fn exit(&self, hash: u32, n: u32) -> Option<&NameItem> {
        self.names
            .iter()
            .find(|x| x.kind == 1 && x.hash == hash && u32::from(x.n) == n)
    }
    pub fn site(&self, hash: u32) -> Option<&SiteItem> {
        self.sites.iter().find(|x| x.hash == hash)
    }
}

/// The first 8 bytes of the sha256 of `b`: the check value of a site.
pub fn check_of(b: &[u8]) -> [u8; 8] {
    Sha256::digest(b)[..8].try_into().unwrap()
}

/// The site facts of one hook, as `port_resolve` writes them into the port TOML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub hook: String,
    pub len: u32,
    pub kind: SiteKind,
    pub check: [u8; 8],
}

/// The declaration of `hook` that applies on `device`: the entry without a `devices` filter or with this device.
fn decl_for<'a>(hooks: &'a [HookDecl], name: &str, device: &str) -> Option<&'a HookDecl> {
    hooks
        .iter()
        .find(|h| h.name == name && (h.devices.is_empty() || h.devices.iter().any(|d| d == device)))
}

/// The kind a declaration gives: `tail = true`, `stock = true`, else raw.
fn kind_of(h: &HookDecl) -> SiteKind {
    if h.tail {
        SiteKind::Tail
    } else if h.stock {
        SiteKind::Stock
    } else {
        SiteKind::Raw
    }
}

/// The site facts of every hook of the port in `port_text` that a service uses, read from the stock image `fw`: the displaced length, the kind and the
/// check value over the displaced bytes. A copied site (tail, stock) must be a plain copy: its tail is the displaced bytes and a jump, with no fixup.
pub fn site_facts(fw: &[u8], port_text: &str, decls: &[&str]) -> Result<Vec<Facts>> {
    let port = Port::parse(port_text)?;
    let reg = Registry {
        ports: vec![port.clone()],
    };
    let (_, hooks) = service_decls(decls)?;
    let device = port.port.device.clone();
    let mut out = vec![];
    for h in &port.hook {
        let Some(d) = decl_for(&hooks, &h.name, &device) else {
            continue;
        };
        let site = crate::apply::resolve_hook_site(fw, &reg, &h.name)?;
        let kind = kind_of(d);
        if kind.copies() {
            let tail = prim::cf_tail(&site.cf, 0, site.ret).map_err(|e| err!("{}: {e}", h.name))?;
            let mut want = site.old.clone();
            want.extend_from_slice(&prim::CF_JMP_L.to_be_bytes());
            want.extend_from_slice(&(site.ret as u32).to_be_bytes());
            if tail != want {
                return Err(err!(
                    "{}: the displaced instructions need a fixup (they are not a plain copy); the runtime has no fixup encoder",
                    h.name
                ));
            }
        }
        out.push(Facts {
            hook: h.name.clone(),
            len: site.site_len as u32,
            kind,
            check: check_of(&site.old),
        });
    }
    Ok(out)
}

fn fw_id_of(port: &Port) -> Result<[u8; 8]> {
    unhex(&port.port.syx_sha256)?[..8]
        .try_into()
        .map_err(|_| err!("port {}: syx_sha256 is not a sha256", port.id()))
}

/// The anchors of `port` that a declared name stands for: the name itself or, for a multi-site rule, `name.<n>`.
fn anchors_of<'a>(port: &'a Port, decl: &str) -> Vec<&'a str> {
    port.anchor
        .iter()
        .map(|a| a.name.as_str())
        .filter(|n| {
            *n == decl
                || n.strip_prefix(decl)
                    .and_then(|r| r.strip_prefix('.'))
                    .is_some_and(|r| !r.is_empty() && r.bytes().all(|c| c.is_ascii_digit()))
        })
        .collect()
}

fn to32(v: u64, what: &str) -> Result<u32> {
    u32::try_from(v).map_err(|_| err!("{what}: {v:#x} is not 32 bits"))
}

/// Does a port of another device than `device` have a hook or an anchor of this name? The reference of a name that only another device has resolves to 0.
fn elsewhere(reg: &Registry, device: &str, name: &str, hook: bool) -> bool {
    reg.ports
        .iter()
        .filter(|p| p.port.device != device)
        .any(|p| {
            if hook {
                p.hook(name).is_some()
            } else {
                !anchors_of(p, name).is_empty()
            }
        })
}

/// Does the port have a hook that a service uses? A port of a device that no service targets has no site to list and gets no port file.
pub fn targeted(port: &Port, decls: &[&str]) -> Result<bool> {
    let (_, hooks) = service_decls(decls)?;
    Ok(hooks.iter().any(|h| port.hook(&h.name).is_some()))
}

/// May a declared name or hook of a service be missing on every committed port? Only when its `devices` filter names devices alone that have no committed port.
fn unported_only(reg: &Registry, devices: &[String]) -> bool {
    !devices.is_empty()
        && devices
            .iter()
            .all(|d| !reg.ports.iter().any(|p| p.port.device == *d))
}

/// The data of the port file of `port`, from the committed ports `reg` and the declarations `decls`.
pub fn build_data(reg: &Registry, port: &Port, decls: &[&str]) -> Result<PortData> {
    let (names, hooks) = service_decls(decls)?;
    let device = port.port.device.as_str();
    let who = port.id();
    let mut out = PortData {
        fw_id: fw_id_of(port)?,
        names: vec![],
        sites: vec![],
    };
    let mut seen = BTreeSet::new();
    for d in &names {
        if !seen.insert(d.name.clone()) {
            continue;
        }
        let found = anchors_of(port, &d.name);
        if found.is_empty() {
            if !elsewhere(reg, device, &d.name, false) {
                if unported_only(reg, &d.devices) {
                    // declared only for devices without a committed port: nothing to list yet
                    continue;
                }
                return Err(err!(
                    "{who}: the name {} is used by a service and exists on no committed port",
                    d.name
                ));
            }
            out.names.push(NameItem {
                hash: name_hash(&d.name),
                addr: 0,
                n: 0,
                kind: 0,
                absent: true,
            });
            continue;
        }
        for n in found {
            let a = port.anchor(n).unwrap();
            if a.cpu != "cf" {
                return Err(err!(
                    "{who}: the name {n} is used by a service but its anchor is not a ColdFire anchor"
                ));
            }
            out.names.push(NameItem {
                hash: name_hash(n),
                addr: to32(a.addr, n)?,
                n: 0,
                kind: 0,
                absent: false,
            });
        }
    }
    let addr_of = |n: &str| -> Result<u64> {
        port.anchor(n)
            .filter(|a| a.cpu == "cf")
            .map(|a| a.addr)
            .ok_or_else(|| err!("no ColdFire anchor {n} on {who}"))
    };
    let mut seen = BTreeSet::new();
    for d in &hooks {
        if !seen.insert(d.name.clone()) {
            continue;
        }
        let hash = name_hash(&d.name);
        let Some(h) = port.hook(&d.name) else {
            if !elsewhere(reg, device, &d.name, true) {
                if unported_only(reg, &d.devices) {
                    continue;
                }
                return Err(err!(
                    "{who}: the hook {} is used by a service and exists on no committed port",
                    d.name
                ));
            }
            // listed for another device only: the hook and its exits resolve to 0
            out.sites.push(SiteItem {
                hash,
                addr: 0,
                len: 0,
                kind: SiteKind::Raw,
                absent: true,
                check: [0; 8],
            });
            for n in 0..=d.exits.len() {
                out.names.push(NameItem {
                    hash,
                    addr: 0,
                    n: n as u16,
                    kind: 1,
                    absent: true,
                });
            }
            continue;
        };
        let decl = decl_for(&hooks, &d.name, device)
            .ok_or_else(|| err!("{}: not declared for {device}", d.name))?;
        let (site, ret) = site_of(port, h, decl, &who)?;
        out.sites.push(site);
        out.names.push(NameItem {
            hash,
            addr: ret,
            n: 0,
            kind: 1,
            absent: false,
        });
        for (i, e) in decl.exits.iter().enumerate() {
            let v = eval_exit_with(e, &addr_of)
                .map_err(|e2| err!("{}: exit {}: {e2}", d.name, i + 1))?;
            out.names.push(NameItem {
                hash,
                addr: v,
                n: (i + 1) as u16,
                kind: 1,
                absent: false,
            });
        }
    }
    out.names.sort_by_key(|x| (x.kind, x.hash, x.n));
    out.sites.sort_by_key(|x| x.hash);
    for w in out.names.windows(2) {
        if (w[0].kind, w[0].hash, w[0].n) == (w[1].kind, w[1].hash, w[1].n) {
            return Err(err!("{who}: two names share the hash {:#010x}", w[0].hash));
        }
    }
    for w in out.sites.windows(2) {
        if w[0].hash == w[1].hash {
            return Err(err!("{who}: two hooks share the hash {:#010x}", w[0].hash));
        }
    }
    Ok(out)
}

/// The SITES item of a hook of the committed port, and its return address.
fn site_of(port: &Port, h: &Hook, decl: &HookDecl, who: &str) -> Result<(SiteItem, u32)> {
    let (Some(len), Some(kind), Some(check)) = (h.len, h.kind.as_deref(), h.check.as_deref())
    else {
        return Err(err!(
            "{who}: hook {} has no site facts (len, kind, check): run `port_resolve` with the local firmware",
            h.name
        ));
    };
    if !h.fixups.is_empty() {
        return Err(err!(
            "{who}: hook {} needs fixups, which the runtime cannot apply yet",
            h.name
        ));
    }
    let kind = SiteKind::parse(kind)?;
    if kind != kind_of(decl) {
        return Err(err!(
            "{who}: hook {}: the port says kind {kind:?}, the declaration says {:?}: run `port_resolve`",
            h.name,
            kind_of(decl)
        ));
    }
    if len < 6 || len as usize > dkneeds::keys::BIND_STOCK_MAX {
        return Err(err!("{who}: hook {}: site length {len}", h.name));
    }
    let anchor = port
        .anchor(&h.site)
        .ok_or_else(|| err!("{who}: hook {}: no anchor {}", h.name, h.site))?;
    let addr = to32((anchor.addr as i64 + h.offset) as u64, &h.name)?;
    let check: [u8; 8] = unhex(check)?
        .try_into()
        .map_err(|_| err!("{who}: hook {}: check is not 8 bytes", h.name))?;
    Ok((
        SiteItem {
            hash: name_hash(&h.name),
            addr,
            len: len as u16,
            kind,
            absent: false,
            check,
        },
        addr + len,
    ))
}

fn put_entry(t: &mut Vec<u8>, tag: u16, count: usize, body: &[u8]) {
    t.extend_from_slice(&tag.to_be_bytes());
    t.extend_from_slice(&(count as u16).to_be_bytes());
    t.extend_from_slice(&(body.len() as u32).to_be_bytes());
    t.extend_from_slice(body);
}

/// The port file: a role 2 container with no code and the table of `data`.
pub fn pack(data: &PortData) -> Result<Vec<u8>> {
    let provides = [Pair {
        a: name_hash(PORT_SERVICE),
        b: PORT_VERSION,
    }];
    let fw = [data.fw_id];
    let t = Table {
        provides: &provides,
        fw: &fw,
        ..Table::default()
    };
    let mut table = vec![0u8; table_size(&t)];
    write_table(&t, &mut table).map_err(|e| err!("header table: {e:?}"))?;
    // the writer knows no port tags: append the two entries (tags above FW, so the order stays ascending)
    let mut names = Vec::with_capacity(NAME_ITEM * data.names.len());
    for x in &data.names {
        names.extend_from_slice(&x.hash.to_be_bytes());
        names.extend_from_slice(&x.addr.to_be_bytes());
        names.extend_from_slice(&x.n.to_be_bytes());
        names.push(x.kind);
        names.push(if x.absent { FLAG_ABSENT } else { 0 });
    }
    let mut sites = Vec::with_capacity(SITE_ITEM * data.sites.len());
    for x in &data.sites {
        sites.extend_from_slice(&x.hash.to_be_bytes());
        sites.extend_from_slice(&x.addr.to_be_bytes());
        sites.extend_from_slice(&x.len.to_be_bytes());
        sites.push(x.kind as u8);
        sites.push(if x.absent { FLAG_ABSENT } else { 0 });
        sites.extend_from_slice(&x.check);
    }
    let n = u16::from_be_bytes([table[6], table[7]]) + 2;
    table[6..8].copy_from_slice(&n.to_be_bytes());
    put_entry(&mut table, TAG_FWNAMES, data.names.len(), &names);
    put_entry(&mut table, TAG_SITES, data.sites.len(), &sites);
    emit(&EmitParts {
        name: PORT_SERVICE,
        version: PORT_VERSION,
        role: ROLE_PORT,
        runtime_word: 0,
        svc_abi: 0,
        image: &[],
        bss: 0,
        align: 4,
        imports: &[],
        relocs: &[],
        entries: [None; 4],
        table: &table,
        dsp: &[],
        max_file: max_file(),
    })
}

/// The data of a port file (the inverse of [`pack`]).
pub fn parse(file: &[u8]) -> Result<PortData> {
    let d = Dkm::parse(file)?;
    if d.role != ROLE_PORT {
        return Err(err!("{}: not a port file (role {})", d.name, d.role));
    }
    let t = &d.register;
    let bad = || err!("port file: bad header table");
    if t.len() < 8 {
        return Err(bad());
    }
    let n = u16::from_be_bytes([t[6], t[7]]) as usize;
    let (mut at, mut names, mut sites, mut fw_id) = (8, vec![], vec![], None);
    for _ in 0..n {
        let head = t.get(at..at + 8).ok_or_else(bad)?;
        let tag = u16::from_be_bytes([head[0], head[1]]);
        let count = u16::from_be_bytes([head[2], head[3]]) as usize;
        let len = u32::from_be_bytes(head[4..8].try_into().unwrap()) as usize;
        let body = t.get(at + 8..at + 8 + len).ok_or_else(bad)?;
        at += 8 + len;
        let u32at = |b: &[u8], i: usize| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
        match tag {
            TAG_FWNAMES => {
                if body.len() != count * NAME_ITEM {
                    return Err(bad());
                }
                for b in body.chunks(NAME_ITEM) {
                    names.push(NameItem {
                        hash: u32at(b, 0),
                        addr: u32at(b, 4),
                        n: u16::from_be_bytes([b[8], b[9]]),
                        kind: b[10],
                        absent: b[11] & FLAG_ABSENT != 0,
                    });
                }
            }
            TAG_SITES => {
                if body.len() != count * SITE_ITEM {
                    return Err(bad());
                }
                for b in body.chunks(SITE_ITEM) {
                    sites.push(SiteItem {
                        hash: u32at(b, 0),
                        addr: u32at(b, 4),
                        len: u16::from_be_bytes([b[8], b[9]]),
                        kind: match b[10] {
                            0 => SiteKind::Raw,
                            1 => SiteKind::Tail,
                            2 => SiteKind::Stock,
                            k => return Err(err!("port file: site kind {k}")),
                        },
                        absent: b[11] & FLAG_ABSENT != 0,
                        check: b[12..20].try_into().unwrap(),
                    });
                }
            }
            x if x == dkneeds::keys::tag::FW => {
                fw_id = Some(<[u8; 8]>::try_from(body.get(..8).ok_or_else(bad)?).unwrap());
            }
            _ => {}
        }
    }
    Ok(PortData {
        fw_id: fw_id.ok_or_else(|| err!("port file: no FW item"))?,
        names,
        sites,
    })
}

/// The file name of a port: `<device>-<version>.dks`.
pub fn file_name(port: &Port) -> String {
    format!("{}.dks", port.id())
}

/// The firmware references of an unbound service that the port file does not list (any device's port): the names `patchkit service-check` refuses.
pub fn outside_set(dks: &Dkm, data: &PortData) -> Vec<String> {
    outside_imports(&dks.imports, data)
}

fn outside_imports(imports: &[String], data: &PortData) -> Vec<String> {
    let mut out = vec![];
    for i in imports {
        let Some(r) = parse_import(i) else { continue };
        let known = match r {
            FwRef::Name(h) => data.name(h).is_some(),
            FwRef::Exit(h, n) => data.exit(h, n).is_some(),
            FwRef::Tail(h) | FwRef::Stock(h) | FwRef::Ret(h) => data.site(h).is_some(),
        };
        if !known {
            out.push(i.clone());
        }
    }
    out
}

/// The one-time check that the port file says what `patchkit bind` does for `dks` on the stock image `fw`: for each hook the service takes, the site address,
/// the return address, the check value, the kind and the tail bytes (the displaced bytes and `jmp.l ret`, read from the bound file); for each firmware reference
/// the place it resolves to. Returns the differences (empty: equal) and the number of facts compared.
pub fn equals_bind(
    dks: &[u8],
    fw: &[u8],
    reg: &Registry,
    decls: &[&str],
    data: &PortData,
) -> Result<(Vec<String>, usize)> {
    let dkm = Dkm::parse(dks)?;
    let (bound_file, bound, refs) = bind_with_refs(dks, fw, reg, decls)?;
    let bound_dkm = Dkm::parse(&bound_file)?;
    let keys =
        dkneeds::keys::Keys::parse(&dkm.register).map_err(|e| err!("header table: {e:?}"))?;
    let hooks: Vec<u32> = keys.hooks().map(|p| p.a).collect();
    let (mut diff, mut n) = compare(&dkm.imports, &hooks, &bound, &bound_dkm.image, &refs, data);
    // a file with no HOOKS compares no site and would pass: when devices/fw names hooks for this service on this device, an empty HOOKS is a stale file
    let device = Firmware::parse(fw)?.device.short();
    let (_, decl_hooks) = service_decls(decls)?;
    let declared: Vec<&str> = decl_hooks
        .iter()
        .filter(|h| {
            h.users.contains(&dkm.name)
                && (h.devices.is_empty() || h.devices.iter().any(|d| d == device))
        })
        .map(|h| h.name.as_str())
        .collect();
    if hooks.is_empty() && !declared.is_empty() {
        n += 1;
        diff.push(format!(
            "{}: devices/fw declares {} hook(s) for this service (first: {}), the file's HOOKS is empty: stale file?",
            dkm.name,
            declared.len(),
            declared[0]
        ));
    }
    Ok((diff, n))
}

/// The kind of a hook as `bind` made it: a `dk_tail_` reference makes a tail site (the file may also use `dk_stock_` of it: both resolve to the same tail,
/// as `source_draw` of machines does), a `dk_stock_` reference alone a stock site, else raw.
fn bind_kind(b: &BoundHook, imports: &[String], refs: &[(FwRef, Place)]) -> SiteKind {
    let stock = refs
        .iter()
        .any(|(r, _)| matches!(r, FwRef::Stock(h) if *h == b.site_hash));
    let tail = imports
        .iter()
        .any(|i| matches!(parse_import(i), Some(FwRef::Tail(h)) if h == b.site_hash));
    if tail {
        SiteKind::Tail
    } else if stock {
        SiteKind::Stock
    } else {
        SiteKind::Raw
    }
}

/// The comparison of `equals_bind`, on what bind made: the imports and the declared hook hashes of the service, the bound hooks with the image of the bound
/// file, and the resolved references.
fn compare(
    imports: &[String],
    hooks: &[u32],
    bound: &[BoundHook],
    bound_image: &[u8],
    refs: &[(FwRef, Place)],
    data: &PortData,
) -> (Vec<String>, usize) {
    let (mut diff, mut n) = (vec![], 0usize);
    let mut d = |what: String, ok: bool, diff: &mut Vec<String>| {
        n += 1;
        if !ok {
            diff.push(what);
        }
    };
    for i in outside_imports(imports, data) {
        d(format!("{i}: not in the port file"), false, &mut diff);
    }
    // a hook that the service declares and the port file lists for this device must have been bound and compared
    for h in hooks {
        if data.site(*h).is_some_and(|s| !s.absent) && !bound.iter().any(|b| b.site_hash == *h) {
            d(
                format!("hook {h:#010x}: the port lists a site, bind bound none"),
                false,
                &mut diff,
            );
        }
    }
    for b in bound {
        let Some(s) = data.site(b.site_hash) else {
            d(format!("{}: no SITES item", b.hook), false, &mut diff);
            continue;
        };
        d(
            format!("{}: absent in the port file but bound", b.hook),
            !s.absent,
            &mut diff,
        );
        d(
            format!("{}: site {:#x} vs port {:#x}", b.hook, b.site_addr, s.addr),
            b.site_addr == s.addr,
            &mut diff,
        );
        d(
            format!("{}: length {} vs port {}", b.hook, b.site_len, s.len),
            b.site_len == u32::from(s.len),
            &mut diff,
        );
        d(
            format!(
                "{}: return {:#x} vs port {:#x}",
                b.hook,
                b.ret,
                s.addr + u32::from(s.len)
            ),
            b.ret == s.addr + u32::from(s.len),
            &mut diff,
        );
        d(
            format!("{}: check value differs", b.hook),
            check_of(&b.stock) == s.check,
            &mut diff,
        );
        let bk = bind_kind(b, imports, refs);
        d(
            format!(
                "{}: bind makes kind {bk:?}, the port says {:?}",
                b.hook, s.kind
            ),
            bk == s.kind,
            &mut diff,
        );
        if let Some(off) = b.tail_off {
            let mut want = b.stock.clone();
            want.extend_from_slice(&prim::CF_JMP_L.to_be_bytes());
            want.extend_from_slice(&b.ret.to_be_bytes());
            let at = off as usize;
            d(
                format!(
                    "{}: the tail bytes are not the displaced bytes and a jump",
                    b.hook
                ),
                bound_image.get(at..at + want.len()) == Some(&want[..]),
                &mut diff,
            );
        }
    }
    for (r, p) in refs {
        match (r, p) {
            (FwRef::Name(h), Place::Abs(a)) => match data.name(*h) {
                Some(x) => d(
                    format!("name {h:#010x}: bind {a:#x} vs port {:#x}", x.addr),
                    *a == x.addr,
                    &mut diff,
                ),
                None => d(
                    format!("name {h:#010x}: not in the port file"),
                    false,
                    &mut diff,
                ),
            },
            (FwRef::Exit(h, k), Place::Abs(a)) => match data.exit(*h, *k) {
                Some(x) => d(
                    format!("exit {h:#010x}_{k}: bind {a:#x} vs port {:#x}", x.addr),
                    *a == x.addr,
                    &mut diff,
                ),
                None => d(
                    format!("exit {h:#010x}_{k}: not in the port file"),
                    false,
                    &mut diff,
                ),
            },
            (FwRef::Stock(h), p) => match data.site(*h) {
                // an idle hook (listed for another device) resolves to 0; a bound one needs a copied site
                Some(s) => d(
                    format!("stock {h:#010x}: bind {p:?} vs kind {:?}", s.kind),
                    s.absent && *p == Place::Rel(0) || s.kind.copies() && *p != Place::Rel(0),
                    &mut diff,
                ),
                None => d(
                    format!("stock {h:#010x}: not in the port file"),
                    false,
                    &mut diff,
                ),
            },
            (FwRef::Ret(h), _) => d(
                format!("ret {h:#010x}: not in the port file"),
                data.site(*h).is_some(),
                &mut diff,
            ),
            _ => d(format!("{r:?} resolved to {p:?}"), false, &mut diff),
        }
    }
    (diff, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> PortData {
        PortData {
            fw_id: [1, 2, 3, 4, 5, 6, 7, 8],
            names: vec![
                NameItem {
                    hash: 0x10,
                    addr: 0x4000_0100,
                    n: 0,
                    kind: 0,
                    absent: false,
                },
                NameItem {
                    hash: 0x20,
                    addr: 0,
                    n: 0,
                    kind: 0,
                    absent: true,
                },
                NameItem {
                    hash: 0x30,
                    addr: 0x4000_0206,
                    n: 0,
                    kind: 1,
                    absent: false,
                },
            ],
            sites: vec![SiteItem {
                hash: 0x30,
                addr: 0x4000_0200,
                len: 6,
                kind: SiteKind::Tail,
                absent: false,
                check: [9; 8],
            }],
        }
    }

    #[test]
    fn a_port_file_packs_and_reads_back_with_no_firmware_bytes() {
        let d = data();
        let f = pack(&d).unwrap();
        assert_eq!(parse(&f).unwrap(), d);
        let dkm = Dkm::parse(&f).unwrap();
        assert_eq!(
            (dkm.role, dkm.name.as_str(), dkm.image.len()),
            (ROLE_PORT, "port", 0)
        );
        // it provides the service `port`
        let t = dkm.register.as_slice();
        let provides = name_hash(PORT_SERVICE).to_be_bytes();
        assert!(t.windows(4).any(|w| w == provides));
        // the writer's own parser does not know the port tags 12 and 13 yet and refuses them as unknown: PR2 moves them into dkneeds and flips this
        assert!(dkneeds::keys::Keys::parse(&dkm.register).is_err());
    }

    fn bound(hook: &str, len: u32, stock: &[u8], tail_off: Option<u32>) -> BoundHook {
        BoundHook {
            hook: hook.into(),
            site_hash: 0x30,
            site_addr: 0x4000_0200,
            site_len: len,
            stock: stock.to_vec(),
            tramp_off: 0,
            displaced: vec![],
            ret: 0x4000_0200 + len,
            tail_off,
        }
    }

    /// The bound image with the tail of a bound hook at offset 0: the stock bytes, then `jmp.l ret`.
    fn tail_image(b: &BoundHook) -> Vec<u8> {
        let mut v = b.stock.clone();
        v.extend_from_slice(&prim::CF_JMP_L.to_be_bytes());
        v.extend_from_slice(&b.ret.to_be_bytes());
        v
    }

    #[test]
    fn equals_bind_reports_a_wrong_length_check_kind_or_missing_site() {
        let stock = [0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71];
        let mut d = data();
        d.sites[0].check = check_of(&stock);
        let b = bound("h", 6, &stock, Some(0));
        let img = tail_image(&b);
        let imports = vec!["dk_tail_00000030".to_string()];
        let run = |d: &PortData, b: &BoundHook, hooks: &[u32], refs: &[(FwRef, Place)]| {
            compare(&imports, hooks, std::slice::from_ref(b), &img, refs, d).0
        };
        // all equal
        assert!(run(&d, &b, &[0x30], &[]).is_empty());
        // a wrong length
        let mut w = d.clone();
        w.sites[0].len = 8;
        assert!(
            run(&w, &b, &[0x30], &[])
                .iter()
                .any(|x| x.contains("length"))
        );
        // a wrong check value
        let mut w = d.clone();
        w.sites[0].check = [0; 8];
        assert!(
            run(&w, &b, &[0x30], &[])
                .iter()
                .any(|x| x.contains("check value"))
        );
        // a wrong kind: the port says raw or stock where bind makes a tail
        for k in [SiteKind::Raw, SiteKind::Stock] {
            let mut w = d.clone();
            w.sites[0].kind = k;
            assert!(
                run(&w, &b, &[0x30], &[]).iter().any(|x| x.contains("kind")),
                "{k:?}"
            );
        }
        // a stock reference (the same tail) is fine on a tail site and wrong on a raw site
        let stock_ref = [(FwRef::Stock(0x30), Place::Rel(0x10))];
        assert!(run(&d, &b, &[0x30], &stock_ref).is_empty());
        let mut w = d.clone();
        w.sites[0].kind = SiteKind::Raw;
        let diff = run(&w, &b, &[0x30], &stock_ref);
        assert!(diff.iter().any(|x| x.contains("stock 0x")), "{diff:?}");
        // a site that the service declares for this device and bind did not bind
        assert!(
            compare(&[], &[0x30], &[], &[], &[], &d)
                .0
                .iter()
                .any(|x| x.contains("bind bound none"))
        );
        // a hook with no SITES item at all
        let mut w = d.clone();
        w.sites.clear();
        assert!(
            run(&w, &b, &[0x30], &[])
                .iter()
                .any(|x| x.contains("no SITES item"))
        );
    }

    #[test]
    fn a_service_name_outside_the_port_set_is_listed() {
        let d = data();
        let mut dkm = Dkm::parse(&pack(&d).unwrap()).unwrap();
        dkm.imports = vec![
            "dk_fw_00000010".to_string(),
            "dk_fw_00000099".to_string(),
            "dk_log".into(),
        ];
        assert_eq!(outside_set(&dkm, &d), ["dk_fw_00000099".to_string()]);
    }

    #[test]
    fn a_hook_without_site_facts_is_refused_and_a_declared_kind_must_match() {
        let h = Hook {
            name: "cf.x.hook.y".into(),
            site: "cf.x.site.y".into(),
            offset: 0,
            mode: "replace".into(),
            clobbers: vec![],
            len: None,
            kind: None,
            check: None,
            fixups: vec![],
        };
        let decl = HookDecl {
            name: h.name.clone(),
            devices: vec![],
            tail: true,
            stock: false,
            exits: vec![],
            users: vec![],
        };
        let port = Port::parse(&format!(
            "[port]\ndevice=\"d\"\nversion=\"1\"\nsyx_sha256=\"{0}\"\nmain_sha256=\"{0}\"\nresolved_by=\"t\"\n[limits]\ncontainer_max=1\nmain_staged_max=1\n[[anchor]]\nname=\"cf.x.site.y\"\ncpu=\"cf\"\naddr=0x4000_0200\nlen=6\nsha256=\"{0}\"\nmark=\"D\"\nrule=\"r\"\n",
            "a".repeat(64)
        ))
        .unwrap();
        let e = site_of(&port, &h, &decl, "d-1").unwrap_err().0;
        assert!(e.contains("no site facts"), "{e}");
        let mut h2 = h.clone();
        (h2.len, h2.kind, h2.check) = (Some(6), Some("stock".into()), Some("00".repeat(8)));
        let e = site_of(&port, &h2, &decl, "d-1").unwrap_err().0;
        assert!(e.contains("declaration says"), "{e}");
        h2.kind = Some("tail".into());
        let (s, ret) = site_of(&port, &h2, &decl, "d-1").unwrap();
        assert_eq!(
            (s.addr, ret, s.kind),
            (0x4000_0200, 0x4000_0206, SiteKind::Tail)
        );
        h2.fixups = vec![crate::port::Fixup {
            at: 0,
            form: "x".into(),
        }];
        assert!(
            site_of(&port, &h2, &decl, "d-1")
                .unwrap_err()
                .0
                .contains("fixups")
        );
    }

    fn committed() -> (Registry, Vec<String>) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let reg = Registry::from_dir(&root.join("devices/ports")).unwrap();
        (reg, crate::fwgen::fw_texts(&root).unwrap())
    }

    #[test]
    fn the_committed_ports_build_a_port_file_without_firmware() {
        let (reg, texts) = committed();
        let decls: Vec<&str> = texts.iter().map(String::as_str).collect();
        let built: Vec<(&Port, PortData)> = reg
            .ports
            .iter()
            .map(|p| (p, build_data(&reg, p, &decls).unwrap()))
            .collect();
        for (_, d) in &built {
            assert_eq!(parse(&pack(d).unwrap()).unwrap(), *d);
        }
        // a hook that only some devices have is listed and absent on the others: one port of each kind
        let h = name_hash("cf.lfo.hook.wave_name3");
        let site_present = |d: &PortData, h: u32| d.site(h).is_some_and(|s| !s.absent);
        let graph_vec = name_hash("cf.lfo.hook.graph_vec");
        let (pa, a) = built
            .iter()
            .find(|(_, d)| site_present(d, graph_vec) && d.site(h).is_some_and(|s| s.absent))
            .expect("a port that lacks wave_name3");
        let (_, b) = built
            .iter()
            .find(|(_, d)| site_present(d, h))
            .expect("a port that has wave_name3");
        assert!(a.site(h).unwrap().addr == 0);
        assert!(b.site(h).unwrap().addr != 0);
        // the declared kinds: a tail hook and a stock hook copy their displaced bytes, a raw hook does not
        let kind = |d: &PortData, n: &str| d.site(name_hash(n)).unwrap().kind;
        assert_eq!(kind(a, "cf.lfo.hook.graph_vec"), SiteKind::Tail);
        assert_eq!(kind(a, "cf.lfo.hook.t1_range"), SiteKind::Stock);
        assert_eq!(kind(a, "cf.page.hook.page_desc"), SiteKind::Stock);
        assert!(!kind(a, "cf.lfo.hook.read_call").copies());
        // exit 0 is the end of the site
        let s = a.site(graph_vec).unwrap();
        let e = a.exit(graph_vec, 0).unwrap();
        assert_eq!(e.addr, s.addr + u32::from(s.len));
        assert_eq!(a.fw_id.to_vec(), unhex(&pa.port.syx_sha256).unwrap()[..8]);
    }

    #[test]
    fn a_service_name_on_no_committed_port_fails_the_build() {
        let (reg, texts) = committed();
        let mut decls: Vec<&str> = texts.iter().map(String::as_str).collect();
        let extra = r#"[[fn]]
name = "cf.nowhere.fn.x"
cpu = "cf"
rule = { kind = "cf_func", hash = "00", offset = 0x0, len = 6 }
mark = "D"
doc = "x"
users = ["core/services/lfo"]
"#;
        decls.push(extra);
        let e = build_data(&reg, &reg.ports[0], &decls).unwrap_err().0;
        assert!(
            e.contains("cf.nowhere.fn.x") && e.contains("no committed port"),
            "{e}"
        );
        // declared for a device without a committed port only: nothing to list
        let only = extra.replace("users =", "devices = [\"zz\"]\nusers =");
        let mut decls: Vec<&str> = texts.iter().map(String::as_str).collect();
        decls.push(&only);
        assert!(build_data(&reg, &reg.ports[0], &decls).is_ok());
    }

    #[test]
    fn a_site_of_a_stock_hook_is_declared_exclusive_of_tail() {
        let bad = r#"[[site]]
name = "cf.x.site.y"
cpu = "cf"
rule = { kind = "cf_func", hash = "00", offset = 0x0, len = 6 }
mark = "D"
doc = "x"

[[hook]]
name = "cf.x.hook.y"
site = "cf.x.site.y"
cpu = "cf"
mode = "replace"
tail = true
stock = true
regs = { in = [], clobbers = [] }
doc = "x"
"#;
        let e = crate::fwgen::render(&[("x.toml", bad)]).unwrap_err().0;
        assert!(e.contains("stock = true"), "{e}");
    }
}
