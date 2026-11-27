// The gallery's cheap checks (the module summaries) and the patch worker's file
// names. No wasm and no command line: the tests that run patchkit.wasm or the CLI, `shipModules` (which packs with it) and `buildModuleData` (seconds: it reads every module) are in gallery.wasm.test.ts.
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { parse } from 'smol-toml';
import { test, vi } from 'vitest';
import { patchedFileName } from '@digi/workbench-core/patch/patchkit-core.ts';
import { manifestFiles, root } from './support/patchkit-env.ts';

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
  }
});

test('the gallery names only the devices the site offers, whatever the modules list in [targets]', async () => {
  const { modules } = await import('../src/data/modules.ts');
  const { SITE_DEVICES } = await import('../src/firmware/site-policy.ts');
  assert.ok(modules.length > 0);
  for (const m of modules) {
    for (const d of m.devices) assert.ok(SITE_DEVICES.includes(d), `${m.id} names ${d}`);
  }
});

test('adding a device to the site list is the only change the gallery needs', async () => {
  vi.resetModules();
  vi.doMock('../src/firmware/site-policy.ts', async (orig) => {
    const real = await orig<typeof import('../src/firmware/site-policy.ts')>();
    return { ...real, siteDevices: () => real.siteDevices([...real.SITE_DEVICES, 'st']) };
  });
  try {
    const { modules } = await import('../src/data/modules.ts');
    assert.ok(
      modules.some((m) => m.devices.includes('st')),
      'a module that lists the Syntakt in [targets] shows it',
    );
  } finally {
    vi.doUnmock('../src/firmware/site-policy.ts');
    vi.resetModules();
  }
});
