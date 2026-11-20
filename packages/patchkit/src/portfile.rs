//! The port file (docs/plans/port-file.md): one `.dkport` of role 2 ("port") per device and firmware version, built from committed data only: the anchors,
//! hooks and site facts of `devices/ports/<device>-<version>.toml` and the declarations of `devices/fw/`. It holds no firmware bytes: addresses, lengths and
//! a truncated sha256 of each site.
//!
//! The file has the container of a `.dkm` (header, sha256, table) with no code. It provides the name `port`, carries the identity of the firmware
//! (`FW`, the first 8 bytes of the stock `.syx` sha256) and two table entries of a version 3 table (`dkneeds::keys`, which the runtime reads too):
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

use crate::dkm::{Dkm, EmitParts, ROLE_PORT, emit, max_file};
use crate::error::*;
use crate::fwgen::{HookDecl, service_decls, service_decls_with};
use crate::fwnames::{FwRef, eval_exit_with, parse_import};
use crate::port::{Hook, Port, Registry};
use crate::prim;
use dkneeds::keys::{
    FLAG_ABSENT, Keys, NameRec, Pair, SITE_RAW, SITE_STOCK, SITE_TAIL, SiteRec, Table, name_hash,
    table_size, write_table,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// The service a port file provides.
pub const PORT_SERVICE: &str = "port";
/// The version of the service `port` (major << 16 | minor << 8 | patch).
pub const PORT_VERSION: u32 = 0x000100;

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
    /// The site address and the stock bytes of the displaced instructions, read from the image: for a host test that puts them in a fake firmware. Never written to
    /// a committed file (`port_resolve` takes only `len`, `kind` and `check`).
    pub addr: u32,
    pub stock: Vec<u8>,
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
    // the facts of the test fixture's hook too: the port file of the test image lists it
    let (_, hooks) = service_decls_with(decls, true)?;
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
            addr: site.site as u32,
            stock: site.old.clone(),
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

/// The data of the port file of `port`, from the committed ports `reg` and the declarations `decls`. `test_hooks`: the port file of the test image, which also lists the hooks of
/// the test fixture service (a production port file lists none).
pub fn build_data(
    reg: &Registry,
    port: &Port,
    decls: &[&str],
    test_hooks: bool,
) -> Result<PortData> {
    let (names, hooks) = service_decls_with(decls, test_hooks)?;
    build_from(reg, port, &names, &hooks)
}

/// [`build_data`] over the parsed declarations.
fn build_from(
    reg: &Registry,
    port: &Port,
    names: &[crate::fwgen::NameDecl],
    hooks: &[HookDecl],
) -> Result<PortData> {
    let device = port.port.device.as_str();
    let who = port.id();
    let mut out = PortData {
        fw_id: fw_id_of(port)?,
        names: vec![],
        sites: vec![],
    };
    let mut seen = BTreeSet::new();
    for d in names {
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
    for d in hooks {
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
            // listed for another device only: the hook and its exits resolve to 0. The exits are the most that any declaration of the hook has (`d` may be the declaration of
            // another device, with fewer exits), so an exit that this device never had still resolves to 0 and is not unlisted.
            let most = hooks
                .iter()
                .filter(|x| x.name == d.name)
                .map(|x| x.exits.len())
                .max()
                .unwrap_or(0);
            out.sites.push(SiteItem {
                hash,
                addr: 0,
                len: 0,
                kind: SiteKind::Raw,
                absent: true,
                check: [0; 8],
            });
            for n in 0..=most {
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
        if decl.handler {
            return Err(err!(
                "{who}: the hook {} has a handler: a service takes raw hooks only (the port file has no trampolines)",
                d.name
            ));
        }
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

/// The port file: a role 2 container with no code and the table of `data`.
pub fn pack(data: &PortData) -> Result<Vec<u8>> {
    let provides = [Pair {
        a: name_hash(PORT_SERVICE),
        b: PORT_VERSION,
    }];
    let fw = [data.fw_id];
    let names: Vec<NameRec> = data
        .names
        .iter()
        .map(|x| NameRec {
            hash: x.hash,
            addr: x.addr,
            n: x.n,
            kind: x.kind,
            flags: if x.absent { FLAG_ABSENT } else { 0 },
        })
        .collect();
    let sites: Vec<SiteRec> = data
        .sites
        .iter()
        .map(|x| SiteRec {
            hash: x.hash,
            addr: x.addr,
            len: x.len,
            kind: x.kind as u8,
            flags: if x.absent { FLAG_ABSENT } else { 0 },
            check: x.check,
        })
        .collect();
    let t = Table {
        provides: &provides,
        fw: &fw,
        names: &names,
        sites: &sites,
        ..Table::default()
    };
    let mut table = vec![0u8; table_size(&t)];
    write_table(&t, &mut table).map_err(|e| err!("header table: {e:?}"))?;
    // the image is the `rts` that every `dk_ret_` resolves to (and a `nop` to fill the word); the bss holds the copy of each tail and stock site (the displaced bytes
    // and `jmp.l`), which the runtime fills from flash after it checked them
    let bss: usize = data
        .sites
        .iter()
        .filter(|x| !x.absent && x.kind.copies())
        .map(|x| usize::from(x.len) + 6)
        .sum();
    emit(&EmitParts {
        name: PORT_SERVICE,
        version: PORT_VERSION,
        role: ROLE_PORT,
        runtime_word: 0,
        svc_abi: 0,
        image: &[0x4e, 0x75, 0x4e, 0x71],
        bss: bss as u32,
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
    let k = Keys::parse(&d.register).map_err(|e| err!("port file: bad header table: {e:?}"))?;
    let names = (0..k.name_count())
        .filter_map(|i| k.name(i))
        .map(|x| NameItem {
            hash: x.hash,
            addr: x.addr,
            n: x.n,
            kind: x.kind,
            absent: x.flags & FLAG_ABSENT != 0,
        })
        .collect();
    let mut sites = vec![];
    for x in (0..k.site_count()).filter_map(|i| k.site(i)) {
        sites.push(SiteItem {
            hash: x.hash,
            addr: x.addr,
            len: x.len,
            kind: match x.kind {
                SITE_RAW => SiteKind::Raw,
                SITE_TAIL => SiteKind::Tail,
                SITE_STOCK => SiteKind::Stock,
                k => return Err(err!("port file: site kind {k}")),
            },
            absent: x.flags & FLAG_ABSENT != 0,
            check: x.check,
        });
    }
    Ok(PortData {
        fw_id: k.fw(0).ok_or_else(|| err!("port file: no FW item"))?,
        names,
        sites,
    })
}

/// The file name of a port: `<device>-<version>.dkport`.
pub fn file_name(port: &Port) -> String {
    format!("{}.dkport", port.id())
}

/// The firmware references of a service that the port file does not list (any device's port): the names `patchkit service-check` refuses.
pub fn outside_set(dks: &Dkm, data: &PortData) -> Vec<String> {
    outside_imports(&dks.imports, data)
}

/// The hooks a service file takes (its `HOOKS` items) that the port file has no `SITES` item for, as name hashes: the file would be refused at load with `E_IMPORT`.
pub fn unlisted_hooks(dks: &Dkm, data: &PortData) -> Result<Vec<u32>> {
    let keys =
        dkneeds::keys::Keys::parse(&dks.register).map_err(|e| err!("header table: {e:?}"))?;
    Ok(keys
        .hooks()
        .map(|p| p.a)
        .filter(|h| data.site(*h).is_none())
        .collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fwgen::image_hooks;

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
            (ROLE_PORT, "port", 4)
        );
        // it provides the service `port`
        let t = dkm.register.as_slice();
        let provides = name_hash(PORT_SERVICE).to_be_bytes();
        assert!(t.windows(4).any(|w| w == provides));
        // the table is a version 3 table of dkneeds, which the runtime parses
        let k = dkneeds::keys::Keys::parse(&dkm.register).unwrap();
        assert_eq!(
            (k.version(), k.name_count(), k.site_count()),
            (3, d.names.len(), d.sites.len())
        );
    }

    /// A hook that only device `b` has, with more exits there than the declaration of device `a` lists: on `a` every exit of the hook, also the one only `b`'s declaration has,
    /// is listed as absent and resolves to 0 (it is not unlisted, which would refuse the service file).
    #[test]
    fn the_absent_exits_of_a_hook_cover_the_most_that_any_declaration_has() {
        let hex = "a".repeat(64);
        let port = |dev: &str, hook: &str| {
            Port::parse(&format!(
                "[port]\ndevice=\"{dev}\"\nversion=\"1\"\nsyx_sha256=\"{hex}\"\nmain_sha256=\"{hex}\"\nresolved_by=\"t\"\n[limits]\ncontainer_max=1\nmain_staged_max=1\n[[anchor]]\nname=\"cf.x.site.y\"\ncpu=\"cf\"\naddr=0x4000_0200\nlen=6\nsha256=\"{hex}\"\nmark=\"D\"\nrule=\"r\"\n{hook}"
            ))
            .unwrap()
        };
        let a = port("a", "");
        let b = port(
            "b",
            "[[hook]]\nname=\"cf.x.hook.h\"\nsite=\"cf.x.site.y\"\nmode=\"replace\"\nlen=6\nkind=\"raw\"\ncheck=\"0000000000000000\"\n",
        );
        let reg = Registry {
            ports: vec![a.clone(), b.clone()],
        };
        let decl = |dev: &str, exits: &[&str]| HookDecl {
            name: "cf.x.hook.h".into(),
            devices: vec![dev.into()],
            tail: false,
            stock: false,
            handler: false,
            exits: exits.iter().map(|e| e.to_string()).collect(),
            users: vec![],
        };
        // the declaration of `a` comes first and has one exit; that of `b` has two
        let hooks = [
            decl("a", &["cf.x.site.y"]),
            decl("b", &["cf.x.site.y + 8", "cf.x.site.y + 10"]),
        ];
        let on_a = build_from(&reg, &a, &[], &hooks).unwrap();
        let h = name_hash("cf.x.hook.h");
        assert!(on_a.site(h).unwrap().absent);
        for n in 0..=2 {
            assert!(
                on_a.exit(h, n).is_some_and(|x| x.absent),
                "exit {n} of the hook on a"
            );
        }
        assert!(on_a.exit(h, 3).is_none());
        // on `b` the hook and its two exits are present
        let on_b = build_from(&reg, &b, &[], &hooks).unwrap();
        assert!(
            on_b.exit(h, 2)
                .is_some_and(|x| !x.absent && x.addr == 0x4000_020a)
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
            handler: false,
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

    /// A copied site (a tail or a stock path) is compared with its check value at the load of the port file, so it must hold the stock bytes on a patched image: no site that a
    /// module of the image patches, by a hook, a stub, a write or a retarget of a module of any patch set of the device (`devices/*.toml` `[patch_sets]`, with their requires).
    /// The test fixture's hook shares the site of `core/digikit-menu`'s hook, so a production port file lists none of the fixture's hooks; the port file of the test image
    /// (which runs without digikit-menu) lists them.
    #[test]
    fn a_production_port_file_has_no_test_hook_and_no_copied_site_on_a_site_an_image_module_patches()
     {
        let (reg, texts) = committed();
        let decls: Vec<&str> = texts.iter().map(String::as_str).collect();
        let images = image_hooks(&decls).unwrap();
        assert!(
            images
                .iter()
                .any(|(n, _)| n == "cf.menu.hook.settings_list.tail"),
            "digikit-menu's hook is an image hook"
        );
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let lib = crate::module::library_with(
            &root,
            &crate::module::LibrarySource::default(),
            true,
            Some(&[]),
        )
        .unwrap();
        let test_hook = name_hash("cf.menu.hook.settings_list.test");
        let mut with_sites = 0;
        for p in &reg.ports {
            let prod = build_data(&reg, p, &decls, false).unwrap();
            assert!(
                prod.site(test_hook).is_none(),
                "{}: a production port file lists the test hook",
                p.id()
            );
            let test = build_data(&reg, p, &decls, true).unwrap();
            assert!(
                test.sites.len() > prod.sites.len()
                    || p.hook("cf.menu.hook.settings_list.test").is_none(),
                "{}: the test port file lists the test hook",
                p.id()
            );
            let copied: Vec<_> = prod
                .sites
                .iter()
                .filter(|s| !s.absent && s.kind.copies())
                .collect();
            // a port whose firmware has no service hook that copies bytes (a device without services) has no site to check
            if copied.is_empty() {
                continue;
            }
            with_sites += 1;
            // the ranges that the modules of every patch set of this device change
            let info = crate::devdata::by_short(&p.port.device).expect("the device of a port");
            let mut ids: Vec<String> = info.patch_sets.values().flatten().cloned().collect();
            ids.sort();
            ids.dedup();
            let (ids, _) = lib.with_requires(&ids).unwrap();
            let mut ranges = vec![];
            for id in &ids {
                ranges.extend(crate::apply::patched_ranges(lib.get(id).unwrap(), p).unwrap());
            }
            assert!(!ranges.is_empty(), "{}: no patched range found", p.id());
            for s in copied {
                let (lo, hi) = (u64::from(s.addr), u64::from(s.addr) + u64::from(s.len));
                for r in &ranges {
                    assert!(
                        hi <= r.lo || r.hi <= lo,
                        "{}: the copied site {:#x}..{:#x} overlaps {} of {} ({:#x}..{:#x}), which an image module patches",
                        p.id(),
                        lo,
                        hi,
                        r.what,
                        r.module,
                        r.lo,
                        r.hi
                    );
                }
            }
        }
        assert!(
            with_sites >= 2,
            "the copied sites of at least one port of each service device were checked"
        );
    }

    #[test]
    fn the_committed_ports_build_a_port_file_without_firmware() {
        let (reg, texts) = committed();
        let decls: Vec<&str> = texts.iter().map(String::as_str).collect();
        let built: Vec<(&Port, PortData)> = reg
            .ports
            .iter()
            .map(|p| (p, build_data(&reg, p, &decls, false).unwrap()))
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
        let e = build_data(&reg, &reg.ports[0], &decls, false)
            .unwrap_err()
            .0;
        assert!(
            e.contains("cf.nowhere.fn.x") && e.contains("no committed port"),
            "{e}"
        );
        // declared for a device without a committed port only: nothing to list
        let only = extra.replace("users =", "devices = [\"zz\"]\nusers =");
        let mut decls: Vec<&str> = texts.iter().map(String::as_str).collect();
        decls.push(&only);
        assert!(build_data(&reg, &reg.ports[0], &decls, false).is_ok());
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
