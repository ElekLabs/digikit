// Service files as dependencies (src/modules/services.ts): the NEEDS and PROVIDES readers, the service set of an install and the delete plan.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { HEADER_BYTES, TABLE_MAGIC } from '@digi/dk-session';
import {
  PROVIDER_MAX,
  nameHash,
  portFileOf,
  planDelete,
  readNeeds,
  readProvides,
  resolveServices,
  serviceFileOf,
  type Need,
  type PortFile,
  type RuntimeIdentity,
  type ServiceFile,
} from '../src/modules/services.ts';

const v = (major: number, minor: number) => (major << 16) | (minor << 8);
const need = (name: string, min = 0, hard = true): Need => ({ hash32: nameHash(name), minVersion: min, hard });

/** A file with a header, the role word and a header table of the given version behind it. */
function build(opts: {
  version?: number;
  role?: number;
  needs?: Need[];
  provides?: [number, number];
  magic?: number;
  abi?: number;
  builtFor?: number;
  /** The FW item (8 bytes) of a port file. */
  fw?: number[];
}): Uint8Array {
  const entries: number[][] = [];
  const u32 = (n: number) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
  const u16 = (n: number) => [(n >>> 8) & 255, n & 255];
  if (opts.needs)
    entries.push([
      ...u16(1),
      ...u16(opts.needs.length),
      ...u32(12 * opts.needs.length),
      ...opts.needs.flatMap((n) => [...u32(n.hash32), ...u32(n.minVersion), ...u32(n.hard ? 1 : 0)]),
    ]);
  if (opts.provides)
    entries.push([...u16(9), ...u16(1), ...u32(8), ...u32(opts.provides[0]), ...u32(opts.provides[1])]);
  if (opts.fw) entries.push([...u16(11), ...u16(1), ...u32(8), ...opts.fw]);
  const table = [
    ...u32(opts.magic ?? TABLE_MAGIC),
    ...u16(opts.version ?? 2),
    ...u16(entries.length),
    ...entries.flat(),
  ];
  const f = new Uint8Array(HEADER_BYTES + table.length);
  const d = new DataView(f.buffer);
  d.setUint32(0x30, opts.builtFor ?? 0);
  d.setUint32(0x38, opts.role ?? 0);
  d.setUint32(0x3c, opts.abi ?? 0);
  d.setUint32(0x90, HEADER_BYTES);
  d.setUint32(0x94, table.length);
  f.set(table, HEADER_BYTES);
  return f;
}

/** A port file of the firmware `fw` (role 2, a version 3 table with PROVIDES `port` and the FW item). */
function port(stem: string, fw: number[]): PortFile {
  const bytes = build({ version: 3, role: 2, provides: [nameHash('port'), v(0, 1)], fw });
  const p = portFileOf(stem, bytes, `h-${stem}`);
  assert.ok(p);
  return p;
}

const FW_A = [1, 2, 3, 4, 5, 6, 7, 8];
const FW_B = [9, 9, 9, 9, 9, 9, 9, 9];
const hexOf = (b: number[]) => b.map((x) => x.toString(16).padStart(2, '0')).join('');

function svc(stem: string, version: number, needs: Need[] = [], abi = 0, builtFor = 0): ServiceFile {
  const bytes = build({ role: 1, provides: [nameHash(stem), version], needs, abi, builtFor });
  const s = serviceFileOf(stem, bytes, `h-${stem}`);
  assert.ok(s);
  return s;
}

const RT: RuntimeIdentity = { version: v(0, 9), fingerprint: null, fw: null };
const RUNTIME = new Set([nameHash('runtime')]);

test('nameHash is FNV-1a 32', () => {
  assert.equal(nameHash(''), 0x811c9dc5);
  assert.equal(nameHash('a'), 0xe40c292c);
  assert.equal(nameHash('foobar'), 0xbf9cf968);
  assert.ok(nameHash('runtime') > 0);
  assert.notEqual(nameHash('fx'), nameHash('machines'));
});

test('readNeeds reads v1 and v2 tables and gives [] without a table', () => {
  const needs = [need('fx', v(1, 2)), need('ui', 0, false)];
  assert.deepEqual(readNeeds(build({ version: 1, needs })), needs);
  assert.deepEqual(readNeeds(build({ version: 2, needs })), needs);
  assert.deepEqual(readNeeds(new Uint8Array(HEADER_BYTES)), []);
  assert.deepEqual(readNeeds(new Uint8Array(4)), []);
});

test('a table with the wrong magic or version is not read', () => {
  const needs = [need('fx')];
  assert.deepEqual(readNeeds(build({ needs, magic: 0x12345678 })), []);
  assert.deepEqual(readNeeds(build({ needs, version: 4 })), []);
  assert.equal(readProvides(build({ provides: [1, 2], magic: 0x12345678 })), null);
});

test('readProvides and serviceFileOf read a service file', () => {
  const bytes = build({ role: 1, provides: [nameHash('fx'), v(1, 3)], needs: [need('runtime')] });
  assert.deepEqual(readProvides(bytes), { hash32: nameHash('fx'), version: v(1, 3) });
  const s = serviceFileOf('fx', bytes, 'abc');
  assert.ok(s);
  assert.equal(s.label, 'fx service');
  assert.equal(s.path, 'services/fx.dks');
  assert.equal(s.file_size, bytes.length);
  assert.equal(s.hash, 'abc');
  assert.deepEqual(s.needs, [need('runtime')]);
});

test('serviceFileOf is null for role 0 and for a file without PROVIDES', () => {
  assert.equal(serviceFileOf('fx', build({ role: 0, provides: [1, 2] }), 'h'), null);
  assert.equal(serviceFileOf('fx', build({ role: 1, needs: [need('ui')] }), 'h'), null);
  assert.equal(readProvides(build({ role: 1 })), null);
});

test('a module that needs only the runtime needs no service', () => {
  const r = resolveServices([{ id: 'a', needs: [need('runtime', v(0, 8))] }], [], RT, RUNTIME);
  assert.deepEqual(r.byModule.get('a'), []);
  assert.deepEqual(r.order, []);
  assert.deepEqual(r.refused, []);
});

test('a module gets the service it needs', () => {
  const fx = svc('fx', v(1, 2));
  const r = resolveServices([{ id: 'a', needs: [need('fx', v(1, 0))] }], [fx], RT, RUNTIME);
  assert.deepEqual(r.byModule.get('a'), [fx]);
  assert.deepEqual(r.order, [fx]);
});

test('a service that needs a service comes after it, and each service is listed once', () => {
  const fx = svc('fx', v(1, 0));
  const machines = svc('machines', v(1, 0), [need('fx', v(1, 0))]);
  const r = resolveServices(
    [
      { id: 'a', needs: [need('machines')] },
      { id: 'b', needs: [need('fx'), need('machines')] },
    ],
    [machines, fx],
    RT,
    RUNTIME,
  );
  assert.deepEqual(r.order, [fx, machines]);
  assert.deepEqual(r.byModule.get('a'), [fx, machines]);
  assert.deepEqual(r.refused, []);
});

test('a hash nobody provides refuses that module only', () => {
  const fx = svc('fx', v(1, 0));
  const r = resolveServices(
    [
      { id: 'a', needs: [need('nothing')] },
      { id: 'b', needs: [need('fx')] },
    ],
    [fx],
    RT,
    RUNTIME,
  );
  assert.equal(r.refused.length, 1);
  assert.equal(r.refused[0]!.module, 'a');
  assert.equal(r.refused[0]!.kind, 'unknown-hash');
  assert.equal(r.byModule.has('a'), false);
  assert.deepEqual(r.byModule.get('b'), [fx]);
  assert.deepEqual(r.order, [fx]);
});

test('an older minor or another major of a service is refused as version', () => {
  const fx = svc('fx', v(1, 2));
  const older = resolveServices([{ id: 'a', needs: [need('fx', v(1, 3))] }], [fx], RT, RUNTIME);
  assert.equal(older.refused[0]!.kind, 'version');
  assert.equal(older.refused[0]!.need, '1.3');
  assert.equal(older.refused[0]!.have, '1.2');
  const other = resolveServices([{ id: 'a', needs: [need('fx', v(2, 0))] }], [fx], RT, RUNTIME);
  assert.equal(other.refused[0]!.kind, 'version');
  const newerMajor = resolveServices([{ id: 'a', needs: [need('fx', v(0, 1))] }], [fx], RT, RUNTIME);
  assert.equal(newerMajor.refused[0]!.kind, 'version');
});

test('a service built for another ABI fingerprint refuses that module only', () => {
  const fx = svc('fx', v(1, 0), [], 0xaaaa, v(0, 5));
  const lfo = svc('lfo', v(1, 0), [], 0xbbbb, v(0, 5));
  const rt: RuntimeIdentity = { version: (0 << 16) | (5 << 8) | 2, fingerprint: 0xbbbb, fw: null };
  const r = resolveServices(
    [
      { id: 'a', needs: [need('fx')] },
      { id: 'b', needs: [need('lfo')] },
    ],
    [fx, lfo],
    rt,
    RUNTIME,
  );
  assert.deepEqual(r.refused, [
    {
      module: 'a',
      service: 'fx',
      kind: 'fingerprint',
      runtime: '0.5.2',
      builtFor: '0.5.0',
      runtimeAbi: '0000bbbb',
      serviceAbi: '0000aaaa',
    },
  ]);
  assert.deepEqual(r.order, [lfo]);
});

test('a service without a fingerprint (abi 0, a service in C) is accepted like the loader does', () => {
  const c = svc('svc-test', v(1, 0), [], 0, v(0, 5));
  const rt: RuntimeIdentity = { version: (0 << 16) | (5 << 8) | 2, fingerprint: 0xbbbb, fw: null };
  const r = resolveServices([{ id: 'a', needs: [need('svc-test')] }], [c], rt, RUNTIME);
  assert.deepEqual(r.refused, []);
  assert.deepEqual(r.order, [c]);
});

test('a runtime that reports no fingerprint accepts every service', () => {
  const fx = svc('fx', v(1, 0), [], 0xaaaa);
  const r = resolveServices([{ id: 'a', needs: [need('fx')] }], [fx], RT, RUNTIME);
  assert.deepEqual(r.refused, []);
});

test('more than PROVIDER_MAX services refuse the modules that push it over', () => {
  const set = Array.from({ length: PROVIDER_MAX + 1 }, (_, i) => svc(`s${i}`, v(1, 0)));
  const mods = set.map((s, i) => ({ id: `m${i}`, needs: [need(s.stem)] }));
  const r = resolveServices(mods, set, RT, RUNTIME);
  assert.equal(r.order.length, PROVIDER_MAX);
  assert.deepEqual(
    r.refused.map((x) => [x.module, x.kind]),
    [[`m${PROVIDER_MAX}`, 'too-many']],
  );
  assert.equal(r.byModule.size, PROVIDER_MAX);
});

const card = (...e: [string, Need[]][]) => e.map(([path, needs]) => ({ path, needs }));

test('deleting the only module of a service deletes the service too', () => {
  const c = card(['modules/a.dkm', [need('fx')]], ['modules/b.dkm', []], ['services/fx.dks', []]);
  const p = planDelete(c, ['modules/a.dkm']);
  assert.deepEqual(p.delete.sort(), ['modules/a.dkm', 'services/fx.dks']);
  assert.deepEqual(p.kept, []);
  assert.deepEqual(p.refused, []);
});

test('a service another module still needs is kept', () => {
  const c = card(['modules/a.dkm', [need('fx')]], ['modules/b.dkm', [need('fx')]], ['services/fx.dks', []]);
  const p = planDelete(c, ['modules/a.dkm']);
  assert.deepEqual(p.delete, ['modules/a.dkm']);
  assert.deepEqual(p.kept, [{ path: 'services/fx.dks', by: ['modules/b.dkm'] }]);
  assert.deepEqual(p.refused, []);
});

test('deleting a needed service alone is refused', () => {
  const c = card(['modules/a.dkm', [need('fx')]], ['services/fx.dks', []]);
  const p = planDelete(c, ['services/fx.dks']);
  assert.deepEqual(p.delete, []);
  assert.deepEqual(p.refused, [{ path: 'services/fx.dks', by: ['modules/a.dkm'] }]);
});

test('deleting a module and its service together deletes both', () => {
  const c = card(['modules/a.dkm', [need('fx')]], ['services/fx.dks', []]);
  const p = planDelete(c, ['modules/a.dkm', 'services/fx.dks']);
  assert.deepEqual(p.delete.sort(), ['modules/a.dkm', 'services/fx.dks']);
  assert.deepEqual(p.refused, []);
  assert.deepEqual(p.kept, []);
});

test('a port file is read: role 2, the name `port` and its FW item as 16 hex digits', () => {
  const p = port('dt-1', FW_A);
  assert.equal(p.fw, hexOf(FW_A));
  assert.equal(p.path, 'ports/dt-1.dkport');
  // another role, no FW item or another provided name is no port file
  assert.equal(portFileOf('x', build({ version: 3, role: 1, provides: [nameHash('port'), 0], fw: FW_A }), 'h'), null);
  assert.equal(portFileOf('x', build({ version: 3, role: 2, provides: [nameHash('port'), 0] }), 'h'), null);
  assert.equal(portFileOf('x', build({ version: 3, role: 2, provides: [nameHash('fx'), 0], fw: FW_A }), 'h'), null);
});

test('a service that needs `port` gets the port file whose FW equals the device, whatever the file names', () => {
  const lfo = svc('lfo', v(1, 0), [need('port')]);
  const ports = [port('other-1.16', FW_B), port('any-name', FW_A)];
  const rt = { version: v(0, 9), fingerprint: null, fw: hexOf(FW_A) };
  const r = resolveServices([{ id: 'a', needs: [need('lfo')] }], [lfo], rt, RUNTIME, ports);
  assert.deepEqual(r.refused, []);
  assert.equal(r.port?.stem, 'any-name');
  assert.deepEqual(r.order, [lfo]);
  // a module that needs no service-with-port has none
  const plain = svc('samples', v(1, 0));
  assert.equal(resolveServices([{ id: 'a', needs: [need('samples')] }], [plain], rt, RUNTIME, ports).port, null);
});

test('no port file for the device refuses the modules that need one, with the firmware named', () => {
  const lfo = svc('lfo', v(1, 0), [need('port')]);
  const plain = svc('samples', v(1, 0));
  const mods = [
    { id: 'a', needs: [need('lfo')] },
    { id: 'b', needs: [need('samples')] },
  ];
  const rt = { version: v(0, 9), fingerprint: null, fw: hexOf(FW_A) };
  const r = resolveServices(mods, [lfo, plain], rt, RUNTIME, [port('other', FW_B)]);
  assert.deepEqual(r.refused, [{ module: 'a', service: 'port', kind: 'no-port', fw: hexOf(FW_A) }]);
  assert.deepEqual(r.order, [plain], 'the other module still installs');
  assert.equal(r.port, null);
  // a runtime that reports no firmware identity has no port file to pick
  const none = resolveServices(mods, [lfo, plain], { ...rt, fw: null }, RUNTIME, [port('a', FW_A)]);
  assert.deepEqual(none.refused, [{ module: 'a', service: 'port', kind: 'no-port' }]);
});

test('the port file takes a provider slot: PROVIDER_MAX services that need it do not fit', () => {
  const set = Array.from({ length: PROVIDER_MAX }, (_, i) => svc(`s${i}`, v(1, 0), [need('port')]));
  const mods = set.map((s, i) => ({ id: `m${i}`, needs: [need(s.stem)] }));
  const rt = { version: v(0, 9), fingerprint: null, fw: hexOf(FW_A) };
  const r = resolveServices(mods, set, rt, RUNTIME, [port('p', FW_A)]);
  assert.equal(r.order.length, PROVIDER_MAX - 1);
  assert.deepEqual(
    r.refused.map((x) => [x.module, x.kind]),
    [[`m${PROVIDER_MAX - 1}`, 'too-many']],
  );
});

test('the port file goes with the last module that needs a service, and stays while one does', () => {
  const c = card(
    ['modules/a.dkm', [need('lfo')]],
    ['modules/b.dkm', [need('lfo')]],
    ['services/lfo.dks', [need('port')]],
    ['ports/dt-1.dkport', []],
  );
  const one = planDelete(c, ['modules/a.dkm']);
  assert.deepEqual(one.delete, ['modules/a.dkm']);
  assert.deepEqual(one.kept.map((k) => k.path).sort(), ['ports/dt-1.dkport', 'services/lfo.dks']);
  const both = planDelete(c, ['modules/a.dkm', 'modules/b.dkm']);
  assert.deepEqual(both.delete.sort(), ['modules/a.dkm', 'modules/b.dkm', 'ports/dt-1.dkport', 'services/lfo.dks']);
  // the port file alone is refused while a service needs it
  assert.deepEqual(
    planDelete(c, ['ports/dt-1.dkport']).refused.map((r) => r.path),
    ['ports/dt-1.dkport'],
  );
});
