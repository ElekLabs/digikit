#!/usr/bin/env node
// Workbench actions from the command line, through the workbench's own code: the dk-session TypeScript (prepareUpload, openSamples, put,
// verify) over the dkmidi Rust MIDI port (packages/dkmidi, the code the desktop app uses), here as a child process on stdin and stdout.
//
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] ports
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] info
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] samples ls [FOLDER]
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] samples put FILE... [--folder F] [--bulk auto|off|force] [--bulk-bin PATH] [--verify auto|always|never] [--speed MASK]
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] samples rm FOLDER...
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] store ls [PREFIX]
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] store put LOCAL REMOTE
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] store rm REMOTE
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] [--bulk-bin PATH] [--gap MS] [--wait MS] [--card-log] [--continue] takeover-cycle N
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] log card [on|off|get [--out DIR]|summary [DIR]]
//     node tools/devtools/wb.mjs [--bulk-bin PATH] bulk-probe [--bytes N]
//
// `bulk-probe` writes N bytes (default 1 MiB) to bulk OUT endpoint 0x02 of interface 5 in USB AUDIO/MIDI (`dkbulk send`) and prints bytes and MB/s.
// With the debug module it reads the MOD_LOG events "ua.state" and "ua.rx" before and after and prints the device's byte count and whether it matches.
// `log card` prints the card log state of the debug module (the log written to data/digikit/debug on the card, kept across restarts); `on` / `off` switch it;
// `get` reads the card log files and writes each raw (.bin) and decoded (.txt) to DIR (default out/runs/cardlog/<YYYYmmdd-HHMMSS>/), the older boot first.
// `summary [DIR]` prints one line per USB switch from the saved .bin files in DIR (default: the newest folder of out/runs/cardlog); it works offline.
// `takeover-cycle --card-log` switches the card log on before the first cycle.
// `takeover-cycle --continue` does not stop at a failed cycle: it waits for the device (polls INFO every 2 s; asks for a replug, after 60 s for a power cycle),
// reads the card log into the run folder, prints its summary and goes on with the next cycle. At the end it prints the totals.
//
// `takeover-cycle N` is a hardware experiment for the USB take-over (docs/findings/123-usb-takeover.md): N times TAKEOVER, bulk INFO, RELEASE, and per switch it reads
// the macOS USB object ids of the Elektron device (`ioreg`, vendor 0x1935) to see whether the host saw the device leave and return. It stops at the first cycle
// where the ids did not change or MIDI does not answer, and always sends RELEASE on exit or Ctrl-C. It needs the device in USB AUDIO/MIDI with usb-fast loaded.
// `store` works on the card's store by path (for example data/tools/bootscreen/x.dkbs); `store put` verifies the file after the upload.
// --port: a part of the MIDI port name (default "Digitakt"). --bin: the dkmidi binary (default $DKMIDI_BIN, else the nogen target dir;
// `mise run dkmidi:build` builds it). `samples put` needs apps/web/public/dksample.wasm (`mise run wasm:dksample`).
// `samples put` sends the file bytes over the USB bulk link when the device offers its interface (usb-fast; the `dkbulk` bridge, `mise run dkbulk:build`,
// $DKBULK_BIN or --bulk-bin) and over SysEx or MidiRpc otherwise; --bulk off forces SysEx, --bulk force fails when there is no bulk link. It prints the
// In USB AUDIO/MIDI with core/usb-bulk-audio, `samples put` uses the OUT-only bulk interface automatically (frames over `dkbulk open --out-only`, replies over
// the SysEx 'R' poll; audio and MIDI keep running). Without that interface (old firmware) it stays on SysEx; with --takeover (experimental: on macOS the device
// can end up unreachable until a power cycle) it takes USB over for the run (the device switches to USB MIDI in RAM, USB audio pauses) and gives it back when
// the run ends. --bulk off never touches the USB mode.
// --speed MASK (4 bits, decimal or 0x): sets the four speed switches through usb-fast STATUS 'S' before the put (bit 0 hash once, bit 1 USB receive ring,
// bit 2 read-back hash overlap, bit 3 large card commands; all on at boot); the run prints the resulting mask and the usb-fast STATUS words 18 (staging bytes)
// and 19 (receive ring slots): the hardware A/B of docs/findings/126-usb-fast-transfer-ceilings.md.
// link and MB/s of every file and of the run: the repeatable bench (docs/plans/usb-fast.md step 6): run it once with --bulk off and once with auto.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  BulkTakeOver,
  DkSession,
  ProcessBulkLink,
  chooseBulk,
  eventLine,
  parseCardLog,
  phyIdLine,
  phyIds,
  runtimeEvent,
  summarizeCardLog,
  switchLine,
} from '../../packages/dk-session/src/index.ts';
import { ProcessTransport } from '../../packages/dk-session/src/process-transport.ts';
import { Sample, prepareUpload } from '../../packages/dk-session/src/dksample.ts';
import { openSamples, skipVerifyAfterBulk } from '../../packages/dk-session/src/samples.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
// the patchkit rule (tools/emulator/native_target.py): a worktree builds into its own target dir; no generated core is the "nogen" key
const targetRoot = existsSync(path.join(root, '.git', 'HEAD'))
  ? path.join(root, 'out/native/target')
  : path.join(root, 'dist/native-target');
const usage =
  'usage: node tools/devtools/wb.mjs [--port SUB] [--bin PATH] ports | info | samples ls [FOLDER] | samples put FILE... [--folder F] | samples rm FOLDER... | store ls [PREFIX] | store put LOCAL REMOTE | store rm REMOTE | log card [on|off|get [--out DIR]|summary [DIR]] | takeover-cycle N [--gap MS] [--wait MS] [--card-log] [--continue] | bulk-probe [--bytes N] [--bulk-bin PATH]; --takeover: take USB over for a bulk run in USB AUDIO/MIDI (experimental, off by default)';

// options anywhere on the line; the rest are the command words
const args = process.argv.slice(2);
const opts = {
  port: 'Digitakt',
  bin: process.env.DKMIDI_BIN || path.join(targetRoot, 'nogen/release/dkmidi'),
  folder: '',
  bulk: 'auto',
  verify: 'auto',
  speed: undefined,
  'bulk-bin': process.env.DKBULK_BIN || path.join(targetRoot, 'nogen/release/dkbulk'),
  takeover: false,
  gap: '2000',
  wait: '10000',
};
const words = [];
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (
    a === '--port' ||
    a === '--bin' ||
    a === '--folder' ||
    a === '--bulk' ||
    a === '--verify' ||
    a === '--bulk-bin' ||
    a === '--speed' ||
    a === '--gap' ||
    a === '--wait' ||
    a === '--bytes' ||
    a === '--out'
  ) {
    if (i + 1 >= args.length) fail(`${a} needs a value`);
    opts[a.slice(2)] = args[++i];
  } else if (a === '--takeover') opts.takeover = true;
  else if (a === '--card-log') opts['card-log'] = true;
  else if (a === '--continue') opts.continue = true;
  else words.push(a);
}

/** The four speed switches of usb-fast through STATUS 'S' (SysEx): each bit of MASK, then the mask the device reports. */
async function setSpeed(session, mask) {
  let now;
  const refused = [];
  for (let bit = 0; bit < 4; bit++) {
    const r = await session.request(0x37, Uint8Array.from([0x53, bit, (mask >> bit) & 1]));
    if (r.status === 0) now = Buffer.from(r.body).readUInt32BE(0);
    else refused.push(bit);
  }
  if (refused.length)
    fail(`--speed: the device refused bit ${refused.join(', ')} (status E_STATE: the adapter is not in the image)`, 1);
  return now;
}

/** The usb-fast STATUS words 18 (staging bytes) and 19 (receive ring slots of the live block), and the speed mask. */
async function speedReport(session) {
  const r = await session.request(0x37, new Uint8Array());
  const m = await session.request(0x37, Uint8Array.from([0x53]));
  const b = Buffer.from(r.body);
  const word = (i) => (b.length >= 4 * i + 4 ? b.readUInt32BE(4 * i) : undefined);
  const mask = m.status === 0 ? Buffer.from(m.body).readUInt32BE(0) : undefined;
  return `speed mask ${mask === undefined ? '?' : `0x${mask.toString(16)}`} staging bytes ${word(18) ?? '?'} ring slots ${word(19) ?? '?'}`;
}

function fail(msg, code = 2) {
  console.error(`wb: ${msg}`);
  process.exit(code);
}

if (!existsSync(opts.bin)) fail(`no dkmidi binary at ${opts.bin} (run: mise run dkmidi:build)`);

/** Start the bridge on the port and open a session on it. */
function connect(timeoutMs = 60000) {
  const child = spawn(opts.bin, ['open', opts.port], { stdio: ['pipe', 'pipe', 'inherit'] });
  const exited = new Promise((resolve) => child.on('exit', resolve));
  child.on('error', (e) => fail(`cannot start ${opts.bin}: ${e.message}`, 1));
  child.stdin.on('error', () => {});
  const transport = new ProcessTransport(
    (b) => child.stdin.write(b),
    (cb) => child.stdout.on('data', (d) => cb(new Uint8Array(d))),
  );
  const session = new DkSession(transport, { timeoutMs });
  return {
    session,
    transport,
    async close() {
      child.stdin.end();
      // the bridge of a port that vanished may not exit by itself
      const timer = setTimeout(() => child.kill(), 3000);
      await exited;
      clearTimeout(timer);
    },
  };
}

/**
 * `connect`, then one quick INFO; a device that does not reply is tried again for up to `waitMs` with a new bridge. The device may still re-enumerate after
 * the previous run (RELEASE): its MIDI port is gone or does not answer yet. After the time the last link is returned anyway (a stock device has no INFO) and
 * the first real request reports the error. An answer with an error status is no re-enumeration: no retry.
 */
async function connectReady(waitMs = 8000) {
  const deadline = Date.now() + waitMs;
  for (;;) {
    const link = connect();
    try {
      await link.session.info(true);
      return link;
    } catch (e) {
      if (e?.kind !== 'timeout' || Date.now() > deadline) return link;
      await link.close();
      await new Promise((r) => setTimeout(r, 500));
    }
  }
}

async function withSession(run) {
  const link = await connectReady();
  try {
    return await run(link.session);
  } finally {
    await link.close();
  }
}

/** The `dkbulk` bridge as a BulkOpener; the exit promises of the children it starts go to `children`. */
function bulkOpener(bin, children) {
  /** `dkbulk list`: pair interfaces, or the OUT-only ones (USB AUDIO/MIDI, marked `out-only` in the fourth field). */
  const list = (outOnly) => {
    const listed = spawnSync(bin, ['list'], { encoding: 'utf8' });
    if (listed.status !== 0) throw new Error(`dkbulk list failed: ${listed.stderr.trim()}`);
    return listed.stdout
      .split('\n')
      .filter(Boolean)
      .map((line) => {
        const [id, name, , flag] = line.split('\t');
        return { id, name, outOnly: flag === 'out-only' };
      })
      .filter((c) => c.outOnly === outOnly)
      .map(({ id, name }) => ({ id, name }));
  };
  const start = (args, onData, poll) => {
    const child = spawn(bin, args, { stdio: ['pipe', 'pipe', 'inherit'] });
    const exited = new Promise((resolve) => child.on('exit', resolve));
    child.stdin.on('error', () => {});
    children.push(exited);
    return new ProcessBulkLink(
      (b) => child.stdin.write(b),
      (cb) => onData && child.stdout.on('data', (d) => cb(new Uint8Array(d))),
      () => child.stdin.end(),
      poll,
    );
  };
  return {
    async candidates() {
      return list(false);
    },
    async open(id) {
      return start(['open', id], true);
    },
    async outOnlyCandidates() {
      return list(true);
    },
    // frames go OUT through `dkbulk open --out-only`; the replies come from the SysEx 'R' poll
    async openOutOnly(id, poll) {
      return start(['open', '--out-only', id], false, poll);
    },
  };
}

/**
 * The bulk session for the device on the MIDI port (--port), or undefined (--bulk off, no dkbulk binary, no candidate, no candidate with this device's nonce).
 * The selection is the dk-session `chooseBulk` the desktop app uses: SET_NONCE over SysEx to the MIDI target, then each `dkbulk list` candidate is opened
 * (`dkbulk open ID`, a child process) and asked INFO; only the one that returns the nonce is used. --bulk force: those are errors.
 * In USB AUDIO/MIDI with core/usb-bulk-audio, `chooseBulk` finds the OUT-only interface (`dkbulk open --out-only`, replies over the SysEx 'R' poll): no take-over, no USB CONFIG change.
 * When no device offers any interface and usb-fast reports that the device is not in USB MIDI mode (an old firmware without core/usb-bulk-audio), only with --takeover the device is taken over (`BulkTakeOver`
 * of dk-session: TAKEOVER, the device re-enumerates in USB MIDI mode, the handshake) and given back by `close`, at once. `holder.link` is the SysEx link; the
 * take-over replaces it (the MIDI port goes away and returns), so callers read `holder.link.session` after this returns.
 */
async function connectBulk(holder) {
  const force = opts.bulk === 'force';
  if (!['auto', 'off', 'force'].includes(opts.bulk)) fail('--bulk is auto, off or force');
  if (opts.bulk === 'off') return undefined;
  const none = (why) => (force ? fail(why, 1) : (console.error(`wb: no bulk link (${why}); using SysEx`), undefined));
  const bin = opts['bulk-bin'];
  if (!existsSync(bin)) return none(`no dkbulk binary at ${bin}; run: mise run dkbulk:build`);
  const children = [];
  const opener = bulkOpener(bin, children);
  const notes = [];
  let choice = await chooseBulk(holder.link.session, opener, (n) => notes.push(n), { timeoutMs: 20000 });
  let takeover;
  if (!choice && (await holder.link.session.usbFastWrongMode())) {
    if (!opts.takeover)
      return none(
        'the device is in USB AUDIO/MIDI and has no OUT-only bulk interface (core/usb-bulk-audio not loaded?); set USB CONFIG to USB MIDI on the device, or pass --takeover (experimental)',
      );
    console.error('wb: the device is in USB AUDIO/MIDI: taking USB over for this run (USB audio pauses)');
    takeover = new BulkTakeOver(
      {
        sysex: () => holder.link.session,
        // the device leaves the bus about 50 ms after the reply and returns in USB MIDI mode: reopen the MIDI port until it answers
        async reenumerated(timeoutMs) {
          const deadline = Date.now() + timeoutMs;
          await new Promise((r) => setTimeout(r, 1500));
          await holder.link.close();
          for (;;) {
            const link = connect(2000);
            try {
              await link.session.info();
              holder.link = link;
              return;
            } catch (e) {
              await link.close();
              if (Date.now() > deadline) throw e;
              await new Promise((r) => setTimeout(r, 500));
            }
          }
        },
      },
      opener,
      { lingerMs: 0, note: (n) => notes.push(n), bulk: { timeoutMs: 20000 } },
    );
    choice = await takeover.begin();
  }
  if (!choice) {
    await Promise.all(children);
    return none(notes.at(-1) ?? 'no device offers a bulk interface; is usb-fast loaded?');
  }
  for (const n of notes) console.error(`wb: ${n}`);
  return {
    session: choice.session,
    takenOver: takeover !== undefined,
    async close() {
      await choice.link.close();
      await Promise.all(children);
      if (takeover) {
        // wb exits: the mode goes back at once (RELEASE; the device switches about 50 ms after the reply)
        takeover.end();
        await takeover.close();
      }
    },
  };
}

async function loadSample() {
  const wasm = path.join(root, 'apps/web/public/dksample.wasm');
  if (!existsSync(wasm)) fail('apps/web/public/dksample.wasm is missing (run: mise run wasm:dksample)', 1);
  return Sample.create(Uint8Array.from(readFileSync(wasm)));
}

/** The card log state in one line. */
function cardLogText(s) {
  if (!s) return 'not supported (this debug module has no card log)';
  const since = s.msSinceWrite === null ? 'no write yet' : `last write ${(s.msSinceWrite / 1000).toFixed(1)} s ago`;
  return `${s.state} (switch ${s.on ? 'on' : 'off'}), file cardlog-${s.file}.bin, ${s.writes} writes, ${since}, last status ${s.lastStatus}`;
}

/** The default run folder name, in local time: out/runs/cardlog/<YYYYmmdd-HHMMSS>. */
function cardLogStampDir() {
  const now = new Date();
  const p2 = (n) => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}${p2(now.getMonth() + 1)}${p2(now.getDate())}-${p2(now.getHours())}${p2(now.getMinutes())}${p2(now.getSeconds())}`;
  return path.join(root, 'out/runs/cardlog', stamp);
}

/** The parsed card log files of the saved .bin files in `dir` (a file that does not parse is skipped). */
function readCardLogDir(dir) {
  const files = [];
  for (const name of readdirSync(dir).filter((n) => n.endsWith('.bin')))
    try {
      files.push(parseCardLog(Uint8Array.from(readFileSync(path.join(dir, name)))));
    } catch (e) {
      console.error(`wb: skipped ${name}: ${e instanceof Error ? e.message : e}`);
    }
  return files;
}

/** `log card summary [DIR]`: one line per USB switch of the saved files; DIR defaults to the newest folder of out/runs/cardlog. */
function cardLogSummary(dirArg) {
  let dir = dirArg && path.resolve(dirArg);
  if (!dir) {
    const base = path.join(root, 'out/runs/cardlog');
    const names = existsSync(base) ? readdirSync(base).filter((n) => statSync(path.join(base, n)).isDirectory()) : [];
    if (names.length === 0) fail(`no card log folder in ${base} (run: log card get)`, 1);
    dir = path.join(base, names.sort().at(-1));
  }
  if (!existsSync(dir)) fail(`no folder ${dir}`, 1);
  console.log(`# ${dir}`);
  printSwitches(readCardLogDir(dir));
}

function printSwitches(files) {
  for (const p of phyIds(files)) console.log(phyIdLine(p));
  const switches = summarizeCardLog(files);
  if (switches.length === 0) return console.log('no USB switch in the card log');
  for (const s of switches) console.log(switchLine(s));
}

/** `log card get`: the card log files, raw and decoded, to `outDir` (else --out, else a new folder in out/runs/cardlog). Returns the files. */
async function cardLogGet(session, outDir) {
  const files = await session.readCardLog();
  if (files.length === 0) {
    console.log('no card log file on the card');
    return files;
  }
  const dir = path.resolve(outDir ?? opts.out ?? cardLogStampDir());
  mkdirSync(dir, { recursive: true });
  for (const f of files) {
    const name = `boot${f.bootSeq}-cardlog-${f.file}${f.kind === 'incident' ? '-inc' : ''}`;
    const lines = [
      `# file ${f.file} ${f.kind}, boot ${f.bootSeq}, write ${f.writeNo}, ${f.events.length} events, seq ${f.oldestSeq}..${f.newestSeq}`,
    ];
    lines.push(...f.events.map((e) => eventLine(runtimeEvent(e))));
    if (f.raw) writeFileSync(path.join(dir, `${name}.bin`), f.raw);
    writeFileSync(path.join(dir, `${name}.txt`), `${lines.join('\n')}\n`);
    console.log(path.join(dir, `${name}.bin`));
    console.log(path.join(dir, `${name}.txt`));
  }
  return files;
}

// ---- takeover-cycle: the hardware experiment of the USB take-over -------------------------------------------------------------------------------
// the Elektron USB vendor id is 6453 (0x1935)

/** The ioreg keys ("class:id") of the Elektron USB device and interface objects now. The ids change when macOS enumerates the device again. */
function usbIds() {
  const keys = new Set();
  for (const cls of ['IOUSBHostDevice', 'IOUSBHostInterface']) {
    const r = spawnSync('ioreg', ['-r', '-c', cls, '-l', '-w0'], { encoding: 'utf8', maxBuffer: 256 << 20 });
    if (r.status !== 0) throw new Error(`ioreg failed: ${r.stderr.trim()}`);
    let cur = null;
    for (const line of r.stdout.split('\n')) {
      const h = /<class (\w+), id (0x[0-9a-f]+)/.exec(line);
      if (h) cur = { key: `${h[1]}:${h[2]}`, vendor: false };
      else if (cur && /"idVendor" = 6453\b/.test(line)) cur.vendor = true;
      if (cur?.vendor) keys.add(cur.key);
    }
  }
  return [...keys].sort();
}

/**
 * Poll ioreg until an object that is not in `before` appears. Returns { goneMs, backMs }: when the first old object was missing and when a new one was there
 * (null: not seen within `timeoutMs`; the ioreg call itself takes a few 100 ms, so the times are good to about 0.5 s).
 */
async function waitIdsChange(before, timeoutMs) {
  const t0 = Date.now();
  let goneMs = null;
  for (;;) {
    const now = usbIds();
    const t = Date.now() - t0;
    if (goneMs === null && before.some((k) => !now.includes(k))) goneMs = t;
    if (now.some((k) => !before.includes(k))) return { goneMs, backMs: t, ids: now };
    if (t >= timeoutMs) return { goneMs, backMs: null, ids: now };
    await new Promise((r) => setTimeout(r, 150));
  }
}

/** Universal identity request over the open link: true when the firmware sends any universal non-real-time reply (F0 7E ...) within `ms`.
 * The Elektron firmware does not send the standard identity reply (06 02); on 2026-11-12 the DT2 answered F0 7E 00 7C 00 F7. Any F0 7E
 * reply proves the firmware's own MIDI input and output work. */
async function identityAnswers(link, ms) {
  link.transport.send(Uint8Array.of(0xf0, 0x7e, 0x7f, 0x06, 0x01, 0xf7));
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    const m = await link.transport.recv(Math.max(1, deadline - Date.now()));
    if (m && m[0] === 0xf0 && m[1] === 0x7e) return true;
  }
  return false;
}

/** Open the MIDI port until the runtime answers INFO, at most `ms`. Returns the link or undefined. */
async function midiBack(ms) {
  const deadline = Date.now() + ms;
  for (;;) {
    const link = connect(2000);
    try {
      await link.session.info();
      return link;
    } catch {
      await link.close();
      if (Date.now() > deadline) return undefined;
      await new Promise((r) => setTimeout(r, 500));
    }
  }
}

async function takeoverCycle(count) {
  if (!Number.isInteger(count) || count < 1) fail('takeover-cycle needs a cycle count N >= 1');
  const gap = Number(opts.gap);
  const wait = Number(opts.wait);
  if (!(gap >= 0) || !(wait >= 1000)) fail('--gap is >= 0 ms, --wait is >= 1000 ms');
  const bin = opts['bulk-bin'];
  if (!existsSync(bin)) fail(`no dkbulk binary at ${bin} (run: mise run dkbulk:build)`);
  const holder = { link: connect() };
  const children = [];
  const opener = bulkOpener(bin, children);
  const st = { takeover: undefined, inFlight: false, cleaning: undefined };
  // Never leave the device taken over: RELEASE on exit and on Ctrl-C, over the link we hold or a fresh one.
  const cleanup = () =>
    (st.cleaning ??= (async () => {
      if (st.inFlight) {
        try {
          await st.takeover?.close();
          st.inFlight = false;
        } catch {
          try {
            const l = connect(3000);
            await l.session.usbFastRelease().finally(() => l.close());
            st.inFlight = false;
          } catch (e) {
            console.error(
              `wb: RELEASE failed (${e instanceof Error ? e.message : e}); the device gives back by itself after 30 s without traffic`,
            );
          }
        }
      }
      await Promise.resolve(holder.link.close()).catch(() => {});
    })());
  for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => void cleanup().finally(() => process.exit(130)));
  const ms = (v) => (v === null ? 'no' : `${(v / 1000).toFixed(1)}s`);
  const yn = (v) => (v ? 'Y' : 'N');
  try {
    if (!(await holder.link.session.usbFastWrongMode()))
      fail('the device is not in USB AUDIO/MIDI (or usb-fast is not loaded): there is nothing to take over');
    if (opts['card-log']) console.log(`card log: ${cardLogText(await holder.link.session.setCardLog(true))}`);
    // --continue: the totals, the run folder of the card logs, and the newest boot seen so far (a higher one after a failure means the device restarted)
    const tot = { cycles: 0, ok: 0, failed: 0, replug: 0, power: 0, none: 0 };
    const runDir = opts.out ? path.resolve(opts.out) : cardLogStampDir();
    let lastBoot = -1;
    const fetchCardLog = async (session) => {
      try {
        return await cardLogGet(session, runDir);
      } catch (e) {
        console.log(`card log not read: ${e instanceof Error ? e.message : e}`);
        return [];
      }
    };
    if (opts.continue)
      lastBoot = Math.max(-1, ...(await holder.link.session.readCardLog().catch(() => [])).map((f) => f.bootSeq));
    console.log(
      'cycle | takeover: gone back newids midi ident | bulk info | release: gone back newids midi ident rtinfo | verdict',
    );
    for (let n = 1; n <= count; n++) {
      const row = { to: {}, rel: {} };
      let ids0 = usbIds();
      if (ids0.length === 0) fail('no Elektron USB device in ioreg (is it attached?)');
      let verdict = 'ok';
      // takeover (BulkTakeOver: TAKEOVER, wait for the device, nonce handshake)
      const takeover = new BulkTakeOver(
        {
          sysex: () => holder.link.session,
          async reenumerated() {
            const c = await waitIdsChange(ids0, wait);
            row.to = { gone: c.goneMs, back: c.backMs };
            await holder.link.close();
            const link = await midiBack(wait);
            if (!link) throw new Error('the runtime does not answer INFO after the switch');
            holder.link = link;
            row.to.midi = true;
            row.to.ident = await identityAnswers(link, 2000);
          },
        },
        opener,
        { lingerMs: 0, startTimeoutMs: wait + 10000, bulk: { timeoutMs: 20000 } },
      );
      st.takeover = takeover;
      st.inFlight = true;
      let choice;
      try {
        choice = await takeover.begin();
      } catch (e) {
        verdict = `takeover failed: ${e instanceof Error ? e.message : e}`;
      }
      if (!choice && verdict === 'ok') verdict = 'no bulk link';
      let infoMs = null;
      if (choice) {
        const t = Date.now();
        try {
          await choice.session.info();
          infoMs = Date.now() - t;
        } catch (e) {
          verdict = `bulk INFO failed: ${e instanceof Error ? e.message : e}`;
        }
      }
      if (row.to.back === null || row.to.back === undefined)
        verdict = verdict === 'ok' ? 'ids unchanged after TAKEOVER' : verdict;
      // release (also when the take-over failed)
      const ids1 = usbIds();
      if (choice) await Promise.resolve(choice.link.close()).catch(() => {});
      takeover.end();
      const releaseSent = Date.now();
      let relOk = true;
      try {
        await takeover.close();
        st.inFlight = false;
      } catch (e) {
        relOk = false;
        verdict = `RELEASE failed: ${e instanceof Error ? e.message : e}`;
      }
      if (relOk || holder.link) {
        const c = await waitIdsChange(ids1, wait);
        row.rel = { gone: c.goneMs, back: c.backMs };
        if (c.backMs === null && verdict === 'ok') verdict = 'ids unchanged after RELEASE';
        await holder.link.close();
        const link = await midiBack(wait);
        if (link) {
          holder.link = link;
          row.rel.midi = true;
          row.rel.ident = await identityAnswers(link, 2000);
          row.rel.rt = true;
        } else {
          // the runtime is silent: ask the firmware on a fresh link, to tell a dead MIDI path from a dead runtime
          holder.link = connect(2000);
          row.rel.midi = false;
          row.rel.ident = await identityAnswers(holder.link, 3000);
          row.rel.rt = false;
          if (verdict === 'ok') verdict = 'MIDI dead after RELEASE';
        }
      }
      // ident is informational: runtime INFO after RELEASE (rtinfo) and new ids decide the verdict
      console.log(
        `${String(n).padStart(5)} |           ${ms(row.to.gone)} ${ms(row.to.back)} ${yn(row.to.back !== null && row.to.back !== undefined)}      ${yn(row.to.midi)}    ${yn(row.to.ident)}   | ${infoMs === null ? 'no' : `${infoMs} ms`.padEnd(7)}   |         ${ms(row.rel.gone)} ${ms(row.rel.back)} ${yn(row.rel.back !== null && row.rel.back !== undefined)}      ${yn(row.rel.midi)}    ${yn(row.rel.ident)}   ${yn(row.rel.rt)}      | ${verdict} (${((Date.now() - releaseSent) / 1000).toFixed(1)} s release)`,
      );
      tot.cycles++;
      if (verdict !== 'ok') {
        status = 1;
        // a take-over that never produced a link released over a link that was closed: send RELEASE again on the one we hold now
        if (!choice) await holder.link.session.usbFastRelease().catch(() => {});
        if (!opts.continue) break;
        tot.failed++;
        // wait for the device: INFO every 2 s; a replug first, a power cycle after 60 s
        await Promise.resolve(holder.link.close()).catch(() => {});
        const t0 = Date.now();
        let replugAsked = false;
        let powerAsked = false;
        for (;;) {
          const link = connect(2000);
          try {
            await link.session.info();
            holder.link = link;
            break;
          } catch {
            await link.close().catch(() => {});
          }
          if (!replugAsked) console.log('replug the USB cable now');
          replugAsked = true;
          if (!powerAsked && Date.now() - t0 >= 60000) {
            console.log('power cycle the device now');
            powerAsked = true;
          }
          await new Promise((r) => setTimeout(r, 2000));
        }
        const files = await fetchCardLog(holder.link.session);
        const boot = Math.max(-1, ...files.map((f) => f.bootSeq));
        const restarted = powerAsked || boot > lastBoot;
        if (boot > lastBoot) lastBoot = boot;
        if (!replugAsked) tot.none++;
        else if (restarted) tot.power++;
        else tot.replug++;
        console.log(
          `device back after ${((Date.now() - t0) / 1000).toFixed(1)} s: ${!replugAsked ? 'no action needed' : restarted ? 'power cycle (the device restarted)' : 'replug'}`,
        );
        if (files.length) printSwitches(files);
      } else tot.ok++;
      await new Promise((r) => setTimeout(r, gap));
    }
    if (opts.continue) {
      const files = await fetchCardLog(holder.link.session);
      const all = existsSync(runDir) ? readCardLogDir(runDir) : files;
      const heals = summarizeCardLog(all).filter((s) => s.selfHealed).length;
      console.log(
        `totals: cycles ${tot.cycles}, ok ${tot.ok}, failed ${tot.failed}, recovered by replug ${tot.replug}, needed power cycle ${tot.power}, no action needed ${tot.none}, device self-heals (RECOVERY then DONE) ${heals}`,
      );
    }
  } finally {
    await cleanup();
    await Promise.all(children);
  }
}

/**
 * `bulk-probe`: write N bytes to EP 0x02 OUT of interface 5 in USB AUDIO/MIDI (`dkbulk send`; only interface 5 is claimed, the class drivers keep 0-4).
 * The device counters come from the debug module's live log: the adapter writes MOD_LOG "ua.state" (value = usb_bulk_audio_state) at its first tick and
 * "ua.rx" (value = total bytes received) when a receive burst ends. Without the debug module (LOG refused) only the send result is printed.
 */
async function bulkProbe() {
  const bytes = Number(opts.bytes ?? 1048576);
  if (!Number.isInteger(bytes) || bytes < 1) fail('--bytes is an integer >= 1');
  const bin = opts['bulk-bin'];
  if (!existsSync(bin)) fail(`no dkbulk binary at ${bin} (run: mise run dkbulk:build)`);
  const tagOf = (e) =>
    String(runtimeEvent(e).fields.msg ?? '')
      .replace(/\0/g, '')
      .trim();
  const valueOf = (e) => runtimeEvent(e).args[4] >>> 0;
  const latest = (events, tag) => events.filter((e) => tagOf(e) === tag).at(-1);
  let link;
  let before;
  try {
    link = await connectReady();
    before = await link.session.readLog();
  } catch (e) {
    console.log(`debug log not available (${e instanceof Error ? e.message : e}): printing the send result only`);
    await link?.close();
    link = undefined;
  }
  const baseSeq = before?.at(-1)?.seq ?? -1;
  const stateEv = before && latest(before, 'ua.state');
  const rxEv = before && latest(before, 'ua.rx');
  const base = rxEv ? valueOf(rxEv) : 0;
  const t = Date.now();
  const r = spawnSync(bin, ['send', '--interface', '5', '--ep', '0x02', '--bytes', String(bytes)], {
    stdio: 'inherit',
  });
  const secs = (Date.now() - t) / 1000;
  status = r.status ?? 1;
  if (!link) return;
  try {
    let state = stateEv && valueOf(stateEv);
    let counted;
    const deadline = Date.now() + 3000;
    for (;;) {
      const fresh = (await link.session.readLog()).filter((e) => e.seq > baseSeq);
      const rx = latest(fresh, 'ua.rx');
      const st = latest(fresh, 'ua.state');
      if (st) state = valueOf(st);
      if (rx) {
        counted = valueOf(rx) - base;
        break;
      }
      if (Date.now() > deadline) break;
      await new Promise((res) => setTimeout(res, 250));
    }
    const names = { 0: 'not run', 1: 'prepared', 2: 'refused' };
    console.log(
      `ua.state ${state === undefined ? 'not seen' : `${state} (${names[state] ?? '?'})`}; sent ${bytes} bytes; device counted ${counted ?? 'no new ua.rx within 3 s'}; match ${counted === bytes ? 'yes' : 'no'}; ${(bytes / 1e6 / secs).toFixed(2)} MB/s (host, whole run incl. process start)`,
    );
  } finally {
    await link.close();
  }
}

const [cmd, sub, ...rest] = words;
let status = 0;
if (cmd === 'ports') {
  const r = spawnSync(opts.bin, ['ports'], { stdio: 'inherit' });
  status = r.status ?? 1;
} else if (cmd === 'bulk-probe') {
  await bulkProbe();
} else if (cmd === 'takeover-cycle') {
  await takeoverCycle(Number(sub));
} else if (cmd === 'info') {
  await withSession(async (session) => {
    console.log(JSON.stringify(await session.info(), null, 2));
  });
} else if (cmd === 'samples' && sub === 'ls') {
  const sample = await loadSample();
  await withSession(async (session) => {
    const backend = openSamples('stock-rpc', { session, hashOf: (d) => sample.hash(d) });
    for (const e of await backend.list(rest[0] ?? ''))
      console.log(`${e.kind === 'folder' ? `${e.name}/` : e.name}\t${e.size}`);
  });
} else if (cmd === 'samples' && sub === 'put') {
  if (rest.length === 0) fail(usage);
  if (!['auto', 'always', 'never'].includes(opts.verify)) fail('--verify is auto, always or never');
  const sample = await loadSample();
  let bulk;
  const holder = { link: await connectReady() };
  let speedMask;
  if (opts.speed !== undefined) {
    speedMask = Number(opts.speed);
    if (!Number.isInteger(speedMask) || speedMask < 0 || speedMask > 15) fail('--speed is a 4-bit mask (0 to 15)');
  }
  try {
    await (async () => {
      // the switches first: the receive ring is sized when the buffers are allocated (the take-over or enable of the bulk link)
      if (speedMask !== undefined) await setSpeed(holder.link.session, speedMask);
      bulk = await connectBulk(holder);
      const session = holder.link.session;
      // the profile names the backend (`[storage] samples`); the Digitakt II is the stock +Drive, the Digitone II the digikit store
      // (a stock Digitakt II has no runtime to answer INFO: the +Drive then)
      let isDt2 = false;
      let devInfo = null;
      const kind = await session.info().then(
        (i) => {
          devInfo = i;
          isDt2 = i.port === 'dt2';
          return i.port === 'dn2' ? 'digikit' : 'stock-rpc';
        },
        () => 'stock-rpc',
      );
      const backend = openSamples(kind, {
        session,
        hashOf: (d) => sample.hash(d),
        bulk: bulk?.session,
        note: (n) => console.error(`wb: ${n}`),
      });
      const link = () => (backend.bulkState === 'bulk' ? 'bulk' : 'sysex');
      if (opts.folder) await backend.mkdir(opts.folder);
      // one listing of the folder for the whole run: the `exists` hint of every put (REPLACE when the name is there); each put adds its name
      const names = new Set((await backend.list(opts.folder)).filter((e) => e.kind === 'file').map((e) => e.name));
      let total = 0;
      let tPrep = 0;
      let tPut = 0;
      let tVerify = 0;
      let count = 0;
      const t0 = performance.now();
      for (const file of rest) {
        try {
          if (!statSync(file).isFile()) throw new Error('not a file');
          let t = performance.now();
          const prepared = prepareUpload(sample, path.basename(file), Uint8Array.from(readFileSync(file)));
          const prep = performance.now() - t;
          t = performance.now();
          await backend.put(opts.folder, prepared, undefined, { exists: names.has(prepared.name), folderReady: true });
          names.add(prepared.name);
          const putMs = performance.now() - t;
          const s = putMs / 1000;
          const n = prepared.stored.length;
          total += n;
          // the bulk END of the Digitakt II +Drive and of a runtime with the READBACK capability compares a hash the device computed from the written
          // file; the SysEx verify is skipped then (see skipVerifyAfterBulk)
          const skip = skipVerifyAfterBulk(opts.verify, link(), kind, devInfo, isDt2);
          t = performance.now();
          const v = skip
            ? { ok: true, message: opts.verify === 'never' ? 'verify skipped' : 'device hash checked at END' }
            : await backend.verify(opts.folder, prepared);
          const ver = performance.now() - t;
          tPrep += prep;
          tPut += putMs;
          tVerify += ver;
          count++;
          console.log(
            `${v.ok ? 'ok  ' : 'FAIL'} ${kind === 'digikit' ? 'samples' : '+Drive'}/${opts.folder ? `${opts.folder}/` : ''}${prepared.name}  ${v.message}  ${n} bytes ${s.toFixed(2)} s ${(n / 1e6 / s).toFixed(2)} MB/s via ${link()} prep ${prep.toFixed(0)} ms put ${putMs.toFixed(0)} ms verify ${ver.toFixed(0)} ms`,
          );
          if (!v.ok) status = 1;
        } catch (e) {
          console.error(`FAIL ${file}: ${e instanceof Error ? e.message : e}`);
          status = 1;
        }
      }
      const s = (performance.now() - t0) / 1000;
      console.log(
        `total ${total} bytes ${s.toFixed(2)} s ${(total / 1e6 / s).toFixed(2)} MB/s via ${link()} (put and verify)`,
      );
      if (speedMask !== undefined) console.log(await speedReport(holder.link.session));
      console.log(
        `phases prep ${tPrep.toFixed(0)} ms put ${tPut.toFixed(0)} ms verify ${tVerify.toFixed(0)} ms, ${count} files ${(count / s).toFixed(2)} files/s`,
      );
    })();
  } finally {
    // the bulk link first: a take-over is released over the SysEx link, which stays open until then
    await bulk?.close();
    await holder.link.close();
  }
} else if (cmd === 'samples' && sub === 'rm') {
  if (rest.length === 0) fail(usage);
  // the folders are cleaned first: a bad name stops the run before the device is touched
  const folders = rest.map((arg) => {
    const f = arg.replace(/\/+$/, '');
    if (f === '' || f === '.' || arg.split('/').includes('..')) fail(`refusing to remove "${arg}"`);
    return f.replace(/^\/+/, '');
  });
  const sample = await loadSample();
  await withSession(async (session) => {
    const kind = await session.info().then(
      (i) => (i.port === 'dn2' ? 'digikit' : 'stock-rpc'),
      () => 'stock-rpc',
    );
    const backend = openSamples(kind, {
      session,
      hashOf: (d) => sample.hash(d),
      note: (n) => console.error(`wb: ${n}`),
    });
    for (const folder of folders) {
      try {
        const cut = folder.lastIndexOf('/');
        const parent = cut < 0 ? '' : folder.slice(0, cut);
        const name = folder.slice(cut + 1);
        const listed = await backend.list(parent);
        if (!listed.some((e) => e.kind === 'folder' && e.name === name)) throw new Error('no such folder');
        const r = await backend.removeTree(folder);
        if (r.stopped) throw new Error(`stopped after ${r.removed} of ${r.total} files`);
        console.log(`removed ${folder} (${r.removed} files)`);
      } catch (e) {
        console.error(`FAIL ${folder}: ${e instanceof Error ? e.message : e}`);
        status = 1;
      }
    }
  });
} else if (cmd === 'log' && sub === 'card') {
  if (rest[0] === 'summary') cardLogSummary(rest[1]);
  else {
    await withSession(async (session) => {
      const [what] = rest;
      if (what === 'on' || what === 'off') console.log(cardLogText(await session.setCardLog(what === 'on')));
      else if (what === 'get') await cardLogGet(session);
      else if (what === undefined) console.log(cardLogText(await session.cardLog()));
      else fail(usage);
    });
  }
} else if (cmd === 'store' && sub === 'ls') {
  await withSession(async (session) => {
    for (const e of await session.list(rest[0] || undefined)) console.log(`${e.path}\t${e.size}`);
  });
} else if (cmd === 'store' && sub === 'put') {
  if (rest.length !== 2) fail(usage);
  const [local, remote] = rest;
  await withSession(async (session) => {
    try {
      if (!statSync(local).isFile()) throw new Error('not a file');
      const bytes = Uint8Array.from(readFileSync(local));
      await session.put(remote, bytes);
      const ok = await session.verifyFile(remote, bytes);
      console.log(`${ok ? 'ok  ' : 'FAIL'} ${remote}  ${bytes.length} bytes`);
      if (!ok) status = 1;
    } catch (e) {
      console.error(`FAIL ${local}: ${e instanceof Error ? e.message : e}`);
      status = 1;
    }
  });
} else if (cmd === 'store' && sub === 'rm') {
  if (rest.length !== 1) fail(usage);
  await withSession(async (session) => {
    await session.delete(rest[0]);
    console.log(`removed ${rest[0]}`);
  });
} else fail(usage);
process.exit(status);
