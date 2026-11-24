import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { DkProf } from '@digi/dk-session/dkprof';
import { hotspotLabel, readSavedProfile, resolveSavedHotspots } from '../src/profile/hotspots';

const hash = 'ab'.repeat(32);
const otherHash = 'cd'.repeat(32);
const map = { key: { image_sha256: hash } };
const doc = {
  schema: 'dk.profile/1',
  run: { firmware_sha256: hash.toUpperCase() },
  spans: [{ name: 'early', sum_ticks: 1 }],
  span_elapsed_ticks: 10,
  raw: [
    { cpu: 'cf', series: 'pc', addr: 0x100, count: 6 },
    { cpu: 'cf', series: 'pc', addr: 0x110, count: 4 },
    { cpu: 'cf', series: 'idle_pc', addr: 0x111, count: 999 },
    { cpu: 'sharc', series: 'pc', addr: 0x100, count: 3 },
  ],
};

test('saved hotspots use the capture SHA and preserve span comparison, CPU and raw addresses', async () => {
  const profile = readSavedProfile(doc);
  assert.equal(profile.shares?.get('early'), 0.1);
  const maps = async (sha: string) => {
    assert.equal(sha, hash);
    return [map, { key: { image_sha256: otherHash } }];
  };
  const addrLookup: DkProf['addrLookup'] = (req) => {
    assert.deepEqual(req.maps, [map]);
    assert.equal(req.loads, undefined);
    assert.deepEqual(req.addrs, [
      { cpu: 'cf', addr: 0x100 },
      { cpu: 'cf', addr: 0x110 },
      { cpu: 'sharc', addr: 0x100 },
    ]);
    return {
      unloaded: [],
      results: req.addrs.map((addr, i) => ({
        ...addr,
        text: ['tick+0x4', null, 'mix+0x1'][i]!,
        name: null,
        layer: null,
        sym_addr: null,
        offset: null,
        size: null,
        module: null,
      })),
    };
  };
  const named = await resolveSavedHotspots(profile, { maps }, async () => ({ addrLookup }));
  assert.deepEqual(named.cf.map(hotspotLabel), ['0x100 tick+0x4', '0x110']);
  assert.deepEqual(named.sharc.map(hotspotLabel), ['0x100 mix+0x1']);
  assert.equal(named.cf[0]?.share, 0.6);
});

test('legacy or unmatched captures keep raw addresses; profiles without spans still display samples', async () => {
  const legacy = readSavedProfile({ ...doc, run: { firmware_sha256: hash.slice(0, 16) } });
  const noMaps = readSavedProfile({ ...doc, spans: [] });
  let lookups = 0;
  const prof = async () => ({
    addrLookup: () => {
      lookups++;
      throw new Error('no map');
    },
  });
  const maps = async () => [{ key: { image_sha256: otherHash } }];
  assert.equal(legacy.sha256, undefined);
  assert.equal(noMaps.shares, undefined);
  assert.equal(await resolveSavedHotspots(legacy, { maps }, prof), legacy.pcs);
  assert.equal(await resolveSavedHotspots(noMaps, { maps }, prof), noMaps.pcs);
  assert.equal(lookups, 0);
});
