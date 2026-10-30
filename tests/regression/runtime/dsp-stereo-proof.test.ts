import { execFile } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { button, repoRoot, resolveInputs } from '@digi/test-support';
import { describe, expect, test } from 'vitest';
import { withSession } from '../support.ts';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';

const binary = process.env.DIGI_STEREO_PROOF_EMUD;
const patchkit = process.env.DIGI_PATCHKIT_BIN;
const enabled = Boolean(binary && patchkit);
const root = repoRoot();
const exec = promisify(execFile);
const hash = (file: string) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const total = 13_200_000; // Fixed CF instruction window; no wall-clock note scheduling.
const counters = [
  'render_calls',
  'shadow_calls',
  'reset_calls',
  'reset_mirrors',
  'mismatches',
  'nonzero_input',
  'nonidentity_coeff',
  'fallbacks',
  'mode_errors',
  'nonfinite',
  'opposite_audio',
  'opposite_history',
] as const;
const firstFailure = ['slot', 'state', 'input', 'output', 'sample', 'left', 'right', 'mode1'] as const;
const contextFields = [
  'mode',
  'active_mode',
  'numeric_mode',
  'low_entry_frame',
  'low_entry_sp',
  'coeff_slot',
  'coeff21',
  'coeff22',
  'coeff23',
] as const;

async function bounded<T>(operation: Promise<T>, ms: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      operation,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} exceeded ${ms} ms`)), ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function pcm(file: string): Buffer {
  const wav = fs.readFileSync(file);
  expect(wav.toString('ascii', 0, 4)).toBe('RIFF');
  expect(wav.readUInt32LE(4) + 8).toBe(wav.length);
  expect(wav.toString('ascii', 8, 16)).toBe('WAVEfmt ');
  expect(wav.readUInt32LE(16)).toBe(16);
  expect(wav.readUInt16LE(20)).toBe(3);
  expect(wav.readUInt16LE(22)).toBe(2);
  expect(wav.readUInt32LE(24)).toBe(48000);
  expect(wav.readUInt16LE(32)).toBe(8);
  expect(wav.readUInt16LE(34)).toBe(32);
  expect(wav.toString('ascii', 36, 40)).toBe('data');
  expect(wav.readUInt32LE(40)).toBe(wav.length - 44);
  const data = wav.subarray(44);
  expect(data.length % 8).toBe(0);
  for (const channel of [0, 1]) {
    let nonzero = 0;
    for (let i = channel * 4; i < data.length; i += 8) {
      const value = data.readFloatLE(i);
      expect(Number.isFinite(value)).toBe(true);
      if (Math.abs(value) > 1e-6) nonzero++;
    }
    expect(nonzero, `audible channel ${channel}`).toBeGreaterThan(1000);
  }
  return data;
}

// Same bounded whole-stereo-frame alignment as pk_proofkit.aligned_equal.
// Keep both interleaved channels, including zeros between active samples.
function alignedStereo(reference: Buffer, candidate: Buffer): number | null {
  for (const shift of Array.from({ length: 513 }, (_, n) => n - 256).sort((a, b) => Math.abs(a) - Math.abs(b))) {
    const x = shift >= 0 ? reference.subarray(shift * 8) : reference;
    const y = shift >= 0 ? candidate : candidate.subarray(-shift * 8);
    const length = Math.min(x.length, y.length);
    let first = -1,
      last = -1,
      active = 0;
    for (let i = 0; i < length; i += 4) {
      if (Math.abs(x.readFloatLE(i)) > 1e-6) {
        if (first < 0) first = i;
        last = i;
        active++;
      }
    }
    if (active > 2000 && x.subarray(first, last + 4).equals(y.subarray(first, last + 4))) return shift;
  }
  return null;
}

// Explicit immutable daemon and existing registry-ready DN2 1.12 inputs only.
// It applies only this proof module; no daemon build, ready-input regeneration or default-set change.
describe.skipIf(!enabled).each([0, 1])('private DN2 first-stage observer mode %i', (mode) => {
  const runDir = path.join(root, 'out/runs/dsp-stereo-proof/native', randomUUID());
  if (enabled) {
    for (const file of [binary!, patchkit!]) {
      if (!path.isAbsolute(file) || !fs.existsSync(file))
        throw new Error(`existing absolute executable required: ${file}`);
    }
    fs.mkdirSync(runDir, { recursive: true });
  }
  const handle = withSession({ runDir, binary, device: 'dn2', firmware: '1.12', dsp: 'sharc', restart: false });

  test('keeps audible stereo baseline and observes private histories/reset identity', async () => {
    const session = handle.session;
    const evidence: Record<string, unknown> = { requestedMode: mode };
    let stopped = false;
    let failureDiagnostic: (() => Promise<Record<string, number>>) | undefined;
    const save = (name: string, value: unknown) =>
      fs.writeFileSync(path.join(runDir, name), JSON.stringify(value, null, 2));
    const rpc = async <T>(method: string, params: object = {}, ms = 5000): Promise<T> => {
      if (stopped) throw new Error('stereo proof has stopped');
      return bounded(session.client.call<T>(method, params), ms, method);
    };
    const capture = async (name: string) => {
      await rpc('run.mode', { mode: 'paused' });
      const before = await rpc<{ elapsed: number }>('status');
      expect(Number.isSafeInteger(before.elapsed)).toBe(true);
      const file = path.join(runDir, `${name}.wav`);
      await rpc('pcm.tap', { action: 'start' });
      await rpc('pcm.tap', { action: 'capture', path: file });
      try {
        await rpc('input.schedule', {
          events: [{ at_instr: before.elapsed + 132_000, button: button('TRIG 1'), down: true }],
        });
        await rpc('step', { instructions: total, timeout_wall_s: 120 }, 125000);
        await rpc('run.mode', { mode: 'paused' });
      } finally {
        await rpc('input.button', { code: button('TRIG 1'), down: false });
        await rpc('pcm.tap', { action: 'capture_stop' });
      }
      await bounded(session.expectNoFault(name), 5000, `${name} health`);
      const data = pcm(file);
      evidence[name] = { file, sha256: hash(file), stereoFrames: data.length / 8, status: await rpc('status') };
      return data;
    };
    const run = async () => {
      const inputs = resolveInputs('dn2', '1.12');
      const stockSha = hash(inputs.syx);
      evidence.identity = {
        inputs,
        stockSha,
        readyHashes: {
          cfSnapshot: hash(inputs.cfSnapshot),
          dspImage: hash(inputs.dspImage),
          dspState: hash(inputs.dspState),
        },
        stockLoad: session.loadResult,
        binary: { path: binary, sha256: hash(binary!) },
        patchkit: { path: patchkit, sha256: hash(patchkit!) },
      };
      expect(session.loadResult).toMatchObject({ start_used: 'snapshot', snapshot_ok: true, audio: true });
      const baseline = await capture('stock');
      const checked = await exec(patchkit!, ['--root', root, 'build', 'core/dsp-stereo-proof', '--check'], {
        cwd: root,
        timeout: 30000,
      });
      fs.writeFileSync(path.join(runDir, 'object-check.log'), checked.stdout + checked.stderr);
      const output = path.join(runDir, 'stereo-proof.syx');
      const planFile = path.join(runDir, 'stereo-proof.plan.json');
      const applied = await exec(
        patchkit!,
        [
          '--root',
          root,
          'apply',
          '--firmware',
          inputs.syx,
          '--modules',
          'core/dsp-stereo-proof',
          '-o',
          output,
          '--plan',
          planFile,
        ],
        { cwd: root, timeout: 30000, maxBuffer: 4 * 1024 * 1024 },
      );
      fs.writeFileSync(path.join(runDir, 'patch.log'), applied.stdout + applied.stderr);
      const plan = JSON.parse(fs.readFileSync(planFile, 'utf8')) as RuntimePlan;
      expect(plan.base).toMatchObject({ device: 'dn2', version: '1.12', syx_sha256: stockSha });
      expect(plan.modules.map((m) => m.split('@')[0])).toEqual(['core/dsp-stereo-proof']);
      for (const name of counters) planSymbol(plan, `_sp_${name}`);
      for (const name of firstFailure) planSymbol(plan, `_sp_first_${name}`);
      const loaded = await rpc<{ syx_sha256: string; start_used: string; snapshot_ok: boolean; audio: boolean }>(
        'firmware.load',
        { syx: output, base_sha256: stockSha, start: 'snapshot', force: true, auto_no: true },
        180000,
      );
      expect(loaded.syx_sha256).toBe(plan.output?.syx_sha256);
      expect(loaded).toMatchObject({ start_used: 'snapshot', snapshot_ok: true, audio: true });
      evidence.patchedLoad = loaded;
      const snapshotDiagnostic = async () => {
        const read = async (name: string) => {
          const address = planSymbol(plan, name);
          const result = await rpc<{ hex: string; unknown: number }>('dsp.peek', { addr: address, len: 4 });
          expect(result.unknown, name).toBe(0);
          return Buffer.from(result.hex, 'hex').readUInt32LE();
        };
        const diagnostic: Record<string, number> = {};
        for (const name of [...counters, ...contextFields]) diagnostic[name] = await read(`_sp_${name}`);
        for (const name of firstFailure) diagnostic[`first_${name}`] = await read(`_sp_first_${name}`);
        return diagnostic;
      };
      failureDiagnostic = snapshotDiagnostic;
      // The private knob is written only while paused, before any accepted entry.
      const initial = await snapshotDiagnostic();
      evidence.beforeMode = initial;
      expect(initial.render_calls).toBe(0);
      expect(initial.reset_calls).toBe(0);
      expect(initial.active_mode).toBe(0xffff_ffff);
      expect(initial.mode).toBe(0);
      await rpc('dsp.patch', {
        text: `BASE ${stockSha}\nDM ${planSymbol(plan, '_sp_mode').toString(16)} ${mode.toString(16).padStart(8, '0')} was 00000000\n`,
      });
      const armed = await snapshotDiagnostic();
      expect(armed.mode).toBe(mode);
      expect(armed.active_mode).toBe(0xffff_ffff);
      const observed = await capture('patched');
      const shift = alignedStereo(baseline, observed);
      evidence.stereoFrameShift = shift;
      expect(shift, 'unchanged active audio in both actual channels').not.toBeNull();
      const diagnostic = await snapshotDiagnostic();
      evidence.diagnostic = diagnostic;
      save('diagnostic.json', diagnostic);
      const word = async (address: number) => {
        const result = await rpc<{ hex: string; unknown: number }>('dsp.peek', { addr: address, len: 4 });
        expect(result.unknown, `stack metadata at ${address.toString(16)}`).toBe(0);
        return Buffer.from(result.hex, 'hex').readUInt32LE();
      };
      // DN2 1.12 original-byte audit: AudioTask TCB publication and creator
      // store establish this allocation. This is scenario-specific headroom,
      // not a bound on all interrupt or scheduler nesting.
      const tcb = await word(0x268a84);
      expect(tcb % 4).toBe(0);
      expect(tcb).toBeGreaterThan(0);
      expect(tcb).toBeLessThan(0xffff_ffcc);
      const stackBase = await word(tcb + 0x30);
      expect(stackBase % 4).toBe(0);
      expect(stackBase).toBeGreaterThan(0);
      expect(stackBase).toBeLessThan(0xffff_f060);
      const exercisedDepth = 172;
      const lowFrame = diagnostic.low_entry_frame!;
      const lowSp = diagnostic.low_entry_sp!;
      const stack = {
        tcb,
        base: stackBase,
        end: stackBase + 4000,
        lowFrame,
        lowSp,
        exercisedDepth,
        remainingBelowObservedStores: lowFrame - exercisedDepth - stackBase,
        scope: 'Observed entry minimum minus exercised model store depth only; no universal stack-headroom claim.',
      };
      evidence.stack = stack;
      save('stack.json', stack);
      expect(lowFrame - exercisedDepth).toBeGreaterThanOrEqual(stackBase);
      expect(lowFrame).toBeLessThan(stackBase + 4000);
      expect(lowSp).toBeGreaterThanOrEqual(stackBase);
      expect(lowSp).toBeLessThan(stackBase + 4000);
      evidence.nonidentityCoefficientsObserved = diagnostic.nonidentity_coeff! > 0;
      expect(diagnostic.render_calls).toBeGreaterThan(0);
      expect(diagnostic.shadow_calls).toBe(diagnostic.render_calls);
      expect(diagnostic.nonzero_input, 'identity was exercised with real nonzero input').toBeGreaterThan(0);
      expect(diagnostic.mode).toBe(mode);
      expect(diagnostic.active_mode).toBe(mode);
      expect(diagnostic.mode_errors).toBe(0);
      expect(diagnostic.nonfinite).toBe(0);
      expect(diagnostic.mismatches).toBe(0);
      for (const name of ['opposite_audio', 'opposite_history']) {
        if (mode === 1) expect(diagnostic[name], name).toBeGreaterThan(0);
        else expect(diagnostic[name], name).toBe(0);
      }
      expect(diagnostic.fallbacks).toBe(0);
      expect(diagnostic.reset_mirrors, 'actual initialized private reset was exercised').toBeGreaterThan(0);
      expect(diagnostic.reset_calls).toBeGreaterThanOrEqual(diagnostic.reset_mirrors!);
      evidence.scope =
        'Private first-order finite same-MODE1 identity/polarity relation only; metamorphic independence evidence, not an independent correctness oracle or40-bit proof. Nonidentity coefficient count is observed, not assumed. No production routing, DDR ownership, IRQ exclusion or hardware claim.';
    };
    try {
      await bounded(run(), 480000, 'private stereo proof');
      save('result.json', { passed: true, ...evidence });
    } catch (error) {
      if (failureDiagnostic) {
        try {
          evidence.failureDiagnostic = await bounded(failureDiagnostic(), 5000, 'failure diagnostic snapshot');
          save('failure-diagnostic.json', evidence.failureDiagnostic);
        } catch (diagnosticError) {
          evidence.failureDiagnostic = { error: String(diagnosticError) };
          save('failure-diagnostic.json', evidence.failureDiagnostic);
        }
      } else {
        evidence.failureDiagnostic = { unavailable: 'patched load did not complete' };
      }
      save('result.json', { passed: false, error: String(error), ...evidence });
      throw error;
    } finally {
      stopped = true;
      await bounded(session.close(), 10000, 'owned daemon shutdown');
    }
  }, 500000);
});
