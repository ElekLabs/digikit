import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import {
  MidiOut,
  poll,
  waitClock,
  type DkSession,
  type EmuRpcCall,
  type MidiSink,
} from '../../packages/dk-session/src/index.ts';
import type { MidiMap, Registry } from '../../packages/devices/src/index.ts';
import type { Sample } from '../../packages/dk-session/src/dksample.ts';
import type { SamplesBackend } from '../../packages/dk-session/src/samples.ts';
import { parseWave, type Wave } from '../../packages/sharc-rig-kit/src/wave.ts';
import { remoteInput } from '../../packages/test-support/src/input-path.ts';
import { captureFirmwareIdentity } from '../../packages/workbench-core/src/calibration/calprofile.ts';
import { isContentModule } from '../../packages/workbench-core/src/calibration/effects.ts';
import {
  captureStockDelay,
  delayReference as spec,
} from '../../packages/workbench-core/src/reference/dt2-stock-delay.ts';
import { compareDelay, measureDelay } from '../../packages/workbench-core/src/reference/delay.ts';
import { readStockDelayCaptureState } from '../../packages/workbench-core/src/reference/stock-delay-state.ts';
import {
  dt2StockSampleReader,
  identifyNativeSample,
} from '../../packages/workbench-core/src/reference/dt2-stock-sample-reader.ts';
import { emulatorAudio, listAudioInputs, usbAudio } from './reference_capture.ts';
import { prepareProject } from './reference_project.ts';

export const referenceUsage = `wb audio reference prepare --out DIR
wb audio reference prepare-project --emu [--firmware FILE] [--scratch-project NAME] --out DIR
wb audio reference prepare-project --port PORT --scratch-project NAME [--out DIR]
wb audio reference inspect [--emu --run-dir DIR | --port PORT] [--format json]
wb audio reference run --emu [--run-dir DIR] --scratch-project NAME --out DIR
wb audio reference inputs [--format json]
wb audio reference run --port PORT --audio-input ID_OR_EXACT_NAME --scratch-project NAME --out DIR
wb audio reference compare BASELINE.json CANDIDATE.json [--format json]
The reference case defaults to ${spec.id} (or pass --case ${spec.id}).`;

const schema = 'dk.audio-reference/1';
const sha = (b: Buffer) => createHash('sha256').update(b).digest('hex');
const json = (v: unknown) => JSON.stringify(v, null, 2);
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const hexWord = (raw: number) => `0x${(raw & 0xffff).toString(16).padStart(4, '0')}`;

interface ReferenceOptions {
  emu?: boolean;
  out?: string;
  format?: string;
  case?: string[];
  firmware?: string;
  'scratch-project'?: string;
  'audio-input'?: string;
  'audio-bin': string;
}

interface ReferenceLink {
  session: DkSession;
  midiSink: MidiSink;
  call?: EmuRpcCall;
  close(): Promise<void>;
}

interface Command {
  root: string;
  rest: string[];
  opts: ReferenceOptions;
  devices: Registry;
  connectReady(): Promise<ReferenceLink>;
  loadSample(): Promise<Sample>;
  openUploadBackend(session: DkSession): Promise<SamplesBackend>;
}

export function renderFixture() {
  const count = 9600;
  const wav = Buffer.alloc(44 + count * 2);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write('WAVEfmt ', 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(spec.rate, 24);
  wav.writeUInt32LE(2 * spec.rate, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(count * 2, 40);
  for (let n = 0; n < 1200; n++) {
    const hann = 0.5 - 0.5 * Math.cos((2 * Math.PI * n) / 1199);
    const tone = Math.sin((2 * Math.PI * n) / 48);
    wav.writeInt16LE(Math.round(6500 * hann * tone), 44 + 2 * (1440 + n));
  }
  if (sha(wav) !== spec.fixtureSha256) throw new Error('Generated probe does not match the reference fixture hash.');
  return wav;
}

function prepare(dir: string) {
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, spec.fixture);
  if (!existsSync(file)) writeFileSync(file, renderFixture(), { flag: 'wx' });
  else if (sha(readFileSync(file)) !== spec.fixtureSha256)
    throw new Error(`Not overwriting a different sample at ${file}`);
  return file;
}

function makeTarget(link: ReferenceLink, map: MidiMap, emu: EmuRpcCall | undefined) {
  const { session } = link;
  const clock = session.clock;
  const midi = new MidiOut(link.midiSink);
  const channel = map.trackChannels[0];
  const cc = map.params.send_delay?.cc;
  if (channel === undefined || cc === undefined) throw new Error('The MIDI map has no Track 1 Delay send.');
  return {
    wait: (ms: number) => waitClock(clock, ms),
    tap: remoteInput(session, clock).tap,
    async setDelaySend(value: number) {
      await midi.cc(channel, cc, value);
      if (emu)
        await poll(
          clock,
          () => emu<{ queued: number }>('midi.usb.in', { bytes: '' }),
          (reply) => reply.queued === 0,
          { intervalMs: 20, withinMs: 1000, message: 'USB MIDI input taken by firmware' },
        );
    },
  };
}

function verifyFirmware(info: { port: string; fw_id?: string | null }, devices: Registry) {
  const [device, version] = spec.port.split('-');
  if (info.port !== device && info.port !== spec.port) throw new Error(`Expected ${spec.port}, reported ${info.port}.`);
  const record = devices.get(info.port);
  const firmware = captureFirmwareIdentity(info.fw_id ?? '', record.firmware);
  const sha256 = firmware.stock_firmware_sha256;
  if (!sha256 || !record.firmware.some((f) => f.version === version && f.sha256 === sha256))
    throw new Error(`The target did not identify the stock ${spec.port} firmware hash.`);
  return { record, firmware: { ...firmware, stock_firmware_sha256: sha256 }, version: version! };
}

async function observeBeforeCapture(
  command: Command,
  session: DkSession,
  { record, version, firmware }: ReturnType<typeof verifyFirmware>,
) {
  const reader = dt2StockSampleReader(
    readFileSync(path.join(command.root, 'ports', `${spec.port}.toml`), 'utf8'),
    firmware.stock_firmware_sha256,
  );
  const expected = identifyNativeSample(await command.loadSample(), renderFixture());
  return readStockDelayCaptureState(session, record, version, reader, expected);
}

function delayTimeText(state: Awaited<ReturnType<typeof observeBeforeCapture>>): string {
  const [min, max] = state.expected_delay_time_raw_range.map(hexWord);
  return `Delay TIME ${hexWord(state.delay_time_raw)} raw (reference range ${min}–${max})`;
}

async function inspect(command: Command) {
  const { opts, devices, connectReady } = command;
  const link = await connectReady();
  try {
    const target = verifyFirmware(await link.session.info(), devices);
    const state = await observeBeforeCapture(command, link.session, target);
    if (opts.format === 'json') console.log(json({ port: spec.port, ...target.firmware, ...state }));
    else {
      const sample = state.sample_matches ? 'probe verified' : state.loaded_sample ? 'different sample' : 'empty';
      console.log(
        `Track ${state.track} · sample slot ${state.sample_slot} (${sample}) · ${delayTimeText(state)} · Delay feedback raw ${state.delay_feedback_raw}`,
      );
    }
    return 0;
  } finally {
    await link.close();
  }
}

async function run(command: Command) {
  const { opts, devices, connectReady } = command;
  const scratchProject = opts['scratch-project']?.trim();
  const audioInput = opts['audio-input']?.trim();
  if (!scratchProject || !opts.out || (!opts.emu && !audioInput))
    throw new Error(
      'Choose a prepared scratch project, a new output directory and (on hardware) an exact USB audio input.',
    );
  const dir = path.resolve(opts.out);
  const link = await connectReady();
  const { session } = link;
  try {
    const verified = verifyFirmware(await session.info(), devices);
    const { record, firmware } = verified;
    if (!record.midi) throw new Error('Target has no registered MIDI map.');
    if ((await session.modules()).some(isContentModule))
      throw new Error('Use a scratch project without digikit content modules for the stock reference.');
    if (!(await session.remoteState()).enabled) throw new Error('Enable the digikit remote module first.');
    const emu = opts.emu ? link.call : undefined;
    if (opts.emu && !emu) throw new Error('The emulator connection has no control socket.');
    if (emu && (await emu<{ mode: string }>('status')).mode !== 'free')
      throw new Error('Resume the emulator before recording.');

    const observed = await observeBeforeCapture(command, session, verified);
    if (!observed.sample_matches) {
      const key = observed.loaded_sample;
      const actual = key ? `${key.content_hash}, ${key.stored_bytes} stored bytes` : 'no loaded sample';
      throw new Error(
        `Track ${observed.track} slot ${observed.sample_slot} has ${actual}. Assign the ${spec.fixture} probe before recording.`,
      );
    }
    if (!observed.delay_time_in_range) {
      throw new Error(
        `${delayTimeText(observed)}. Set the project to ${spec.project.tempoBpm} BPM and adjust Delay TIME near 1/32 before recording. Use audio reference inspect to recheck.`,
      );
    }
    const target = makeTarget(link, record.midi, emu);
    const recordAudio = emu ? emulatorAudio(emu) : usbAudio(opts['audio-bin'], audioInput!);
    mkdirSync(path.dirname(dir), { recursive: true });
    mkdirSync(dir); // no overwriting or mixing with another run
    const waves = new Map<string, Wave>();
    const captures = await captureStockDelay(target, async (kind, send, play) => {
      const file = `${kind}.wav`;
      const source = await recordAudio(path.join(dir, file), play);
      const wave = parseWave(readFileSync(path.join(dir, file)));
      waves.set(kind, wave);
      return {
        file,
        send,
        ...source,
        rate_hz: wave.rate,
        channels: wave.channels,
        frames: wave.samples.length / wave.channels,
      };
    });

    let measurements;
    let error;
    try {
      measurements = measureDelay(waves.get('dry')!, waves.get('delay')!);
    } catch (e) {
      error = message(e);
    }
    const passed = measurements?.passed ?? false;
    const report = {
      schema,
      case: spec.id,
      target: emu ? 'emulator' : 'hardware',
      port: spec.port,
      ...firmware,
      fixture: { file: spec.fixture, sha256: spec.fixtureSha256, assignment: 'loaded-native-key' },
      project: { name: scratchProject, identity: 'operator-declared' },
      validation: {
        settings: 'acoustic',
        observed_before_capture: observed,
        spacing_ms: spec.spacingMs,
        loss_db_per_repeat: spec.lossDb,
      },
      captures,
      passed,
      measurements,
      error,
    };
    const file = path.join(dir, 'reference.json');
    writeFileSync(file, json(report) + '\n', { flag: 'wx' });
    if (opts.format === 'json') console.log(json(report));
    else
      console.log(
        measurements?.passed
          ? `${measurements.spacingMs.toFixed(3)} ms · ${measurements.lossDbPerRepeat.toFixed(3)} dB/repeat · ${file}`
          : `Reference did not pass: ${error ?? 'outside the expected spacing or decay range'} · ${file}`,
      );
    return passed ? 0 : 1;
  } finally {
    await link.close();
  }
}

function compare(files: string[], format?: string) {
  const reports = files.map((f) => JSON.parse(readFileSync(f, 'utf8')));
  for (const r of reports) {
    if (
      r.schema !== schema ||
      r.case !== spec.id ||
      r.port !== spec.port ||
      r.fixture?.sha256 !== spec.fixtureSha256 ||
      !/^[a-f0-9]{64}$/.test(r.stock_firmware_sha256 ?? '') ||
      !r.passed ||
      !r.measurements?.passed
    )
      throw new Error('Both inputs must be passing reports for the same reference case and fixture.');
  }
  if (reports[0].stock_firmware_sha256 !== reports[1].stock_firmware_sha256)
    throw new Error('Reports were recorded with different firmware.');
  const result = compareDelay(reports[0].measurements, reports[1].measurements);
  if (format === 'json') console.log(json({ schema: 'dk.audio-reference-compare/1', case: spec.id, ...result }));
  else
    console.log(
      `${result.passed ? 'PASS' : 'DIFFER'} · Δ ${result.deltaSpacingMs.toFixed(3)} ms · Δ ${result.deltaLossDbPerRepeat.toFixed(3)} dB/repeat (candidate − baseline)`,
    );
  return result.passed ? 0 : 1;
}

export async function referenceCommand(args: Command) {
  const [action, ...paths] = args.rest;
  try {
    const cases = args.opts.case ?? [];
    if (cases.length > 1 || (cases.length === 1 && cases[0] !== spec.id))
      throw new Error(`Choose the reference case with --case ${spec.id}.`);
    if (action === 'prepare' && args.opts.out && !paths.length) {
      const file = prepare(path.resolve(args.opts.out));
      console.log(args.opts.format === 'json' ? json({ file, sha256: spec.fixtureSha256 }) : file);
      return 0;
    }
    if (action === 'prepare-project' && !paths.length) {
      const result = await prepareProject({
        root: args.root,
        registry: args.devices,
        emulator: Boolean(args.opts.emu),
        firmware: args.opts.firmware,
        name: args.opts['scratch-project'],
        output: args.opts.out,
        wav: renderFixture(),
        async openSampleTarget() {
          const link = await args.connectReady();
          try {
            const { record } = verifyFirmware(await link.session.info(), args.devices);
            if (record.storage?.samples !== 'stock-rpc')
              throw new Error('The target has no stock +Drive sample backend.');
            const backend = await args.openUploadBackend(link.session);
            return { backend, close: () => link.close() };
          } catch (e) {
            await link.close();
            throw e;
          }
        },
      });
      if (args.opts.format === 'json') console.log(json(result));
      else
        console.log(
          [
            result.target === 'emulator'
              ? `Workbench session created at ${result.session}`
              : `${result.sample.reused ? 'Reused' : 'Uploaded'} and verified ${result.sample.destination}`,
            'Panel setup required',
            ...result.setupRemaining.map((line, i) => `  ${i + 1}. ${line}`),
          ].join('\n'),
        );
      return 0;
    }
    if (action === 'inspect' && !paths.length) return await inspect(args);
    if (action === 'run' && !paths.length) return await run(args);
    if (action === 'inputs' && !paths.length) {
      const inputs = await listAudioInputs(args.opts['audio-bin']);
      console.log(args.opts.format === 'json' ? json(inputs) : inputs.map((i) => `${i.id}\t${i.name}`).join('\n'));
      return 0;
    }
    if (action === 'compare' && paths.length === 2) return compare(paths, args.opts.format);
    throw new Error(referenceUsage);
  } catch (e) {
    console.error(`wb: ${message(e)}`);
    return 1;
  }
}
