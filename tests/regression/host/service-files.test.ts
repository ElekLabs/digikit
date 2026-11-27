// The services a module needs go on the card with it: the NEEDS of a packed .dkm name them, and the install step puts the packed file at services/<name>.dks (and the port file at ports/) before the
// selection and the restart, with the port file of the firmware.
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
  repoRoot: () => dir,
}));
import { installModules } from '../runtime/module-install.ts';
import { derivedModule, neededServices, portFile, rawModule } from '../runtime/service-files.ts';
import { bindServiceSymbols, serviceMapEntries } from '../runtime/service-symbols.ts';

// the packed service the harness finds under dist/modules/<device>/<leaf>.dks: a whole header (the DSP size word is read to see whether the card needs a coupled
// session), a zero DSP part
const PACKED = Buffer.concat([Buffer.from('packed'), Buffer.alloc(0xa0)]);
fs.mkdirSync(path.join(dir, 'dist/modules/dt2'), { recursive: true });
fs.writeFileSync(path.join(dir, 'dist/modules/dt2/fx.dks'), PACKED);
// the port file `patchkit:ports` wrote
fs.mkdirSync(path.join(dir, 'dist/ports'), { recursive: true });
fs.writeFileSync(path.join(dir, 'dist/ports/dt2-1.17.dkport'), 'port file');
const FX = { service: 'fx', moduleId: 'core/services/fx' };
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

test('a hand-built module file declares that it needs no service', () => {
  expect(neededServices([rawModule(path.join(dir, 'blob.dkm'))])).toEqual([]);
});

test('a copy of a packed module needs what the original needs', () => {
  const copy = derivedModule(write('gain.dkm'), path.join(dir, 'gain-copy.dkm'));
  expect(neededServices([copy])).toEqual([FX]);
  expect(() => derivedModule(path.join(dir, 'unpacked.dkm'), path.join(dir, 'x.dkm'))).toThrow(
    /missing .*unpacked\.services\.json/,
  );
});

test('install puts the port file and the packed service on the card before the selection and the restart', async () => {
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
  expect(order).toEqual([
    'put modules/tool/test-dyn-fx-gain.dkm',
    'put ports/dt2-1.17.dkport',
    'put services/fx.dks',
    'select',
    'restart',
    'boot',
  ]);
  expect(sent.get('services/fx.dks'), 'the packed file of the Nx pack target goes on the card').toBe(PACKED.toString());
  expect(sent.get('ports/dt2-1.17.dkport'), 'the port file of the Nx ports target goes on the card').toBe('port file');
  expect(portFile('dt2').name).toBe('ports/dt2-1.17.dkport');
  // without a device the module goes alone (a test that installs by hand)
  order.length = 0;
  await installModules(dk, [{ id: 'test/dyn-fx-gain', artifact: effect }]);
  expect(order).toEqual(['put modules/tool/test-dyn-fx-gain.dkm', 'select', 'restart', 'boot']);
});

test('a missing port file is an error naming the Nx target', () => {
  fs.rmSync(path.join(dir, 'dist/ports/dt2-1.17.dkport'));
  expect(() => portFile('dt2')).toThrow(/missing .*dt2-1\.17\.dkport: run nx run patchkit:ports/);
  fs.writeFileSync(path.join(dir, 'dist/ports/dt2-1.17.dkport'), 'port file');
});

test('service symbols come from the address map of the pack: the file loads as packed', () => {
  const file = path.join(dir, 'dist/modules/dt2/fx.addrmap.json');
  expect(() => serviceMapEntries('dt2', 'fx')).toThrow(/fx:pack/);
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
  fs.rmSync(path.join(dir, 'dist/modules/dt2/fx.addrmap.json'), { force: true });
  await expect(bindServiceSymbols(runtime, {} as never, 'fx')).rejects.toThrow(/fx service.*fx:pack/);
  await expect(bindServiceSymbols(runtime, {} as never, 'fx', { expected: false })).resolves.toBeUndefined();
});
