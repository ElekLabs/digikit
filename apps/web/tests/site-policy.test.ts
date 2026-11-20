// The site's device list is one place (src/data/site-devices.mjs, with src/firmware/site-policy.ts on top): every list, name, filter, build step and publication of a
// device on the site derives from it.
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, relative } from 'node:path';
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { deviceNameOf } from '@digi/workbench-core/data/device-profiles.ts';
import {
  SITE_DEVICES,
  siteDeviceNamesText,
  siteDevices,
  siteFaceplateIds,
  siteNewestReleasesText,
} from '../src/firmware/site-policy.ts';
import { SITE_DEVICES as LIST } from '../src/data/site-devices.mjs';
import { root } from './support/patchkit-env.ts';

test('the site lists only the devices of SITE_DEVICES, and a longer list is followed', () => {
  assert.deepEqual([...SITE_DEVICES], [...LIST]);
  assert.deepEqual([...siteDevices()], [...SITE_DEVICES]);
  assert.ok(siteFaceplateIds().every((d) => SITE_DEVICES.includes(d)));
  assert.ok(!siteDeviceNamesText('and').includes(deviceNameOf('st')));
  assert.ok(!siteNewestReleasesText().includes(deviceNameOf('st')));
  const more = [...SITE_DEVICES, 'st'];
  assert.ok(siteDevices(more).includes('st'));
  assert.ok(siteFaceplateIds(more).includes('st'));
  assert.ok(siteDeviceNamesText('or', more).includes(deviceNameOf('st')));
  assert.ok(siteNewestReleasesText(more).includes(deviceNameOf('st')));
  assert.deepEqual([...siteDevices(['dt2'])], ['dt2']);
});

// Every way to get the registry's whole device list; the site's files take their devices from the site list instead.
const ALL_DEVICE_LISTS =
  /\b(runtimeDeviceIds|runtimeFaceplateIds|deviceNamesText|newestReleasesText|deviceIds|faceplateIds)\b|registry\s*\.\s*(runtimeIds|devices|ids)\b|Object\.(keys|values|entries)\(\s*(deviceProfiles|panels|registry)\b|\.runtimeIds\b/;
// The files that may name them, and why: the policy itself; the label table (a short name per device, not a list the site shows); the site build, which filters
// `loadDevices().runtimeIds` by the site list (checked below).
const ALLOWED = new Set(['src/firmware/site-policy.ts', 'src/data/module-labels.ts', 'scripts/gen-modules.mjs']);
const EXT = /\.(astro|tsx?|mts|cts|jsx?|mjs|mdx)$/;

test('no page, component, script or config of the site lists the registry devices directly', () => {
  const web = join(root, 'apps/web');
  const walk = (d: string): string[] =>
    readdirSync(d).flatMap((n) => {
      const p = join(d, n);
      return statSync(p).isDirectory() ? walk(p) : EXT.test(n) ? [p] : [];
    });
  const files = [
    ...walk(join(web, 'src')),
    ...walk(join(web, 'scripts')),
    ...['site.config.mjs', 'astro.config.mjs'].map((n) => join(web, n)).filter(existsSync),
  ];
  assert.ok(files.length > 20);
  for (const f of files) {
    const rel = relative(web, f);
    if (ALLOWED.has(rel)) continue;
    assert.ok(
      !ALL_DEVICE_LISTS.test(readFileSync(f, 'utf8')),
      `${rel} lists the registry's devices: use site-policy.ts`,
    );
  }
});

test('the site build ships modules only for the site list', () => {
  const src = readFileSync(join(root, 'apps/web/scripts/gen-modules.mjs'), 'utf8');
  assert.match(src, /import \{ SITE_DEVICES \} from '..\/src\/data\/site-devices\.mjs'/);
  assert.match(src, /buildModuleData\(repoRoot, SITE_DEVICES\)/);
  assert.match(src, /runtimeIds\.filter\(\(d\) => SITE_DEVICES\.includes\(d\)\)/);
});
