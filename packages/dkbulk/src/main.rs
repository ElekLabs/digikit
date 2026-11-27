//! `dkbulk`: the USB bulk link of the digikit usb-fast module, for the command-line tools.
//!
//!     dkbulk list                          the devices that offer a bulk interface: ID (bus:address), name, interface number, `out-only` for an interface without IN endpoint
//!     dkbulk descriptors [ID]              read-only check (macOS/Linux): read the device, configuration, BOS and MS OS 2.0 descriptors Windows needs for WinUSB and validate them; exit 0 all pass, 1 a check fails or no BOS, 2 no device or a transfer fails
//!     dkbulk open [--out-only] [ID]       bridge: whole request frames on stdin go to the device, every reply frame comes out on stdout
//!                                          (the TypeScript session code of tools/devtools/wb.mjs speaks the protocol over it);
//!                                          --out-only: claim the OUT-only interface (USB AUDIO/MIDI), nothing comes out on stdout (the replies come over SysEx)
//!     dkbulk echo [--id ID] [--sizes N,N,...]
//!                                          the first hardware run (test/usb-echo, step 3): send each size of bytes, read the same bytes back, compare, print MB/s
//!     dkbulk send [--id ID] [--interface N] [--ep ADDR] [--bytes N]
//!                                          claim only interface N (default 5) and write N bytes (default 1 MiB) to its bulk OUT endpoint ADDR (default 0x02), no IN endpoint; print MB/s
//!     dkbulk put [--id ID] [--root N] [--chunk N] [--trailer] [--replace] LOCAL REMOTE [LOCAL REMOTE ...]
//!                                          upload files with the Rust client and print MB/s (--trailer: the expected hash covers 16 zero bytes more, as on the DT2 +Drive)
use std::io::{Read, Write};
use std::process::exit;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use dkbulk::link::InPipe;
use dkbulk::usb::{self, UsbLink};
use dkbulk::{Client, Options, gen_bulk, proto};

fn die(code: i32, msg: &str) -> ! {
    eprintln!("dkbulk: {msg}");
    exit(code);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = if args.is_empty() {
        String::new()
    } else {
        args.remove(0)
    };
    match cmd.as_str() {
        "list" => match usb::list() {
            Ok(v) => v.iter().for_each(|c| {
                println!(
                    "{}\t{}\tinterface {}{}",
                    c.id,
                    c.name,
                    c.interface,
                    if c.out_only { "\tout-only" } else { "" }
                )
            }),
            Err(e) => die(1, &e),
        },
        "open" => {
            let out_only = args
                .iter()
                .position(|a| a == "--out-only")
                .map(|i| args.remove(i))
                .is_some();
            let id = args.first().map(String::as_str);
            if out_only {
                bridge_out_only(id)
            } else {
                bridge(id)
            }
        }
        "descriptors" => descriptors(args.first().map(String::as_str)),
        "put" => put(args),
        "echo" => echo(args),
        "send" => send(args),
        _ => die(
            2,
            "usage: dkbulk list | dkbulk descriptors [ID] | dkbulk open [--out-only] [ID] | dkbulk send [--id ID] [--interface N] [--ep ADDR] [--bytes N] |dkbulk echo [--id ID] [--sizes N,N,...] | dkbulk put [--id ID] [--root N] [--chunk N] [--trailer] [--replace] LOCAL REMOTE ...",
        ),
    }
}

/// Read the descriptors Windows needs for WinUSB (device, configuration, BOS, MS OS 2.0 set), print them and the PASS/FAIL list.
fn descriptors(id: Option<&str>) {
    use dkbulk::msos::{self, Check};
    let (device, name) = usb::open_device(id).unwrap_or_else(|e| die(2, &e));
    let read = |vendor: bool, req: u8, value: u16, index: u16, len: usize| {
        usb::control_in(&device, vendor, req, value, index, len as u16)
    };
    println!("device {name}");
    let mut checks: Vec<Check> = Vec::new();

    let dev = read(false, 6, 0x0100, 0, 18)
        .unwrap_or_else(|e| die(2, &format!("device descriptor: {e}")));
    println!("device descriptor: {}", msos::hex(&dev));
    let bcd = msos::bcd_usb(&dev).unwrap_or_else(|| die(2, "device descriptor: too short"));
    println!("  bcdUSB {bcd:#06x}");
    checks.push(Check {
        name: format!("bcdUSB {bcd:#06x} >= 0x0201"),
        ok: bcd >= 0x0201,
    });

    let head = read(false, 6, 0x0200, 0, 9)
        .unwrap_or_else(|e| die(2, &format!("configuration descriptor: {e}")));
    let total = msos::config_total_length(&head)
        .unwrap_or_else(|| die(2, "configuration descriptor: bad header"));
    let cfg = read(false, 6, 0x0200, 0, total)
        .unwrap_or_else(|e| die(2, &format!("configuration descriptor: {e}")));
    println!("configuration descriptor: {}", msos::hex(&cfg));
    let interface = msos::vendor_interface(&cfg);
    match interface {
        Some(n) => println!("  vendor-class (0xff) interface {n}"),
        None => println!("  no vendor-class (0xff) interface"),
    }
    checks.push(Check {
        name: "configuration has a vendor-class (0xff) interface".into(),
        ok: interface.is_some(),
    });

    fn no_bos(why: String) -> ! {
        println!("no BOS (firmware without MS OS 2.0 descriptors): {why}");
        exit(1);
    }
    let bos_head = read(false, 6, 0x0F00, 0, 5).unwrap_or_else(|e| no_bos(e.to_string()));
    let bos_total =
        msos::bos_total_length(&bos_head).unwrap_or_else(|| no_bos("bad BOS header".into()));
    let bos = read(false, 6, 0x0F00, 0, bos_total).unwrap_or_else(|e| no_bos(e.to_string()));
    println!("BOS descriptor: {}", msos::hex(&bos));
    let (bos_checks, info) = msos::check_bos(&bos);
    checks.extend(bos_checks);

    if let Some(b) = info {
        println!(
            "  MS OS 2.0 platform capability: dwWindowsVersion {:#010x}, set length {}, vendor code {:#04x}",
            b.windows_version, b.set_len, b.vendor_code
        );
        let set = read(true, b.vendor_code, 0, 7, b.set_len as usize).unwrap_or_else(|e| {
            die(
                2,
                &format!(
                    "MS OS 2.0 descriptor set (vendor request {:#04x}): {e}",
                    b.vendor_code
                ),
            )
        });
        println!(
            "MS OS 2.0 descriptor set ({} bytes): {}",
            set.len(),
            msos::hex(&set)
        );
        msos::describe_set(&set)
            .iter()
            .for_each(|l| println!("  {l}"));
        checks.extend(msos::check_set(&set, b.set_len, interface));
    }

    let mut failed = false;
    for c in &checks {
        failed |= !c.ok;
        println!("{}  {}", if c.ok { "PASS" } else { "FAIL" }, c.name);
    }
    exit(i32::from(failed));
}

/// Write N bytes to a bulk OUT endpoint of one interface that has no IN endpoint (default: interface 5, EP 0x02, 1 MiB) and print bytes and MB/s.
fn send(mut args: Vec<String>) {
    let mut take = |flag: &str| -> Option<String> {
        let at = args.iter().position(|a| a == flag)?;
        args.remove(at);
        (at < args.len()).then(|| args.remove(at))
    };
    let device = take("--id");
    let interface: u8 =
        take("--interface").map_or(5, |v| v.parse().unwrap_or_else(|_| die(2, "--interface N")));
    let ep: u8 = take("--ep").map_or(0x02, |v| {
        let h = v.trim_start_matches("0x");
        u8::from_str_radix(h, 16).unwrap_or_else(|_| die(2, "--ep ADDR (hex)"))
    });
    let bytes: usize = take("--bytes").map_or(1 << 20, |v| {
        v.parse().unwrap_or_else(|_| die(2, "--bytes N"))
    });
    let mut link =
        usb::OutOnly::open(device.as_deref(), interface, ep).unwrap_or_else(|e| die(1, &e));
    let start = Instant::now();
    let sent = link
        .write(bytes, 16384, 4, Duration::from_secs(10))
        .unwrap_or_else(|e| die(1, &format!("send: {e}")));
    let s = start.elapsed().as_secs_f64();
    println!(
        "sent {sent} bytes to interface {interface} endpoint {ep:#04x} in {s:.3} s, {:.2} MB/s",
        sent as f64 / 1e6 / s
    );
}

/// The bridge for an OUT-only interface (USB AUDIO/MIDI): whole request frames on stdin go OUT; nothing comes out on stdout, the caller reads the replies over SysEx.
fn bridge_out_only(id: Option<&str>) {
    let mut link = usb::OutOnly::open_auto(id).unwrap_or_else(|e| die(1, &e));
    eprintln!("open");
    let (mut stdin, mut buf) = (std::io::stdin().lock(), Vec::<u8>::new());
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        match stdin.read(&mut chunk) {
            Ok(0) => exit(0),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => die(1, &format!("stdin: {e}")),
        }
        while let Ok(total) = proto::frame_len(&buf) {
            if buf.len() < total {
                break;
            }
            let frame: Vec<u8> = buf.drain(..total).collect();
            if let Err(e) = link.send(&frame, Duration::from_secs(10)) {
                die(1, &e.to_string());
            }
        }
        if buf.len() >= gen_bulk::HEADER && proto::frame_len(&buf).is_err() {
            die(1, "stdin: not a bulk frame");
        }
    }
}

fn bridge(id: Option<&str>) {
    let link = UsbLink::open(id).unwrap_or_else(|e| die(1, &e));
    eprintln!("open");
    let (mut tx, mut rx, _interface) = link.split();
    std::thread::spawn(move || {
        let (mut stdin, mut buf) = (std::io::stdin().lock(), Vec::<u8>::new());
        let mut chunk = vec![0u8; 1 << 16];
        loop {
            match stdin.read(&mut chunk) {
                Ok(0) => exit(0),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) => die(1, &format!("stdin: {e}")),
            }
            while let Ok(total) = proto::frame_len(&buf) {
                if buf.len() < total {
                    break;
                }
                let frame: Vec<u8> = buf.drain(..total).collect();
                if let Err(e) = tx.send(&frame, Duration::from_secs(10)) {
                    die(1, &e.to_string());
                }
            }
            if buf.len() >= gen_bulk::HEADER && proto::frame_len(&buf).is_err() {
                die(1, "stdin: not a bulk frame");
            }
        }
    });
    let mut out = std::io::stdout().lock();
    loop {
        match rx.recv(Duration::from_secs(3600)) {
            Ok(f) => {
                if out.write_all(&f).and_then(|_| out.flush()).is_err() {
                    exit(0);
                }
            }
            Err(dkbulk::LinkError::Timeout) => {}
            Err(e) => die(1, &e.to_string()),
        }
    }
}

fn put(mut args: Vec<String>) {
    let mut take = |flag: &str| -> Option<String> {
        let at = args.iter().position(|a| a == flag)?;
        args.remove(at);
        (at < args.len()).then(|| args.remove(at))
    };
    let device = take("--id");
    let root: u32 = take("--root").map_or(1, |v| v.parse().unwrap_or_else(|_| die(2, "--root N")));
    let chunk: usize = take("--chunk").map_or(gen_bulk::DATA_MAX, |v| {
        v.parse().unwrap_or_else(|_| die(2, "--chunk N"))
    });
    let replace = args
        .iter()
        .position(|a| a == "--replace")
        .map(|i| args.remove(i))
        .is_some();
    let trailer = args
        .iter()
        .position(|a| a == "--trailer")
        .map(|i| args.remove(i))
        .is_some();
    if args.is_empty() || !args.len().is_multiple_of(2) {
        die(2, "put wants LOCAL REMOTE pairs");
    }
    let link = UsbLink::open(device.as_deref()).unwrap_or_else(|e| die(1, &e));
    let mut client = Client::new(
        link,
        Options {
            chunk,
            ..Options::default()
        },
    );
    match client.info() {
        Ok(i) => eprintln!(
            "device: bulk version {}, data_max {}, align {}",
            i.version, i.data_max, i.align
        ),
        Err(e) => die(1, &format!("INFO: {e}")),
    }
    let cancel = AtomicBool::new(false);
    let (mut bytes, start) = (0usize, Instant::now());
    for pair in args.chunks(2) {
        let data = std::fs::read(&pair[0]).unwrap_or_else(|e| die(1, &format!("{}: {e}", pair[0])));
        let mut hashed = data.clone();
        if trailer {
            hashed.extend_from_slice(&[0; 16]);
        }
        let expected = dksample::hash::content_hash(&hashed) | 1;
        let t = Instant::now();
        match client.put(
            root,
            &pair[1],
            &data,
            Some(expected),
            replace,
            &cancel,
            &mut |_, _| {},
        ) {
            Ok(h) => {
                let s = t.elapsed().as_secs_f64();
                println!(
                    "{}\t{} bytes\t{:.3} s\t{:.2} MB/s\thash {h:08x}",
                    pair[1],
                    data.len(),
                    s,
                    data.len() as f64 / 1e6 / s
                );
                bytes += data.len();
            }
            Err(e) => die(1, &format!("{}: {e}", pair[1])),
        }
    }
    let s = start.elapsed().as_secs_f64();
    println!(
        "total {bytes} bytes {:.3} s {:.2} MB/s",
        s,
        bytes as f64 / 1e6 / s
    );
}

/// The sizes of the first hardware run (docs/plans/usb-fast-hardware.md, DN2 run 1): around the 512-byte packet, a 16 KiB buffer, a frame and 1 MiB.
const ECHO_SIZES: &[usize] = &[1, 511, 512, 513, 4096, 16384, 65536, 1_048_576];

/// Send `size` bytes of a pattern to the echo module of `test/usb-echo` and read them back: the bytes must be identical. Raw bytes, not frames.
fn echo(mut args: Vec<String>) {
    let mut take = |flag: &str| -> Option<String> {
        let at = args.iter().position(|a| a == flag)?;
        args.remove(at);
        (at < args.len()).then(|| args.remove(at))
    };
    let device = take("--id");
    let sizes: Vec<usize> = take("--sizes").map_or_else(
        || ECHO_SIZES.to_vec(),
        |v| {
            v.split(',')
                .map(|n| {
                    n.trim()
                        .parse()
                        .unwrap_or_else(|_| die(2, "--sizes N,N,..."))
                })
                .collect()
        },
    );
    let mut link = UsbLink::open(device.as_deref()).unwrap_or_else(|e| die(1, &e));
    let mut failed = false;
    for (i, &size) in sizes.iter().enumerate() {
        let data: Vec<u8> = (0..size)
            .map(|k| (k.wrapping_mul(7) + (k >> 8) + i) as u8)
            .collect();
        let start = Instant::now();
        // The reads run beside the send, because the device holds only a few buffers and waits for the host to take its replies.
        let back = std::thread::scope(|scope| {
            let tx = &mut link.tx;
            let data = &data;
            let sender = scope.spawn(move || tx.send(data, Duration::from_secs(10)));
            let mut back = Vec::with_capacity(size);
            while back.len() < size {
                match link.rx.read(Duration::from_secs(10)) {
                    Ok(b) => back.extend_from_slice(&b),
                    Err(e) => die(
                        1,
                        &format!("read after {} of {size} bytes: {e}", back.len()),
                    ),
                }
            }
            if let Err(e) = sender.join().expect("sender thread") {
                die(1, &format!("send of {size} bytes: {e}"));
            }
            back
        });
        let s = start.elapsed().as_secs_f64();
        let same = back == data;
        failed |= !same;
        println!(
            "{size}\t{}\t{:.3} s\t{:.2} MB/s (both ways)",
            if same { "identical" } else { "DIFFERENT" },
            s,
            2.0 * size as f64 / 1e6 / s
        );
    }
    if failed {
        exit(1);
    }
}
