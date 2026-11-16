//! `patchkit bind`: a service `.dks` is packed without firmware. Binding adds, for each hook the service declares (the `HOOKS` entry of its
//! header table), a trampoline to the image and a `BIND` item, using the user's firmware image. The trampoline saves the registers the
//! hook may not clobber, calls the service's handler with the arguments the hook declares, restores, runs the displaced instructions
//! and jumps back. The bound file holds firmware bytes (the stock bytes of each site): it is never committed.

use crate::apply::{AnchorView, HookSite, resolve_hook_site};
use crate::dkm::{Dkm, EmitParts, FLAG_TRACE, ROLE_SERVICE, emit, max_file};
use crate::error::*;
use crate::firmware::Firmware;
use crate::fwgen::{
    HandlerArg, HandlerPlan, handler_plan, hook_exits, hook_has_handler, hook_has_tail,
};
use crate::port::{Port, Registry};
use crate::prim::{self, CF_JMP_L, CfSite, cf_tail};
use dkneeds::keys::{
    BIND_STOCK_MAX, Bind, KeyGroup, Keys, Pair, Table, name_hash, table_size, write_table,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Most `BIND` items of one service (`[limits.format] bind_max` of sdk/sdk.toml; the runtime sizes its copy with it).
pub const BIND_MAX: usize = dkneeds::BIND_MAX as usize;

/// One bound hook, for the report.
#[derive(Clone, Debug)]
pub struct BoundHook {
    pub hook: String,
    /// `name_hash` of the hook name: the `site` of the `BIND` item.
    pub site_hash: u32,
    pub site_addr: u32,
    pub site_len: u32,
    /// The stock bytes at the site.
    pub stock: Vec<u8>,
    /// The offset of the trampoline in the image.
    pub tramp_off: u32,
    /// The displaced instructions, as plan lines (see [`prim::fmt_displaced`]); empty when the hook drops them.
    pub displaced: Vec<String>,
    /// Where the trampoline jumps back to.
    pub ret: u32,
    /// A raw hook with a `tail` exit: the image offset of its generated tail (the displaced instructions, then `jmp.l ret`).
    pub tail_off: Option<u32>,
}

/// The name of the undefined symbol with which the asm of a raw hook refers to the tail of `hook` (an exit `tail = true` in `devices/fw/`):
/// `dk_tail_` and the `name_hash` of the hook name in 8 hex digits (an import name has at most 31 characters). `patchkit pack` keeps it as an import
/// and `patchkit bind` replaces it with a base-relative relocation to the generated tail, so an unbound `.dks` holds no firmware bytes.
pub fn tail_import(hook: &str) -> String {
    format!("dk_tail_{:08x}", name_hash(hook))
}

/// Is `import` the tail reference of some hook (a name `tail_import` makes)?
pub fn is_tail_import(import: &str) -> bool {
    import
        .strip_prefix("dk_tail_")
        .is_some_and(|h| h.len() == 8 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')))
}

/// A reference of a service to the firmware, as an import name `patchkit bind` resolves. Every name carries the 8 hex digits of a `name_hash` and fits the 31
/// characters of an import name (the longest, `dk_exit_<hash>_<n>` with a two-digit `n`, has 19). `patchkit pack` makes the names from the `[object.externs]` of a
/// service module and from the firmware names (`cf.*`) that a Rust service takes from the `dkfw` crate.
///
/// | import | resolves to | word in the file |
/// | --- | --- | --- |
/// | `dk_fw_<H(name)>` | the address of the firmware anchor `name` (an offset is the addend of the reference) | absolute, no relocation |
/// | `dk_exit_<H(hook)>_<n>` | `n` = 0: the end of the hook's site; `n` from 1: the expression `exits[n-1]` of the hook in `devices/fw` | absolute, no relocation |
/// | `dk_tail_<H(hook)>` | the generated tail of the hook (its displaced instructions, then a jump behind the site) | base-relative |
/// | `dk_stock_<H(hook)>` | the same code, called with `jsr` from inside the hook; control comes back | base-relative |
/// | `dk_ret_<H(hook)>` | one `rts` that `bind` appends | base-relative |
///
/// A name that only another device has, and a hook the file lists only for another device, resolve to 0: the code is never entered on this firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FwRef {
    /// `dk_fw_`: the `name_hash` of the firmware name.
    Name(u32),
    /// `dk_tail_`: the `name_hash` of the hook name.
    Tail(u32),
    /// `dk_stock_`: the hook.
    Stock(u32),
    /// `dk_ret_`: the hook.
    Ret(u32),
    /// `dk_exit_`: the hook and the exit number.
    Exit(u32, u32),
}

/// `dk_fw_<hash>`: the import for the firmware name `name` (`cf.draw.fn.blit`).
pub fn fw_import(name: &str) -> String {
    format!("dk_fw_{:08x}", name_hash(name))
}

/// `dk_stock_<hash>`: the import for the stock path of `hook`.
pub fn stock_import(hook: &str) -> String {
    format!("dk_stock_{:08x}", name_hash(hook))
}

/// `dk_ret_<hash>`: the import for leaving `hook` as `rts` does.
pub fn ret_import(hook: &str) -> String {
    format!("dk_ret_{:08x}", name_hash(hook))
}

/// `dk_exit_<hash>_<n>`: the import for exit `n` of `hook`.
pub fn exit_import(hook: &str, n: u32) -> String {
    format!("dk_exit_{:08x}_{n}", name_hash(hook))
}

/// The 8 lower-case hex digits of `s` as a number.
fn hex8(s: &str) -> Option<u32> {
    (s.len() == 8 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| u32::from_str_radix(s, 16).ok())
        .flatten()
}

/// The reference an import name stands for; `None` for every other name (a function of the runtime, a service's import).
pub fn parse_import(name: &str) -> Option<FwRef> {
    if let Some(h) = name.strip_prefix("dk_fw_") {
        return hex8(h).map(FwRef::Name);
    }
    if let Some(h) = name.strip_prefix("dk_tail_") {
        return hex8(h).map(FwRef::Tail);
    }
    if let Some(h) = name.strip_prefix("dk_stock_") {
        return hex8(h).map(FwRef::Stock);
    }
    if let Some(h) = name.strip_prefix("dk_ret_") {
        return hex8(h).map(FwRef::Ret);
    }
    let rest = name.strip_prefix("dk_exit_")?;
    let (h, n) = rest.split_at_checked(8)?;
    let n = n.strip_prefix('_')?;
    if n.is_empty() || n.len() > 2 || !n.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(FwRef::Exit(hex8(h)?, n.parse().ok()?))
}

/// The import a target of `[object.externs]` in a service module stands for: `cf.<name>` is the firmware name (`dk_fw_`), and `hook.<hook>.tail`, `.stock`,
/// `.ret`, `.exit` (exit 0) and `.exit.<n>` are the hook references of the table above. `None` for a target that is no firmware reference.
pub fn import_for_target(target: &str) -> Option<String> {
    if target.starts_with("cf.") {
        return Some(fw_import(target));
    }
    let rest = target.strip_prefix("hook.")?;
    let (hook, last) = rest.rsplit_once('.')?;
    if let Ok(n) = last.parse::<u32>() {
        let hook = hook.strip_suffix(".exit")?;
        return (n >= 1 && n <= crate::fwgen::MAX_EXITS as u32).then(|| exit_import(hook, n));
    }
    match last {
        "tail" => Some(tail_import(hook)),
        "stock" => Some(stock_import(hook)),
        "ret" => Some(ret_import(hook)),
        "exit" => Some(exit_import(hook, 0)),
        _ => None,
    }
}

fn put16(out: &mut Vec<u8>, w: u16) {
    out.extend_from_slice(&w.to_be_bytes());
}

/// The bit of a register in a `movem` mask: d0-d7 are bits 0-7, a0-a7 bits 8-15.
fn reg_bit(r: &str) -> Result<u32> {
    let n: u32 = r
        .get(1..)
        .and_then(|n| n.parse().ok())
        .filter(|n| *n <= 7)
        .ok_or_else(|| err!("bad register {r:?}"))?;
    match r.as_bytes()[0] {
        b'd' => Ok(n),
        b'a' => Ok(8 + n),
        _ => Err(err!("bad register {r:?}")),
    }
}

/// The machine code of `gas_hook` (`fwgen.rs`) without its last `jmp`: save the registers, push the arguments last first, `jsr` the handler
/// (a 32-bit absolute address, written as 0), pop the arguments, restore. `handler_reloc_at` gets the offset of the address word, in the
/// returned bytes. The caller stores the handler's image offset there and adds a base-relative relocation.
///
/// ```text
/// lea -N(sp),sp          4fef d16        N = 4 * saved registers (only with saved registers)
/// movem.l regs,(sp)      48d7 mask       bit n = d<n>, bit 8+n = a<n>
/// move.l Dn,-(sp)        2f00+n          move.l An,-(sp): 2f08+n
/// move.l d16(sp),-(sp)   2f2f d16        d16 = sp+N of the hook + N + 4 * (arguments pushed before)
/// jsr abs.l              4eb9 addr32
/// addq.l #P,sp           508f | (P&7)<<9 (P = 4 * arguments, up to 8); above 8: lea P(sp),sp = 4fef P
/// movem.l (sp),regs      4cd7 mask
/// lea N(sp),sp           4fef N
/// ```
pub fn encode_prologue(plan: &HandlerPlan, handler_reloc_at: &mut Vec<usize>) -> Result<Vec<u8>> {
    let mut o = Vec::new();
    let size = 4 * plan.save.len() as u32;
    let mut mask = 0u32;
    for r in &plan.save {
        mask |= 1 << reg_bit(r)?;
    }
    if !plan.save.is_empty() {
        put16(&mut o, 0x4fef);
        put16(&mut o, (-(size as i32)) as i16 as u16);
        put16(&mut o, 0x48d7);
        put16(&mut o, mask as u16);
    }
    for (k, a) in plan.args.iter().rev().enumerate() {
        match a {
            HandlerArg::Reg(r) => put16(&mut o, 0x2f00 | reg_bit(r)? as u16),
            HandlerArg::Stack(n) => {
                let d16 = n + size + 4 * k as u32;
                if d16 > i16::MAX as u32 {
                    return Err(err!("handler argument sp+{n}: offset {d16} is too far"));
                }
                put16(&mut o, 0x2f2f);
                put16(&mut o, d16 as u16);
            }
        }
    }
    put16(&mut o, 0x4eb9);
    handler_reloc_at.push(o.len());
    o.extend_from_slice(&[0; 4]);
    let pop = 4 * plan.args.len() as u32;
    if pop > 8 {
        put16(&mut o, 0x4fef);
        put16(&mut o, pop as u16);
    } else if pop > 0 {
        put16(&mut o, 0x508f | ((pop & 7) << 9) as u16);
    }
    if !plan.save.is_empty() {
        put16(&mut o, 0x4cd7);
        put16(&mut o, mask as u16);
        put16(&mut o, 0x4fef);
        put16(&mut o, size as u16);
    }
    Ok(o)
}

/// The trampoline of a hook: the prologue, then (mode `before`) the displaced instructions with `jmp.l ret` from [`cf_tail`], or (mode
/// `replace`) only `jmp.l ret`. Also the offsets of the handler address words. The displaced instructions are copied into the file, so they must
/// not depend on where they run: [`cf_tail`] must give the same bytes at two bases.
fn trampoline(
    cf: &CfSite,
    mode: &str,
    plan: &HandlerPlan,
    hook: &str,
) -> Result<(Vec<u8>, Vec<usize>)> {
    let ret = cf.addr + cf.len as u64;
    if ret > u32::MAX as u64 {
        return Err(err!("{hook}: return address {ret:#x} is not 32 bits"));
    }
    let mut at = Vec::new();
    let mut t = encode_prologue(plan, &mut at)?;
    match mode {
        "before" => {
            let a = cf_tail(cf, 0, ret).map_err(|e| err!("{hook}: {e}"))?;
            let b = cf_tail(cf, 0x1000, ret).map_err(|e| err!("{hook}: {e}"))?;
            if a != b {
                return Err(err!("PC-relative displaced instruction at {hook}"));
            }
            t.extend(a);
        }
        "replace" => {
            t.extend_from_slice(&CF_JMP_L.to_be_bytes());
            t.extend_from_slice(&(ret as u32).to_be_bytes());
        }
        m => {
            return Err(err!(
                "{hook}: hook mode {m:?} cannot be bound (only \"before\" and \"replace\")"
            ));
        }
    }
    Ok((t, at))
}

/// The declared hook name whose `name_hash` is `hash`.
fn find_hook<'a>(declared: impl Iterator<Item = &'a str>, hash: u32) -> Result<&'a str> {
    let mut declared = declared;
    declared
        .find(|n| name_hash(n) == hash)
        .ok_or_else(|| err!("no hook declared on this firmware has the name hash {hash:#010x}"))
}

/// Appends the trampoline of one hook to `image` (4-byte aligned) and the relocation of its handler call; `handler` is the handler's image offset.
fn bind_site(
    hook: &str,
    site: &HookSite,
    plan: &HandlerPlan,
    handler: u32,
    image: &mut Vec<u8>,
    relocs: &mut Vec<(u32, u32)>,
) -> Result<BoundHook> {
    if site.old.len() > BIND_STOCK_MAX {
        return Err(err!(
            "{hook}: the site has {} bytes, over the {BIND_STOCK_MAX} a BIND item holds",
            site.old.len()
        ));
    }
    let (tramp, at) = trampoline(&site.cf, &site.mode, plan, hook)?;
    let to32 = |v: u64, what: &str| {
        u32::try_from(v).map_err(|_| err!("{hook}: {what} {v:#x} is not 32 bits"))
    };
    while image.len() % 4 != 0 {
        image.push(0);
    }
    let tramp_off = image.len();
    image.extend_from_slice(&tramp);
    for o in at {
        let w = tramp_off + o;
        image[w..w + 4].copy_from_slice(&handler.to_be_bytes());
        relocs.push((w as u32, 0));
    }
    Ok(BoundHook {
        hook: hook.to_string(),
        site_hash: name_hash(hook),
        site_addr: to32(site.site, "site address")?,
        site_len: site.site_len as u32,
        stock: site.old.clone(),
        tramp_off: tramp_off as u32,
        displaced: if site.mode == "before" {
            prim::fmt_displaced(&site.cf)
        } else {
            vec![]
        },
        ret: to32(site.ret, "return address")?,
        tail_off: None,
    })
}

/// A hook with no `handler` (`devices/fw/`): its entry is a routine that takes the whole control flow of the site (it runs in place of the stock instructions and
/// ends in the stock code by itself, as the effect pages' and machines' hooks do). The `BIND` item's trampoline is the entry itself and the runtime writes
/// `jmp entry` at the site. Only `replace` hooks can be like this (a `before` hook would need the displaced instructions copied).
///
/// With `tail` (the declared exit `tail = true`) the displaced instructions of the site are appended to `image` as a tail, in the bytes of the user's firmware
/// ([`cf_tail`], with the same check as a `before` trampoline that they do not depend on where they run), ending in `jmp.l` to the first instruction after the site.
/// The asm of the entry reaches it through [`tail_import`], which `bind` resolves to the returned `tail_off`.
fn bind_raw(
    hook: &str,
    site: &HookSite,
    entry: u32,
    tail: bool,
    image: &mut Vec<u8>,
) -> Result<BoundHook> {
    if site.old.len() > BIND_STOCK_MAX {
        return Err(err!(
            "{hook}: the site has {} bytes, over the {BIND_STOCK_MAX} a BIND item holds",
            site.old.len()
        ));
    }
    if site.mode != "replace" {
        return Err(err!(
            "{hook}: a hook without a handler is entered directly and must have mode \"replace\" (it is {:?})",
            site.mode
        ));
    }
    let to32 = |v: u64, what: &str| {
        u32::try_from(v).map_err(|_| err!("{hook}: {what} {v:#x} is not 32 bits"))
    };
    let ret = to32(site.ret, "return address")?;
    let mut tail_off = None;
    let mut displaced = vec![];
    if tail {
        let a = cf_tail(&site.cf, 0, ret as u64).map_err(|e| err!("{hook}: {e}"))?;
        let b = cf_tail(&site.cf, 0x1000, ret as u64).map_err(|e| err!("{hook}: {e}"))?;
        if a != b {
            return Err(err!("PC-relative displaced instruction at {hook}"));
        }
        while image.len() % 4 != 0 {
            image.push(0);
        }
        tail_off = Some(image.len() as u32);
        image.extend(a);
        displaced = prim::fmt_displaced(&site.cf);
    }
    Ok(BoundHook {
        hook: hook.to_string(),
        site_hash: name_hash(hook),
        site_addr: to32(site.site, "site address")?,
        site_len: site.site_len as u32,
        stock: site.old.clone(),
        tramp_off: entry,
        displaced,
        ret,
        tail_off,
    })
}

/// Replaces the references to the tails with base-relative relocations. A relocation against an import named by [`tail_import`] becomes a
/// base-relative one and its word (the addend) gets the tail's image offset added; the import is dropped from the list and the indices of the others move
/// down. `tails` are (hook name, image offset of its tail). `idle` are the name hashes of the hooks that the service lists for other devices: their tails
/// are never entered on this firmware (`init` declares and the runtime installs only the hooks of the running device), so a reference to one resolves to the
/// image offset 0. An import of the form of a tail reference that no bound hook owns is refused: such a file would never load. Returns the imports that stay.
fn resolve_tail_refs(
    image: &mut [u8],
    relocs: &mut [(u32, u32)],
    imports: &[String],
    tails: &[(&str, u32)],
    idle: &[u32],
) -> Result<Vec<String>> {
    let is_idle = |n: &str| idle.iter().any(|h| format!("dk_tail_{h:08x}") == n);
    let mut keep: Vec<String> = vec![];
    let mut new_index: Vec<Option<usize>> = vec![];
    for n in imports {
        if is_tail_import(n) {
            if !tails.iter().any(|(h, _)| tail_import(h) == *n) && !is_idle(n) {
                return Err(err!(
                    "{n}: the file refers to a tail that no hook of its HOOKS declares with `tail = true`"
                ));
            }
            new_index.push(None);
        } else {
            new_index.push(Some(keep.len()));
            keep.push(n.clone());
        }
    }
    for (off, k) in relocs.iter_mut() {
        if *k >> 24 != 1 {
            continue;
        }
        let i = (*k & 0x00ff_ffff) as usize;
        match new_index.get(i).copied().flatten() {
            Some(j) => *k = 1 << 24 | j as u32,
            None => {
                let name = &imports[i];
                let to = tails
                    .iter()
                    .find(|(h, _)| tail_import(h) == *name)
                    .map_or(0, |(_, to)| *to);
                let at = *off as usize;
                let w = u32::from_be_bytes(image[at..at + 4].try_into().unwrap());
                image[at..at + 4].copy_from_slice(&to.wrapping_add(w).to_be_bytes());
                *k = 0;
            }
        }
    }
    Ok(keep)
}

/// Where an import of [`FwRef`] points: an absolute address, or an offset in the image of the file (relocated with the file's base at load).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Abs(u32),
    Rel(u32),
}

/// Replaces the references of [`FwRef`] other than the tails (`resolve_tail_refs` did those) with their places: `place` says where an import points. A word
/// of the file that holds an addend keeps it (the address is added). An absolute place drops the relocation entry, a relative one makes it a base-relative
/// entry. The resolved imports leave the list and the indices of the others move down. Returns the imports that stay.
fn resolve_fw_refs(
    image: &mut [u8],
    relocs: &mut Vec<(u32, u32)>,
    imports: &[String],
    place: &mut dyn FnMut(FwRef) -> Result<Place>,
) -> Result<Vec<String>> {
    let mut keep: Vec<String> = vec![];
    // per import: Err(new index) when it stays, Ok(place) when it is resolved
    let mut fate: Vec<std::result::Result<Place, usize>> = vec![];
    for n in imports {
        match parse_import(n).filter(|r| !matches!(r, FwRef::Tail(_))) {
            Some(r) => fate.push(Ok(place(r)?)),
            None => {
                fate.push(Err(keep.len()));
                keep.push(n.clone());
            }
        }
    }
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(relocs.len());
    for (off, k) in relocs.iter().copied() {
        if k >> 24 != 1 {
            out.push((off, k));
            continue;
        }
        let i = (k & 0x00ff_ffff) as usize;
        let at = off as usize;
        match fate.get(i) {
            Some(Err(j)) => out.push((off, 1 << 24 | *j as u32)),
            Some(Ok(p)) => {
                let w = u32::from_be_bytes(image[at..at + 4].try_into().unwrap());
                match *p {
                    Place::Abs(to) => {
                        image[at..at + 4].copy_from_slice(&to.wrapping_add(w).to_be_bytes());
                    }
                    Place::Rel(to) => {
                        image[at..at + 4].copy_from_slice(&to.wrapping_add(w).to_be_bytes());
                        out.push((off, 0));
                    }
                }
            }
            None => out.push((off, k)),
        }
    }
    *relocs = out;
    Ok(keep)
}

/// The anchors a hook exit expression names, with the view that checks them: an expression of `exits` is evaluated over the addresses of the port.
struct ExitEnv<'a>(BTreeMap<&'a str, i64>);

impl crate::expr::Env for ExitEnv<'_> {
    fn symbol(&self, name: &str) -> Option<i64> {
        self.0.get(name).copied()
    }
    fn field(&self, _: &str) -> Option<i64> {
        None
    }
    fn image(&self, _: &str, _: i64, _: u8) -> Result<i64> {
        Err(err!(
            "an exit is an anchor and an offset: image reads are not allowed"
        ))
    }
}

/// The value of the exit expression `text` on this firmware: every anchor it names is checked against the image.
fn eval_exit(text: &str, view: &AnchorView) -> Result<u32> {
    let expr = crate::expr::parse(text)?;
    let mut names = vec![];
    expr.names(&mut names);
    let mut known: Vec<(String, i64)> = vec![];
    for n in names {
        known.push((n.clone(), view.addr(&n)? as i64));
    }
    let env = ExitEnv(known.iter().map(|(n, v)| (n.as_str(), *v)).collect());
    let v = expr.eval(&env)?;
    u32::try_from(v).map_err(|_| err!("exit {text:?} is {v:#x}, not a 32-bit address"))
}

/// The name with `hash` for a message: a firmware name of a port, a name in the declarations, or the hash.
fn name_of_hash(reg: &Registry, decls: &[&str], hash: u32) -> String {
    reg.ports
        .iter()
        .flat_map(|p| p.anchor.iter().map(|a| a.name.as_str()))
        .find(|n| name_hash(n) == hash)
        .map(str::to_string)
        .or_else(|| name_from_decls(decls, hash))
        .unwrap_or_else(|| format!("#{hash:08x}"))
}

/// What `bind` knows to resolve the references of one file on one firmware.
struct RefCtx<'a> {
    reg: &'a Registry,
    port: &'a Port,
    view: &'a AnchorView<'a>,
    decls: &'a [&'a str],
    device: &'a str,
    bound: &'a [BoundHook],
    /// The name hashes of the hooks the file lists for other devices.
    idle: &'a [u32],
    /// The image offset of the `rts` stub (made when the file refers to a `dk_ret_`).
    ret_stub: Option<u32>,
}

impl RefCtx<'_> {
    fn place(&self, r: FwRef) -> Result<Place> {
        match r {
            FwRef::Name(h) => self.name(h),
            FwRef::Tail(h) => Err(err!(
                "{}: a tail reference is resolved with the tails",
                name_of_hash(self.reg, self.decls, h)
            )),
            FwRef::Stock(h) => match self.hook(h, "dk_stock")? {
                None => Ok(Place::Rel(0)),
                Some(b) => b.tail_off.map(Place::Rel).ok_or_else(|| {
                    err!(
                        "{}: dk_stock needs a hook without a handler (a raw hook has the stock path as its tail)",
                        b.hook
                    )
                }),
            },
            FwRef::Ret(h) => match self.hook(h, "dk_ret")? {
                None => Ok(Place::Rel(0)),
                Some(_) => self
                    .ret_stub
                    .map(Place::Rel)
                    .ok_or_else(|| err!("no rts stub was made for dk_ret")),
            },
            FwRef::Exit(h, n) => match self.hook(h, "dk_exit")? {
                None => Ok(Place::Abs(0)),
                Some(b) if n == 0 => Ok(Place::Abs(b.ret)),
                Some(b) => {
                    let exits = hook_exits(self.decls, self.device, &b.hook)?;
                    let text = exits.get(n as usize - 1).ok_or_else(|| {
                        err!(
                            "{}: dk_exit_{h:08x}_{n}: the hook declares {} exit(s) in `exits`",
                            b.hook,
                            exits.len()
                        )
                    })?;
                    Ok(Place::Abs(
                        eval_exit(text, self.view).map_err(|e| err!("{}: exit {n}: {e}", b.hook))?,
                    ))
                }
            },
        }
    }

    /// The hook of a reference: bound on this device (`Some`), or listed by the file for another device (`None`: its references resolve to 0). Any other
    /// hook is an error: the file refers to a hook it does not take.
    fn hook(&self, h: u32, what: &str) -> Result<Option<&BoundHook>> {
        if let Some(b) = self.bound.iter().find(|b| b.site_hash == h) {
            return Ok(Some(b));
        }
        if self.idle.contains(&h) {
            return Ok(None);
        }
        Err(err!(
            "{what}: the file refers to the hook {}, which its HOOKS do not take",
            name_of_hash(self.reg, self.decls, h)
        ))
    }

    /// `dk_fw_`: the anchor of this firmware with the name hash. A name that no firmware of this device has but another device has resolves to 0 (the code that
    /// uses it is never entered here); a name that none has, or that this device has on another firmware only, is an error.
    fn name(&self, h: u32) -> Result<Place> {
        fn on(p: &Port, h: u32) -> Option<&crate::port::Anchor> {
            p.anchor.iter().find(|a| name_hash(&a.name) == h)
        }
        if let Some(a) = on(self.port, h) {
            let at = self.view.addr(&a.name)?;
            return u32::try_from(at)
                .map(Place::Abs)
                .map_err(|_| err!("{}: address {at:#x} is not 32 bits", a.name));
        }
        let (mine, others) = (
            |p: &&Port| p.port.device == self.device,
            |p: &&Port| p.port.device != self.device,
        );
        if !self
            .reg
            .ports
            .iter()
            .filter(mine)
            .any(|p| on(p, h).is_some())
            && self
                .reg
                .ports
                .iter()
                .filter(others)
                .any(|p| on(p, h).is_some())
        {
            return Ok(Place::Abs(0));
        }
        Err(err!(
            "{}: no anchor of this name on {} {}",
            name_of_hash(self.reg, self.decls, h),
            self.port.port.device,
            self.port.port.version
        ))
    }
}

/// The identity of the stock firmware a bound file records: the first 8 bytes of the SHA-256 of its `.syx`.
fn fw_id_of(syx_sha256: &str) -> Result<[u8; 8]> {
    let b = unhex(syx_sha256)?;
    b.get(..8)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| err!("firmware hash {syx_sha256:?} is not a SHA-256"))
}

/// The header table of `reg` with the `BIND` list `binds` (site hash, address, trampoline offset, stock bytes) and, when `hooks` is given, that `HOOKS` list
/// (the hooks of this device); every other list is kept.
fn table_with_bind(
    reg: &[u8],
    binds: &[(u32, u32, u32, Vec<u8>)],
    hooks: Option<&[Pair]>,
    fw: &[[u8; 8]],
) -> Result<Vec<u8>> {
    let k = Keys::parse(reg).map_err(|e| err!("header table: {e:?}"))?;
    let needs: Vec<_> = k.needs().collect();
    let uses: Vec<_> = k.uses().collect();
    let kinds: Vec<_> = k.kinds().collect();
    let slots: Vec<_> = k.slots().collect();
    let hooks: Vec<Pair> = match hooks {
        Some(h) => h.to_vec(),
        None => k.hooks().collect(),
    };
    let exports: Vec<_> = k.exports().collect();
    let resources: Vec<_> = k.resources().collect();
    let provides: Vec<Pair> = k.provides().map(|(a, b)| Pair { a, b }).collect();
    let groups: Vec<(u32, Vec<u32>)> = k
        .key_groups()
        .map(|(kind, ks)| (kind, ks.collect()))
        .collect();
    let key_groups: Vec<KeyGroup> = groups
        .iter()
        .map(|(kind, keys)| KeyGroup { kind: *kind, keys })
        .collect();
    let bind: Vec<Bind> = binds
        .iter()
        .map(|(site, addr, tramp, stock)| Bind {
            site: *site,
            addr: *addr,
            tramp: *tramp,
            stock,
        })
        .collect();
    let t = Table {
        needs: &needs,
        uses: &uses,
        keys: &key_groups,
        kinds: &kinds,
        slots: &slots,
        hooks: &hooks,
        exports: &exports,
        resources: &resources,
        provides: &provides,
        bind: &bind,
        fw,
    };
    let mut out = vec![0u8; table_size(&t)];
    write_table(&t, &mut out).map_err(|e| err!("header table: {e:?}"))?;
    Ok(out)
}

/// The module's `.bss` starts at the end of the image, so the trampolines appended to the image (`image.len() - old` bytes) move it. Every
/// base-relative relocation whose stored word is an image offset at or past `old` addresses `.bss`: add the growth to it. A kind 1 word is
/// an addend to an import address, not an image offset: it stays. A word past `old + bss` is a malformed file. The entry points and the
/// `HOOKS` and `EXPORTS` offsets must be code offsets below `old`: they do not move.
fn shift_bss_words(image: &mut [u8], dkm: &Dkm, old: usize) -> Result<()> {
    let grow = image.len() - old;
    let end = old as u64 + dkm.bss as u64;
    for e in [dkm.init, dkm.fini, dkm.tick, dkm.menu]
        .into_iter()
        .flatten()
    {
        if e as usize >= old {
            return Err(err!("{}: entry point {e:#x} is not in the image", dkm.name));
        }
    }
    let keys = Keys::parse(&dkm.register).map_err(|e| err!("header table: {e:?}"))?;
    for p in keys.exports().chain(keys.hooks().map(|h| Pair {
        a: h.a,
        b: h.b & crate::dkm::HOOK_ENTRY_MASK,
    })) {
        if p.b as usize >= old {
            return Err(err!(
                "{}: table entry {:#x} is not in the image",
                dkm.name,
                p.b
            ));
        }
    }
    for (off, k) in &dkm.relocs {
        if k.is_some() {
            continue;
        }
        let at = *off as usize;
        let w = u32::from_be_bytes(image[at..at + 4].try_into().unwrap());
        if w as u64 > end {
            return Err(err!(
                "{}: relocated word at {at:#x} is {w:#x}, past the image and bss ({end:#x})",
                dkm.name
            ));
        }
        if w as usize >= old {
            image[at..at + 4].copy_from_slice(&(w + grow as u32).to_be_bytes());
        }
    }
    Ok(())
}

/// The name of the hook with `hash`, found as a word of the `devices/fw/` declaration texts; `None` when no declaration names it.
fn name_from_decls(decls: &[&str], hash: u32) -> Option<String> {
    decls.iter().find_map(|t| {
        t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .find(|w| !w.is_empty() && name_hash(w) == hash)
            .map(str::to_string)
    })
}

/// The hooks of `hooks` (the `HOOKS` list of the file lists those of every device) that this `device` binds. The top byte of an entry is its device mask
/// ([`crate::dkm::hook_device_mask`]; 0: every device): a hook for another device is skipped, and a hook for this device that the port (`port_hooks`) does not
/// declare is an error naming it (`decls` give the name of the hash). The pairs returned have the plain image offset.
fn select_hooks(
    module: &str,
    hooks: &[Pair],
    port_hooks: &[&str],
    decls: &[&str],
    device: &str,
) -> Result<Vec<Pair>> {
    let bit = crate::devdata::all()
        .iter()
        .position(|d| d.short == device)
        .map(|i| 1u32 << i)
        .ok_or_else(|| err!("{module}: unknown device {device}"))?;
    let mut out = vec![];
    for h in hooks {
        let mask = h.b >> 24;
        if mask != 0 && mask & bit == 0 {
            continue;
        }
        if find_hook(port_hooks.iter().copied(), h.a).is_err() {
            let name = name_from_decls(decls, h.a).unwrap_or_else(|| format!("#{:08x}", h.a));
            return Err(err!(
                "{module}: hook {name} is declared for {device} but the port of this firmware has no such hook"
            ));
        }
        out.push(Pair {
            a: h.a,
            b: h.b & crate::dkm::HOOK_ENTRY_MASK,
        });
    }
    Ok(out)
}

/// The address map of the packed service (`patchkit pack --addrmap`, offsets from the load address) for the bound file: the trampolines and tails that `bind`
/// appends move `.bss`, so every entry at or past the packed image length `old` moves by `grow` bytes. Entries below it are unchanged.
pub fn shift_addrmap(map: &str, old: usize, grow: usize) -> Result<String> {
    let mut v: serde_json::Value =
        serde_json::from_str(map).map_err(|e| err!("address map: {e}"))?;
    let entries = v
        .get_mut("entries")
        .and_then(|e| e.as_array_mut())
        .ok_or_else(|| err!("address map: no entries"))?;
    for e in entries {
        if e["cpu"] != "cf" {
            continue;
        }
        let addr = e["addr"]
            .as_u64()
            .ok_or_else(|| err!("address map: an entry has no addr"))?;
        if addr >= old as u64 {
            e["addr"] = serde_json::json!(addr + grow as u64);
        }
    }
    Ok(v.to_string())
}

/// Binds the service `dks` to the stock firmware `fw`: the bound file and one report per hook. The file is refused when it is no service, declares no
/// hook or more than [`BIND_MAX`], is bound already, or a hook is not declared for this firmware or cannot be copied to another place.
pub fn bind(
    dks: &[u8],
    fw: &[u8],
    reg: &Registry,
    decls: &[&str],
) -> Result<(Vec<u8>, Vec<BoundHook>)> {
    let dkm = Dkm::parse(dks)?;
    if dkm.role != ROLE_SERVICE {
        return Err(err!("{}: not a service (role {})", dkm.name, dkm.role));
    }
    let keys = Keys::parse(&dkm.register).map_err(|e| err!("header table: {e:?}"))?;
    let hooks: Vec<Pair> = keys.hooks().collect();
    let refs: Vec<FwRef> = dkm.imports.iter().filter_map(|n| parse_import(n)).collect();
    // a service that hooks no firmware site and refers to no firmware name (lfo) has nothing to bind: it is finished as built, and the firmware is not looked at
    if hooks.is_empty() && refs.is_empty() {
        return Ok((dks.to_vec(), Vec::new()));
    }
    if hooks.len() > BIND_MAX {
        return Err(err!(
            "{}: declares {} hooks, over the {BIND_MAX} a service binds",
            dkm.name,
            hooks.len()
        ));
    }
    if keys.bind_count() != 0 || keys.fw_count() != 0 {
        return Err(err!("{}: is already bound (it has BIND entries)", dkm.name));
    }
    let f = Firmware::parse(fw)?;
    let port = reg.find(&f.syx_sha256).ok_or_else(|| {
        err!(
            "no port for firmware {}: it is not a stock image patchkit knows",
            f.syx_sha256
        )
    })?;
    let device = f.device.short();
    let mut image = dkm.image.clone();
    let old = image.len();
    let mut relocs: Vec<(u32, u32)> = dkm
        .relocs
        .iter()
        .map(|(off, k)| (*off, k.map_or(0, |i| 1 << 24 | i as u32)))
        .collect();
    let mut bound = Vec::new();
    let on_device = if hooks.is_empty() {
        vec![]
    } else {
        select_hooks(
            &dkm.name,
            &hooks,
            &port
                .hook
                .iter()
                .map(|x| x.name.as_str())
                .collect::<Vec<_>>(),
            decls,
            device,
        )?
    };
    if on_device.is_empty() && !hooks.is_empty() {
        return Err(err!(
            "{}: none of its {} hooks is declared for {device} on this firmware",
            dkm.name,
            hooks.len()
        ));
    }
    // the hooks that need a stock path for a `dk_stock_` reference get a tail like the ones with `tail = true`
    let stock_for: Vec<u32> = refs
        .iter()
        .filter_map(|r| match r {
            FwRef::Stock(h) => Some(*h),
            _ => None,
        })
        .collect();
    let mut tail_declared: Vec<u32> = vec![];
    for h in &on_device {
        let name = find_hook(port.hook.iter().map(|x| x.name.as_str()), h.a)?;
        if h.b as usize >= dkm.image.len() {
            return Err(err!(
                "{name}: handler offset {:#x} is outside the image ({} bytes)",
                h.b,
                dkm.image.len()
            ));
        }
        let site = resolve_hook_site(fw, reg, name)?;
        if hook_has_handler(decls, device, name)? {
            let plan = handler_plan(decls, device, name)?;
            bound.push(bind_site(name, &site, &plan, h.b, &mut image, &mut relocs)?);
        } else {
            let tail = hook_has_tail(decls, device, name)?;
            if tail {
                tail_declared.push(h.a);
            }
            bound.push(bind_raw(
                name,
                &site,
                h.b,
                tail || stock_for.contains(&h.a),
                &mut image,
            )?);
        }
    }
    let tails: Vec<(&str, u32)> = bound
        .iter()
        .filter(|b| tail_declared.contains(&b.site_hash))
        .filter_map(|b| Some((b.hook.as_str(), b.tail_off?)))
        .collect();
    let idle: Vec<u32> = hooks
        .iter()
        .filter(|h| !on_device.iter().any(|d| d.a == h.a))
        .map(|h| h.a)
        .collect();
    // one `rts` serves every `dk_ret_` reference
    let ret_stub = if refs.iter().any(|r| matches!(r, FwRef::Ret(_))) {
        while image.len() % 4 != 0 {
            image.push(0);
        }
        let at = image.len() as u32;
        image.extend_from_slice(&[0x4e, 0x75, 0x4e, 0x71]);
        Some(at)
    } else {
        None
    };
    let after_tails = resolve_tail_refs(&mut image, &mut relocs, &dkm.imports, &tails, &idle)?;
    let imports_kept = if after_tails
        .iter()
        .any(|n| parse_import(n).is_some_and(|r| !matches!(r, FwRef::Tail(_))))
    {
        let view = AnchorView::new(&f, port)?;
        let ctx = RefCtx {
            reg,
            port,
            view: &view,
            decls,
            device,
            bound: &bound,
            idle: &idle,
            ret_stub,
        };
        resolve_fw_refs(&mut image, &mut relocs, &after_tails, &mut |r| ctx.place(r))?
    } else {
        after_tails
    };
    // the file holds a firmware address when it has a hook (BIND) or a reference that was resolved
    let resolved = dkm.imports.len() - imports_kept.len();
    let fw_items: Vec<[u8; 8]> = if !bound.is_empty() || resolved > 0 {
        vec![fw_id_of(&f.syx_sha256)?]
    } else {
        vec![]
    };
    shift_bss_words(&mut image, &dkm, old)?;
    relocs.sort();
    let items: Vec<(u32, u32, u32, Vec<u8>)> = bound
        .iter()
        .map(|b| (b.site_hash, b.site_addr, b.tramp_off, b.stock.clone()))
        .collect();
    let table = table_with_bind(&dkm.register, &items, Some(&on_device), &fw_items)?;
    let imports: Vec<&str> = imports_kept.iter().map(String::as_str).collect();
    let file = emit(&EmitParts {
        name: &dkm.name,
        version: dkm.version,
        role: dkm.role,
        runtime_word: dkm.runtime_min | if dkm.trace { FLAG_TRACE } else { 0 },
        svc_abi: dkm.svc_abi,
        image: &image,
        bss: dkm.bss,
        align: dkm.align,
        imports: &imports,
        relocs: &relocs,
        entries: [dkm.init, dkm.fini, dkm.tick, dkm.menu],
        table: &table,
        dsp: &dkm.dsp,
        max_file: max_file(),
    })?;
    Ok((file, bound))
}

/// The report of [`bind`] as JSON text: one object per hook.
pub fn bind_json(hooks: &[BoundHook]) -> String {
    let list: Vec<_> = hooks
        .iter()
        .map(|h| {
            serde_json::json!({
                "hook": h.hook,
                "site_hash": format!("{:#010x}", h.site_hash),
                "site_addr": format!("{:#010x}", h.site_addr),
                "site_len": h.site_len,
                "stock": hex(&h.stock),
                "tramp_off": h.tramp_off,
                "displaced": h.displaced,
                "ret": format!("{:#010x}", h.ret),
                "tail_off": h.tail_off,
            })
        })
        .collect();
    serde_json::to_string_pretty(&list).unwrap_or_default() + "\n"
}

/// `p` as an absolute path without `.` and `..` parts, made without touching the file system (a symbolic link is not followed).
fn lexical_abs(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut v = PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::ParentDir => {
                v.pop();
            }
            std::path::Component::CurDir => {}
            c => v.push(c),
        }
    }
    v
}

/// Whether the bound file may be written to `out`: a bound file holds firmware bytes and must never be committed, so a path in the source tree
/// `root` is refused unless it is under `<root>/dist/` or `<root>/out/` (both ignored by git). A path outside the tree is allowed.
pub fn output_allowed(root: &Path, out: &Path) -> bool {
    let (root, out) = (lexical_abs(root), lexical_abs(out));
    !out.starts_with(&root)
        || out.starts_with(root.join("dist"))
        || out.starts_with(root.join("out"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dkm::{Options, Provided, pack_keys};

    fn plan(save: &[&'static str], args: &[&str]) -> HandlerPlan {
        HandlerPlan {
            save: save.to_vec(),
            args: args
                .iter()
                .map(|a| HandlerArg::Reg(a.to_string()))
                .collect(),
        }
    }

    // Layout of the synthetic firmware: moveq #1,d0 (2 bytes) ; move.l #0x12345678,d1 (6 bytes) ; rts. A 6-byte jump covers the first two
    // instructions: an 8-byte site.
    const CODE: [u8; 12] = [
        0x70, 0x01, 0x22, 0x3c, 0x12, 0x34, 0x56, 0x78, 0x4e, 0x75, 0x4e, 0x71,
    ];
    const BASE: u64 = 0x4000_0000;

    fn site_of(code: &[u8], mode: &str) -> Result<HookSite> {
        let cf = prim::cf_site(code, BASE, BASE, BASE)?;
        Ok(HookSite {
            port_id: "t-1".into(),
            anchor: "a".into(),
            mode: mode.into(),
            site: BASE,
            site_len: cf.len,
            old: code[..cf.len].to_vec(),
            ret: BASE + cf.len as u64,
            cf,
        })
    }

    #[test]
    fn prologue_for_one_stack_free_argument() {
        // cf.menu.hook.settings_list.tail: args [a2], clobbers d0-d1 and a0-a1, so nothing is saved.
        //   move.l a2,-(sp)   = 0x2f00 | (An: 8 + 2)        = 2f0a
        //   jsr abs.l         = 4eb9 + 32-bit address (0)   = 4eb9 00000000   (the address word is at offset 2 + 2 = 4)
        //   addq.l #4,sp      = 0x508f | (4 & 7) << 9       = 588f
        let mut at = vec![];
        let p = encode_prologue(&plan(&[], &["a2"]), &mut at).unwrap();
        assert_eq!(hex(&p), "2f0a4eb900000000588f");
        assert_eq!(at, [4]);
    }

    #[test]
    fn prologue_saves_and_restores_the_registers() {
        // save d0, d1, a0, a1 (16 bytes), argument a2:
        //   lea -16(sp),sp          = 4fef fff0
        //   movem.l d0-d1/a0-a1,(sp) = 48d7 mask; bits d0=0, d1=1, a0=8, a1=9 = 0x0303
        //   move.l a2,-(sp)         = 2f0a
        //   jsr abs.l               = 4eb9 00000000   (address word at offset 12)
        //   addq.l #4,sp            = 588f
        //   movem.l (sp),d0-d1/a0-a1 = 4cd7 0303
        //   lea 16(sp),sp           = 4fef 0010
        let mut at = vec![];
        let p = encode_prologue(&plan(&["d0", "d1", "a0", "a1"], &["a2"]), &mut at).unwrap();
        assert_eq!(
            hex(&p),
            "4feffff048d703032f0a4eb900000000588f4cd703034fef0010"
        );
        assert_eq!(at, [12]);
    }

    #[test]
    fn stacked_arguments_are_pushed_last_first() {
        // args sp+4, sp+8, nothing saved: k = 0 pushes sp+8 as 8(sp), k = 1 pushes sp+4 as 4+4 = 8(sp) again (the first push moved sp by 4).
        //   move.l 8(sp),-(sp) = 2f2f 0008 ; move.l 8(sp),-(sp) = 2f2f 0008 ; jsr ; addq.l #8,sp = 508f | 0 = 508f
        let p = HandlerPlan {
            save: vec![],
            args: vec![HandlerArg::Stack(4), HandlerArg::Stack(8)],
        };
        let mut at = vec![];
        let b = encode_prologue(&p, &mut at).unwrap();
        assert_eq!(hex(&b), "2f2f00082f2f00084eb900000000508f");
        assert_eq!(at, [10]);
    }

    #[test]
    fn a_site_binds_with_its_stock_bytes_and_a_handler_relocation() {
        let site = site_of(&CODE, "before").unwrap();
        assert_eq!(site.site_len, 8);
        let mut image = vec![0x70, 0x00, 0x4e, 0x75, 0x4e, 0x71];
        let mut relocs = vec![];
        let h = bind_site(
            "x.hook",
            &site,
            &plan(&[], &["a2"]),
            0x24,
            &mut image,
            &mut relocs,
        )
        .unwrap();
        // the stock bytes are the site's; the trampoline starts after the image padded to 4
        assert_eq!(h.stock, CODE[..8]);
        assert_eq!(
            (h.tramp_off, h.site_addr, h.site_len, h.ret),
            (8, BASE as u32, 8, 0x4000_0008)
        );
        assert_eq!(h.site_hash, name_hash("x.hook"));
        assert_eq!(h.displaced.len(), 2);
        // prologue, the displaced instructions as they were, jmp.l ret
        let mut want = vec![0x2f, 0x0a, 0x4e, 0xb9, 0, 0, 0, 0x24, 0x58, 0x8f];
        want.extend_from_slice(&CODE[..8]);
        want.extend_from_slice(&[0x4e, 0xf9, 0x40, 0x00, 0x00, 0x08]);
        assert_eq!(&image[..6], [0x70, 0x00, 0x4e, 0x75, 0x4e, 0x71]);
        assert_eq!(&image[6..8], [0, 0]);
        assert_eq!(&image[8..], want);
        // the handler word is relocated base-relative
        assert_eq!(relocs, [(12, 0)]);
    }

    #[test]
    fn a_site_over_the_stock_limit_is_refused() {
        let mut site = site_of(&CODE, "before").unwrap();
        site.old = vec![0; BIND_STOCK_MAX];
        let mut image = vec![0; 4];
        assert!(bind_site("x.hook", &site, &plan(&[], &[]), 0, &mut image, &mut vec![]).is_ok());
        site.old = vec![0; BIND_STOCK_MAX + 1];
        let e =
            bind_site("x.hook", &site, &plan(&[], &[]), 0, &mut image, &mut vec![]).unwrap_err();
        assert!(e.0.contains("over the 64"), "{e}");
    }

    fn synth_service(image: Vec<u8>, relocs: Vec<(u32, Option<usize>)>, bss: u32) -> Dkm {
        let t = Table::default();
        let mut table = vec![0u8; table_size(&t)];
        write_table(&t, &mut table).unwrap();
        Dkm {
            name: "svc".into(),
            version: 0x010000,
            runtime_min: 0,
            trace: false,
            role: ROLE_SERVICE,
            svc_abi: 0,
            sha256: String::new(),
            image,
            bss,
            align: 4,
            relocs,
            imports: vec![],
            init: Some(0),
            fini: None,
            menu: None,
            tick: None,
            register: table,
            dsp: vec![],
        }
    }

    #[test]
    fn bss_references_move_with_the_appended_trampolines() {
        // image of 8 bytes: word at 0 addresses the image (2), word at 4 addresses bss + 0 (= old = 8)
        let mut image = vec![0, 0, 0, 2, 0, 0, 0, 8];
        let old = image.len();
        let dkm = synth_service(image.clone(), vec![(0, None), (4, None)], 16);
        let site = site_of(&CODE, "before").unwrap();
        let mut relocs = vec![];
        let h = bind_site(
            "x.hook",
            &site,
            &plan(&[], &["a2"]),
            0,
            &mut image,
            &mut relocs,
        )
        .unwrap();
        let grow = image.len() - old;
        assert_eq!(h.tramp_off as usize, old);
        assert_eq!(grow, 10 + 8 + 6);
        shift_bss_words(&mut image, &dkm, old).unwrap();
        let word = |at: usize| u32::from_be_bytes(image[at..at + 4].try_into().unwrap());
        assert_eq!(word(4), (old + grow) as u32);
        assert_eq!(word(0), 2);
        assert_eq!(image.len(), old + grow);
        assert_eq!(dkm.bss, 16);
        assert_eq!(&image[old..old + 2], [0x2f, 0x0a]);
    }

    #[test]
    fn a_word_past_image_and_bss_is_refused() {
        let mut image = vec![0, 0, 0, 0x19, 0, 0, 0, 0];
        let old = image.len();
        // old 8 + bss 16 = 24 (0x18): 0x19 is out of range
        let dkm = synth_service(image.clone(), vec![(0, None)], 16);
        image.extend_from_slice(&[0; 4]);
        let e = shift_bss_words(&mut image, &dkm, old).unwrap_err();
        assert!(e.0.contains("past the image and bss"), "{e}");
    }

    #[test]
    fn a_pc_relative_displaced_instruction_is_refused() {
        // bne.w (4 bytes) ; nop ; nop ; nop: the branch becomes the 32-bit form whose displacement depends on where it runs
        let code = [0x66, 0x00, 0x00, 0x20, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71];
        let site = site_of(&code, "before").unwrap();
        let mut image = vec![0; 4];
        let e =
            bind_site("x.hook", &site, &plan(&[], &[]), 0, &mut image, &mut vec![]).unwrap_err();
        assert!(
            e.0.contains("PC-relative displaced instruction at x.hook"),
            "{e}"
        );
        // an abs.l form of the same kind of operand is position independent: lea (0x10,pc),a0 ; nop
        let code = [0x41, 0xfa, 0x00, 0x10, 0x4e, 0x71, 0x4e, 0x71];
        let site = site_of(&code, "before").unwrap();
        assert!(bind_site("x.hook", &site, &plan(&[], &[]), 0, &mut image, &mut vec![]).is_ok());
    }

    #[test]
    fn an_unknown_hook_hash_is_refused() {
        let names = ["cf.menu.hook.a", "cf.menu.hook.b"];
        assert_eq!(
            find_hook(names.into_iter(), name_hash("cf.menu.hook.b")).unwrap(),
            "cf.menu.hook.b"
        );
        let e = find_hook(names.into_iter(), name_hash("cf.menu.hook.c")).unwrap_err();
        assert!(e.0.contains("no hook declared"), "{e}");
    }

    fn service_file(hooks: &[Pair], binds: &[Bind]) -> Vec<u8> {
        let t = Table {
            hooks,
            bind: binds,
            ..Table::default()
        };
        let mut table = vec![0u8; table_size(&t)];
        write_table(&t, &mut table).unwrap();
        let o = Options {
            name: "svc".into(),
            version: 0x010000,
            trace: false,
            stereo: false,
            gate: false,
            service: Some(Provided {
                service: "svc-test".into(),
                exports: vec![],
                hooks: vec![],
                fills: vec![],
            }),
        };
        pack_keys(None, &table, &[], &o).unwrap()
    }

    #[test]
    fn a_bound_file_is_refused() {
        let stock = [0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71];
        let h = name_hash("cf.menu.hook.settings_list.tail");
        let hooks = [Pair { a: h, b: 0 }];
        let binds = [Bind {
            site: h,
            addr: 0x4000_0000,
            tramp: 0,
            stock: &stock,
        }];
        let dks = service_file(&hooks, &binds);
        // refused before the firmware is looked at
        let e = bind(&dks, &[], &Registry::default(), &[]).unwrap_err();
        assert!(e.0.contains("already bound"), "{e}");
    }

    #[test]
    fn more_than_24_hooks_are_refused() {
        let hooks: Vec<Pair> = (0..BIND_MAX as u32 + 1).map(|a| Pair { a, b: 0 }).collect();
        let dks = service_file(&hooks, &[]);
        let e = bind(&dks, &[], &Registry::default(), &[]).unwrap_err();
        assert!(e.0.contains("over the 24"), "{e}");
        let dks = service_file(&hooks[..BIND_MAX], &[]);
        // 24 pass the count check and fail on the firmware
        let e = bind(&dks, &[], &Registry::default(), &[]).unwrap_err();
        assert!(!e.0.contains("over the 24"), "{e}");
    }

    #[test]
    fn a_service_with_no_hooks_is_returned_unchanged() {
        let dks = service_file(&[], &[]);
        let (file, hooks) = bind(&dks, &[], &Registry::default(), &[]).unwrap();
        assert_eq!(file, dks, "bound = built");
        assert!(hooks.is_empty());
        assert_eq!(bind_json(&hooks).trim(), "[]");
    }

    #[test]
    fn bind_items_round_trip_through_the_table() {
        let h = name_hash("cf.menu.hook.settings_list.tail");
        let hooks = [Pair { a: h, b: 0x20 }];
        let dks = service_file(&hooks, &[]);
        let dkm = Dkm::parse(&dks).unwrap();
        // a 6-byte stock is padded to 8 in the table; the item reports 6
        let stock = vec![1, 2, 3, 4, 5, 6];
        let items = [
            (h, 0x400a_5eac, 0x30, stock.clone()),
            (h ^ 1, 0x400a_6000, 0x58, vec![9; BIND_STOCK_MAX]),
        ];
        let t = table_with_bind(&dkm.register, &items, None, &[]).unwrap();
        let k = Keys::parse(&t).unwrap();
        assert_eq!(k.bind_count(), 2);
        let b = k.bind(0).unwrap();
        assert_eq!(
            (b.site, b.addr, b.len, b.tramp, b.stock),
            (h, 0x400a_5eac, 6, 0x30, &stock[..])
        );
        let b = k.bind(1).unwrap();
        assert_eq!(
            (b.site, b.addr, b.len, b.tramp, b.stock.len()),
            (h ^ 1, 0x400a_6000, 64, 0x58, 64)
        );
        assert!(k.bind(2).is_none());
        // the other lists stay
        assert!(k.hooks().eq(hooks.iter().copied()));
        assert!(
            k.provides()
                .eq(Keys::parse(&dkm.register).unwrap().provides())
        );
    }

    #[test]
    fn a_raw_hook_with_a_tail_binds_to_the_stock_bytes_of_the_image() {
        let site = site_of(&CODE, "replace").unwrap();
        let mut image = vec![0x70, 0x00, 0x4e, 0x75, 0x4e, 0x71];
        let h = bind_raw("x.hook", &site, 0x24, true, &mut image).unwrap();
        // the entry stays the BIND trampoline; the tail is the displaced instructions as they were, then jmp.l ret
        assert_eq!((h.tramp_off, h.tail_off), (0x24, Some(8)));
        let mut want = CODE[..8].to_vec();
        want.extend_from_slice(&[0x4e, 0xf9, 0x40, 0x00, 0x00, 0x08]);
        assert_eq!(&image[8..], want);
        assert_eq!(h.displaced.len(), 2);
        // without a tail exit nothing is appended
        let mut image = vec![0; 4];
        let h = bind_raw("x.hook", &site, 0x24, false, &mut image).unwrap();
        assert_eq!((image.len(), h.tail_off), (4, None));
        // a PC-relative displaced instruction cannot be copied
        let code = [0x66, 0x00, 0x00, 0x20, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71];
        let site = site_of(&code, "replace").unwrap();
        let e = bind_raw("x.hook", &site, 0, true, &mut vec![0; 4]).unwrap_err();
        assert!(
            e.0.contains("PC-relative displaced instruction at x.hook"),
            "{e}"
        );
        // a hook that is not "replace" is not entered directly
        let site = site_of(&CODE, "before").unwrap();
        assert!(bind_raw("x.hook", &site, 0, false, &mut vec![]).is_err());
    }

    #[test]
    fn tail_references_become_base_relative_relocations() {
        let (a, b) = ("x.hook.a", "x.hook.b");
        assert_eq!(tail_import(a).len(), 16);
        assert!(
            is_tail_import(&tail_import(a))
                && !is_tail_import("dk_tail_xyz")
                && !is_tail_import("dk_log")
        );
        // imports: the log function, the tail of a, the tail of b; words at 0 (log), 4 (a, addend 2), 8 (b), 12 (a)
        let imports = vec!["dk_log".to_string(), tail_import(a), tail_import(b)];
        let mut image = vec![0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut relocs = vec![
            (0, 1 << 24),
            (4, 1 << 24 | 1),
            (8, 1 << 24 | 2),
            (12, 1 << 24 | 1),
        ];
        let keep = resolve_tail_refs(
            &mut image,
            &mut relocs,
            &imports,
            &[(a, 0x40), (b, 0x60)],
            &[],
        )
        .unwrap();
        assert_eq!(keep, ["dk_log"]);
        assert_eq!(relocs, [(0, 1 << 24), (4, 0), (8, 0), (12, 0)]);
        let word = |at: usize| u32::from_be_bytes(image[at..at + 4].try_into().unwrap());
        assert_eq!((word(0), word(4), word(8), word(12)), (0, 0x42, 0x60, 0x40));
        // a tail reference of a hook that has no tail exit is refused
        let e =
            resolve_tail_refs(&mut image, &mut relocs, &imports, &[(a, 0x40)], &[]).unwrap_err();
        assert!(e.0.contains("no hook of its HOOKS declares"), "{e}");
        // the tail of a hook that is listed for another device resolves to offset 0 (never entered here)
        let mut image = vec![0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut relocs = vec![
            (0, 1 << 24),
            (4, 1 << 24 | 1),
            (8, 1 << 24 | 2),
            (12, 1 << 24 | 1),
        ];
        resolve_tail_refs(
            &mut image,
            &mut relocs,
            &imports,
            &[(a, 0x40)],
            &[name_hash(b)],
        )
        .unwrap();
        assert_eq!(relocs, [(0, 1 << 24), (4, 0), (8, 0), (12, 0)]);
        assert_eq!(u32::from_be_bytes(image[8..12].try_into().unwrap()), 0);
    }

    #[test]
    fn import_names_round_trip_and_fit_the_import_limit() {
        let (n, hook) = ("cf.draw.fn.blit", "cf.page.hook.source_draw");
        let h = name_hash(hook);
        for (name, want) in [
            (fw_import(n), FwRef::Name(name_hash(n))),
            (tail_import(hook), FwRef::Tail(h)),
            (stock_import(hook), FwRef::Stock(h)),
            (ret_import(hook), FwRef::Ret(h)),
            (exit_import(hook, 0), FwRef::Exit(h, 0)),
            (exit_import(hook, 12), FwRef::Exit(h, 12)),
        ] {
            assert!(name.len() <= 19, "{name}");
            assert_eq!(parse_import(&name), Some(want), "{name}");
        }
        for bad in [
            "dk_log",
            "dk_fw_xyz",
            "dk_exit_00000000",
            "dk_exit_00000000_",
            "dk_exit_00000000_123",
            "dk_ret_ABCDEF01",
        ] {
            assert_eq!(parse_import(bad), None, "{bad}");
        }
    }

    #[test]
    fn extern_targets_map_to_the_import_families() {
        let hook = "cf.page.hook.source_draw";
        let t = |x: &str| import_for_target(x);
        assert_eq!(t("cf.draw.fn.blit"), Some(fw_import("cf.draw.fn.blit")));
        assert_eq!(t(&format!("hook.{hook}.tail")), Some(tail_import(hook)));
        assert_eq!(t(&format!("hook.{hook}.stock")), Some(stock_import(hook)));
        assert_eq!(t(&format!("hook.{hook}.ret")), Some(ret_import(hook)));
        assert_eq!(t(&format!("hook.{hook}.exit")), Some(exit_import(hook, 0)));
        assert_eq!(
            t(&format!("hook.{hook}.exit.3")),
            Some(exit_import(hook, 3))
        );
        assert_eq!(t(&format!("hook.{hook}.exit.0")), None);
        assert_eq!(t(&format!("hook.{hook}.nope")), None);
        assert_eq!(t("machines.first"), None);
    }

    /// Words at 0 (addend 2) and 4 and 8, each against another import; import 0 is a runtime function that stays.
    fn refs_fixture() -> (Vec<u8>, Vec<(u32, u32)>, Vec<String>) {
        let imports = vec![
            "dk_log".to_string(),
            fw_import("cf.a"),
            exit_import("h", 0),
            ret_import("h"),
        ];
        let image = vec![0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        let relocs = vec![
            (0, 1 << 24 | 1),
            (4, 1 << 24 | 2),
            (8, 1 << 24 | 3),
            (12, 1 << 24),
        ];
        (image, relocs, imports)
    }

    #[test]
    fn firmware_names_and_exits_are_absolute_words_and_rets_are_base_relative() {
        let (mut image, mut relocs, imports) = refs_fixture();
        let keep = resolve_fw_refs(&mut image, &mut relocs, &imports, &mut |r| {
            Ok(match r {
                FwRef::Name(_) => Place::Abs(0x4000_1000),
                FwRef::Exit(..) => Place::Abs(0x4000_2000),
                _ => Place::Rel(0x40),
            })
        })
        .unwrap();
        assert_eq!(keep, ["dk_log"]);
        let word = |at: usize| u32::from_be_bytes(image[at..at + 4].try_into().unwrap());
        // the addend of the reference stays: 0x4000_1000 + 2; an absolute word has no relocation entry; the rts stub is base-relative (kind 0)
        assert_eq!(
            (word(0), word(4), word(8)),
            (0x4000_1002, 0x4000_2000, 0x41)
        );
        assert_eq!(relocs, [(8, 0), (12, 1 << 24)]);
    }

    #[test]
    fn a_name_that_does_not_resolve_is_an_error_naming_it_and_another_device_resolves_to_zero() {
        let (mut image, mut relocs, imports) = refs_fixture();
        let e = resolve_fw_refs(&mut image, &mut relocs, &imports, &mut |r| match r {
            FwRef::Name(_) => Err(err!("cf.a: no anchor of this name on dn2 1.12")),
            _ => Ok(Place::Abs(0)),
        })
        .unwrap_err();
        assert!(e.0.contains("cf.a"), "{e}");
        // an idle hook or a name for another device is Abs(0)/Rel(0): the word is the addend only
        let (mut image, mut relocs, imports) = refs_fixture();
        resolve_fw_refs(&mut image, &mut relocs, &imports, &mut |r| {
            Ok(if matches!(r, FwRef::Ret(_)) {
                Place::Rel(0)
            } else {
                Place::Abs(0)
            })
        })
        .unwrap();
        assert_eq!(u32::from_be_bytes(image[0..4].try_into().unwrap()), 2);
        assert_eq!(relocs, [(8, 0), (12, 1 << 24)]);
    }

    #[test]
    fn the_fw_item_is_written_only_when_given() {
        let dks = service_file(&[], &[]);
        let dkm = Dkm::parse(&dks).unwrap();
        let id = [1, 2, 3, 4, 5, 6, 7, 8];
        let t = table_with_bind(&dkm.register, &[], None, &[]).unwrap();
        assert_eq!(Keys::parse(&t).unwrap().fw_count(), 0);
        let t = table_with_bind(&dkm.register, &[], None, &[id]).unwrap();
        let k = Keys::parse(&t).unwrap();
        assert_eq!((k.fw_count(), k.fw(0)), (1, Some(id)));
        assert_eq!(
            fw_id_of(&"0123456789abcdef".repeat(4)).unwrap(),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
    }

    #[test]
    fn the_output_is_refused_in_the_source_tree_outside_dist_and_out() {
        let root = Path::new("/src/digikit");
        assert!(output_allowed(root, Path::new("/src/digikit/dist/svc.dks")));
        assert!(output_allowed(
            root,
            Path::new("/src/digikit/out/a/svc.dks")
        ));
        assert!(output_allowed(root, Path::new("/tmp/svc.dks")));
        assert!(!output_allowed(root, Path::new("/src/digikit/svc.dks")));
        assert!(!output_allowed(
            root,
            Path::new("/src/digikit/modules/svc.dks")
        ));
        assert!(!output_allowed(
            root,
            Path::new("/src/digikit/distant/svc.dks")
        ));
        // `..` does not get around the rule
        assert!(!output_allowed(
            root,
            Path::new("/src/digikit/dist/../modules/svc.dks")
        ));
        assert!(!output_allowed(
            root,
            Path::new("/src/digikit/dist/../../digikit/x")
        ));
    }

    #[test]
    fn hooks_of_other_devices_are_skipped_and_missing_ones_refused() {
        use crate::dkm::hook_device_mask;
        let mask = |d: &str| hook_device_mask(&[d.to_string()]).unwrap();
        let pair = |n: &str, d: &str| Pair {
            a: name_hash(n),
            b: 0x40 | mask(d) << 24,
        };
        let hooks = [pair("h.dt2", "dt2"), pair("h.dn2", "dn2")];
        // dn2's port lacks the dt2 hook: skipped, and the pair has the plain offset
        let got = select_hooks("m", &hooks, &["h.dn2"], &[], "dn2").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].a, got[0].b), (name_hash("h.dn2"), 0x40));
        // dn2 declares h.dn2 for itself but the port lacks it: refused, naming the hook (from the declaration texts)
        let e = select_hooks("m", &hooks, &[], &["hook h.dn2 {}"], "dn2").unwrap_err();
        assert!(e.0.contains("h.dn2") && e.0.contains("no such hook"), "{e}");
        // mask 0 is every device
        let all = [Pair {
            a: name_hash("h.x"),
            b: 8,
        }];
        assert!(select_hooks("m", &all, &[], &[], "dt2").is_err());
        assert_eq!(
            select_hooks("m", &all, &["h.x"], &[], "dn2").unwrap().len(),
            1
        );
    }

    #[test]
    fn bound_address_map_moves_bss_entries_only() {
        let map = r#"{"entries":[{"cpu":"cf","addr":4,"name":"a"},{"cpu":"cf","addr":100,"name":"b"},{"cpu":"sharc","addr":100,"name":"c"}]}"#;
        let v: serde_json::Value =
            serde_json::from_str(&shift_addrmap(map, 100, 24).unwrap()).unwrap();
        let addrs: Vec<u64> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["addr"].as_u64().unwrap())
            .collect();
        assert_eq!(addrs, [4, 124, 100]);
    }
}
