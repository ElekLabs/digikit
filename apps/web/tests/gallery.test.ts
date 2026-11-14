// The gallery's build step (scripts/gen-modules.mjs: the bundle of the committed modules and ports, the summaries) and the patch worker's file
// names. No wasm and no command line: the tests that run patchkit.wasm or the CLI (and `shipModules`, which packs with it) are in gallery.wasm.test.ts.
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { parse } from 'smol-toml';
import { test, vi } from 'vitest';
import { buildModuleData } from '../scripts/gen-modules.mjs';
import { patchedFileName } from '../src/patch/patchkit-core.ts';
import { manifestFiles, root } from './support/patchkit-env.ts';

// buildModuleData reads every module of the repository: seconds on a busy machine
vi.setConfig({ testTimeout: 60_000 });

test('bundle lists the committed modules and ports', () => {
  const { bundle } = buildModuleData(root);
  assert.equal(
    bundle.modules.length,
    manifestFiles().length + manifestFiles(join(root, 'tests/fixtures/modules')).length,
  );
  assert.ok(bundle.modules.some((m) => m.manifest.includes('id = "core/fx"')));
  assert.equal(bundle.ports.length, readdirSync(join(root, 'devices/ports')).length);
  // Module content only: no firmware blobs or out/modules oracles.
  assert.ok(!JSON.stringify(bundle).includes('out/modules'));
});

test('bundle carries the device knob models and the gallery JSON of typed modules', () => {
  const { bundle } = buildModuleData(root);
  assert.equal(bundle.devices.length, 2);
  assert.ok(bundle.devices.every((t: string) => 'modules' in parse(t)));
  const sine = bundle.modules.find((m: { manifest: string }) => m.manifest.includes('id = "test/sine"'))!;
  assert.ok(!sine.manifest.includes('[[entry]]'));
  const g = JSON.parse((sine.files as Record<string, string>)['build/gen/gallery.json']);
  assert.equal(g.module, 'test/sine');
  const m = g.machines[0];
  assert.deepEqual(Object.keys(m.devices).sort(), ['dn2', 'dt2']);
  assert.equal(m.devices.dt2.knobs[1].param, 'level');
  assert.equal(m.devices.dn2.knobs[1].slot, 26);
  assert.deepEqual(
    g.params.dt2.map((p: { short: string }) => p.short),
    ['LEVEL'],
  );
  for (const p of g.params.dt2 as { min: number; default: number; max: number; id: string }[])
    assert.ok(p.min <= p.default && p.default <= p.max, p.id);
});

test('public module summaries are one short sentence', () => {
  for (const file of manifestFiles()) {
    const { module: m } = parse(readFileSync(file, 'utf8')) as {
      module: { id: string; category: string; summary: string };
    };
    if (m.category === 'core' || m.category === 'test') continue;
    assert.ok(m.summary.length <= 120, `${m.id}: summary is longer than 120 characters`);
    assert.ok(m.summary.endsWith('.'), `${m.id}: summary does not end with a full stop`);
  }
});

test('patched file names', () => {
  assert.equal(patchedFileName('Digitakt II', '1.17', ['core/fx']), 'Digitakt_II_OS1.17+core-fx.syx');
});

test('built site (when present) carries no firmware', { skip: !existsSync(join(root, 'apps/web/dist')) }, () => {
  const walk = (d: string): string[] =>
    readdirSync(d, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)],
    );
  for (const f of walk(join(root, 'apps/web/dist'))) {
    assert.ok(!/\.(syx|snap|mstate)$/i.test(f), f);
    assert.ok(statSync(f).size < 5 * 1024 * 1024 || f.endsWith('emulator-core.wasm'), f);
  }
});
