// The site's device list is one place (src/firmware/site-policy.ts): every list, name and filter of a device on the site derives from it.
import { readFileSync, readdirSync, statSync } from 'node:fs';
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
import { root } from './support/patchkit-env.ts';

test('the site lists only the devices of SITE_DEVICES, and a longer list is followed', () => {
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

// The pages, components and environments of the site take their devices from site-policy.ts; these names list every device of the registry.
const ALL_DEVICE_LISTS = /\b(runtimeDeviceIds|runtimeFaceplateIds|deviceNamesText|newestReleasesText)\b/;
const ALLOWED = new Set(['src/firmware/site-policy.ts']);

test('no page, component or data file of the site lists the registry devices directly', () => {
  const src = join(root, 'apps/web/src');
  const walk = (d: string): string[] =>
    readdirSync(d).flatMap((n) => {
      const p = join(d, n);
      return statSync(p).isDirectory() ? walk(p) : /\.(astro|tsx?|mjs)$/.test(n) ? [p] : [];
    });
  for (const f of walk(src)) {
    const rel = relative(join(root, 'apps/web'), f);
    if (ALLOWED.has(rel)) continue;
    assert.ok(
      !ALL_DEVICE_LISTS.test(readFileSync(f, 'utf8')),
      `${rel} lists the registry's devices: use site-policy.ts`,
    );
  }
});
