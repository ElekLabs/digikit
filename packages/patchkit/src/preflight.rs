//! `patchkit preflight PATCHED.syx`: the checks to read before an image goes to a device.
//!
//! It answers: is this a patched image whose stock base is a known firmware (`[[firmware]]` of `devices/*.toml`) that is present in
//! `firmware/`; are the bootstrap (2), updater (4), meta (5), boot data (8) and every other section except MAIN OS (3) and the DSP (7)
//! byte-identical to the base, stored and decoded; which of 3 and 7 differ; do the container checksum, the HMAC and the size limits of the
//! port hold (`verify`). It never writes anything. Any failed line makes the whole result a failure.

use crate::error::*;
use crate::firmware::Firmware;
use crate::port::Registry;
use crate::verify;

/// A stock firmware named by a device file.
#[derive(Clone, Debug)]
pub struct Known {
    pub device: String,
    pub version: String,
    pub sha256: String,
    pub filename: String,
}

/// The `[[firmware]]` tables of device files (the text of each `devices/*.toml`).
pub fn known_from_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Vec<Known>> {
    let mut out = vec![];
    for t in texts {
        let v: toml::Value = toml::from_str(t)?;
        let Some(short) = v
            .get("device")
            .and_then(|d| d.get("short"))
            .and_then(|s| s.as_str())
        else {
            continue;
        };
        for f in v
            .get("firmware")
            .and_then(|f| f.as_array())
            .into_iter()
            .flatten()
        {
            let s = |k: &str| f.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            out.push(Known {
                device: short.to_string(),
                version: s("version"),
                sha256: s("sha256"),
                filename: s("filename"),
            });
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct Line {
    pub ok: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub lines: Vec<Line>,
}

impl Outcome {
    fn ok(&mut self, t: impl Into<String>) {
        self.lines.push(Line {
            ok: true,
            text: t.into(),
        });
    }
    fn fail(&mut self, t: impl Into<String>) {
        self.lines.push(Line {
            ok: false,
            text: t.into(),
        });
    }
    pub fn passed(&self) -> bool {
        self.lines.iter().all(|l| l.ok)
    }
}

/// Sections that a patch may change: the device's MAIN OS and, when it has a DSP, its boot stream (`[container]`).
fn patchable(d: crate::syx::Device) -> Vec<u32> {
    std::iter::once(d.main_section())
        .chain(d.dsp_section())
        .collect()
}

/// Check `patched` against its base. `stock` returns the bytes of the base file for a known firmware (`None`: not present). `code`:
/// the DSP code ranges of the patch's plan, for the ISA check (`dspscan::scan`).
pub fn preflight(
    patched: &[u8],
    reg: &Registry,
    known: &[Known],
    stock: &dyn Fn(&Known) -> Option<Vec<u8>>,
    code: Option<&[(u64, u64)]>,
) -> Outcome {
    let mut o = Outcome::default();
    let fw = match Firmware::parse(patched) {
        Ok(f) => f,
        Err(e) => {
            o.fail(format!("not a firmware .syx: {e}"));
            return o;
        }
    };
    o.ok(format!(
        "patched image: {}, {} bytes, sha256 {}",
        fw.device.short(),
        patched.len(),
        fw.syx_sha256
    ));
    if let Some(k) = known.iter().find(|k| k.sha256 == fw.syx_sha256) {
        o.fail(format!(
            "not patched: this is the stock {} {} ({}); a patched image carries a patch record",
            k.device, k.version, k.filename
        ));
        return o;
    }
    let record = match verify::patch_record(patched) {
        Ok(Some(r)) => r,
        Ok(None) => {
            o.fail("not patched: the image has no patch record (DKPR) and is not a known stock firmware");
            return o;
        }
        Err(e) => {
            o.fail(format!("patch record unreadable: {e}"));
            return o;
        }
    };
    o.ok(format!(
        "patch record: base sha256 {}, modules {}",
        record.base_sha256,
        if record.modules.is_empty() {
            "none".into()
        } else {
            record.modules.join(", ")
        }
    ));

    let Some(k) = known.iter().find(|k| k.sha256 == record.base_sha256) else {
        o.fail(format!(
            "the base sha256 {} is not a known stock firmware (no [[firmware]] with it in devices/*.toml)",
            record.base_sha256
        ));
        return o;
    };
    o.ok(format!(
        "base is the stock {} {} (a [[firmware]] of devices/*.toml, file name {})",
        k.device, k.version, k.filename
    ));
    if k.device != fw.device.short() {
        o.fail(format!(
            "the image is for {} but its base is a {} firmware",
            fw.device.short(),
            k.device
        ));
        return o;
    }
    let Some(base_bytes) = stock(k) else {
        o.fail(format!(
            "the stock file is not in firmware/ (looked for sha256 {}, usually {})",
            k.sha256, k.filename
        ));
        return o;
    };
    let base = match Firmware::parse(&base_bytes) {
        Ok(b) if b.syx_sha256 == k.sha256 => b,
        Ok(_) => {
            o.fail("the stock file found does not have the base sha256");
            return o;
        }
        Err(e) => {
            o.fail(format!("the stock file does not parse: {e}"));
            return o;
        }
    };
    o.ok(format!(
        "stock file present in firmware/, sha256 {}",
        base.syx_sha256
    ));

    // Every section: the ids match, and all but the patchable ones are byte-identical.
    let ids = |f: &Firmware| f.sections.iter().map(|s| s.id).collect::<Vec<_>>();
    if ids(&base) != ids(&fw) {
        o.fail(format!(
            "section ids differ: base {:?}, image {:?}",
            ids(&base),
            ids(&fw)
        ));
        return o;
    }
    let patchable = patchable(fw.device);
    // the role of a section is the device file's; the ids that every container has keep their names
    let c = fw.device.container();
    let name = |id: u32| match id {
        _ if id == c.main => "MAIN OS",
        _ if Some(id) == c.dsp => "DSP",
        _ if Some(id) == c.cf2 => "second CPU image",
        _ if Some(id) == c.fpga => "FPGA bitstream",
        2 => "bootstrap",
        4 => "updater",
        5 => "meta",
        8 => "boot data",
        _ => "other",
    };
    let mut changed = vec![];
    for id in ids(&base) {
        let (b, p) = (base.section(id).unwrap(), fw.section(id).unwrap());
        let same = b.stored == p.stored && b.data == p.data && b.dest == p.dest;
        let sizes = format!(
            "{} bytes decoded, {} stored, sha256 {}",
            p.data.len(),
            p.stored.len(),
            &sha256_hex(&p.stored)[..16]
        );
        if patchable.contains(&id) {
            if same {
                o.ok(format!("section {id} ({}) unchanged: {sizes}", name(id)));
            } else {
                changed.push(id);
                o.ok(format!(
                    "section {id} ({}) differs from the stock base (allowed): {} decoded (stock {}), {} stored (stock {})",
                    name(id),
                    p.data.len(),
                    b.data.len(),
                    p.stored.len(),
                    b.stored.len()
                ));
            }
        } else if same {
            o.ok(format!(
                "section {id} ({}) identical to the stock base, stored and decoded: {sizes}",
                name(id)
            ));
        } else if fw.device.is_repackable(id) && b.data == p.data && b.dest == p.dest {
            // the stored form is packed by a module that unpacks it; the verify below checks that the patch record names the module and layout
            o.ok(format!(
                "section {id} ({}) stored packed, decoded bytes identical to the stock base: {sizes} (stock {} stored)",
                name(id),
                b.stored.len()
            ));
        } else {
            o.fail(format!(
                "section {id} ({}) DIFFERS from the stock base (never allowed): stored {}/{} bytes equal {}, decoded {}/{} equal {}, dest {:#x}/{:#x}",
                name(id),
                p.stored.len(),
                b.stored.len(),
                b.stored == p.stored,
                p.data.len(),
                b.data.len(),
                b.data == p.data,
                p.dest,
                b.dest
            ));
        }
    }
    if changed.is_empty() {
        o.fail("the patchable sections are all identical to the base: nothing is patched");
    } else {
        let s: Vec<String> = changed.iter().map(|i| i.to_string()).collect();
        let main = fw.device.main_section();
        let only = if changed == [main] && fw.device.dsp_section().is_some() {
            " (MAIN OS only; the DSP section is stock)"
        } else if changed == [main] {
            " (MAIN OS only)"
        } else if Some(changed[0]) == fw.device.dsp_section() && changed.len() == 1 {
            " (DSP only; MAIN OS is stock)"
        } else {
            ""
        };
        o.ok(format!("changed sections: {}{only}", s.join(" and ")));
    }

    // Container checksum, HMAC, section decode, expansion segments, size limits (verify), without a base.
    match verify::verify(patched, Some(reg), Some(&base_bytes)) {
        Ok(rep) => {
            for c in &rep.checks {
                o.ok(c.clone());
            }
            match reg.find(&base.syx_sha256) {
                Some(p) => {
                    let l = &p.limits;
                    // the flash path copies the image into a RAM stage without a size check, so it is refused here
                    if rep.container_bytes > rep.container_max {
                        o.fail(format!(
                            "container is {} bytes, over the {} limit",
                            rep.container_bytes, l.container_max
                        ));
                    }
                    let main = fw.main().map(|m| m.data.len()).unwrap_or(0);
                    match fw.dsp() {
                        Some(dsp) => o.ok(format!(
                            "port {}: container {} of {} bytes, DSP stored {} of {} / decoded {} of {}, MAIN staged {} of {}",
                            p.id(),
                            rep.container_bytes,
                            l.container_max,
                            dsp.stored.len(),
                            l.dsp_stored_max,
                            dsp.data.len(),
                            l.dsp_depacked_max,
                            main,
                            l.main_staged_max
                        )),
                        None => o.ok(format!(
                            "port {}: container {} of {} bytes, MAIN staged {} of {}",
                            p.id(),
                            rep.container_bytes,
                            l.container_max,
                            main,
                            l.main_staged_max
                        )),
                    }
                }
                None => o.fail(format!(
                    "no port lockfile for the base {} (ports/)",
                    base.syx_sha256
                )),
            }
            if rep.record.as_ref().map(|r| r.base_sha256.as_str()) != Some(base.syx_sha256.as_str())
            {
                o.fail("the record's base differs from the stock file");
            }
        }
        Err(e) => o.fail(format!("verify: {e}")),
    }
    match (fw.device.dsp_section(), fw.dsp()) {
        (None, _) => o.ok("ISA check: the device has no DSP section"),
        (Some(_), Some(dsp)) => match crate::dspscan::check_section7(&dsp.data, code) {
            Ok(t) => o.ok(format!("ISA check: {t}")),
            Err(e) => o.fail(format!("ISA check: {e}")),
        },
        (Some(_), None) => o.fail("ISA check: the image has no DSP section"),
    }
    o.ok(format!("sha256 patched {}", fw.syx_sha256));
    o.ok(format!("sha256 base    {}", base.syx_sha256));
    o
}
