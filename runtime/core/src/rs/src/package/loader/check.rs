//! The checks of a file before anything is written: the head, the imports and the body (`Loader::check_file`).
use super::*;

/// What `check_file` found in a file, for the load that follows (or for a reload's checks, which only read it). The pointers are into the staging buffer.
pub(super) struct Checked {
    pub(super) f: *const u8,
    pub(super) size: u32,
    pub(super) head: image::Head,
    pub(super) role: u32,
    /// The address of each import of the file (the runtime's table, or a provider's export).
    pub(super) import_addrs: [u32; dkneeds::MAX_CF_IMPORTS as usize],
    pub(super) reg_off: u32,
    pub(super) reg_size: u32,
    pub(super) keys: crate::package::collect::KeysCopy,
    pub(super) dsp_size: u32,
    pub(super) job: dsp::Job,
}

/// What `place_image` leaves for the steps that follow: the arguments of the role's place step and the addresses of `init` and `tick` (0: none).
pub(super) struct Placed {
    pub(super) place: PlaceArgs,
    pub(super) init: u32,
    pub(super) tick: u32,
}

impl Checked {
    pub(super) const fn empty() -> Checked {
        Checked {
            f: core::ptr::null(),
            size: 0,
            head: image::Head { needs: 0, image_off: 0, image_size: 0, bss: 0, reloc_off: 0, reloc_n: 0, import_off: 0, import_n: 0, init: 0, fini: 0, menu: 0, tick: 0 },
            role: 0,
            import_addrs: [0; dkneeds::MAX_CF_IMPORTS as usize],
            reg_off: 0,
            reg_size: 0,
            keys: crate::package::collect::KeysCopy::empty(),
            dsp_size: 0,
            job: dsp::Job::empty(),
        }
    }
}


impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    /// The selection index of module record `rec`, `u8::MAX` when it has none.
    pub(super) fn sel_of(&self, rec: usize) -> u8 {
        self.sel_of_rec[rec].wrapping_sub(1)
    }

    /// The port file that is loaded: its record, the copy of its table and the base and image size of its block. `None` when no port file is loaded.
    pub(super) unsafe fn port_keys(&self) -> Option<(Keys<'static>, u32, u32)> {
        let mut j = 0;
        while j < PROVIDER_MAX {
            let rec = MAX_MODULES + j;
            if self.roles[rec] == 2 && records::module(rec).used != 0 {
                let pr = &self.provs[j];
                let blk = pr.block?;
                let keys = Keys::parse(core::slice::from_raw_parts(blk.ptr as *const u8, pr.len as usize)).ok()?;
                return Some((keys, records::module(rec).base, pr.image_size));
            }
            j += 1;
        }
        None
    }

    /// Resolve the import `name` of a service file against the loaded port file. A name that is no firmware reference is `NotPort`; a firmware reference with no port
    /// file loaded is `Unlisted` (the hash is the reference's), as a name the port file does not list.
    unsafe fn port_lookup(&self, name: &[u8]) -> port::Lookup {
        let Some(r) = port::parse_import(name) else {
            return port::Lookup::NotPort;
        };
        match self.port_keys() {
            Some((keys, base, image_size)) => port::lookup(&keys, base, image_size, r),
            None => port::Lookup::Unlisted(r.hash),
        }
    }

    /// The checks of a file: everything that can be refused before a record or a byte of the arena is taken. The staged file stays in the staging buffer (`c.f`)
    /// until the next `stage`. `slot` says which role the file must have: a service file only for a provider record, a module file only for the others.
    /// Three parts in this order: the header and the role's check (`check_head`), the imports (`resolve_imports`), the sections (`check_body`).
    pub(super) unsafe fn check_file(&mut self, path: *const u8, slot: Slot, c: &mut Checked) -> i32 {
        let r = self.check_head(path, slot, c);
        if r != OK {
            return r;
        }
        let r = self.resolve_imports(c, None);
        if r != OK {
            return r;
        }
        self.check_body(c)
    }

    /// The first part of `check_file`: stage the file, parse its header, the role and the role's own checks.
    pub(super) unsafe fn check_head(&mut self, path: *const u8, slot: Slot, c: &mut Checked) -> i32 {
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let f = staged.ptr;
        let size = staged.size;
        c.f = f;
        c.size = size;
        self.last_name = [0; LOG_NAME];

        // ---- checks (nothing is written outside the staging buffer) ----
        let head = match image::head(f, size) {
            Ok(h) => h,
            Err(e) => return e,
        };
        c.head = head;
        // the header is read: the log names the module by its id, not by its file
        self.last_name = name_part(core::slice::from_raw_parts(f.add(H_NAME), dkneeds::NAME_LEN as usize));
        // the steps of the file's role
        let role = rd32(f.add(dkneeds::H_ROLE as usize));
        c.role = role;
        if (role == 1 || role == 2) != matches!(slot, Slot::Provider(_)) {
            return E_FORMAT;
        }
        let steps = RoleSteps::<P, M, R>::of(role);
        (steps.check)(self, f, &head)
    }

    /// The second part of `check_file`: the address of every import of the staged file, into `c.import_addrs`. An explicit service import names its provider; legacy
    /// SDK imports keep the global lookup. `needs`: when given, an import that resolves to the export of a loaded provider resolves only if that provider's record is one of
    /// `needs` (the services of the package's needs, `PackageId` is the record); the runtime's table, the service-only imports and the loaded port file are not
    /// bound by it.
    pub(super) unsafe fn resolve_imports(&mut self, c: &mut Checked, needs: Option<&[PackageId]>) -> i32 {
        let (f, size, role) = (c.f, c.size, c.role);
        let image::Head { import_off, import_n, .. } = c.head;
        let allowed = |rec: usize| needs.map_or(true, |n| n.iter().any(|p| p.0 as usize == rec));
        // An explicit service import names its provider; legacy SDK imports keep the global lookup.
        let (reg_off, reg_size) = (rd32(f.add(H_REG_OFF)), rd32(f.add(H_REG_SIZE)));
        let import_owners = if import_n != 0 && in_file(reg_off, reg_size, size) {
            Keys::parse(core::slice::from_raw_parts(f.add(reg_off as usize), reg_size as usize)).ok()
        } else {
            None
        };
        let mut i = 0;
        while i < import_n as usize {
            let p = f.add(import_off as usize + i * dkneeds::CF_IMPORT_LEN as usize);
            let n = cstr_len(p, dkneeds::CF_IMPORT_LEN as usize);
            if n == dkneeds::CF_IMPORT_LEN as usize {
                return E_FORMAT;
            }
            let name = core::slice::from_raw_parts(p, n);
            let name_hash = dkneeds::keys::name_hash_bytes(name);
            let mut a = self.m.import_addr(p);
            if a == 0 && role == 1 {
                // a service file also links the service-only imports (`SVC_IMPORTS`)
                a = self.m.svc_import_addr(p);
            }
            if let Some(keys) = import_owners {
                let service = match service_import_owner(keys, name_hash) {
                    Ok(s) => s,
                    Err(e) => return e,
                };
                if let Some((service, minimum)) = service {
                    if a != 0 || port::parse_import(name).is_some() {
                        return E_IMPORT;
                    }
                    match self.exports.owned(name_hash) {
                        Some((addr, rec)) => {
                            if let Some((owner, version)) = self.provs[rec - MAX_MODULES].provides {
                                if allowed(rec) && owner == service && version >= minimum && version >> 16 == minimum >> 16 {
                                    c.import_addrs[i] = addr;
                                    i += 1;
                                    continue;
                                }
                            }
                        }
                        None => {}
                    }
                    return E_IMPORT;
                }
            }
            if a == 0 && role == 1 {
                // the firmware references of a service file are the port file's: an address, 0 for a name that this firmware lacks, or a refusal
                let found = self.port_lookup(name);
                if let port::Lookup::Addr(x) = found {
                    a = x;
                } else if found == port::Lookup::Absent {
                    c.import_addrs[i] = 0;
                    i += 1;
                    continue;
                } else if let port::Lookup::Unlisted(h) = found {
                    self.import_hash = h;
                    return E_IMPORT;
                }
            }
            if a == 0 {
                // the runtime's table has no such name: a loaded provider may export it
                a = match self.exports.owned(name_hash) {
                    Some((addr, rec)) if allowed(rec) => self.exports.resolve(name_hash).map_or(0, |_| addr),
                    _ => 0,
                };
            }
            if a == 0 {
                // the first import that belongs to a module that is not in the image: the boot keeps its name (`MISSING`)
                if let (Some(n), true) = (self.m.import_needs(p), self.missing[0] == 0) {
                    let b = n.as_bytes();
                    let mut k = 0;
                    while k < b.len() && k < NEEDS_LEN {
                        self.missing[k] = b[k];
                        k += 1;
                    }
                }
                return E_IMPORT;
            }
            c.import_addrs[i] = a;
            i += 1;
        }
        OK
    }

    /// The third part of `check_file`: the relocation table, the `register` table and the DSP section.
    pub(super) unsafe fn check_body(&mut self, c: &mut Checked) -> i32 {
        let (f, size, role, head) = (c.f, c.size, c.role, c.head);
        let (reg_off, reg_size) = (rd32(f.add(H_REG_OFF)), rd32(f.add(H_REG_SIZE)));
        // relocations: kind, bounds, import index
        if let Err(e) = image::sections(f, &head) {
            return e;
        }
        // `register` records (machines, parameters): validated now, written after the copy
        c.reg_off = reg_off;
        c.reg_size = reg_size;
        if !in_file(reg_off, reg_size, size) {
            return E_FORMAT;
        }
        let reg = f.add(reg_off as usize);
        // the table is the keys table; the records come from the builder calls of `init` (complete)
        c.keys = match crate::package::collect::KeysCopy::from_bytes(core::slice::from_raw_parts(reg, reg_size as usize)) {
            Ok(k) => k,
            Err(e) => return e,
        };
        if role == 1 {
            role::filter_hooks(self, &mut c.keys);
        }
        // the DSP section, if any
        let (dsp_off, dsp_size) = (rd32(f.add(H_DSP_OFF)), rd32(f.add(H_DSP_SIZE)));
        c.dsp_size = dsp_size;
        if dsp_size != 0 {
            let r = dsp::check(f, dsp_off, dsp_size, size, &mut c.job);
            if r != OK {
                return r;
            }
            let (v, stride) = (c.job.voices, c.job.vstride as usize);
            let r = crate::package::collect::check_dsp_keys(&c.keys, c.job.nvoice as usize, |i| rd32(v.add(stride * i)), c.job.nfx as usize);
            if r != OK {
                return r;
            }
        }
        OK
    }
}
