//! The names by which a service file refers to the firmware (docs/plans/port-file.md). A service is packed without firmware: it names anchors, hook exits, tails and
//! stock paths through imports (`dk_fw_`, `dk_exit_`, `dk_tail_`, `dk_stock_`, `dk_ret_`), and the runtime resolves them from the port file of the firmware it runs
//! on. This module makes and reads those names; `patchkit pack` writes them, `patchkit port` checks them against a port file.

use crate::error::*;
use dkneeds::keys::name_hash;
use std::collections::BTreeMap;

/// The name of the undefined symbol with which the asm of a raw hook refers to the tail of `hook` (an exit `tail = true` in `devices/fw/`):
/// `dk_tail_` and the `name_hash` of the hook name in 8 hex digits (an import name has at most 31 characters). `patchkit pack` keeps it as an import
/// and the runtime resolves it from the port file, so a `.dks` holds no firmware bytes.
pub fn tail_import(hook: &str) -> String {
    format!("dk_tail_{:08x}", name_hash(hook))
}

/// Is `import` the tail reference of some hook (a name `tail_import` makes)?
pub fn is_tail_import(import: &str) -> bool {
    import
        .strip_prefix("dk_tail_")
        .is_some_and(|h| h.len() == 8 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')))
}

/// A reference of a service to the firmware, as an import name the runtime resolves from the port file. Every name carries the 8 hex digits of a `name_hash` and fits the 31
/// characters of an import name (the longest, `dk_exit_<hash>_<n>` with a two-digit `n`, has 19). `patchkit pack` makes the names from the `[object.externs]` of a
/// service module and from the firmware names (`cf.*`) that a Rust service takes from the `dkfw` crate.
///
/// | import | resolves to | |
/// | --- | --- | --- |
/// | `dk_fw_<H(name)>` | the address of the firmware anchor `name` (an offset is the addend of the reference) | |
/// | `dk_exit_<H(hook)>_<n>` | `n` = 0: the end of the hook's site; `n` from 1: the expression `exits[n-1]` of the hook in `devices/fw` | |
/// | `dk_tail_<H(hook)>` | the generated tail of the hook (its displaced instructions, then a jump behind the site) | |
/// | `dk_stock_<H(hook)>` | the same code, called with `jsr` from inside the hook; control comes back | |
/// | `dk_ret_<H(hook)>` | one `rts` (the first word of the port file's image) | |
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

/// The value of the exit expression `text` with `addr` giving the address of each anchor it names (`patchkit port build` takes them from the committed port).
pub(crate) fn eval_exit_with(text: &str, addr: &dyn Fn(&str) -> Result<u64>) -> Result<u32> {
    let expr = crate::expr::parse(text)?;
    let mut names = vec![];
    expr.names(&mut names);
    let mut known: Vec<(String, i64)> = vec![];
    for n in names {
        known.push((n.clone(), addr(&n)? as i64));
    }
    let env = ExitEnv(known.iter().map(|(n, v)| (n.as_str(), *v)).collect());
    let v = expr.eval(&env)?;
    u32::try_from(v).map_err(|_| err!("exit {text:?} is {v:#x}, not a 32-bit address"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
