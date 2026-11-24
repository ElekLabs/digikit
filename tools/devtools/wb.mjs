#!/usr/bin/env node
// Workbench actions from the command line, through the workbench's own code: the dk-session TypeScript (prepareUpload, openSamples, put,
// verify) over the dkmidi Rust MIDI port (packages/dkmidi, the code the desktop app uses), here as a child process on stdin and stdout.
// The workbench's Host (packages/workbench-core/src/host/host.ts) is the NodeHost here (src/node/node-host.ts): `ports` lists the MIDI ports
// and the samples commands read the device profile from the registry (packages/devices), as the desktop app does.
//
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] ports
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] info
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] samples ls [FOLDER]
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] samples put FILE... [--folder F] [--slot N] [--bulk auto|off|force] [--bulk-bin PATH] [--verify auto|always|never] [--speed MASK]
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] samples rm FOLDER...
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] store ls [PREFIX]
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] store put LOCAL REMOTE
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] store rm REMOTE
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] transfer install ID... [--restart] [--reload] [--library DIR] | transfer reload|load|unload PATH...   (modules with the services they need; tools/devtools/wb_transfer.mjs)
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] transfer restart
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] transfer delete PATH...
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] log card [on|off|get [--out DIR]]
//     node tools/devtools/wb.mjs [--format json|text] addrmap lookup ADDR... | module analyse DKM DKO | profile report FILE | profile record --seconds N --out FILE [--cpu cf|sharc]
//
//     node tools/devtools/wb.mjs profile calibrate fit --device D.json --model E.json --out cal.json | profile calibrate report cal.json   (offline; wb_prof.mjs)
//     node tools/devtools/wb.mjs [--port SUB] [--bin PATH] profile spans [--start|--stop|--reset] [--out FILE]   (a device command: reads the runtime span counters; --start resets them, then starts)
//
// The `--format` line above is the offline group (no device, no dkmidi): names for addresses, module DSP analysis and profiles, through dkprof.wasm (`mise run wasm:dkprof`).
// Its options and output are in tools/devtools/wb_prof.mjs and docs/ADDRMAP.md, docs/INSTRUMENTATION.md and packages/dksharc/README.md.
//
// `log card` prints the card log state of the debug module (the log written to data/digikit/debug on the card, kept across restarts); `on` / `off` switch it;
// `get` reads the card log files and writes each raw (.bin) and decoded (.txt) to DIR (default out/runs/cardlog/<YYYYmmdd-HHMMSS>/), the older boot first.
// `store` works on the card's store by path (for example data/tools/bootscreen/x.dkbs); `store put` verifies the file after the upload.
// --emu [--run-dir DIR]: any device command on the running emulator of DIR (default out/native/run) instead of a MIDI port (`mise run emu:serve`).
// --device ID: names the device and skips the identification (wb asks the stock MidiRpc Ping otherwise); for a device that does not answer Ping.
// --port: a part of the MIDI port name (default "Digitakt"). --bin: the dkmidi binary (default $DKMIDI_BIN, else the nogen target dir;
// `mise run dkmidi:build` builds it). `samples put` needs apps/web/public/dksample.wasm (`mise run wasm:dksample`).
// `samples put` sends the file bytes over the USB bulk link when the device offers its interface (usb-fast; the `dkbulk` bridge, `mise run dkbulk:build`,
// $DKBULK_BIN or --bulk-bin) and over SysEx or MidiRpc otherwise; --bulk off forces SysEx, --bulk force fails when there is no bulk link. It prints the
// In USB AUDIO/MIDI with core/usb-bulk-audio, `samples put` uses the OUT-only bulk interface automatically (frames over `dkbulk open --out-only`, replies over
// the SysEx 'R' poll; audio and MIDI keep running). Without that interface (old firmware) it stays on SysEx.
// --slot N writes slot N and replaces what it holds; a cancelled write leaves the slot empty.
// --speed MASK (4 bits, decimal or 0x): sets the four speed switches through usb-fast STATUS 'S' before the put (bit 0 hash once, bit 1 USB receive ring,
// bit 2 read-back hash overlap, bit 3 large card commands; all on at boot); the run prints the resulting mask and the usb-fast STATUS words 18 (staging bytes)
// and 19 (receive ring slots): the hardware A/B of docs/findings/126-usb-fast-transfer-ceilings.md.
// The files go with the batch commit (runtime 0.8.3, usb-fast): the device verifies each file at END and names up to 16 of them in one table commit; `samples put`
// sends COMMIT after the last file and reports the files as saved then (docs/plans/usb-fast-batch.md). A device without the batch
// commit commits at END and says so (4-byte END reply).
// link and MB/s of every file and of the run: the repeatable bench (docs/plans/usb-fast.md step 6): run it once with --bulk off and once with auto.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  DkSession,
  EmuUsbTransport,
  ProcessBulkLink,
  chooseBulk,
  emulatedClock,
  emulatedProbe,
  emuMidiSink,
  eventLine,
  identifyDevice,
  poll,
  RPC_PING,
  runtimeEvent,
  wallClock,
} from '../../packages/dk-session/src/index.ts';
import { ProcessTransport } from '../../packages/dk-session/src/process-transport.ts';
import { Sample, converterOf } from '../../packages/dk-session/src/dksample.ts';
import { openSamples, samplesUseBulk, skipVerifyAfterBulk } from '../../packages/dk-session/src/samples.ts';
import { loadDevices } from '../../packages/devices/src/node.ts';
import { eventText } from '../../packages/workbench-core/src/transfer/transfer-core.ts';
import { createNodeHost } from '../../packages/workbench-core/src/node/node-host.ts';
import { calibrateRun } from './wb_calibrate.mjs';
import { profCommand, profSpans } from './wb_prof.mjs';
import { transferCommand } from './wb_transfer.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
// the patchkit rule (tools/emulator/native_target.py): a worktree builds into its own target dir; no generated core is the "nogen" key
const targetRoot = existsSync(path.join(root, '.git', 'HEAD'))
  ? path.join(root, 'out/native/target')
  : path.join(root, 'dist/native-target');
const usage =
  'usage: node tools/devtools/wb.mjs [--port SUB] [--device ID] [--bin PATH] ports | info | samples ls [FOLDER] | samples put FILE... [--folder F] [--slot N] (--slot N writes slot N and replaces what it holds; a cancelled write leaves the slot empty) | samples rm FOLDER... | store ls [PREFIX] | store put LOCAL REMOTE | store rm REMOTE | transfer install ID... [--restart] [--reload] [--library DIR] | transfer reload|load|unload PATH... | transfer restart | transfer delete PATH... | log card [on|off|get [--out DIR]] | profile spans [--start|--stop|--reset] [--out FILE] | profile calibrate run --profile NAME|FILE [--case ID] [--trace] [--out DIR] [--allow-unidentified-firmware]';

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
  format: undefined,
  ret: false,
};
// the offline group takes these several times
const REPEATED = new Set(['map', 'load', 'dsp-base', 'analyse', 'model', 'case']);
const OFFLINE = ['addrmap', 'module', 'profile'];
const words = [];
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (
    a === '--port' ||
    a === '--device' ||
    a === '--bin' ||
    a === '--folder' ||
    a === '--slot' ||
    a === '--bulk' ||
    a === '--verify' ||
    a === '--bulk-bin' ||
    a === '--speed' ||
    a === '--out' ||
    a === '--profile' ||
    a === '--case' ||
    a === '--library' ||
    [
      '--format',
      '--model',
      '--block-cycles',
      '--model-id',
      '--calibration',
      '--model-profile',
      '--map',
      '--image',
      '--loads',
      '--snapshot',
      '--load',
      '--dsp-base',
      '--analyse',
      '--cpu',
      '--top',
      '--seconds',
      '--run-dir',
      '--run-id',
    ].includes(a)
  ) {
    if (i + 1 >= args.length) fail(`${a} needs a value`);
    const key = a.slice(2);
    if (REPEATED.has(key)) (opts[key] ??= []).push(args[++i]);
    else opts[key] = args[++i];
    // `profile calibrate fit` takes --device FILE several times (the other commands: one device id)
    if (key === 'device') (opts['device-files'] ??= []).push(opts[key]);
  } else if (a === '--trace') opts.trace = true;
  else if (a === '--ret') opts.ret = true;
  else if (a === '--emu' || a === '--restart' || a === '--reload') opts[a.slice(2)] = true;
  else if (a === '--since-boot') opts['since-boot'] = true;
  else if (a === '--allow-unidentified-firmware') opts['allow-unidentified-firmware'] = true;
  else if (a === '--start' || a === '--stop' || a === '--reset') opts.op = a.slice(2);
  else if (a.startsWith('--')) fail(`unknown option ${a}\n${usage}`);
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

const host = createNodeHost({
  root,
  dkmidiBin: opts.bin,
  dkbulkBin: opts['bulk-bin'],
  libraryDir: opts.library ? path.resolve(opts.library) : undefined,
});

const devices = loadDevices(path.join(root, 'devices'));

/** The `[storage]` of the device profile, as the desktop app reads it from the registry. An unknown device or a profile without `[storage]` stops the command. */
function storageOf(id) {
  let device;
  try {
    device = devices.get(id);
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
  }
  if (!device.storage) fail(`the ${device.name} profile has no [storage]`);
  return device.storage;
}

/** The samples backend the device profile names (`[storage] samples`). */
const kindOf = (id) => storageOf(id).samples;

/**
 * The device's INFO and the samples backend its profile names. The device is identified through the stock MidiRpc Ping (dk-session `identifyDevice`),
 * with or without the runtime; `info` is null for a device without the runtime. --device ID skips the identification (for a device that does not
 * answer Ping); `info` is null then.
 */
async function infoAndKind(session) {
  if (opts.device) return { info: null, kind: kindOf(opts.device), storage: storageOf(opts.device) };
  let who;
  try {
    who = await identifyDevice(session, devices);
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    fail(
      `cannot identify the device: ${message}; --device ID names it when it does not answer Ping (one of: ${devices.ids.join(', ')})`,
      1,
    );
  }
  return { info: who.info, kind: kindOf(who.id), storage: storageOf(who.id) };
}

const SPANS = words[0] === 'profile' && words[1] === 'spans';
const CALIBRATE = words[0] === 'profile' && words[1] === 'calibrate' && words[2] === 'run';
if ((!OFFLINE.includes(words[0]) || SPANS || CALIBRATE) && !opts.emu && !existsSync(opts.bin))
  fail(`no dkmidi binary at ${opts.bin} (run: mise run dkmidi:build)`);

/** Start the bridge on the port and open a session on it. */
function connect(timeoutMs = 60000) {
  // a calibration run also sends notes and control changes: `--raw` lets the bridge pass them (it drops them otherwise)
  const child = spawn(opts.bin, ['open', opts.port, ...(CALIBRATE ? ['--raw'] : [])], {
    stdio: ['pipe', 'pipe', 'inherit'],
  });
  const exited = new Promise((resolve) => child.on('exit', resolve));
  child.on('error', (e) => fail(`cannot start ${opts.bin}: ${e.message}`, 1));
  // why the bridge is no longer usable: the MIDI sink of a calibration run reports it instead of dropping notes
  let dead;
  child.on('exit', (code) => (dead ??= `the dkmidi bridge exited (${code})`));
  child.stdin.on('error', (e) => (dead ??= `dkmidi bridge stdin: ${e.message}`));
  const transport = new ProcessTransport(
    (b) => child.stdin.write(b),
    (cb) => child.stdout.on('data', (d) => cb(new Uint8Array(d))),
  );
  const session = new DkSession(transport, { timeoutMs });
  return {
    session,
    transport,
    // one whole message per write: the bridge's pipe keeps the order of the SysEx writes of the session
    // resolves when the bridge has taken the bytes; rejects on a write error or when the bridge is gone
    midiSink: {
      send: (b) =>
        new Promise((resolve, reject) => {
          if (dead) return reject(new Error(dead));
          child.stdin.write(b, (err) => (err ? reject(err) : resolve()));
        }),
    },
    async close() {
      child.stdin.end();
      // the bridge of a port that vanished may not exit by itself
      const timer = setTimeout(() => child.kill(), 3000);
      await exited;
      clearTimeout(timer);
    },
  };
}

/** The running emulator of `--run-dir` (default out/native/run) as the link: the same SysEx session over its control socket (the emulator runs in real time). */
async function connectEmu() {
  const { connect: connectControl, readSocket } = await import('../../packages/control-client/src/index.ts');
  const runDir = path.resolve(root, opts['run-dir'] ?? 'out/native/run');
  const sock = readSocket(runDir);
  if (!sock) fail(`no emulator socket in ${runDir} (start one: mise run emu:serve)`, 1);
  const client = await connectControl(sock);
  const transport = new EmuUsbTransport(client.call.bind(client));
  // the emulator's clock and probe, as the test harness builds them (tests/regression/runtime/waits.ts): a request lost while the guest resets ends after 2 s of emulated time, not after the 60 s timeout
  const clock = emulatedClock(() => client.call('status'));
  const session = new DkSession(transport, { timeoutMs: 60000, clock, emulatedProbe: emulatedProbe(clock) });
  return {
    session,
    transport,
    midiSink: emuMidiSink(client.call.bind(client)),
    call: client.call.bind(client),
    close: async () => client.close(),
  };
}

/**
 * `connect`, then one quick Ping; a device that does not reply is tried again for up to `waitMs` with a new bridge. The device may still re-enumerate after
 * the previous run: its MIDI port may be gone or not answer yet (the device re-enumerates after a USB mode change). After the time the last link is returned anyway
 * and the first real request reports the error. An answer with an error status is no re-enumeration: no retry.
 * The device is ready when a quick INFO or a Ping answers (INFO first, then Ping, about 1 s each). A runtime answers INFO even while the stock service
 * drops requests during the firmware intro (after a restart Ping can time out for seconds); a stock device does not answer INFO but answers Ping, so it
 * is ready after one INFO timeout. An answer of either kind means the port is up again, which is what the loop waits for.
 */
async function connectReady(waitMs = 8000) {
  // a device named by --device skips the identification, so no probe
  if (opts.device && !opts.emu) return connect();
  let link;
  // true: the device answered (or failed with something that is no re-enumeration: the first real request reports it); false: no answer yet
  const answered = async () => {
    const previous = link;
    link = undefined;
    await previous?.close();
    link = opts.emu ? await connectEmu() : connect();
    try {
      try {
        await link.session.probe((q) => link.session.info(q));
      } catch (e) {
        // an error reply of INFO is an answer: the runtime is up
        if (e?.kind === 'timeout') await link.session.rpc(RPC_PING, undefined, 1000);
        else if (e?.kind === 'cancelled') throw e;
      }
      return true;
    } catch (e) {
      if (e?.kind === 'cancelled') throw e;
      return e?.kind !== 'timeout';
    }
  };
  // under --emu the wait counts emulated time, on the clock of the session that connectEmu built for the current try
  const clock = opts.emu
    ? { now: () => link.session.clock.now(), pause: (ms) => link.session.clock.pause(ms) }
    : wallClock;
  try {
    await poll(clock, answered, Boolean, { withinMs: waitMs, intervalMs: 500, message: 'device answers' });
  } catch (e) {
    if (!link || e?.kind === 'cancelled') throw e;
    // after the time the last link is returned anyway and the first real request reports the error
  }
  return link;
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
 * When no device offers any interface and usb-fast reports that the device is not in USB MIDI mode (an old firmware without core/usb-bulk-audio), there is no bulk link.
 * `holder.link` is the SysEx link.
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
  const choice = await chooseBulk(holder.link.session, opener, (n) => notes.push(n), { timeoutMs: 20000 });
  if (!choice && (await holder.link.session.usbFastWrongMode())) {
    return none(
      'the device is in USB AUDIO/MIDI and has no OUT-only bulk interface (core/usb-bulk-audio not loaded?); set USB CONFIG to USB MIDI on the device',
    );
  }
  if (!choice) {
    await Promise.all(children);
    return none(notes.at(-1) ?? 'no device offers a bulk interface; is usb-fast loaded?');
  }
  for (const n of notes) console.error(`wb: ${n}`);
  return {
    session: choice.session,
    async close() {
      await choice.link.close();
      await Promise.all(children);
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

const [cmd, sub, ...rest] = words;
let status = 0;
if (SPANS) {
  await withSession((session) => profSpans({ opts, session, fail }));
} else if (CALIBRATE) {
  const link = await connectReady();
  try {
    status = await calibrateRun({
      opts,
      link,
      host,
      root,
      fail,
      devices,
      emit: (e) => console.log(opts.format === 'text' ? eventText(e) : JSON.stringify(e)),
    });
  } finally {
    await link.close();
  }
} else if (OFFLINE.includes(cmd)) {
  status = await profCommand({ cmd, sub, rest, opts, root, fail });
} else if (cmd === 'ports') {
  try {
    for (const name of await host.ports.midi.ports()) console.log(name);
  } catch (e) {
    console.error(`wb: ${e instanceof Error ? e.message : e}`);
    status = 1;
  }
} else if (cmd === 'info') {
  await withSession(async (session) => {
    console.log(JSON.stringify(await session.info(), null, 2));
  });
} else if (cmd === 'samples' && sub === 'ls') {
  const sample = await loadSample();
  await withSession(async (session) => {
    const { kind, storage } = await infoAndKind(session);
    const backend = openSamples(kind, { session, hashOf: (d) => sample.hash(d), slots: storage.slots });
    for (const e of await backend.list(rest[0] ?? ''))
      console.log(
        `${e.slot !== undefined ? `${e.slot}\t` : ''}${e.kind === 'folder' ? `${e.name}/` : e.name}\t${e.size}`,
      );
  });
} else if (cmd === 'samples' && sub === 'put') {
  if (rest.length === 0) fail(usage);
  if (!['auto', 'always', 'never'].includes(opts.verify)) fail('--verify is auto, always or never');
  if (opts.slot !== undefined) {
    const n = Number(opts.slot);
    if (!Number.isInteger(n) || n < 1 || n > 64) fail('--slot is 1 to 64');
    if (rest.length !== 1) fail('--slot needs exactly one file');
  }
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
      // the switches first: the receive ring is sized when the buffers are allocated (the enable of the bulk link)
      if (speedMask !== undefined) await setSpeed(holder.link.session, speedMask);
      const session = holder.link.session;
      // the profile names the backend (`[storage] samples`); the Digitakt II is the stock +Drive, the Digitone II the digikit store
      const { info: devInfo, kind, storage } = await infoAndKind(session);
      // a numbered-slot device has no bulk link: never open or report one
      if (samplesUseBulk(kind)) bulk = await connectBulk(holder);
      if (opts.slot !== undefined && kind !== 'stock-slots')
        fail('--slot works only on a device with numbered sample slots');
      const backend = openSamples(kind, {
        session,
        hashOf: (d) => sample.hash(d),
        converter: converterOf(sample),
        slots: storage.slots,
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
      let tCommit = 0;
      /** The verify (or its skip) and the line of one file that the device has named in the store. */
      const finish = async ({ prepared, n, prep, putMs, saved }) => {
        // the bulk END of the Digitakt II +Drive and of a runtime with the READBACK capability compares a hash the device computed from the written
        // file; the SysEx verify is skipped then (see skipVerifyAfterBulk)
        const skip = skipVerifyAfterBulk(opts.verify, backend, devInfo);
        const t = performance.now();
        const v = skip
          ? {
              ok: true,
              message:
                opts.verify === 'never'
                  ? 'verify skipped'
                  : `device hash checked at END${saved ? ', saved by the batch commit' : ''}`,
            }
          : await backend.verify(opts.folder, prepared);
        const ver = performance.now() - t;
        tVerify += ver;
        const s = putMs / 1000;
        console.log(
          `${v.ok ? 'ok  ' : 'FAIL'} ${kind === 'digikit' ? 'samples' : kind === 'stock-slots' ? 'slot' : '+Drive'}/${opts.folder ? `${opts.folder}/` : ''}${prepared.name}  ${v.message}  ${n} bytes ${s.toFixed(2)} s ${(n / 1e6 / s).toFixed(2)} MB/s via ${link()} prep ${prep.toFixed(0)} ms put ${putMs.toFixed(0)} ms verify ${ver.toFixed(0)} ms`,
        );
        if (!v.ok) status = 1;
      };
      // files the device verified and holds pending: named by a reply that shows 0 pending, or by the COMMIT after the last file
      let waiting = [];
      const flush = async () => {
        for (const w of waiting.splice(0)) await finish(w);
      };
      // The next file is read and prepared while this one is sent. The promise never rejects: a failure is a value that the next iteration reports on its file.
      const readyFile = async (file) => {
        try {
          if (!statSync(file).isFile()) throw new Error('not a file');
          const t = performance.now();
          const prepared = backend.prepare(path.basename(file), Uint8Array.from(await readFile(file)));
          return { prepared, prep: performance.now() - t };
        } catch (error) {
          return { error };
        }
      };
      let ahead = rest.length > 0 ? readyFile(rest[0]) : undefined;
      for (const [index, file] of rest.entries()) {
        try {
          const got = await ahead;
          ahead = index + 1 < rest.length ? readyFile(rest[index + 1]) : undefined;
          if (got.error) throw got.error;
          const { prepared, prep } = got;
          const t = performance.now();
          const r = await backend.put(opts.folder, prepared, undefined, {
            exists: names.has(prepared.name),
            folderReady: true,
            defer: backend.commit !== undefined,
            slot: opts.slot === undefined ? undefined : Number(opts.slot),
          });
          names.add(prepared.name);
          const putMs = performance.now() - t;
          const n = backend.sizeOf(prepared);
          total += n;
          tPrep += prep;
          tPut += putMs;
          count++;
          const item = { prepared, n, prep, putMs, saved: r !== undefined };
          if (r && r.pending > 0) {
            console.log(`pend ${prepared.name}  ${r.pending} pending on the device`);
            waiting.push(item);
          } else {
            await flush();
            await finish(item);
          }
        } catch (e) {
          console.error(`FAIL ${file}: ${e instanceof Error ? e.message : e}`);
          status = 1;
        }
      }
      if (waiting.length) {
        const t = performance.now();
        try {
          const left = await backend.commit();
          if (left > 0)
            throw new Error(
              `${left} files are still held back on the device (a file they replace is being read): not confirmed, run again`,
            );
          tCommit = performance.now() - t;
          console.log(`commit ${waiting.length} files in ${tCommit.toFixed(0)} ms`);
          await flush();
        } catch (e) {
          console.error(
            `FAIL commit (${waiting.length} files not saved, send again): ${e instanceof Error ? e.message : e}`,
          );
          waiting = [];
          status = 1;
        }
      }
      const s = (performance.now() - t0) / 1000;
      console.log(
        `total ${total} bytes ${s.toFixed(2)} s ${(total / 1e6 / s).toFixed(2)} MB/s via ${link()} (put and verify)`,
      );
      if (speedMask !== undefined) console.log(await speedReport(holder.link.session));
      console.log(
        `phases prep ${tPrep.toFixed(0)} ms put ${tPut.toFixed(0)} ms commit ${tCommit.toFixed(0)} ms verify ${tVerify.toFixed(0)} ms, ${count} files ${(count / s).toFixed(2)} files/s`,
      );
      if (storage.slots?.apply === 'restart')
        console.log('the device uses new samples after it restarts (wb transfer restart)');
    })();
  } finally {
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
    const { kind, storage } = await infoAndKind(session);
    const backend = openSamples(kind, {
      session,
      hashOf: (d) => sample.hash(d),
      slots: storage.slots,
      note: (n) => console.error(`wb: ${n}`),
    });
    if (kind === 'stock-slots') {
      // numbered slots have no folders: each argument is the name `samples ls` shows
      for (const name of rest) {
        try {
          await backend.remove('', name);
          console.log(`removed ${name}`);
        } catch (e) {
          console.error(`FAIL ${name}: ${e instanceof Error ? e.message : e}`);
          status = 1;
        }
      }
      return;
    }
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
  await withSession(async (session) => {
    const [what] = rest;
    if (what === 'on' || what === 'off') console.log(cardLogText(await session.setCardLog(what === 'on')));
    else if (what === 'get') await cardLogGet(session);
    else if (what === undefined) console.log(cardLogText(await session.cardLog()));
    else fail(usage);
  });
} else if (cmd === 'transfer') {
  await withSession(async (session) => {
    status = await transferCommand({ host, root, session, sub, rest, opts, fail });
  });
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
