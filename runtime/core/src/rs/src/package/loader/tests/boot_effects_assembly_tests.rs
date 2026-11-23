//! The effects of a boot, a reload, an unload and a live load on the host, as a committed text per device (`loader/tests/snapshots/boot-effects-<device>.txt`).
//!
//! One standard selection (a machine, an effect, `lfo-shape`, the `local-ping-user` module with its provider, and `entry-demo`, which has a `fini`) goes through the real boot
//! (`boot::run` over the host's `BootEnv`: the selection record and the safe-mode decision over the fake card, the boot counter, the tables, the providers and the package
//! manager's load), then a reload, an unload and a live load of `lfo-shape`, and an unload of `entry-demo`, through the real live job (`reload::Job` over the host's `Ops`: its refusals, checks, phases, loads and
//! ends). After each phase the snapshot lists what the phase did to the world the host can see: the installer's platform calls, the card's sector writes and sectors, the order
//! of the `init` and `fini` calls, the runtime's events in order, the rows of the service table, the arena blocks, the contexts and tick words, the manager's live set, the boot
//! record with its module states, and the module records. Host pointers are never recorded: an address is a name (a slot of the image's table), an arena stand-in or an address
//! of the firmware's code, and any other address fails the test.
//!
//! A step that must not change behaviour (a move of code between crates or files) leaves the files as they are. A deliberate change rewrites them:
//! `UPDATE_GOLDENS=1 pnpm nx:pilot run core/runtime:test-assembly`, and the PR explains each changed line.
use super::assembly_tests::*;
use super::golden::Golden;
use super::host_boot_env::HostBootEnv;
use super::host_ops::HostOps;
use super::*;
use crate::fake_platform::{card_reset, Call, FakePlatform};
use crate::package::reload::{step, Job};
use dkrt_abi::package::PackageId;
use std::string::String;
use std::sync::Mutex;
use std::vec::Vec;

/// What the snapshot does not see on the host path, said at the top of every snapshot.
const NOT_SEEN: [&str; 5] = [
    "# no module or service file crosses the card: the loader reads them from `Card.files` (no store sector traffic), so the only sector write is the selection record's (the boot counter)",
    "# the host runs no module code: `init` and `fini` are logged, never executed, so a module's own events, ticks and declarations do not appear; the DSP is not modelled",
    "# not made: the settings record, the crash record and the keep area's RAM probe (firmware RAM), so no boot after a crash (`fault::at_boot`, the late-crash count, a hang or panic record) can be driven; a card transfer in flight, and the files of `FakeStorage`",
    "# `claims` in the card state counts the claims of a blank card's area (its header copies are written first): the fake card of the test is ours from the start, so no claim is made and it stays 0",
    "# the boot's `result` line is the number of modules loaded (`boot info loaded`): the boot has no result code of its own, each module's code is in its `module` line",
];

const ENTRY_DEMO: Fixture = Fixture {
    leaf: "entry-demo",
    nx: "test/entry-demo",
};

/// The events the runtime logs, in order (the sink the debug module would be).
static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

unsafe extern "C" fn sink(e: *const crate::dlog::Ev) {
    let e = *e;
    // the time is the host's block counter and the sequence number the ring's: neither belongs to the runtime's behaviour
    EVENTS
        .lock()
        .unwrap_or_else(|x| x.into_inner())
        .push(std::format!(
            "kind {} flags {:#06x} args {:x?}",
            e.kind,
            e.flags,
            e.a
        ));
}

/// What the platform, the card and the modules did since the last phase, and the world as it stands after it.
struct Phases {
    golden: Golden,
    calls_seen: usize,
    writes_seen: usize,
    hooks_seen: usize,
}

impl Phases {
    fn new() -> Phases {
        let mut golden = Golden::new();
        for line in NOT_SEEN {
            golden.line(line);
        }
        Phases {
            golden,
            calls_seen: 0,
            writes_seen: 0,
            hooks_seen: 0,
        }
    }

    unsafe fn phase(
        &mut self,
        name: &str,
        result: String,
        l: &Boot,
        rt: &mut crate::package::lifecycle::Rt<FakePlatform, Card, DeviceRegistry>,
    ) {
        self.golden.section(name);
        self.golden.line(result);
        self.platform_calls(l);
        self.card_writes(l);
        self.hook_calls(l);
        self.events();
        let g = &mut self.golden;
        g.line(std::format!("installed sites {}", l.installer.count()));
        g.line("service rows");
        for row in crate::package::svc_rows::rows() {
            match row {
                dkrt_abi::service::Row::Service(p) => g.line(std::format!("  {} kinds {:?}", (**p).name(), (**p).kinds())),
                // a provider row has the hash of its name: the name is the stem of the service file on the card with that hash
                provider => {
                    let hash = provider.name_hash();
                    let name = l
                        .m
                        .files
                        .iter()
                        .filter_map(|(n, _)| std::str::from_utf8(n).ok()?.strip_prefix("services/")?.strip_suffix(".dks").map(|s| s.to_string()))
                        .find(|stem| dkneeds::keys::name_hash(stem) == hash)
                        .unwrap_or_else(|| std::format!("{hash:#010x}"));
                    g.line(std::format!("  {} kinds {:?}", name, provider.kinds()))
                }
            }
        }
        g.line("arena blocks");
        for (addr, mem) in &l.m.live {
            g.line(std::format!("  {addr:#010x} {} bytes", mem.len() * 16));
        }
        g.line("contexts and ticks");
        for rec in 0..crate::state::REC_MAX {
            let c = crate::package::entry::DKRT_CTXS[rec];
            let tick = crate::package::entry::tick_of(rec);
            if c.magic != 0 || tick != 0 {
                let at = if tick == 0 {
                    "0".to_string()
                } else {
                    l.m.arena_ref(tick, 1)
                        .unwrap_or_else(|| panic!("tick of record {rec} is {tick:#x}: not in an arena block"))
                };
                g.line(std::format!("  {rec} magic {:#010x} rec {} tick {at}", c.magic, c.rec));
            }
        }
        let manager = rt.manager().expect("the manager");
        let live: Vec<String> = (0..crate::state::REC_MAX)
            .filter(|k| manager.live().contains(PackageId(*k as u8)))
            .map(|k| k.to_string())
            .collect();
        g.line(std::format!("live set {}", live.join(" ")));
        Self::boot_info(g);
        g.line("records");
        for rec in 0..crate::state::REC_MAX {
            let m = records::module(rec);
            if m.used != 0 {
                let name = m
                    .name
                    .iter()
                    .take_while(|b| **b != 0)
                    .map(|b| *b as char)
                    .collect::<String>();
                let path = records::path(rec)
                    .iter()
                    .take_while(|b| **b != 0)
                    .map(|b| *b as char)
                    .collect::<String>();
                g.line(std::format!(
                    "  {rec} {name} {path} version {:#x} base {:#010x} size {} fini {:#010x}",
                    m.version,
                    m.base,
                    m.size,
                    m.fini
                ));
            }
        }
        g.line("card sectors");
        for (sector, bytes) in l.p.card.borrow().iter() {
            let mut sha = dkrt_abi::sha256::Sha::new();
            sha.update(bytes.as_ptr(), bytes.len() as u32);
            let mut out = [0u8; 32];
            sha.finish(out.as_mut_ptr());
            g.line(std::format!(
                "  {sector} {}",
                out[..6]
                    .iter()
                    .map(|b| std::format!("{b:02x}"))
                    .collect::<String>()
            ));
        }
    }

    /// The boot record the runtime keeps (what the LOADS command shows) with the state of each module.
    unsafe fn boot_info(g: &mut Golden) {
        let info = crate::boot::info();
        g.line(std::format!(
            "boot info magic {:#x} version {} safe {} reason {} counter {} seq {} copy {} count {} loaded {} ui {} core_cause {} stable {} unloads {} reloads {}",
            info.magic, info.version, info.safe, info.reason, info.counter, info.seq, info.copy, info.count, info.loaded, info.ui_reached, info.core_cause, info.stable, info.unloads, info.reloads
        ));
        for k in 0..info.count as usize {
            let name = std::str::from_utf8(&(*crate::boot::loads_view().names)[k])
                .unwrap_or("?")
                .trim_end_matches('\0')
                .to_string();
            g.line(std::format!(
                "  module {k} {name} result {} state {} detail {:x?} needs {:x?}",
                info.results[k],
                info.states[k],
                info.detail[k],
                info.needs[k]
            ));
        }
    }

    /// A boot that loads nothing (safe mode): what it did to the platform, the card and the log, and the boot record.
    unsafe fn safe_phase(&mut self, name: &str, l: &Boot) {
        self.golden.section(name);
        self.platform_calls(l);
        self.card_writes(l);
        self.hook_calls(l);
        self.events();
        self.golden
            .line(std::format!("installed sites {}", l.installer.count()));
        Self::boot_info(&mut self.golden);
    }

    unsafe fn platform_calls(&mut self, l: &Boot) {
        let g = &mut self.golden;
        g.line("installer calls");
        let calls = l.p.calls.borrow();
        for c in &calls[self.calls_seen..] {
            g.line(match c {
                Call::Mask => "  mask".into(),
                Call::Restore => "  restore".into(),
                Call::Write { addr, len } => {
                    std::format!("  write {} {len}", address(l, *addr, *len, false))
                }
                Call::Word { addr, value } => {
                    std::format!("  word {} {value:#010x}", address(l, *addr, 4, true))
                }
                Call::Flush { addr, len } => {
                    std::format!("  flush {} {len}", address(l, *addr, *len, false))
                }
            });
        }
        self.calls_seen = calls.len();
    }

    fn card_writes(&mut self, l: &Boot) {
        let g = &mut self.golden;
        g.line("card writes");
        let writes = l.p.writes.borrow();
        for (sector, bytes) in &writes[self.writes_seen..] {
            g.line(std::format!("  sector {sector} {bytes} bytes"));
        }
        self.writes_seen = writes.len();
        let info = crate::card::info();
        g.line(std::format!(
            "card state {} claims {} writes {} refused {}",
            crate::card::state(),
            info.claims,
            info.writes,
            info.refused
        ));
    }

    fn hook_calls(&mut self, l: &Boot) {
        let g = &mut self.golden;
        g.line("init and fini calls");
        for (fini, rec) in &l.m.hooks[self.hooks_seen..] {
            g.line(std::format!(
                "  {} {rec}",
                if *fini { "fini" } else { "init" }
            ));
        }
        self.hooks_seen = l.m.hooks.len();
    }

    fn events(&mut self) {
        let g = &mut self.golden;
        g.line("events");
        for e in EVENTS.lock().unwrap_or_else(|x| x.into_inner()).drain(..) {
            g.line(std::format!("  {e}"));
        }
    }
}

/// The name of a platform address of the installer path, or a failure: a slot word lives in the platform's table of the image's info entries (at the device address the fake platform placed it) and
/// is named by the entry's name hash and the offset in the entry; an arena stand-in and an address of the firmware's code (a patched site, a flushed line) are their own names. An
/// address that is none of these is a host pointer, and the snapshot would change from run to run. `slot_only`: a slot word, which has no other form.
fn address(l: &Boot, addr: u32, len: u32, slot_only: bool) -> String {
    for (hash, words) in l.p.info_entries.borrow().iter() {
        let at = l.p.addresses.place(words.as_ptr() as *mut u8, words.len() * 4);
        if addr >= at && addr - at < 4 * words.len() as u32 {
            return std::format!("slot:{hash:08x}+{}", addr - at);
        }
    }
    let arena = if slot_only { None } else { l.m.arena_ref(addr, len) };
    if let Some(a) = arena {
        return a;
    }
    let code =
        l.p.code
            .borrow()
            .range(addr..addr.saturating_add(len))
            .next()
            .is_some();
    assert!(
        !slot_only && code,
        "address {addr:#010x} len {len} is no slot, live or freed arena block, or firmware code"
    );
    std::format!("{addr:#010x}")
}

/// One run of the live job `op` on the module at `path`, polled to its end: the job's result.
unsafe fn live_job(l: &mut Boot, rt: &mut Rt, job: &mut Job, op: u32, path: &str) -> i32 {
    let mut ops = HostOps { l, rt };
    let queued = job.queue(&mut ops, op, path.as_ptr());
    if queued != OK {
        return queued;
    }
    for _ in 0..POLLS {
        if job.phase != 1 {
            break;
        }
        job.poll(&mut ops);
        ops.l.p.tick(BLOCKS_PER_POLL);
    }
    assert!(
        job.phase == 2 && job.step == step::DONE,
        "the job did not end: phase {} step {}",
        job.phase,
        job.step
    );
    job.result
}

/// The DSP blocks (1500 a second) that pass between two UI passes of the job (60 passes a second).
const BLOCKS_PER_POLL: u32 = 25;

/// More polls than a job may take (`WAIT_PASSES` for each of its waits).
const POLLS: usize = 8000;

type Rt = crate::package::lifecycle::Rt<FakePlatform, Card, DeviceRegistry>;

fn device_run(dev: Dev) -> Golden {
    let image = Image::read(dev, "emulator");
    let mut services: Vec<Fixture> = SERVICES[..3]
        .iter()
        .map(|f| Fixture {
            leaf: f.leaf,
            nx: f.nx,
        })
        .collect();
    services.push(Fixture {
        leaf: LOCAL_PING.leaf,
        nx: LOCAL_PING.nx,
    });
    let mut phases = Phases::new();
    unsafe {
        let selection = [&SINE, &FX_GAIN, &LFO_SHAPE, &LOCAL_PING_USER, &ENTRY_DEMO];
        let (mut l, plan) = planned(dev, &image, &[], &selection, &services);
        // the host runs no DSP and no module code: the DSP parts of the machine and the effect are left out, and their `init` declares nothing (what the load of a real machine and
        // effect declares needs the DSP rows and the registry of the image, which the host does not model: RC2s-0b)
        for (k, m) in [(0, &SINE), (1, &FX_GAIN)] {
            swap_module(&mut l, k, m.leaf, without_dsp(dev, m));
        }
        let _dsp = dsp_env(dev, &l, &plan);
        let rec = boot_record(&[
            "sine",
            "dyn-fx-gain",
            "lfo-shape",
            "local-ping-user",
            "entry-demo",
        ]);
        crate::dlog::dk_debug_log_sink_set(sink as usize as *mut u8, core::ptr::null_mut());
        EVENTS.lock().unwrap_or_else(|x| x.into_inner()).clear();
        // the card holds the selection record the boot reads (the SELECT command's write)
        card_reset();
        assert_eq!(crate::selection::update_with(&l.p, rec, 0, false), OK);
        let mut rt = Box::new(Rt::new(&mut l as *mut _));
        crate::boot::run(&HostBootEnv::new(&mut l, &mut rt));
        phases.phase(
            "boot",
            std::format!("loaded modules {}", crate::boot::info().loaded),
            &l,
            &mut rt,
        );

        let path = "modules/lfo-shape.dkm\0";
        let mut job = Job::new();
        let result = live_job(&mut l, &mut rt, &mut job, LIVE_RELOAD, path);
        phases.phase("reload", std::format!("result {result}"), &l, &mut rt);

        let result = live_job(&mut l, &mut rt, &mut job, LIVE_UNLOAD, path);
        phases.phase("unload", std::format!("result {result}"), &l, &mut rt);

        let result = live_job(&mut l, &mut rt, &mut job, LIVE_LOAD, path);
        phases.phase("live load", std::format!("result {result}"), &l, &mut rt);

        // a module with a `fini`: its teardown enters it
        let result = live_job(
            &mut l,
            &mut rt,
            &mut job,
            LIVE_UNLOAD,
            "modules/entry-demo.dkm\0",
        );
        phases.phase(
            "unload of a module with fini",
            std::format!("result {result}"),
            &l,
            &mut rt,
        );
        crate::dlog::dk_debug_log_sink_set(core::ptr::null_mut(), core::ptr::null_mut());
    }
    let _ = clean_state();
    phases.golden
}

/// A card with the selection record `rec` and a boot over `l` and `rt` that reads it.
unsafe fn boot_from_card(l: &mut Boot, rt: &mut Rt, mut rec: dkrt_sdk::sel::Record) {
    // a record newer than the copies on the card, written to the other copy (a first write makes copy A)
    let written = match crate::selection::current_with(&l.p) {
        crate::selection::Choice::Use { record, copy, .. } => {
            rec.seq = record.seq;
            crate::selection::update_with(&l.p, rec, copy, true)
        }
        _ => crate::selection::update_with(&l.p, rec, 0, false),
    };
    assert_eq!(written, OK);
    crate::boot::run(&HostBootEnv::new(l, rt));
}

/// The refusals of the runtime on the host paths a good boot does not take: a boot whose module and service files are damaged, a reload and an unload the job refuses, a live load of a
/// damaged file, and the two safe boots that load nothing (the record's one-shot flag, and the boot counter at its limit). The safe boots come last: they leave the loader in safe mode.
fn device_errors_run(dev: Dev) -> Golden {
    let image = Image::read(dev, "emulator");
    let mut services: Vec<Fixture> = SERVICES[..3]
        .iter()
        .map(|f| Fixture {
            leaf: f.leaf,
            nx: f.nx,
        })
        .collect();
    services.push(Fixture {
        leaf: LOCAL_PING.leaf,
        nx: LOCAL_PING.nx,
    });
    let mut phases = Phases::new();
    unsafe {
        let selection = [&LFO_SHAPE, &LOCAL_PING_USER, &ENTRY_DEMO];
        let (mut l, plan) = planned(dev, &image, &[], &selection, &services);
        let _dsp = dsp_env(dev, &l, &plan);
        let leaves = ["lfo-shape", "local-ping-user", "entry-demo"];
        // the files of the card: one module and one service are damaged (a torn write: the first byte), so the loader finds no table for the module and no provider for the other
        let damage = |l: &mut Boot, prefix: &[u8]| {
            l.m.files
                .iter_mut()
                .find(|(n, _)| n.starts_with(prefix))
                .unwrap()
                .1[0] ^= 0xff
        };
        damage(&mut l, b"modules/lfo-shape.dkm");
        damage(&mut l, b"services/local-ping");
        crate::dlog::dk_debug_log_sink_set(sink as usize as *mut u8, core::ptr::null_mut());
        EVENTS.lock().unwrap_or_else(|x| x.into_inner()).clear();
        card_reset();
        let mut rt = Box::new(Rt::new(&mut l as *mut _));
        boot_from_card(&mut l, &mut rt, boot_record(&leaves));
        phases.phase(
            "boot with a damaged module and a damaged service",
            std::format!("loaded modules {}", crate::boot::info().loaded),
            &l,
            &mut rt,
        );

        let mut job = Job::new();
        // an unload of a file no module was loaded from: refused when the job is queued
        let result = live_job(
            &mut l,
            &mut rt,
            &mut job,
            LIVE_UNLOAD,
            "modules/not-there.dkm\0",
        );
        phases.phase(
            "unload of a module that is not loaded",
            std::format!("result {result}"),
            &l,
            &mut rt,
        );

        // a reload whose new file is damaged: the job's check refuses it before anything is unloaded
        damage(&mut l, b"modules/entry-demo.dkm");
        let result = live_job(
            &mut l,
            &mut rt,
            &mut job,
            LIVE_RELOAD,
            "modules/entry-demo.dkm\0",
        );
        phases.phase(
            "reload of a module whose file is damaged",
            std::format!("result {result}"),
            &l,
            &mut rt,
        );
        damage(&mut l, b"modules/entry-demo.dkm");

        // a live load of the file that is still damaged: the check refuses it and the index stays free
        let result = live_job(
            &mut l,
            &mut rt,
            &mut job,
            LIVE_LOAD,
            "modules/lfo-shape.dkm\0",
        );
        phases.phase(
            "live load of a damaged file",
            std::format!("result {result}"),
            &l,
            &mut rt,
        );

        // safe boot by the one-shot flag of the record: the flag is cleared and written, nothing loads
        let mut rec = boot_record(&leaves);
        rec.flags |= dkrt_sdk::sel::FLAG_SAFE_ONCE;
        boot_from_card(&mut l, &mut rt, rec);
        phases.safe_phase("safe boot by the record's flag", &l);

        // safe boot by the boot counter at its limit
        let mut rec = boot_record(&leaves);
        rec.boot_counter = dkrt_sdk::SAFE_AFTER;
        boot_from_card(&mut l, &mut rt, rec);
        phases.safe_phase("safe boot by the boot counter", &l);
        crate::dlog::dk_debug_log_sink_set(core::ptr::null_mut(), core::ptr::null_mut());
    }
    let _ = clean_state();
    phases.golden
}

/// What the child prints before its result, so that the parent knows the run happened: `<MARK> ran <device>`, or `<MARK> skipped <device>` for a device without firmware.
const MARK: &str = "boot-effects-child";
const CHILD: &str = "BOOT_EFFECTS_DEVICE";
const SCENARIO: &str = "BOOT_EFFECTS_SCENARIO";
/// The scenarios and the snapshot of each: the good boot with the live job, and the error paths.
const SCENARIOS: [(&str, &str); 2] = [("good", "boot-effects"), ("errors", "boot-effects-errors")];

/// The run of one device, in a process of its own. Tried in one process (RC2s-1 S3): the 32-bit narrowing is gone (the arena, the slot words and the code addresses are names
/// of the fake platform's address map), and the same scenario repeats for both devices in one process with the same text. A scenario run after another differs: the runtime
/// keeps state that nothing resets between two runs (the path words and tick words of the records), and so do the services (rows, builds and assignments in
/// statics, until RC3 gives them an owner that resets). So each scenario and device starts a fresh process: the parent below starts this test (`--ignored`: it is no test of
/// its own), and it fails when the variable is not set.
#[test]
#[ignore = "run by the_boot_effects_match_the_snapshots, one process per device"]
fn boot_effects_of_one_device() {
    let name = std::env::var(CHILD).unwrap_or_else(|_| {
        panic!("{CHILD} is not set: this test is started by the_boot_effects_match_the_snapshots")
    });
    let dev = *DEVICES
        .iter()
        .find(|d| d.name == name)
        .expect("a device name");
    let _g = clean();
    let scenario = std::env::var(SCENARIO).expect("a scenario");
    let (_, file) = *SCENARIOS
        .iter()
        .find(|(s, _)| *s == scenario)
        .expect("a scenario name");
    if dev.has_firmware() {
        let golden = if scenario == "good" {
            device_run(dev)
        } else {
            device_errors_run(dev)
        };
        // only the cleanup of a failed load touches a block after its free
        assert_eq!(golden.contains("freed:"), scenario == "errors", "freed blocks in the {scenario} scenario");
        golden.check(&std::format!("{file}-{}.txt", dev.name));
        std::println!("{MARK} ran {name}");
    } else {
        std::println!("{MARK} skipped {name}");
    }
}

#[test]
fn the_boot_effects_match_the_snapshots() {
    // the path of the child as the filter of the test binary knows it (without the crate name)
    let child = concat!(module_path!(), "::boot_effects_of_one_device")
        .split_once("::")
        .unwrap()
        .1;
    for (scenario, _) in SCENARIOS {
        for dev in DEVICES {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    child,
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env(CHILD, dev.name)
                .env(SCENARIO, scenario)
                .output()
                .unwrap();
            let text = std::format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.status.success(), "{} {scenario}: {text}", dev.name);
            // a filter that matches no test (a rename) also exits with success
            assert!(
                text.contains("1 passed;"),
                "{} {scenario}: the child ran no test:\n{text}",
                dev.name
            );
            let want = std::format!(
                "{MARK} {} {}",
                if dev.has_firmware() { "ran" } else { "skipped" },
                dev.name
            );
            assert!(
                text.contains(&want),
                "{} {scenario}: the child did not report `{want}`:\n{text}",
                dev.name
            );
        }
    }
}
