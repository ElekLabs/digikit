import { execFile } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { button, repoRoot, resolveInputs, type Session } from '@digi/test-support';
import { describe, expect, test } from 'vitest';
import { withSession } from '../support.ts';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';

const root = repoRoot();
const binary = process.env.DIGI_EMUD_DN2;
const patchkit = process.env.DIGI_PATCHKIT_BIN;
// Explicit diagnostic opt-in. Generic runtime suites skip this file without preflighted binaries.
const preflighted = Boolean(binary && patchkit);
const MAGIC = 0xddf4a601;
const TOKEN = 0x81234567;
const BASE = 0x80800000;
const BYTES = 960000;
const CANARY = 0xa55a3cc3;
const exec = promisify(execFile);

function checksum(words: number[]): number {
  let s1 = 0;
  let s2 = 0;
  for (const word of words) {
    s1 = (s1 + word) >>> 0;
    s2 = (s2 + s1) >>> 0;
  }
  return (s1 ^ s2) >>> 0;
}

function frame(seq: number, op: number, offset = 0, data = Buffer.alloc(0), token = TOKEN): Buffer {
  if (data.length % 4 || data.length > 2560) throw new Error('payload must be at most 640 complete words');
  const header = [4, MAGIC, token, seq, op, offset, data.length / 4];
  const payload = Array.from({ length: data.length / 4 }, (_, i) => data.readUInt32LE(i * 4));
  const words = [...header, checksum([...header, ...payload]), ...Array<number>(24).fill(0), ...payload];
  const out = Buffer.alloc(2688);
  words.forEach((word, i) => {
    out.writeUInt16BE(word & 65535, i * 4);
    out.writeUInt16BE(word >>> 16, i * 4 + 2);
  });
  return out;
}

interface RawChunk {
  addr: number;
  len: number;
  hex: string;
  unknown: number;
}

async function rawPeek(session: Session, addr: number, length: number): Promise<RawChunk[]> {
  const chunks: RawChunk[] = [];
  for (let off = 0; off < length; off += 4096) {
    const len = Math.min(4096, length - off);
    const reply = (await session.client.call('dsp.peek', { addr: addr + off, len })) as {
      hex: string;
      unknown: number;
    };
    expect(reply.hex.length).toBe(len * 2);
    chunks.push({ addr: addr + off, len, hex: reply.hex, unknown: reply.unknown });
  }
  return chunks;
}

async function peek(session: Session, addr: number, length: number): Promise<Buffer> {
  const chunks = await rawPeek(session, addr, length);
  return Buffer.concat(
    chunks.map((chunk) => {
      expect(chunk.unknown, 'successfully written memory is fully known').toBe(0);
      expect(chunk.hex).toMatch(/^[0-9a-fA-F]+$/);
      return Buffer.from(chunk.hex, 'hex');
    }),
  );
}

async function prepare(session: Session) {
  const output = path.join(session.runDir, 'ddr-proof.syx');
  const planFile = path.join(session.runDir, 'ddr-proof.plan.json');
  // Explicit prebuilt CLI: this diagnostic must never trigger a Cargo/daemon build.
  await exec(
    patchkit!,
    [
      '--root',
      root,
      'apply',
      '--firmware',
      resolveInputs('dn2', '1.12').syx,
      '--modules',
      'core/dsp-ddr-proof',
      '-o',
      output,
      '--plan',
      planFile,
    ],
    { cwd: root, timeout: 30000, maxBuffer: 4 * 1024 * 1024 },
  );
  const plan: RuntimePlan = JSON.parse(fs.readFileSync(planFile, 'utf8'));
  const stockSha = createHash('sha256')
    .update(fs.readFileSync(resolveInputs('dn2', '1.12').syx))
    .digest('hex');
  expect(plan.output?.syx_sha256).toMatch(/^[a-f0-9]{64}$/);
  expect(plan.base.device).toBe('dn2');
  expect(plan.base.version).toBe('1.12');
  expect(plan.base.syx_sha256).toBe(stockSha);
  expect(plan.modules.map((module) => module.split('@')[0]).sort()).toEqual([
    'core/dsp-ddr-proof',
    'core/dsp-voice',
    'core/expansion',
    'core/machines',
    'core/params',
  ]);
  fs.writeFileSync(path.join(session.runDir, 'stock-identity.json'), JSON.stringify(session.loadResult, null, 2));
  let timer: ReturnType<typeof setTimeout> | undefined;
  let load;
  try {
    load = await Promise.race([
      session.client.firmwareLoad({ syx: output, base_sha256: stockSha, start: 'auto', auto_no: true }),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error('patched diagnostic startup exceeded 120000 ms')), 120000);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
  expect(load.syx_sha256).toBe(plan.output?.syx_sha256);
  await session.client.call('run.mode', { mode: 'free' });
  await expect.poll(async () => (await session.status()).cf.ready, { timeout: 30000 }).toBe(true);
  const status = await session.status();
  expect(status.firmware?.base_sha256).toBe(stockSha);
  expect(status.firmware?.syx_sha256).toBe(plan.output?.syx_sha256);
  expect(status.firmware?.injected).toBe(true);
  fs.writeFileSync(path.join(session.runDir, 'patched-load.json'), JSON.stringify(load, null, 2));
  fs.writeFileSync(path.join(session.runDir, 'patched-status.json'), JSON.stringify(status, null, 2));
  const mail = planSymbol(plan, 'dd_mail');
  const staging = planSymbol(plan, 'dd_frame');
  const ready = planSymbol(plan, '_dd_ready');
  const words = planSymbol(plan, '_dd_words');
  async function request(data: Buffer, expectedState = 2, expectedError = 0): Promise<void> {
    const state = await session.client.memPeek(mail);
    expect([0, 2], 'one request in flight; error requires a fresh boot').toContain(state);
    await session.client.call('mem.cf.write', { addr: staging, hex: data.toString('hex') });
    await session.client.call('mem.cf.poke', { addr: mail, value: 4 }); // Publish last.
    await expect.poll(async () => session.client.memPeek(mail), { timeout: 30000, interval: 20 }).toBe(expectedState);
    const error = await session.client.memPeek(mail + 4);
    fs.appendFileSync(
      path.join(session.runDir, 'requests.jsonl'),
      JSON.stringify({
        header: data.subarray(0, 32).toString('hex'),
        state: expectedState,
        error,
        stall: await session.client.memPeek(mail + 8),
        sent: await session.client.memPeek(mail + 12),
      }) + '\n',
    );
    expect(error).toBe(expectedError);
  }
  async function dspWord(addr: number): Promise<number> {
    return (await peek(session, addr, 4)).readUInt32LE();
  }
  return { request, ready: () => dspWord(ready), words: () => dspWord(words), plan };
}

function quietProof(name: string, body: (session: Session) => Promise<void>) {
  describe.skipIf(!preflighted)(name, () => {
    const runDir = path.join(root, 'out/runs/ddr-proof', randomUUID());
    if (preflighted) fs.mkdirSync(runDir, { recursive: true });
    const handle = withSession({
      runDir,
      device: 'dn2',
      firmware: '1.12',
      dsp: 'sharc',
      binary,
      restart: false,
    });
    test(
      name,
      async () => {
        await expect.poll(async () => (await handle.session.status()).cf.ready, { timeout: 30000 }).toBe(true);
        fs.writeFileSync(path.join(runDir, 'identity.json'), JSON.stringify(handle.session.loadResult, null, 2));
        try {
          await body(handle.session);
          await handle.session.expectNoFault(name);
          fs.writeFileSync(path.join(runDir, 'result.json'), JSON.stringify({ name, passed: true }));
        } catch (error) {
          fs.writeFileSync(
            path.join(runDir, 'result.json'),
            JSON.stringify({ name, passed: false, error: String(error) }),
          );
          throw error;
        }
      },
      360000,
    );
  });
}

interface Registry {
  name: string;
  addr: number;
  first: number;
  row_size: number;
  fields: [string, string, number][];
  entries: { id: string; index: number; module: string }[];
}

async function playbackProof(session: Session, plan: RuntimePlan): Promise<void> {
  const registries = (plan as RuntimePlan & { registries: Registry[] }).registries;
  const machines = registries.find((registry) => registry.name === 'machines')!;
  const voices = registries.find((registry) => registry.name === 'voices')!;
  const machine = machines.entries.find((entry) => entry.id === 'ddr_proof')!;
  const voice = voices.entries.find((entry) => entry.id === 'ddr_proof')!;
  expect(machine.module.split('@')[0]).toBe('core/dsp-ddr-proof');
  const field = (registry: Registry, index: number, name: string) => {
    const offset = registry.fields.find(([key]) => key === name)?.[2];
    if (offset === undefined) throw new Error(`missing ${registry.name}.${name}`);
    return registry.addr + (index - registry.first) * registry.row_size + offset;
  };
  const position = await session.client.memPeek(field(machines, machine.index, 'position'));
  expect(typeof position).toBe('number');
  expect(position).toBeGreaterThanOrEqual(0);
  expect(position).toBeLessThan(16);
  const word = async (addr: number) => (await peek(session, addr, 4)).readUInt32LE();
  expect(await word(field(voices, voice.index, 'type'))).toBe(machine.index);
  expect(await word(field(voices, voice.index, 'pc'))).toBe(planSymbol(plan, '_dd_play_render'));
  expect(await word(field(voices, voice.index, 'note_on'))).toBe(planSymbol(plan, '_dd_play_note_on'));
  const descriptor = await peek(session, planSymbol(plan, '_dd_play_desc'), 40);
  const descriptorWords = Array.from({ length: 10 }, (_, i) => descriptor.readUInt32LE(i * 4));
  expect(descriptorWords.slice(2, 6)).toEqual([1, 16, 24, 24]);
  fs.writeFileSync(
    path.join(session.runDir, 'playback-binding.json'),
    JSON.stringify(
      {
        machine,
        position,
        voice,
        descriptorWords,
        descriptorHex: descriptor.toString('hex'),
        patchedImageHash: plan.output?.syx_sha256,
      },
      null,
      2,
    ),
  );
  const normalAddress = planSymbol(plan, '_dd_normal_frames');
  const normalBefore = await word(normalAddress);
  await expect.poll(async () => word(normalAddress), { timeout: 10000, interval: 25 }).toBeGreaterThan(normalBefore);

  // Existing panel path on the emulated instruction clock. The displayed list
  // position comes from the live patched registry, rather than a fixed type.
  const clock = async () => {
    const value = (await session.status()).elapsed;
    if (typeof value !== 'number') throw new Error('panel proof requires elapsed instruction clock');
    return value;
  };
  const until = async (target: number) => {
    await expect.poll(clock, { timeout: 20000, interval: 25 }).toBeGreaterThanOrEqual(target);
  };
  const settle = async (ms: number) => until((await clock()) + ms * 132000);
  const tap = async (name: string) => {
    const reply = (await session.client.inputTap(button(name), 20 * 132000)) as { release_at: number };
    expect(typeof reply.release_at).toBe('number');
    await until(reply.release_at + 5 * 132000);
  };
  await tap('NO');
  await session.client.inputButton(button('FUNC'), true);
  try {
    await settle(5);
    await tap('SRC');
  } finally {
    await session.client.inputButton(button('FUNC'), false);
  }
  await settle(200);
  for (let i = 0; i < 16; i++) {
    await tap('UP');
    await settle(40);
  }
  for (let i = 0; i < position!; i++) {
    await tap('DOWN');
    await settle(80);
  }
  fs.writeFileSync(
    path.join(session.runDir, 'machine-list.json'),
    JSON.stringify(await session.client.framebufferGet({ hash: true, data: true })),
  );
  await tap('YES');
  await settle(300);
  await tap('YES');
  await settle(300);
  fs.writeFileSync(
    path.join(session.runDir, 'machine-page.json'),
    JSON.stringify(await session.client.framebufferGet({ hash: true, data: true })),
  );

  const stateAddress = planSymbol(plan, '_dd_play_state');
  const notesAddress = planSymbol(plan, '_dd_play_notes');
  const blocksAddress = planSymbol(plan, '_dd_play_blocks');
  const doneAddress = planSymbol(plan, '_dd_play_done');
  const normalAtNote = await word(normalAddress);
  const notesBefore = await word(notesAddress);
  const snapshots: {
    cursor: number[];
    notes: number;
    blocks: number;
    done: number;
    normal: number;
    seconds: number;
  }[] = [];
  const wavPath = path.join(session.runDir, 'ten-second-ddr.wav');
  await session.client.call('pcm.tap', { action: 'start' });
  await session.client.call('pcm.tap', { action: 'capture', path: wavPath });
  try {
    await session.client.inputButton(button('TRIG 1'), true);
    await expect
      .poll(
        async () => {
          const state = await word(stateAddress);
          const notes = await word(notesAddress);
          if (!state || notes === notesBefore) return false;
          const cursorBytes = await peek(session, state, 24);
          const cursor = Array.from({ length: 6 }, (_, i) => cursorBytes.readUInt32LE(i * 4));
          const tapReport = (await session.client.call('pcm.tap', { action: 'report' })) as { seconds: number };
          const sample = {
            cursor,
            notes,
            blocks: await word(blocksAddress),
            done: await word(doneAddress),
            normal: await word(normalAddress),
            seconds: tapReport.seconds,
          };
          snapshots.push(sample);
          return sample.done === 1 && tapReport.seconds >= 11.5;
        },
        { timeout: 180000, interval: 50 },
      )
      .toBe(true);
  } finally {
    await session.client.inputButton(button('TRIG 1'), false);
    await session.client.call('pcm.tap', { action: 'capture_stop' });
    fs.writeFileSync(path.join(session.runDir, 'playback-states.json'), JSON.stringify(snapshots, null, 2));
  }
  const final = snapshots.at(-1)!;
  const dispatchedType = await word(planSymbol(plan, '_dv_c'));
  const output = await word(planSymbol(plan, '_dd_play_output'));
  const completedOutput = await peek(session, output, 128);
  // This is the stock engine's workspace, which may be reused downstream.
  // The numerical consumer test proves the extra render after DONE is zero.

  const wav = fs.readFileSync(wavPath);
  expect(wav.toString('ascii', 0, 4)).toBe('RIFF');
  expect(wav.readUInt16LE(20)).toBe(3);
  expect(wav.readUInt16LE(22)).toBe(2);
  expect(wav.readUInt32LE(24)).toBe(48000);
  expect(wav.readUInt32LE(40) + 44).toBe(wav.length);
  const pcm = wav.subarray(44);
  expect(pcm.length / 8).toBeGreaterThanOrEqual(11.5 * 48000);
  let finite = true;
  for (let i = 0; i < pcm.length; i += 4) finite &&= Number.isFinite(pcm.readFloatLE(i));
  expect(finite).toBe(true);
  const window = (start: number) => {
    const samples = Array.from({ length: 48000 }, (_, i) => {
      const offset = (start * 48000 + i) * 8;
      return (pcm.readFloatLE(offset) + pcm.readFloatLE(offset + 4)) * 0.5;
    });
    const rms = Math.sqrt(samples.reduce((sum, value) => sum + value * value, 0) / samples.length);
    const correlation = (lag: number) => {
      let dot = 0;
      let a = 0;
      let b = 0;
      for (let i = 0; i < samples.length - lag; i++) {
        dot += samples[i]! * samples[i + lag]!;
        a += samples[i]! ** 2;
        b += samples[i + lag]! ** 2;
      }
      return dot / Math.sqrt(a * b);
    };
    return { rms, period: correlation(240), halfPeriod: correlation(120) };
  };
  const early = window(1);
  const late = window(8);

  fs.writeFileSync(
    path.join(session.runDir, 'playback-result.json'),
    JSON.stringify(
      {
        machine,
        position,
        voice,
        descriptorWords,
        descriptorHex: descriptor.toString('hex'),
        patchedImageHash: plan.output?.syx_sha256,
        final,
        normalBefore,
        normalAtNote,
        dispatchedType,
        finite,
        capturedFrames: pcm.length / 8,
        early,
        late,
        output,
        completedOutput: completedOutput.toString('hex'),
        wavPath,
      },
      null,
      2,
    ),
  );
  expect(final.notes).toBe(notesBefore + 1);
  expect(final.cursor).toEqual([0, 480000, 480000, 0, 65536, 1]);
  expect(final.blocks).toBe(15000);
  expect(snapshots.some((sample) => sample.cursor[2]! > 0 && sample.cursor[2]! < 240000)).toBe(true);
  expect(
    snapshots.some((sample) => sample.cursor[2]! >= 432000 && sample.cursor[2]! < 480000 && sample.done === 0),
  ).toBe(true);
  expect(final.normal).toBeGreaterThan(normalAtNote + 15000);
  expect(dispatchedType).toBe(machine.index);
  for (const content of [early, late]) {
    expect(content.rms, 'audible sustained source at early and late windows').toBeGreaterThan(0.001);
    expect(content.period, 'known 200 Hz source period survives playback').toBeGreaterThan(0.7);
    expect(content.halfPeriod).toBeLessThan(-0.2);
  }
}

quietProof('full quiet DDR transport: 960000 bytes, complete canaries and duplicate', async (session) => {
  const proof = await prepare(session);
  // Capture the entire candidate range and guard bands before INIT owns them.
  const baseline = await rawPeek(session, BASE - 4096, BYTES + 8192);
  fs.writeFileSync(path.join(session.runDir, 'ddr-before-init.json'), JSON.stringify(baseline));
  const data = Buffer.alloc(BYTES);
  for (let i = 0; i < BYTES / 2; i++) {
    const triangle = 8192 * (1 - 4 * Math.abs((i % 240) / 240 - 0.5));
    const fade = Math.min(1, i / 960, (479999 - i) / 960);
    data.writeInt16LE(Math.round(triangle * fade), i * 2);
  }
  await proof.request(frame(0, 1));
  expect(await proof.ready()).toBe(0);
  let last = frame(0, 1);
  for (let seq = 1; seq <= 375; seq++) {
    const offset = (seq - 1) * 2560;
    last = frame(seq, 2, offset / 4, data.subarray(offset, offset + 2560));
    await proof.request(last);
    if (seq === 1) {
      const before = await peek(session, BASE, 2560);
      await proof.request(last);
      expect(await proof.words()).toBe(640);
      expect(await peek(session, BASE, 2560)).toEqual(before);
    }
  }
  expect(await proof.ready()).toBe(0);
  expect(await proof.words()).toBe(240000);
  await proof.request(frame(376, 3, 240000), 2, 1);
  expect(await proof.ready()).toBe(1);
  const after = await peek(session, BASE, BYTES);
  expect(after.equals(data), 'entire transferred payload, not a prefix or hash only').toBe(true);
  const canary = Buffer.alloc(4096);
  for (let i = 0; i < 1024; i++) canary.writeUInt32LE(CANARY, 4 * i);
  expect(await peek(session, BASE - 4096, 4096)).toEqual(canary);
  expect(await peek(session, BASE + BYTES, 4096)).toEqual(canary);
  fs.writeFileSync(
    path.join(session.runDir, 'ddr-proof-result.json'),
    JSON.stringify(
      {
        bytes: BYTES,
        sha256: createHash('sha256').update(after).digest('hex'),
        baselineSha256: createHash('sha256').update(JSON.stringify(baseline)).digest('hex'),
        canaryBytes: 8192,
        ready: await proof.ready(),
        plan: proof.plan,
      },
      null,
      2,
    ),
  );
  await playbackProof(session, proof.plan);
});

quietProof('DDR out-of-range DATA poisons proof without READY', async (session) => {
  const proof = await prepare(session);
  await proof.request(frame(0, 1));
  const before = await rawPeek(session, BASE - 4096, BYTES + 8192);
  fs.writeFileSync(path.join(session.runDir, 'ddr-before-rejection.json'), JSON.stringify(before));
  await proof.request(frame(1, 2, 240000, Buffer.from([1, 2, 3, 4])), 3, 3);
  expect(await proof.ready()).toBe(0);
  expect(await proof.words()).toBe(0);
  const after = await rawPeek(session, BASE - 4096, BYTES + 8192);
  fs.writeFileSync(path.join(session.runDir, 'ddr-after-rejection.json'), JSON.stringify(after));
  expect(after).toEqual(before);
});

quietProof('DDR partial checksum failure retains backing and never publishes READY', async (session) => {
  const proof = await prepare(session);
  await proof.request(frame(0, 1));
  await proof.request(frame(1, 2, 0, Buffer.alloc(2560, 0x59)));
  const before = await rawPeek(session, BASE - 4096, BYTES + 8192);
  fs.writeFileSync(path.join(session.runDir, 'ddr-before-rejection.json'), JSON.stringify(before));
  const bad = frame(2, 2, 640, Buffer.alloc(2560, 0x96));
  bad[28] = bad[28]! ^ 1; // Corrupt checksum, leaving token/sequence/range valid.
  await proof.request(bad, 3, 2);
  expect(await proof.ready()).toBe(0);
  expect(await proof.words()).toBe(640);
  const after = await rawPeek(session, BASE - 4096, BYTES + 8192);
  fs.writeFileSync(path.join(session.runDir, 'ddr-after-rejection.json'), JSON.stringify(after));
  expect(after).toEqual(before);
});

quietProof('DDR stale token cannot adopt or write; CF watchdog resumes stock frames', async (session) => {
  const proof = await prepare(session);
  await proof.request(frame(0, 1));
  const before = await rawPeek(session, BASE - 4096, BYTES + 8192);
  fs.writeFileSync(path.join(session.runDir, 'ddr-before-rejection.json'), JSON.stringify(before));
  await proof.request(frame(1, 2, 0, Buffer.alloc(2560, 0x66), TOKEN + 1), 3, 100);
  expect(await proof.ready()).toBe(0);
  expect(await proof.words()).toBe(0);
  const after = await rawPeek(session, BASE - 4096, BYTES + 8192);
  fs.writeFileSync(path.join(session.runDir, 'ddr-after-rejection.json'), JSON.stringify(after));
  expect(after).toEqual(before);
});
