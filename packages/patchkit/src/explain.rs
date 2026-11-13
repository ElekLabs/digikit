//! `explain`: a human-readable account of a patch: the modules in order, the allocation map, and the
//! before and after of every patched site. ColdFire code is disassembled with `packages/emulator/coldfire`; SHARC+
//! words are listed as stream bytes with their pc (`tools/patchkit/patchkit.py explain` adds a SHARC listing).

use crate::apply::{Applied, Plan, PlanWrite};
use crate::error::*;
use crate::firmware::Firmware;
use std::fmt::Write;

fn cf_listing(main: &[u8], base: u64, lo: u64, hi: u64) -> Vec<String> {
    let mut out = vec![];
    let mut a = lo;
    while a < hi {
        let off = (a - base) as usize;
        match coldfire::decode_at(main, base as u32, off) {
            Some(i) => {
                let n = (i.len as u64).max(2);
                let raw: String = main[off..(off + n as usize).min(main.len())]
                    .chunks(2)
                    .map(hex)
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push(format!("{a:#010x}  {raw:<20} {i}"));
                a += n;
            }
            None => {
                out.push(format!(
                    "{a:#010x}  {:<20} .word",
                    hex(&main[off..(off + 2).min(main.len())])
                ));
                a += 2;
            }
        }
    }
    out
}

fn sharc_pc(addr: u64) -> String {
    if addr >= 0x2800_0000 {
        format!("pc {:#x}", (addr - 0x2800_0000) / 2)
    } else if (0x2000_0000..0x2010_0000).contains(&addr) {
        format!("pc {:#x}", 0xB8_0000 + (addr - 0x2000_0000) / 2)
    } else {
        "dm".to_string()
    }
}

/// The report for one applied plan. `base` and `out` are the stock and the patched image.
pub fn explain(base: &[u8], out: &Applied) -> Result<String> {
    let plan: &Plan = &out.plan;
    let b = Firmware::parse(base)?;
    let p = Firmware::parse(&out.syx)?;
    let (bm, pm) = (b.main().unwrap(), p.main().unwrap());
    // a device without a DSP has no boot stream; its plan has no SHARC site to list
    let stream = |f: &Firmware| -> Result<Option<crate::dsp::Stream>> {
        f.dsp()
            .map(|d| crate::dsp::Stream::parse(d.data.clone()))
            .transpose()
    };
    let (base_stream, out_stream) = (stream(&b)?, stream(&p)?);
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{} {}  (stock {})",
        plan.base.device, plan.base.version, plan.base.syx_sha256
    );
    let _ = writeln!(s, "modules, in the order they are applied:");
    for m in &plan.modules {
        let _ = writeln!(s, "  {m}");
    }
    let _ = writeln!(s, "\nregions (bytes used of capacity):");
    for (r, (u, c)) in &plan.regions {
        let _ = writeln!(s, "  {r:<14} {u:>8} of {c}");
    }
    let _ = writeln!(s, "\nallocation map:");
    let mut placed: Vec<&PlanWrite> = plan.writes.iter().filter(|w| w.kind != "patch").collect();
    placed.sort_by_key(|w| (w.cpu.clone(), w.addr));
    for w in &placed {
        let _ = writeln!(
            s,
            "  {} {:#010x}..{:#010x} {:>6} B  {}  {}",
            w.cpu,
            w.addr,
            w.addr + w.len,
            w.len,
            w.owner,
            w.what
        );
    }
    let _ = writeln!(s, "\npatched sites:");
    for w in plan.writes.iter().filter(|w| w.kind == "patch") {
        let _ = writeln!(
            s,
            "  [{}] {}  {} {:#010x}+{}",
            w.owner, w.what, w.cpu, w.addr, w.len
        );
        if w.cpu == "cf" {
            for (tag, main, base_addr) in [
                ("before", &bm.data, bm.dest as u64),
                ("after", &pm.data, pm.dest as u64),
            ] {
                for l in cf_listing(main, base_addr, w.addr, w.addr + w.len) {
                    let _ = writeln!(s, "    {tag:<6} {l}");
                }
            }
        } else {
            let was = base_stream
                .as_ref()
                .and_then(|s| s.read(w.addr as u32, w.len as usize))
                .map(|v| hex(&v))
                .unwrap_or_default();
            let now = out_stream
                .as_ref()
                .and_then(|s| s.read(w.addr as u32, w.len as usize))
                .map(|v| hex(&v))
                .unwrap_or_default();
            let _ = writeln!(s, "    before {} (stream bytes)  {was}", sharc_pc(w.addr));
            let _ = writeln!(s, "    after  {} (stream bytes)  {now}", sharc_pc(w.addr));
        }
    }
    let _ = writeln!(s, "\nplaced code:");
    for w in plan.writes.iter().filter(|w| w.kind == "place") {
        if w.cpu == "cf" {
            for (lo, hi) in &w.code {
                let _ = writeln!(s, "  [{}] {}", w.owner, w.what);
                // placed bytes live in the segment chain, not at their run-time address: read them from there
                let segs = crate::verify::read_segments(&pm.data, pm.dest as u64)?;
                let mut found = false;
                for (dest, payload, _) in &segs {
                    if *lo >= *dest && *hi <= dest + payload.len() as u64 {
                        for l in cf_listing(payload, *dest, *lo, *hi) {
                            let _ = writeln!(s, "    {l}");
                        }
                        found = true;
                    }
                }
                if !found {
                    for l in cf_listing(&pm.data, pm.dest as u64, *lo, *hi) {
                        let _ = writeln!(s, "    {l}");
                    }
                }
            }
        } else if !w.code.is_empty() {
            let _ = writeln!(
                s,
                "  [{}] {}  sharc {:#010x}  {}",
                w.owner,
                w.what,
                w.addr,
                sharc_pc(w.addr)
            );
        }
    }
    if plan.registries.is_empty() {
        let _ = writeln!(s, "\nregistries: none");
    }
    for r in &plan.registries {
        let _ = writeln!(
            s,
            "\nregistry {}  [{}]  {} {:#010x}  {} bytes  rows of {} bytes, first index {}, {} of at most {} entries",
            r.name, r.module, r.region, r.addr, r.bytes, r.row_size, r.first, r.count, r.max
        );
        let fields: Vec<String> = r
            .fields
            .iter()
            .map(|(n, k, o)| format!("{n}:{k}@{o}"))
            .collect();
        let _ = writeln!(s, "  row: {}", fields.join(" "));
        for e in &r.entries {
            let _ = writeln!(s, "  {:>4}  {}  [{}]", e.index, e.id, e.module);
        }
    }
    if !plan.sections.is_empty() {
        let _ = writeln!(s, "\ncopied sections:");
    }
    for c in &plan.sections {
        let _ = writeln!(
            s,
            "  {}  [{}]  {} {:#010x}  {} bytes = {} copied from {} ({:#010x}) + {} appended row(s)",
            c.name,
            c.module,
            c.region,
            c.addr,
            c.bytes,
            c.copied,
            c.from_anchor,
            c.from_addr,
            c.appended_rows
        );
    }
    if !plan.hooks.is_empty() {
        let _ = writeln!(s, "\nhooks:");
    }
    for h in &plan.hooks {
        let _ = writeln!(
            s,
            "  [{}] {}{} {:+} ({}) at {:#010x}: jump to {:#x}, {} site byte(s), displaced {}, return {:#x}{}",
            h.module,
            h.hook
                .as_ref()
                .map(|n| format!("{n} = "))
                .unwrap_or_default(),
            h.anchor,
            h.offset,
            h.cpu,
            h.addr,
            h.to,
            h.site_len,
            h.displaced,
            h.ret,
            h.tail.map(|t| format!(", tail {t:#x}")).unwrap_or_default()
        );
        for d in &h.displaced_insns {
            let _ = writeln!(s, "      displaced {d}");
        }
    }
    Ok(s)
}
