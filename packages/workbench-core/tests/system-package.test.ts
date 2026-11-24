import { DkError, fileHashHex, fmtVersion, type DkEntry, type DkInfoFull, type Quick } from '@digi/dk-session';
import { CAP } from '@digi/dk-session/gen_proto';
import { afterEach, expect, test, vi } from 'vitest';
import { createFirmwareApp } from '../src/firmware/app';
import type { PreparedFlash } from '../src/patch/patchkit-core';
import { activateSystemPackage, type SystemPackage } from '../src/runtime/activate-system-package';
import { SYSTEM_PACKAGE_PATH, systemPackageStore } from '../src/runtime/system-package';
import { fakeDkm } from '../src/target-ops/fake-store';
import { targetOps } from '../src/target-ops/session-store';
import { createFakeSessions, createFakeTransferApp } from '../src/transfer/fake-app';
import { installModules } from '../src/transfer/transfer-core';

const info: DkInfoFull = {
  port: 'test',
  protocol: 1,
  runtime: 0x001f00,
  runtime_version: 0x001f00,
  free_bytes: 4321,
  max_chunk: 128,
  safe_mode: true,
  upload_open: false,
  caps: (1 << CAP.FOLDERS) | (1 << CAP.MODULES) | (1 << CAP.CAPACITY) | (1 << CAP.LOADS),
  card_read_only: false,
  log_sink: false,
  fault_found: false,
  card_sectors: null,
  card_state: null,
  card_guard: null,
  image_id: null,
  svc_abi: null,
  fw_id: null,
};

const pkgAbsent = () => new DkError({ error: 'Package hook unavailable', kind: 'device', status: 'E_PROTO' });
const build = Uint8Array.of(1, 2, 3);
const entry = (path: string, bytes: Uint8Array): DkEntry => ({ path, size: bytes.length, hash: fileHashHex(bytes) });

function setupStore() {
  const local = fakeDkm({ name: 'test/local' });
  const cardBytes = fakeDkm({ name: 'test/card' });
  const card = entry('modules/tools/test-card.dkm', cardBytes);
  const session = {
    info: vi.fn(async () => info),
    rpc: async () => new Uint8Array(),
    list: vi.fn(async (prefix = '') => [card].filter((file) => file.path.startsWith(prefix))),
    capacity: vi.fn(async () => ({ STORE: { limit: 128, used: 8, largest: 120 } })),
    modules: vi.fn(async () => [{ name: 'test/card' }]),
    loadStates: vi.fn(async () => []),
    read: vi.fn(async () => ({ size: card.size, bytes: cardBytes })),
    verify: vi.fn(async () => card.hash),
  };
  const app = createFakeTransferApp({
    sessions: createFakeSessions(() => session),
    env: { library: { scan: async () => [{ path: 'local.dkm', size: local.length }], read: async () => local } },
  });
  return { app, card, cardBytes, session };
}

test('Files keeps its card inventory, usage, READ and VERIFY in a core-only boot', async () => {
  const { app, card, cardBytes, session } = setupStore();
  const { store } = targetOps(app.target);
  const withPackage = await store.snapshot();
  expect(withPackage.coreOnly).toBeUndefined();
  expect(withPackage.usage).toMatchObject({ free: 120 * 1024, total: 128 * 1024 });
  expect(withPackage.modules).toMatchObject([{ name: 'test/card' }]);

  session.capacity.mockRejectedValue(pkgAbsent());
  session.modules.mockClear();
  session.loadStates.mockClear();
  const snapshot = await store.snapshot();
  expect(snapshot).toMatchObject({
    coreOnly: true,
    entries: [card],
    usage: { free: info.free_bytes, total: null, used: [{ key: 'modules', bytes: card.size, files: 1 }] },
    modules: [],
    states: [],
  });
  expect(session.modules).not.toHaveBeenCalled();
  expect(session.loadStates).not.toHaveBeenCalled();
  expect(await store.readAll(card.path)).toEqual(cardBytes);
  expect(await store.verify(card.path)).toBe(card.hash);
});

test('Transfer keeps both lists and clears the loaded inventory when the package is absent', async () => {
  const { app, card, session } = setupStore();
  const showLibrary = vi.fn();
  const showCard = vi.fn();
  const normal = await app.backend.list(info.port, showLibrary, showCard);
  expect(normal.coreOnly).toBeUndefined();
  expect(normal.loaded).toEqual(['test/card']);
  expect(normal.free_bytes).toBe(120 * 1024);
  expect(installModules(normal, new Set(['test/local']))).toEqual(['test/local']);
  expect(app.inventory.modules()).toHaveLength(1);

  session.modules.mockRejectedValue(pkgAbsent());
  session.capacity.mockClear();
  session.loadStates.mockClear();
  const coreOnly = await app.backend.list(info.port, showLibrary, showCard);
  for (const view of [showCard.mock.lastCall?.[0], coreOnly]) {
    expect(view).toMatchObject({
      coreOnly: true,
      free_bytes: info.free_bytes,
      capacity: null,
      loaded: [],
      modules: [{ id: 'test/local' }],
      card_only: [{ path: card.path, loaded: false }],
    });
  }
  expect(app.inventory.modules()).toEqual([]);
  expect(installModules(coreOnly, new Set(['test/local']))).toEqual([]);
  expect(session.capacity).not.toHaveBeenCalled();
  expect(session.loadStates).not.toHaveBeenCalled();
});

test('Files skips package reads on runtimes without the capabilities', async () => {
  const { app, session } = setupStore();
  session.info.mockResolvedValue({ ...info, caps: null });
  const snapshot = await targetOps(app.target).store.snapshot();
  expect(snapshot.coreOnly).toBeUndefined();
  expect(snapshot.usage).toMatchObject({ free: info.free_bytes, total: null });
  expect(session.capacity).not.toHaveBeenCalled();
  expect(session.modules).not.toHaveBeenCalled();
  expect(session.loadStates).not.toHaveBeenCalled();
});

test('Files and Transfer propagate unrelated failures from their protocol reads', async () => {
  const { app, session } = setupStore();
  const { store } = targetOps(app.target);
  const reason = pkgAbsent();
  session.info.mockResolvedValue({ ...info, safe_mode: false });
  session.capacity.mockRejectedValue(reason);
  await expect(store.snapshot()).rejects.toBe(reason);
  await expect(app.backend.list(info.port, () => {})).rejects.toBe(reason);

  session.info.mockResolvedValue(info);
  const timeout = new DkError({ error: 'Missing answer', kind: 'timeout' });
  session.capacity.mockRejectedValue(timeout);
  await expect(store.snapshot()).rejects.toBe(timeout);
  session.list.mockRejectedValue(reason);
  await expect(app.backend.list(info.port, () => {})).rejects.toBe(reason);
});

function setupWrite(files: Map<string, Uint8Array> = new Map()) {
  const session = {
    list: vi.fn(async (prefix = '') =>
      [...files].filter(([path]) => path.startsWith(prefix)).map(([path, bytes]) => entry(path, bytes)),
    ),
    verify: vi.fn(async (path: string) => {
      const have = files.get(path);
      if (!have) throw new Error('File absent');
      return fileHashHex(have);
    }),
    put: vi.fn(async (path: string, bytes: Uint8Array) => void files.set(path, bytes.slice())),
  };
  return { session, check: vi.fn() };
}

test('a same-size lookalike path is ignored, and an installed package is verified on a second run', async () => {
  const { session, check } = setupWrite(new Map([[`${SYSTEM_PACKAGE_PATH}.bak`, build]]));
  const card = systemPackageStore(session, build, { info, check });
  expect(await card.verifiedOnCard()).toBe(false);
  expect(session.verify).not.toHaveBeenCalled();
  await card.put();
  await card.verify();
  expect(await card.verifiedOnCard()).toBe(true);
  expect(session.put).toHaveBeenCalledOnce();
  expect(session.verify).toHaveBeenCalledTimes(2);
});

test('the fresh VERIFY wins over a matching LIST hash when the bytes have changed', async () => {
  const { session, check } = setupWrite(new Map([[SYSTEM_PACKAGE_PATH, Uint8Array.of(3, 2, 1)]]));
  session.list.mockResolvedValueOnce([entry(SYSTEM_PACKAGE_PATH, build)]);
  const card = systemPackageStore(session, build, { info, check });
  expect(await card.verifiedOnCard()).toBe(false);
  expect(session.put).not.toHaveBeenCalled();
  expect(session.verify).toHaveBeenCalledOnce();
});

test.each(['E_READ', 'E_NOFILE'])('an existing package that returns %s can be replaced', async (status) => {
  const { session, check, pkg } = setupActivation({ have: true });
  session.verify.mockRejectedValueOnce(
    new DkError({ error: 'Cannot verify existing package', kind: 'device', status }),
  );
  await expect(activateSystemPackage(session, pkg, { check })).resolves.toBe(true);
  expect(session.put).toHaveBeenCalledOnce();
  expect(session.verify).toHaveBeenCalledTimes(2);
  expect(session.verify).toHaveBeenCalledBefore(session.restart);
});

test('wrong size skips the existing VERIFY and a failed fresh VERIFY cannot report a ready package', async () => {
  const { session, check } = setupWrite(new Map([[SYSTEM_PACKAGE_PATH, Uint8Array.of(1, 2, 3, 4)]]));
  session.verify.mockResolvedValue(fileHashHex(Uint8Array.of(1, 2, 3, 4)));
  const card = systemPackageStore(session, build, { info, check });
  expect(await card.verifiedOnCard()).toBe(false);
  expect(session.verify).not.toHaveBeenCalled();
  await card.put();
  await expect(card.verify()).rejects.toThrow(/Verification failed/);
  expect(session.put).toHaveBeenCalledOnce();
  expect(session.verify).toHaveBeenCalledOnce();
});

test('a read-only card cannot be written to or listed by the package writer', async () => {
  const { session, check } = setupWrite();
  const card = systemPackageStore(session, build, { info: { ...info, card_read_only: true }, check });
  await expect(card.verifiedOnCard()).rejects.toThrow(/read-only/);
  expect(session.list).not.toHaveBeenCalled();
  expect(session.put).not.toHaveBeenCalled();
});

test('the package writer cannot upload or return a verified result after the target changes', async () => {
  const { session, check } = setupWrite(new Map([[SYSTEM_PACKAGE_PATH, build]]));
  const card = systemPackageStore(session, build, { info, check });
  check
    .mockImplementationOnce(() => {})
    .mockImplementationOnce(() => {
      throw new Error('The target changed.');
    });
  await expect(card.verifiedOnCard()).rejects.toThrow(/target changed/);
  expect(session.verify).not.toHaveBeenCalled();
  expect(session.put).not.toHaveBeenCalled();

  check.mockImplementation(() => {
    if (session.verify.mock.calls.length > 0) throw new Error('The target changed.');
  });
  await expect(card.verifiedOnCard()).rejects.toThrow(/target changed/);
  expect(session.verify).toHaveBeenCalledOnce();
  expect(session.put).not.toHaveBeenCalled();
});

function setupActivation({
  ready = false,
  have = false,
  actions = ['put', 'verify', 'restart', 'confirm_support'],
}: {
  ready?: boolean;
  have?: boolean;
  actions?: Awaited<ReturnType<SystemPackage['plan']>>;
} = {}) {
  const { session: store, check } = setupWrite(have ? new Map([[SYSTEM_PACKAGE_PATH, build]]) : undefined);
  let now = 0;
  const session = {
    ...store,
    clock: { now: async () => now, pause: async (ms: number) => void (now += ms) },
    probe: <T>(run: (quick: Quick) => Promise<T>, quick: Quick = true) => run(quick),
    info: vi.fn(async () => ({ ...info, safe_mode: !ready })),
    modules: vi.fn(async () => {
      if (!ready) throw pkgAbsent();
      return [];
    }),
    retry: vi.fn(async () => {}),
    restart: vi.fn(async () => void (ready = true)),
    waitForBoot: vi.fn(async () => {}),
  };
  return {
    session,
    check,
    pkg: { runtime: info.runtime, bytes: build, plan: vi.fn<SystemPackage['plan']>().mockResolvedValue(actions) },
  };
}

test('core-only activation verifies before restarting and waits for module support to return', async () => {
  const { session, check, pkg } = setupActivation();
  session.info.mockResolvedValue(info);
  session.modules.mockRejectedValueOnce(pkgAbsent()).mockRejectedValueOnce(pkgAbsent());
  expect(await activateSystemPackage(session, pkg, { check })).toBe(true);
  expect(session.verify).toHaveBeenCalledBefore(session.restart);
  expect(pkg.plan).toHaveBeenCalledExactlyOnceWith({
    repair: false,
    moduleSupport: false,
    verifiedPackageOnCard: false,
  });
  expect(session.modules).toHaveBeenCalledTimes(3);
  expect(session.waitForBoot).not.toHaveBeenCalled();
  expect(session.retry).not.toHaveBeenCalled();
});

test('an identical healthy package is left alone; an identical core-only package needs explicit repair', async () => {
  const healthy = setupActivation({ ready: true, have: true, actions: [] });
  expect(await activateSystemPackage(healthy.session, healthy.pkg, { check: healthy.check })).toBe(false);
  expect(healthy.session.restart).not.toHaveBeenCalled();
  expect(healthy.session.put).not.toHaveBeenCalled();

  healthy.session.list.mockClear();
  healthy.pkg.plan.mockRejectedValueOnce(new Error('The target already has module support.'));
  await expect(
    activateSystemPackage(healthy.session, healthy.pkg, { check: healthy.check, repair: true }),
  ).rejects.toThrow(/already has module support/);
  expect(healthy.pkg.plan).toHaveBeenLastCalledWith({
    repair: true,
    moduleSupport: true,
    verifiedPackageOnCard: false,
  });
  expect(healthy.session.list).not.toHaveBeenCalled();

  const { session, check, pkg } = setupActivation({ have: true, actions: ['retry', 'restart', 'confirm_support'] });
  pkg.plan.mockRejectedValueOnce(new Error('Choose Transfer → Modules → Install system package.'));
  await expect(activateSystemPackage(session, pkg, { check })).rejects.toThrow(/Install system package/);
  expect(session.restart).not.toHaveBeenCalled();
  expect(session.retry).not.toHaveBeenCalled();
  await expect(activateSystemPackage(session, pkg, { check, repair: true })).resolves.toBe(true);
  expect(session.retry).toHaveBeenCalledBefore(session.restart);
  expect(session.put).not.toHaveBeenCalled();
});

test('a healthy target with changed bytes waits for boot before confirming module support', async () => {
  const { session, check, pkg } = setupActivation({
    ready: true,
    actions: ['put', 'verify', 'restart', 'wait_for_boot', 'confirm_support'],
  });
  await expect(activateSystemPackage(session, pkg, { check })).resolves.toBe(true);
  expect(session.verify).toHaveBeenCalledBefore(session.restart);
  expect(session.restart).toHaveBeenCalledBefore(session.waitForBoot);
  expect(session.waitForBoot).toHaveBeenCalledWith(90_000, expect.objectContaining({ requireBootEvidence: true }));
  expect(session.modules.mock.invocationCallOrder[1]).toBeGreaterThan(session.waitForBoot.mock.invocationCallOrder[0]!);
});

test('post-restart confirmation survives a native MIDI IPC rejection while the port reopens', async () => {
  const { session, check, pkg } = setupActivation();
  session.info.mockResolvedValueOnce(info).mockResolvedValueOnce(info).mockResolvedValueOnce(info);
  session.info.mockRejectedValueOnce('MIDI port closed');
  await expect(activateSystemPackage(session, pkg, { check })).resolves.toBe(true);
  expect(await session.clock.now()).toBeGreaterThan(0);
  expect(session.modules).toHaveBeenCalledTimes(2);
});

test('a replacement without confirmed boot evidence reports how to recover', async () => {
  const { session, check, pkg } = setupActivation({
    ready: true,
    actions: ['put', 'verify', 'restart', 'wait_for_boot', 'confirm_support'],
  });
  session.waitForBoot.mockRejectedValueOnce(new DkError({ error: 'no new boot', kind: 'timeout' }));
  await expect(activateSystemPackage(session, pkg, { check })).rejects.toThrow(/Restart the device manually/);
  expect(session.modules).toHaveBeenCalledOnce();
});

test.each([
  [{ upload_open: true }, /upload is in progress/],
  [{ card_read_only: true }, /read-only/],
  [{ image_id: 'changed' }, /firmware changed/],
] as const)('fresh INFO with %j refuses PUT before changing the card', async (flag, message) => {
  const { session, check, pkg } = setupActivation();
  session.info.mockResolvedValueOnce(info).mockResolvedValueOnce({ ...info, ...flag });
  await expect(activateSystemPackage(session, pkg, { check })).rejects.toThrow(message);
  expect(session.list).toHaveBeenCalledOnce();
  expect(session.put).not.toHaveBeenCalled();
  expect(session.restart).not.toHaveBeenCalled();
});

test.each(['retry', 'restart'] as const)('a same-port reconnect before %s refuses the command', async (action) => {
  const { session, check, pkg } = setupActivation({ have: true, actions: ['retry', 'restart', 'confirm_support'] });
  session.info.mockResolvedValue(info);
  const reconnect = () => session.info.mockResolvedValue({ ...info, image_id: 'changed' });
  if (action === 'retry') {
    session.verify.mockImplementationOnce(async () => {
      reconnect();
      return fileHashHex(build);
    });
  } else {
    session.retry.mockImplementationOnce(async () => {
      reconnect();
    });
  }
  await expect(activateSystemPackage(session, pkg, { check, repair: true })).rejects.toThrow(/firmware changed/);
  expect(session[action]).not.toHaveBeenCalled();
  expect(session.restart).not.toHaveBeenCalled();
});

test('a different image or runtime cannot receive the desktop package', async () => {
  const { session, check, pkg } = setupActivation();
  await expect(
    activateSystemPackage(session, pkg, { check, identity: { device: info.port, imageId: 'ab' } }),
  ).rejects.toThrow(/firmware changed/);
  await expect(activateSystemPackage(session, { ...pkg, runtime: pkg.runtime - 1 }, { check })).rejects.toThrow(
    /requires runtime/,
  );
  expect(session.list).not.toHaveBeenCalled();
  expect(session.put).not.toHaveBeenCalled();
});

function setupFlash() {
  const { session, pkg } = setupActivation();
  const imageId = '1234567890abcdef';
  session.info.mockImplementation(async () => ({ ...info, safe_mode: true, image_id: imageId }));
  const send = vi.fn();
  const transport = { send, recv: async () => null };
  const release = vi.fn();
  const sessions = createFakeSessions(() => ({
    ...session,
    rpc: async () => new TextEncoder().encode('1.12'),
    reserveTransport: async () => release,
  }));
  let polls = 0;
  const app = createFakeTransferApp({
    sessions,
    env: {
      targets: {
        emulator: true,
        open: async () => transport,
        ports: async () => (polls++ < 2 ? [] : ['MIDI']),
      },
    },
  });
  app.target.startOn({ kind: 'midi', port: 'MIDI' });
  const refresh = vi.spyOn(app.backend, 'refreshServices').mockResolvedValue([]);
  const install = vi.fn(async () => []);
  let next = 0;
  const packageSource = vi.fn(async () => pkg);
  const fw = createFirmwareApp(
    {
      systemPackage: packageSource,
      prepare: async () => {
        throw new Error('not used');
      },
      sender: async () => ({
        next: () => (next++ === 0 ? { kind: 'send', index: 0 } : { kind: 'done' }),
        sent: () => next,
        fail: () => {},
        free: () => {},
      }),
      rpc: async () => {
        throw new Error('not used');
      },
      saveImage: async () => undefined,
      log: () => {},
      defaultModules: { entries: () => [{ id: 'test/module', summary: '' }], install },
    },
    app,
  );
  const p: PreparedFlash = {
    device: info.port,
    imageId,
    os: '1.12',
    runtime: fmtVersion(pkg.runtime),
    restore: false,
    ok: true,
    shaStock: '',
    shaImage: '',
    checks: [],
    paceMs: 2,
    ends: [1],
    image: Uint8Array.of(1),
  };
  fw.state.setRoute('menu');
  fw.state.setPrepared({ kind: 'ready', name: 'stock.syx', size: 1, p });
  return { fw, session, send, refresh, install, packageSource, release, imageId };
}

afterEach(() => vi.useRealTimers());

test('the desktop flash snapshots the package before upload and activates it after releasing raw MIDI, before services and defaults', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] });
  const { fw, session, send, refresh, install, packageSource, release } = setupFlash();
  const run = fw.state.startFlash();
  await vi.runAllTimersAsync();
  await run;
  expect(fw.state.run()).toMatchObject({
    phase: 'done',
    lines: expect.arrayContaining(['System package verified and active.']),
  });
  expect(packageSource).toHaveBeenCalledBefore(send);
  expect(release).toHaveBeenCalledBefore(session.put);
  expect(session.restart).toHaveBeenCalledBefore(refresh);
  expect(refresh).toHaveBeenCalledBefore(install);
});

test('a different INFO image after a successful flash leaves the flash done and skips package and module writes', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] });
  const { fw, session, refresh, install, imageId } = setupFlash();
  session.info
    .mockResolvedValueOnce({ ...info, image_id: imageId })
    .mockResolvedValue({ ...info, image_id: 'different' });
  const run = fw.state.startFlash();
  await vi.runAllTimersAsync();
  await run;
  expect(fw.state.run()).toMatchObject({
    phase: 'done',
    lines: expect.arrayContaining([expect.stringMatching(/System package setup failed.*target firmware changed/i)]),
  });
  expect(session.put).not.toHaveBeenCalled();
  expect(refresh).not.toHaveBeenCalled();
  expect(install).not.toHaveBeenCalled();
});

test('a desktop flash with no build artifact stops before sending', async () => {
  const { fw, send, packageSource } = setupFlash();
  packageSource.mockRejectedValue(new Error('System package missing.'));
  await fw.state.startFlash();
  expect(fw.state.run()).toMatchObject({ phase: 'failed', message: 'System package missing.' });
  expect(send).not.toHaveBeenCalled();
});
