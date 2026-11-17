// The services a module needs go on the card with it: the NEEDS of a packed .dkm name them, and the install step puts the bound file at services/<name>.dks before the
// selection and the restart.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { afterAll, expect, test, vi } from 'vitest';
import type { DkSession } from '@digi/dk-session';

// hoisted: the mock below (and the modules that call `repoRoot` when they load) need the directory first
const dir = vi.hoisted(() => {
  const nodeFs = require('node:fs') as typeof import('node:fs');
  const nodeOs = require('node:os') as typeof import('node:os');
  const nodePath = require('node:path') as typeof import('node:path');
  return nodeFs.mkdtempSync(nodePath.join(nodeOs.tmpdir(), 'svc-files-'));
});

// the device table reads repository files when it loads; the test needs only its firmware versions
vi.mock('../runtime/devices.ts', () => ({ FIRMWARE: { dt2: '1.17', dn2: '1.12' } }));
vi.mock('../runtime/addrmap.ts', () => ({ readLoadTable: vi.fn() }));
vi.mock('@digi/test-support', async (original) => ({
  ...(await original<object>()),
  resolveInputs: () => ({ syx: '/stock.syx' }),
  repoRoot: () => dir,
}));
import { installModules } from '../runtime/install.ts';
import { boundService, neededServices } from '../runtime/service-files.ts';
import { bindServiceSymbols, serviceMapEntries } from '../runtime/service-symbols.ts';

// the packed service the harness finds under dist/modules/<device>/<leaf>.dks
fs.mkdirSync(path.join(dir, 'dist/modules/dt2'), { recursive: true });
fs.writeFileSync(path.join(dir, 'dist/modules/dt2/fx.dks'), 'packed');
// the file the Nx target `core/services/fx:bind` wrote
fs.mkdirSync(path.join(dir, 'dist/svc-bound/dt2'), { recursive: true });
fs.writeFileSync(path.join(dir, 'dist/svc-bound/dt2/fx.dks'), 'bound');
const FX = { service: 'fx', moduleId: 'core/services/fx' };
const sha = (text: string) => createHash('sha256').update(text).digest('hex');
// the sidecar `patchkit bind` writes: what the bound file was made from (the stock image of this test does not exist, so only the packed file is compared)
const sidecar = path.join(dir, 'dist/svc-bound/dt2/fx.src.json');
fs.writeFileSync(sidecar, JSON.stringify({ unbound_sha256: sha('packed'), firmware_sha256: 'x' }));
afterAll(() => fs.rmSync(dir, { recursive: true, force: true }));

// the pack target writes `<leaf>.services.json` (`patchkit services`, one JSON line per service) beside the file: here gain.dkm is an effect
const write = (name: string) => {
  const file = path.join(dir, name);
  fs.writeFileSync(file, 'packed module');
  fs.writeFileSync(
    file.replace(/\.dkm$/, '.services.json'),
    name === 'gain.dkm' ? '{"service":"fx","module_id":"core/services/fx","store_path":"services/fx.dks"}\n' : '',
  );
  return file;
};

test('the needed services are read from the files that the pack target wrote', () => {
  expect(neededServices([write('gain.dkm'), write('plain.dkm')])).toEqual([FX]);
  expect(neededServices([write('plain.dkm')])).toEqual([]);
  expect(() => neededServices([path.join(dir, 'unpacked.dkm')])).toThrow(
    /missing .*unpacked\.services\.json: run nx run/,
  );
});

test('install puts the bound service on the card before the selection and the restart', async () => {
  const order: string[] = [];
  const sent = new Map<string, string>();
  const dk = {
    put: async (p: string, bytes: Buffer) => void (order.push(`put ${p}`), sent.set(p, bytes.toString())),
    verifyFile: async () => true,
    select: async () => void order.push('select'),
    restart: async () => void order.push('restart'),
    waitForBoot: async () => void order.push('boot'),
    modules: async () => [{ name: 'test/dyn-fx-gain' }],
  } as unknown as DkSession;
  const effect = write('gain.dkm');
  await installModules(dk, [{ id: 'test/dyn-fx-gain', artifact: effect }], { device: 'dt2' });
  expect(order).toEqual(['put modules/dyn-fx-gain.dkm', 'put services/fx.dks', 'select', 'restart', 'boot']);
  expect(sent.get('services/fx.dks'), 'the bound file of the Nx bind target goes on the card').toBe('bound');
  // without a device the module goes alone (a test that installs by hand)
  order.length = 0;
  await installModules(dk, [{ id: 'test/dyn-fx-gain', artifact: effect }]);
  expect(order).toEqual(['put modules/dyn-fx-gain.dkm', 'select', 'restart', 'boot']);
});

test('a bound file made from other bytes than the packed service is refused with the Nx target to run, whatever the mtimes say', async () => {
  const packed = path.join(dir, 'dist/modules/dt2/fx.dks');
  const bound = path.join(dir, 'dist/svc-bound/dt2/fx.dks');
  // a cache restore gives the bound file a new mtime: only the content says it is current
  const now = Date.now() / 1000;
  fs.utimesSync(bound, now + 10, now + 10);
  fs.utimesSync(packed, now, now);
  await expect(boundService('dt2', FX, packed)).resolves.toBe(bound);
  fs.writeFileSync(sidecar, JSON.stringify({ unbound_sha256: sha('an older pack'), firmware_sha256: 'x' }));
  await expect(boundService('dt2', FX, packed)).rejects.toThrow(
    /was bound from another fx\.dks: run nx run core\/services\/fx:bind/,
  );
  fs.rmSync(sidecar);
  await expect(boundService('dt2', FX, packed)).rejects.toThrow(/no \.src\.json sidecar .*: run nx run/);
  fs.writeFileSync(sidecar, JSON.stringify({ unbound_sha256: sha('packed'), firmware_sha256: 'x' }));
});

test('service symbols come from the address map that bind wrote, never from the unbound pack', () => {
  const file = path.join(dir, 'dist/svc-bound/dt2/fx.addrmap.json');
  expect(() => serviceMapEntries('dt2', 'fx')).toThrow(/fx:bind/);
  fs.writeFileSync(
    file,
    JSON.stringify({
      entries: [
        { cpu: 'cf', addr: 124, name: 'DKRT_FXS' },
        { cpu: 'sharc', addr: 1, name: 'x' },
      ],
    }),
  );
  expect([...serviceMapEntries('dt2', 'fx')]).toEqual([['DKRT_FXS', 124]]);
});

test('a boot that should have the service fails with the service and the reason; noServices is explicit', async () => {
  const runtime = { session: { options: { device: 'dt2' } } } as never;
  fs.rmSync(path.join(dir, 'dist/svc-bound/dt2/fx.addrmap.json'), { force: true });
  await expect(bindServiceSymbols(runtime, {} as never, 'fx')).rejects.toThrow(/fx service.*fx:bind/);
  await expect(bindServiceSymbols(runtime, {} as never, 'fx', { expected: false })).resolves.toBeUndefined();
});
