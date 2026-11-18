//! The difference of two patched images as the running ColdFire sees it, for a live swap of module code (`dev.reload` of the
//! control daemon).
//!
//! A patched MAIN section is the stock MAIN followed by a chain of segments (`len`, `dest`, `bss` as big-endian words, then `len`
//! payload bytes; the loader copies the payload to `dest` and zeroes `bss` bytes after it) that ends with `0xFFFF_FFFF`. [`View`]
//! is that image as memory: the stock bytes at the section's destination plus each segment's payload and bss. The plan of the
//! newer image says which module owns which range, so every changed range gets an owner.
//!
//! Only ColdFire memory is compared byte by byte. A different DSP section is reported as a flag: the caller decides (a hot DSP
//! injection or a cold boot). The patch record (`kind = "record"`) is metadata of the tool and is left out.

use crate::apply::{CF, Plan, PlanWrite};
use crate::error::*;
use crate::firmware::Firmware;
use serde::Serialize;
use std::collections::BTreeMap;

/// One range to write into a running ColdFire to turn the older image's memory into the newer one's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Change {
    pub addr: u64,
    pub bytes: Vec<u8>,
    /// The module id (without version) that owns the range in the newer plan; empty when no plan write covers it.
    pub owner: String,
    /// The bytes differ from the older image. `false`: the range is only rewritten because another range of its module
    /// differs (the module is replaced as a whole, its data and bss start from the image again).
    pub differs: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub changes: Vec<Change>,
    /// The DSP section (7) differs.
    pub dsp_changed: bool,
    /// Sections other than MAIN (3) and DSP (7) that differ.
    pub other_sections: Vec<u32>,
}

impl Diff {
    /// Nothing differs: the images behave the same.
    pub fn is_empty(&self) -> bool {
        !self.dsp_changed
            && self.other_sections.is_empty()
            && !self.changes.iter().any(|c| c.differs)
    }

    /// The module ids that own a range that differs (an empty id: bytes no plan write covers).
    pub fn owners(&self) -> Vec<String> {
        let mut o: Vec<String> = self
            .changes
            .iter()
            .filter(|c| c.differs)
            .map(|c| c.owner.clone())
            .collect();
        o.sort();
        o.dedup();
        o
    }
}

/// The module id of a plan tag (`id@version`).
pub fn owner_id(tag: &str) -> &str {
    tag.split('@').next().unwrap_or(tag)
}

/// ColdFire memory of a patched MAIN section as the loader leaves it: runs of (start address, bytes).
#[derive(Clone, Debug)]
pub struct View {
    runs: Vec<(u64, Vec<u8>)>,
}

const MAX_BSS: u64 = 64 << 20;

impl View {
    /// `main`: the decoded MAIN section, loaded at `base`; `stock_len`: the length of the stock MAIN, where the segment chain starts
    /// (the chain is absent when `main` is no longer than that).
    pub fn of(main: &[u8], base: u64, stock_len: usize) -> Result<View> {
        if stock_len > main.len() {
            return Err(err!(
                "the stock MAIN ({stock_len} bytes) is longer than this one ({})",
                main.len()
            ));
        }
        let mut runs = vec![(base, main[..stock_len].to_vec())];
        let word = |at: usize| -> Result<u32> {
            main.get(at..at + 4)
                .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
                .ok_or_else(|| err!("the segment chain is cut off at {at:#x}"))
        };
        let mut at = stock_len;
        while at < main.len() {
            let len = word(at)?;
            if len == u32::MAX {
                break;
            }
            let (dest, bss) = (word(at + 4)?, word(at + 8)?);
            let payload = main
                .get(at + 12..at + 12 + len as usize)
                .ok_or_else(|| err!("a segment payload is cut off at {at:#x}"))?;
            if u64::from(bss) > MAX_BSS {
                return Err(err!("a segment has {bss} bytes of bss"));
            }
            runs.push((u64::from(dest), payload.to_vec()));
            if bss != 0 {
                runs.push((u64::from(dest) + u64::from(len), vec![0; bss as usize]));
            }
            at += 12 + len as usize;
        }
        Ok(View { runs })
    }

    fn run_at(&self, addr: u64) -> Option<&(u64, Vec<u8>)> {
        self.runs
            .iter()
            .find(|(s, b)| *s <= addr && addr < *s + b.len() as u64)
    }

    /// `len` bytes from `addr`; `None` when any byte is outside the image.
    pub fn read(&self, addr: u64, len: u64) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(len as usize);
        let mut a = addr;
        while a < addr + len {
            let (s, b) = self.run_at(a)?;
            let from = (a - s) as usize;
            let n = (b.len() - from).min((addr + len - a) as usize);
            out.extend_from_slice(&b[from..from + n]);
            a += n as u64;
        }
        Some(out)
    }

    /// The ranges `[start, end)` of `self` that `old` does not hold with the same bytes (an address `old` lacks counts as different).
    fn differences(&self, old: &View) -> Vec<(u64, u64)> {
        let mut out: Vec<(u64, u64)> = vec![];
        let mut push = |s: u64, e: u64| match out.last_mut() {
            Some(last) if last.1 == s => last.1 = e,
            _ => out.push((s, e)),
        };
        for (start, bytes) in &self.runs {
            let end = start + bytes.len() as u64;
            let mut a = *start;
            while a < end {
                match old.run_at(a) {
                    Some((os, ob)) => {
                        let n = (os + ob.len() as u64).min(end) - a;
                        let mine = &bytes[(a - start) as usize..][..n as usize];
                        let theirs = &ob[(a - os) as usize..][..n as usize];
                        let mut i = 0;
                        while i < mine.len() {
                            if mine[i] == theirs[i] {
                                i += 1;
                                continue;
                            }
                            let j = (i..mine.len())
                                .find(|&j| mine[j] == theirs[j])
                                .unwrap_or(mine.len());
                            push(a + i as u64, a + j as u64);
                            i = j;
                        }
                        a += n;
                    }
                    None => {
                        let next = old
                            .runs
                            .iter()
                            .map(|(s, _)| *s)
                            .filter(|s| *s > a)
                            .min()
                            .unwrap_or(end)
                            .min(end);
                        push(a, next);
                        a = next;
                    }
                }
            }
        }
        out
    }
}

/// The parts of a `.syx` the comparison needs.
struct Parts {
    main: Vec<u8>,
    base: u64,
    dsp: Vec<u8>,
    others: BTreeMap<u32, Vec<u8>>,
}

fn parts(syx: &[u8]) -> Result<Parts> {
    let fw = Firmware::parse(syx)?;
    let main = fw
        .main()
        .ok_or_else(|| err!("the image has no MAIN section"))?;
    // empty on a device without a DSP
    let dsp = match fw.device.dsp_section() {
        Some(_) => fw
            .dsp()
            .ok_or_else(|| err!("the image has no DSP section"))?
            .data
            .clone(),
        None => vec![],
    };
    let (main_id, dsp_id) = (fw.device.main_section(), fw.device.dsp_section());
    Ok(Parts {
        main: main.data.clone(),
        base: u64::from(main.dest),
        dsp,
        others: fw
            .sections
            .iter()
            .filter(|s| s.id != main_id && Some(s.id) != dsp_id)
            .map(|s| (s.id, s.data.clone()))
            .collect(),
    })
}

/// What turns the memory of `old` into the memory of `new`, both patched from the same stock image whose MAIN is `stock_main_len`
/// bytes long; `plan` is the newer image's.
pub fn diff(old: &[u8], new: &[u8], stock_main_len: usize, plan: &Plan) -> Result<Diff> {
    let (o, n) = (parts(old)?, parts(new)?);
    if o.base != n.base {
        return Err(err!("the images load MAIN at different addresses"));
    }
    let mut out = diff_memory(
        &View::of(&o.main, o.base, stock_main_len)?,
        &View::of(&n.main, n.base, stock_main_len)?,
        &plan.writes,
    )?;
    out.dsp_changed = o.dsp != n.dsp;
    out.other_sections = (o.others.keys().chain(n.others.keys()))
        .copied()
        .filter(|id| o.others.get(id) != n.others.get(id))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(out)
}

/// The ColdFire part of [`diff`], on two memory views and the newer plan's writes.
pub fn diff_memory(old: &View, new: &View, writes: &[PlanWrite]) -> Result<Diff> {
    let cf: Vec<&PlanWrite> = writes
        .iter()
        .filter(|w| w.cpu == CF && w.len != 0)
        .collect();
    // Which plan writes differ.
    let mut differs = vec![false; cf.len()];
    let mut bytes: Vec<Vec<u8>> = vec![];
    for (i, w) in cf.iter().enumerate() {
        if w.kind == "record" {
            bytes.push(vec![]);
            continue;
        }
        let now = new.read(w.addr, w.len).ok_or_else(|| {
            err!(
                "{} [{:#x}+{}]: not in the new image",
                w.owner,
                w.addr,
                w.len
            )
        })?;
        differs[i] = old.read(w.addr, w.len).as_ref() != Some(&now);
        bytes.push(now);
    }
    let dirty: std::collections::BTreeSet<&str> = cf
        .iter()
        .zip(&differs)
        .filter(|(_, d)| **d)
        .map(|(w, _)| owner_id(&w.owner))
        .collect();
    let mut changes = vec![];
    for (i, w) in cf.iter().enumerate() {
        let owner = owner_id(&w.owner);
        let whole = w.kind == "place" && dirty.contains(owner);
        if differs[i] || whole {
            changes.push(Change {
                addr: w.addr,
                bytes: bytes[i].clone(),
                owner: owner.to_string(),
                differs: differs[i],
            });
        }
    }
    // Bytes that differ and no plan write covers (the plan lacks them): changes without an owner.
    let mut covered: Vec<(u64, u64)> = cf.iter().map(|w| (w.addr, w.addr + w.len)).collect();
    covered.sort();
    for (s, e) in new.differences(old) {
        let mut a = s;
        for (cs, ce) in &covered {
            if *ce <= a || *cs >= e {
                continue;
            }
            if *cs > a {
                push_unowned(&mut changes, new, a, *cs)?;
            }
            a = a.max(*ce);
            if a >= e {
                break;
            }
        }
        if a < e {
            push_unowned(&mut changes, new, a, e)?;
        }
    }
    changes.sort_by_key(|c| c.addr);
    Ok(Diff {
        changes,
        ..Diff::default()
    })
}

fn push_unowned(changes: &mut Vec<Change>, new: &View, s: u64, e: u64) -> Result<()> {
    let bytes = new
        .read(s, e - s)
        .ok_or_else(|| err!("internal: [{s:#x},{e:#x}) not in the new image"))?;
    changes.push(Change {
        addr: s,
        bytes,
        owner: String::new(),
        differs: true,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 0x4000_0400;

    /// A synthetic MAIN: `stock` bytes, then segments (dest, payload, bss), then the end marker.
    fn main_of(stock: &[u8], segs: &[(u32, &[u8], u32)]) -> Vec<u8> {
        let mut m = stock.to_vec();
        for (dest, payload, bss) in segs {
            m.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            m.extend_from_slice(&dest.to_be_bytes());
            m.extend_from_slice(&bss.to_be_bytes());
            m.extend_from_slice(payload);
        }
        m.extend_from_slice(&u32::MAX.to_be_bytes());
        m
    }

    fn view(stock: &[u8], segs: &[(u32, &[u8], u32)]) -> View {
        View::of(&main_of(stock, segs), BASE, stock.len()).unwrap()
    }

    fn write(kind: &str, owner: &str, addr: u64, len: u64) -> PlanWrite {
        PlanWrite {
            cpu: CF.into(),
            addr,
            len,
            kind: kind.into(),
            owner: owner.into(),
            what: String::new(),
            old_sha256: None,
            new_sha256: String::new(),
            new_hex: None,
            code: vec![],
        }
    }

    const STOCK: [u8; 16] = [7; 16];
    const A: u32 = 0x4010_0000; // core/ui
    const B: u32 = 0x4010_1000; // test/ui-demo, after it
    const C: u32 = 0x4010_2000; // a module that is not swapped

    fn writes(demo_len: u64) -> Vec<PlanWrite> {
        vec![
            write("place", "core/ui@0.1.0", u64::from(A), 8),
            write("place", "test/ui-demo@0.1.0", u64::from(B), demo_len + 4), // 4 bytes of bss
            write("place", "core/runtime@0.1.0", u64::from(C), 8),
            write("patch", "test/ui-demo@0.1.0", BASE + 4, 4),
            write("record", "patchkit", u64::from(C) + 0x100, 8),
        ]
    }

    #[test]
    fn the_view_has_the_stock_bytes_the_payloads_and_the_zeroed_bss() {
        let v = view(&STOCK, &[(B, &[1, 2, 3, 4], 4)]);
        assert_eq!(v.read(BASE, 16).unwrap(), STOCK);
        assert_eq!(v.read(u64::from(B), 8).unwrap(), [1, 2, 3, 4, 0, 0, 0, 0]);
        assert_eq!(v.read(u64::from(B) + 6, 2).unwrap(), [0, 0]);
        assert!(v.read(u64::from(B) + 7, 2).is_none(), "past the bss");
        assert!(v.read(BASE + 15, 2).is_none(), "past the stock bytes");
        assert!(View::of(&[0; 4], BASE, 8).is_err());
        assert!(
            View::of(&main_of(&STOCK, &[(B, &[1, 2, 3, 4], 0)])[..24], BASE, 16).is_err(),
            "cut off"
        );
    }

    #[test]
    fn identical_images_have_no_changes() {
        let a = view(&STOCK, &[(A, &[1; 8], 0), (B, &[2; 4], 4), (C, &[3; 8], 0)]);
        let d = diff_memory(&a, &a.clone(), &writes(4)).unwrap();
        assert!(d.changes.iter().all(|c| !c.differs));
        assert!(d.is_empty());
    }

    #[test]
    fn a_change_in_one_module_rewrites_only_that_module() {
        let old = view(&STOCK, &[(A, &[1; 8], 0), (B, &[2; 4], 4), (C, &[3; 8], 0)]);
        let new = view(&STOCK, &[(A, &[1; 8], 0), (B, &[9; 4], 4), (C, &[3; 8], 0)]);
        let d = diff_memory(&old, &new, &writes(4)).unwrap();
        assert_eq!(d.owners(), ["test/ui-demo"]);
        // the object with its bss (8 bytes), not the other modules; the patch at BASE+4 is unchanged and not listed
        assert_eq!(d.changes.len(), 1);
        assert_eq!(d.changes[0].bytes, [9, 9, 9, 9, 0, 0, 0, 0]);
        assert_eq!(d.changes[0].addr, u64::from(B));
    }

    #[test]
    fn a_hook_site_change_makes_its_module_a_whole_rewrite() {
        let mut stock = STOCK;
        let old = view(&stock, &[(A, &[1; 8], 0), (B, &[2; 4], 4), (C, &[3; 8], 0)]);
        stock[4..8].copy_from_slice(&[0x4e, 0xf9, 0, 0]);
        let new = view(&stock, &[(A, &[1; 8], 0), (B, &[2; 4], 4), (C, &[3; 8], 0)]);
        let d = diff_memory(&old, &new, &writes(4)).unwrap();
        assert_eq!(d.owners(), ["test/ui-demo"]);
        let rewritten: Vec<_> = d.changes.iter().map(|c| (c.addr, c.differs)).collect();
        assert_eq!(rewritten, [(BASE + 4, true), (u64::from(B), false)]);
    }

    #[test]
    fn a_runtime_change_is_attributed_to_the_runtime() {
        let old = view(&STOCK, &[(A, &[1; 8], 0), (B, &[2; 4], 4), (C, &[3; 8], 0)]);
        let new = view(&STOCK, &[(A, &[1; 8], 0), (B, &[9; 4], 4), (C, &[4; 8], 0)]);
        assert_eq!(
            diff_memory(&old, &new, &writes(4)).unwrap().owners(),
            ["core/runtime", "test/ui-demo"]
        );
    }

    #[test]
    fn a_moved_module_changes_its_successors() {
        let old = view(&STOCK, &[(A, &[1; 8], 0), (B, &[2; 4], 4), (C, &[3; 8], 0)]);
        // the demo grew by 8 bytes: the next module is placed 8 bytes later (the plan says so)
        let new = view(
            &STOCK,
            &[(A, &[1; 8], 0), (B, &[2; 12], 4), (C + 8, &[3; 8], 0)],
        );
        let mut w = writes(12);
        w[2].addr += 8;
        let d = diff_memory(&old, &new, &w).unwrap();
        assert_eq!(d.owners(), ["core/runtime", "test/ui-demo"]);
    }

    #[test]
    fn bytes_no_plan_write_covers_have_no_owner() {
        let old = view(&STOCK, &[(A, &[1; 8], 0)]);
        let mut stock = STOCK;
        stock[10] = 1;
        stock[11] = 1;
        let new = view(&stock, &[(A, &[1; 8], 0)]);
        let d = diff_memory(
            &old,
            &new,
            &[write("place", "core/ui@0.1.0", u64::from(A), 8)],
        )
        .unwrap();
        assert_eq!(d.owners(), [""]);
        assert_eq!(
            (d.changes[0].addr, d.changes[0].bytes.clone()),
            (BASE + 10, vec![1, 1])
        );
    }

    #[test]
    fn the_patch_record_is_ignored() {
        let old = view(
            &STOCK,
            &[(A, &[1; 8], 0), (C, &[3; 8], 0), (C + 0x100, &[5; 8], 0)],
        );
        let new = view(
            &STOCK,
            &[(A, &[1; 8], 0), (C, &[3; 8], 0), (C + 0x100, &[6; 8], 0)],
        );
        let d = diff_memory(
            &old,
            &new,
            &writes(4)[..]
                .iter()
                .filter(|w| w.kind != "patch" && w.owner != "test/ui-demo@0.1.0")
                .cloned()
                .collect::<Vec<_>>(),
        )
        .unwrap();
        // only the unowned differences count: the record bytes are covered by a record write, which is not a change
        assert!(d.is_empty(), "{:?}", d.changes);
    }

    #[test]
    fn owner_ids_drop_the_version() {
        assert_eq!(owner_id("core/ui@0.1.0"), "core/ui");
        assert_eq!(owner_id("patchkit"), "patchkit");
    }
}
