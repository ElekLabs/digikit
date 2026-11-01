import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, expect, test, vi } from 'vitest';
import type { Session } from '@digi/test-support';
import type { PatchedSession } from '../support.ts';
import type { RuntimePlan } from '../runtime/runtime-plan.ts';

const fake = vi.hoisted(() => ({
  fixture: vi.fn(),
  prepare: vi.fn(),
  analysis: vi.fn(),
  evidence: vi.fn(),
  exec: vi.fn(),
  ready: vi.fn(),
}));
vi.mock('node:child_process', () => ({
  execFile: Object.assign(vi.fn(), { [Symbol.for('nodejs.util.promisify.custom')]: fake.exec }),
}));
vi.mock('../support.ts', () => ({ withReusablePatchedSession: fake.fixture }));
vi.mock('../runtime/runtime-plan.ts', async (original) => ({
  ...(await original<object>()),
  prepareRuntimePlanOffline: fake.prepare,
}));
vi.mock('../runtime/fx-live.ts', async (original) => ({
  ...(await original<object>()),
  options: (device: string) => ({ device, firmware: device === 'dt2' ? '1.17' : '1.12' }),
  analysisJson: fake.analysis,
  evidence: fake.evidence,
}));
vi.mock('../runtime/live.ts', async (original) => ({ ...(await original<object>()), expectRuntimeUi: fake.ready }));
import { withRouteFxSession, readTextRows, tap, inspectRouteBoot } from '../runtime/fx-route-live.ts';

const directories: string[] = [];
afterEach(() => {
  vi.restoreAllMocks();
  for (const directory of directories.splice(0)) fs.rmSync(directory, { recursive: true, force: true });
});

for (const image of ['routes', 'track', 'track-base'] as const) {
  test(`${image} cards keep the complete inventory and exactly the selected store paths`, async () => {
    vi.resetAllMocks();
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-route-host-'));
    directories.push(directory);
    const artifacts = path.join(directory, 'artifacts');
    fs.mkdirSync(artifacts);
    const mktemp = fs.mkdtempSync.bind(fs);
    vi.spyOn(fs, 'mkdtempSync').mockImplementation(((prefix: string) =>
      String(prefix).includes('digi-fx-') ? artifacts : mktemp(prefix)) as typeof fs.mkdtempSync);
    const exists = fs.existsSync.bind(fs);
    vi.spyOn(fs, 'existsSync').mockImplementation((file) => String(file).endsWith('/plusdrive') || exists(file));
    const count = image === 'routes' ? 5 : 4;
    const ids =
      image === 'routes'
        ? ['sine', 'dyn-fx-gain', 'dyn-fx-gain-send', 'dyn-fx-gain-sendonly', 'dyn-fx-gain-send-3']
        : ['sine', 'dyn-fx-gain-both', 'dyn-fx-gain-both-2', 'dyn-fx-gain-both-3'];
    const modulesRoot = path.join(directory, 'modules');
    fs.mkdirSync(path.join(modulesRoot, 'dt2'), { recursive: true });
    ids.forEach((id) => fs.writeFileSync(path.join(modulesRoot, 'dt2', `${id}.dkm`), id));
    vi.stubEnv('DIGI_RUNTIME_MODULES', modulesRoot);
    const plan: RuntimePlan = {
      base: { device: 'dt2', version: '1.17', syx_sha256: 'a'.repeat(64) },
      modules: ['core/runtime'],
      symbols: { 'core/runtime:DKRT_BOOT': 4096 },
      output: { syx_sha256: 'b'.repeat(64) },
    };
    const preparedImage = {
      syx: '/prepared.syx',
      baseSha256: plan.base.syx_sha256,
      syxSha256: plan.output!.syx_sha256,
      modules: plan.modules,
    };
    fake.prepare.mockResolvedValue({ plan, preparedImage, planFile: '/prepared.plan.json', syx: '/prepared.syx' });
    fake.analysis.mockImplementation(async (_module: string, args: string[]) =>
      args[0] === 'profile'
        ? { fx_slots: 3, send_slots: 2, track_type_word: 1234 }
        : args[0] === 'runtime-check'
          ? { pass: true }
          : {},
    );
    fake.exec.mockResolvedValue({ stdout: '', stderr: '' });
    const result = withRouteFxSession('dt2', 'host route', {
      image,
      initial: 'dry',
      presets: {
        dry: { selection: ['modules/m0.dkm'] },
        wet: { selection: ['modules/m0.dkm', 'modules/m1.dkm'], settings: [['fx/route/test/effect', '01']] },
      },
      requireAudible: false,
    });
    const [opts, lifecycle] = fake.fixture.mock.calls[0]!;
    try {
      await opts.env();
      const session = { runDir: directory, status: vi.fn(async () => ({ epoch: 1 })) } as unknown as Session;
      await lifecycle.prepareBaseline(session);
      for (const tag of ['dry', 'wet']) {
        expect(fs.readdirSync(path.join(directory, tag, 'seed/digikit/modules'))).toEqual(
          Array.from({ length: count }, (_, i) => `m${i}.dkm`),
        );
      }
      expect(fs.readFileSync(path.join(directory, 'wet/seed/digikit/selection.txt'), 'utf8')).toBe(
        'modules/m0.dkm\nmodules/m1.dkm\n',
      );
      expect(JSON.parse(fs.readFileSync(path.join(directory, 'wet/settings.json'), 'utf8'))).toEqual([
        { key: 'fx/route/test/effect', value_hex: '01' },
      ]);
      expect(result.baseline('wet').preparedImage).toBe(preparedImage);
      expect(() => result.baseline('absent')).toThrow('unknown');
      expect(result.slots).toBe(3);
      expect(result.sendSlots).toBe(2);
      expect(result.profile.track_type_word).toBe(1234);
      expect(fake.prepare).toHaveBeenCalledTimes(1);
      await lifecycle.finishCase({ session });
      expect(fake.evidence).toHaveBeenCalledWith({ session }, artifacts, 'case-1', [], false);
      await lifecycle.afterClose();
      if (image === 'routes') {
        for (const failing of ['runtime-check', 'render-shots']) {
          const observed: string[] = [];
          fake.analysis.mockImplementation(async (_module: string, args: string[]) => {
            observed.push(args[0]!);
            if (args[0] === failing) throw new Error(`${failing} failed`);
            return { pass: true };
          });
          await expect(lifecycle.afterClose()).rejects.toThrow(`${failing} failed`);
          expect(observed).toContain('runtime-check');
          expect(observed).toContain('render-shots');
        }
      }
    } finally {
      vi.unstubAllEnvs();
    }
  });
}

test('route diagnostic text decodes big endian words and panel chords release on failure', async () => {
  const session = {
    client: {
      memPeek: vi.fn(async () => 0x41420000),
      inputButton: vi.fn(),
      inputTap: vi.fn().mockRejectedValue(new Error('panel failure')),
      step: vi.fn(),
    },
  } as unknown as Session;
  expect(await readTextRows(session, 4096, 2, 4)).toEqual(['AB', 'AB']);
  await expect(tap(session, 'YES', 0, 'FUNC')).rejects.toThrow('panel failure');
  expect(session.client.inputButton).toHaveBeenLastCalledWith(expect.any(Number), false);
});

test('route boot drains input while free then returns paused for deterministic panel steps', async () => {
  const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-route-inspect-'));
  directories.push(artifacts);
  const order: string[] = [];
  let mode = 'paused';
  let observations = 0;
  const fields = [0x444b4254, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0];
  const plan: RuntimePlan = {
    base: { device: 'dt2', version: '1.17', syx_sha256: 'a'.repeat(64) },
    modules: ['core/runtime'],
    symbols: { 'core/runtime:DKRT_BOOT': 4096 },
    output: { syx_sha256: 'b'.repeat(64) },
  };
  fake.ready.mockImplementation(async () => {
    order.push('ready');
    expect(mode).toBe('free');
  });
  const session = {
    options: { device: 'dt2', firmware: '1.17' },
    loadResult: { syx_sha256: plan.output!.syx_sha256 },
    preparedRuntimeImage: {
      syx: '/prepared.syx',
      baseSha256: plan.base.syx_sha256,
      syxSha256: plan.output!.syx_sha256,
      modules: plan.modules,
    },
    client: {
      call: vi.fn(async (_method: string, params: { mode: string }) => {
        mode = params.mode;
        order.push(mode);
      }),
      memPeek: vi.fn(async (address: number) => fields[(address - 4096) / 4]),
    },
    status: vi.fn(async () => {
      const pending = observations++ === 0 ? 1 : 0;
      order.push(pending ? 'pending' : 'drained');
      if (pending) expect(mode).toBe('free');
      return { epoch: 3, mode, cf: { input_pending: pending }, scheduled_inputs: pending };
    }),
  } as unknown as Session;
  const dk = { modules: vi.fn(async () => [{ name: 'test/sine' }]), capacity: vi.fn(async () => ({})) };
  const inspected = await inspectRouteBoot(
    { session, dk } as unknown as PatchedSession,
    plan,
    ['test/sine'],
    artifacts,
    'boot',
  );
  expect(order).toEqual(['free', 'ready', 'pending', 'drained', 'paused', 'drained']);
  expect(mode).toBe('paused');
  expect(inspected.epoch).toBe(3);
});
