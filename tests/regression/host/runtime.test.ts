import fs from 'node:fs';
import path from 'node:path';
import { beforeEach, expect, test, vi } from 'vitest';

const fake = vi.hoisted(() => {
  const client = {
    eventsSubscribe: vi.fn(),
    cardLoad: vi.fn(),
    firmwareLoad: vi.fn(),
    watchdogConfig: vi.fn(),
    status: vi.fn(),
    drainEvents: vi.fn(),
    guardSet: vi.fn(),
    call: vi.fn(),
  };
  return { client, close: vi.fn(), spawn: vi.fn(), coupledEnv: vi.fn() };
});
vi.mock('@digi/control-client', async (original) => ({
  ...(await original<object>()),
  spawnDaemon: fake.spawn,
}));
vi.mock('../../../packages/test-support/src/inputs.ts', () => ({
  validateRuntimeInputs: vi.fn(),
  coupledEnv: fake.coupledEnv,
  resolveInputs: () => ({ syx: '/stock.syx', cfSnapshot: '/ready.snap', dspImage: '/dsp.bin', dspState: '/dsp.snap' }),
}));
import { Session } from '@digi/test-support';
import { startPatchedSession } from '../support.ts';

const identity = {
  patched: true as const,
  set: 'emulator',
  cache_hit: false,
  modules: ['digikit'],
  base_sha256: 'a'.repeat(64),
  cache_file: '/cache.syx',
  seconds: 0.1,
};
beforeEach(() => {
  vi.resetAllMocks();
  fake.coupledEnv.mockReturnValue({ SHARC_FAST_REGIONS: 'fixture-regions' });
  fake.spawn.mockResolvedValue({ client: fake.client, close: fake.close });
  fake.client.cardLoad.mockResolvedValue({ card: '/card.img' });
  fake.client.firmwareLoad.mockResolvedValue({ runtime: identity, start_used: 'snapshot' });
  fake.client.call.mockResolvedValue({ hex: '' });
  fake.client.status.mockResolvedValue({ epoch: 2, mode: 'free', cf: { ready: true } });
  fake.client.drainEvents.mockReturnValue([]);
});

test('stock defaults retain snapshot load and stepped lifecycle', async () => {
  const session = await Session.start({ device: 'dt2', firmware: '1.17', dsp: 'zero' });
  try {
    expect(fake.client.firmwareLoad).toHaveBeenCalledWith({
      syx: '/stock.syx',
      start: 'snapshot',
      force: true,
      auto_no: true,
    });
    expect(fake.client.call).not.toHaveBeenCalled();
    expect(fake.coupledEnv).not.toHaveBeenCalled();
    expect(fake.spawn.mock.calls[0]?.[0].env).toBeUndefined();
    expect(fake.spawn.mock.calls[0]?.[0].args).toEqual(['--dsp', 'zero', '--cf-snapshot', '/ready.snap']);
  } finally {
    await session.close();
  }
});

test('zero-DSP runtime startup selects the firmware before applying the runtime patch', async () => {
  const session = await Session.start({ device: 'dt2', firmware: '1.17', dsp: 'zero', runtime: {} });
  try {
    expect(fake.spawn.mock.calls[0]?.[0].args).toEqual(['/stock.syx', '--dsp', 'zero', '--cf-snapshot', '/ready.snap']);
    expect(fake.client.firmwareLoad).toHaveBeenCalledWith({
      syx: '/stock.syx',
      start: 'auto',
      force: true,
      auto_no: true,
      runtime: true,
      patch_set: undefined,
    });
    expect(fake.client.call).toHaveBeenCalledWith('run.mode', { mode: 'free' });
  } finally {
    await session.close();
  }
});

test('a mirrored runtime owns its mirror inside the temporary run directory', async () => {
  const runtime = await startPatchedSession({ device: 'dn2', firmware: '1.12', mirror: true, autoNo: false });
  const runDir = runtime.session.runDir;
  try {
    expect(fs.existsSync(runDir)).toBe(true);
    expect(fake.coupledEnv).toHaveBeenCalledWith('/stock.syx');
    expect(fake.spawn.mock.calls[0]?.[0].env).toEqual({ SHARC_FAST_REGIONS: 'fixture-regions' });
    expect(fake.client.firmwareLoad).toHaveBeenCalledWith(expect.objectContaining({ auto_no: false }));
    expect(fake.spawn.mock.calls[0]?.[0].args.slice(-2)).toEqual(['--mirror-dir', `${runDir}/mirror`]);
    fs.mkdirSync(`${runDir}/mirror/digikit/modules`, { recursive: true });
    fs.writeFileSync(`${runDir}/mirror/digikit/modules/pluck.dkm`, 'owned test data');
  } finally {
    await runtime.close();
  }
  expect(fs.existsSync(runDir), 'close removes the owned mirror along with its run directory').toBe(false);
});

test('patched fixture loads the card, requests runtime auto boot and free mode, and wires USB RPC', async () => {
  const runtime = await startPatchedSession({
    device: 'dt2',
    firmware: '1.17',
    card: '/samples',
    patchSet: 'emulator',
  });
  try {
    expect(fake.client.firmwareLoad).toHaveBeenCalledWith({
      syx: '/stock.syx',
      start: 'auto',
      force: true,
      auto_no: true,
      runtime: true,
      patch_set: 'emulator',
      card: '/card.img',
    });
    expect(fake.spawn.mock.calls[0]?.[0].args).toEqual([
      '/stock.syx',
      '--auto-coupled',
      '--audio-profile-syx',
      '/stock.syx',
      '--dsp-image',
      '/dsp.bin',
      '--dsp-state',
      '/dsp.snap',
      '--cf-snapshot',
      '/ready.snap',
      '--no-audio',
    ]);
    expect(fake.client.call).toHaveBeenCalledWith('run.mode', { mode: 'free' });
    expect(runtime.session.loadResult.runtime).toEqual(identity);
    await expect(runtime.session.restart()).rejects.toThrow('fresh daemon');
    await expect(runtime.session.wait(1)).rejects.toThrow('run freely');
    const pending = runtime.dk.request(1);
    const rejected = expect(pending).rejects.toMatchObject({ kind: 'cancelled' });
    await vi.waitFor(() => expect(fake.client.call).toHaveBeenCalledWith('midi.usb.out', undefined));
    expect(fake.client.call.mock.calls.some(([method]) => method === 'midi.usb.in')).toBe(true);
    await runtime.close();
    await rejected;
    await runtime.close();
    expect(fake.close).toHaveBeenCalledTimes(1);
  } finally {
    await runtime.close();
  }
});

for (const failure of ['firmwareLoad', 'watchdogConfig', 'call'] as const) {
  test(`startup ${failure} failure closes its daemon`, async () => {
    fake.client[failure].mockRejectedValueOnce(new Error('startup failed'));
    await expect(startPatchedSession({ device: 'dn2', firmware: '1.12' })).rejects.toThrow('startup failed');
    expect(fake.close).toHaveBeenCalledTimes(1);
  });
}

test('daemon spawn failure removes the owned temporary run directory', async () => {
  fake.spawn.mockRejectedValueOnce(new Error('spawn failed'));
  await expect(startPatchedSession({ device: 'dn2', firmware: '1.12' })).rejects.toThrow('spawn failed');
  const runDir = fake.spawn.mock.calls[0]?.[0].runDir;
  expect(runDir).toBeTruthy();
  expect(fs.existsSync(runDir)).toBe(false);
});

test('runtime startup deadline closes a hung load before rejecting', async () => {
  vi.useFakeTimers();
  let finishClose!: () => void;
  fake.close.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        finishClose = resolve;
      }),
  );
  fake.client.firmwareLoad.mockImplementation(() => new Promise(() => {}));
  try {
    const started = startPatchedSession({ device: 'dn2', firmware: '1.12' });
    let rejected = false;
    const result = expect(started)
      .rejects.toThrow('runtime startup timed out')
      .then(() => {
        rejected = true;
      });
    await vi.advanceTimersByTimeAsync(120_000);
    expect(fake.close).toHaveBeenCalledTimes(1);
    expect(rejected).toBe(false);
    expect(fake.spawn.mock.calls[0]?.[0].signal.aborted).toBe(true);
    finishClose();
    await result;
    const runDir = fake.spawn.mock.calls[0]?.[0].runDir;
    expect(fs.existsSync(runDir)).toBe(false);
  } finally {
    vi.useRealTimers();
  }
});

test('runtime startup expiry closes a late spawn handle before rejecting', async () => {
  vi.useFakeTimers();
  let spawned!: (daemon: object) => void;
  fake.spawn.mockImplementation(
    () =>
      new Promise((resolve) => {
        spawned = resolve;
      }),
  );
  try {
    const started = startPatchedSession({ device: 'dn2', firmware: '1.12' });
    const result = expect(started).rejects.toThrow('runtime startup timed out');
    await vi.advanceTimersByTimeAsync(120_000);
    spawned({ client: fake.client, close: fake.close });
    await result;
    expect(fake.close).toHaveBeenCalledTimes(1);
    expect(fake.client.eventsSubscribe).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});

test('concurrent fixture and Session closes await the same daemon cleanup', async () => {
  let finishClose!: () => void;
  fake.close.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        finishClose = resolve;
      }),
  );
  const runtime = await startPatchedSession({ device: 'dn2', firmware: '1.12' });
  const first = runtime.close();
  const second = runtime.close();
  expect(second).toBe(first);
  const sessionClose = runtime.session.close();
  expect(runtime.session.close()).toBe(sessionClose);
  expect(fake.close).toHaveBeenCalledTimes(1);
  let closed = false;
  void second.then(() => {
    closed = true;
  });
  await Promise.resolve();
  expect(closed).toBe(false);
  finishClose();
  await Promise.all([first, second, sessionClose]);
  expect(closed).toBe(true);
});

async function reusableSession() {
  const session = await Session.start({ device: 'dn2', firmware: '1.12', dsp: 'sharc', runtime: { reuse: true } });
  const directory = path.join(session.runDir, 'pristine');
  fs.mkdirSync(directory);
  const cardImage = path.join(directory, 'card.img');
  fs.writeFileSync(cardImage, 'host fixture card');
  const baseline = { cardImage, guard: { protect: [{ lo: 16, hi: 32, label: 'probe' }], allow_pc: [] } };
  return { session, baseline };
}

test('reusable runtime rejects mirroring before starting a daemon', async () => {
  await expect(
    Session.start({ device: 'dn2', firmware: '1.12', dsp: 'sharc', mirror: true, runtime: { reuse: true } }),
  ).rejects.toThrow('cannot mirror');
  expect(fake.spawn).not.toHaveBeenCalled();
});

test('baseline reload replaces Session identity/card/events and reapplies actor and guard policy', async () => {
  const { session, baseline } = await reusableSession();
  try {
    fake.client.drainEvents.mockReturnValueOnce([{ kind: 'fault', message: 'old case' }]);
    expect(await session.faults()).toHaveLength(1);
    const loaded = { runtime: { ...identity, cache_file: '/reloaded.syx' }, start_used: 'cold' };
    fake.client.firmwareLoad.mockResolvedValueOnce(loaded);
    fake.client.status
      .mockResolvedValueOnce({ epoch: 2, job: null })
      .mockResolvedValueOnce({ epoch: 3, mode: 'free', cf: { ready: true } });
    expect(await session.reloadRuntimeBaseline(baseline)).toBe(3);
    expect(session.cardPath).toBe(fs.realpathSync(baseline.cardImage));
    expect(session.loadResult).toBe(loaded);
    expect(await session.faults()).toEqual([]);
    expect(fake.client.firmwareLoad).toHaveBeenLastCalledWith(
      expect.objectContaining({ card: fs.realpathSync(baseline.cardImage), replace: true, runtime: true }),
    );
    expect(fake.client.call).toHaveBeenCalledWith('cpu.exceptions', { deliver: false });
    expect(fake.client.guardSet).toHaveBeenLastCalledWith(baseline.guard);
    expect(fake.spawn).toHaveBeenCalledTimes(1);
  } finally {
    await session.close();
  }
});

for (const invalid of ['overlay', 'outside', 'symlink', 'same-epoch', 'busy', 'load-error'] as const) {
  test(`unusable ${invalid} baseline discards only the owned daemon and invalidates Session metadata`, async () => {
    const { session, baseline } = await reusableSession();
    if (invalid === 'overlay')
      fs.writeFileSync(path.join(path.dirname(baseline.cardImage), 'overlay.bin'), 'old writes');
    if (invalid === 'outside') baseline.cardImage = '/private/tmp/card.img';
    if (invalid === 'symlink') {
      fs.unlinkSync(baseline.cardImage);
      fs.symlinkSync('/etc/hosts', baseline.cardImage);
    }
    if (invalid === 'busy') fake.client.firmwareLoad.mockRejectedValueOnce(new Error('patch job busy'));
    if (invalid === 'load-error') fake.client.firmwareLoad.mockRejectedValueOnce(new Error('load failed'));
    await expect(session.reloadRuntimeBaseline(baseline)).rejects.toThrow();
    expect(fake.close).toHaveBeenCalledTimes(1);
    expect(() => session.loadResult).toThrow('not loaded');
    expect(session.cardPath).toBeUndefined();
    await expect(session.reloadRuntimeBaseline(baseline)).rejects.toThrow('closed');
    expect(fs.existsSync(path.join(session.runDir, 'runtime-baseline-error.json'))).toBe(true);
    fs.rmSync(session.runDir, { recursive: true, force: true });
  });
}

for (const stalled of ['status', 'firmwareLoad'] as const) {
  test(`unanswered baseline ${stalled} is bounded and closes its owned daemon`, async () => {
    const { session, baseline } = await reusableSession();
    fake.client[stalled].mockImplementationOnce(() => new Promise(() => {}));
    const started = Date.now();
    await expect(session.reloadRuntimeBaseline({ ...baseline, timeoutMs: 30 })).rejects.toThrow('deadline');
    expect(Date.now() - started).toBeLessThan(500);
    expect(fake.close).toHaveBeenCalledTimes(1);
    expect(fs.existsSync(path.join(session.runDir, 'runtime-baseline-error.json'))).toBe(true);
    fs.rmSync(session.runDir, { recursive: true, force: true });
  });
}
