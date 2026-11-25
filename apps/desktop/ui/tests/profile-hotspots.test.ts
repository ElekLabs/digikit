import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import { DkProf } from '@digi/dk-session/dkprof';
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

test('saved hotspots use the capture SHA and preserve CPU and raw addresses', async () => {
  const profile = readSavedProfile(doc);
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
  const { pcs: named } = await resolveSavedHotspots(profile, { maps }, async () => ({ addrLookup }));
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
  assert.equal((await resolveSavedHotspots(legacy, { maps }, prof)).pcs, legacy.pcs);
  assert.equal((await resolveSavedHotspots(noMaps, { maps }, prof)).pcs, noMaps.pcs);
  assert.equal(lookups, 0);
});

test('shared dkprof resolves stock links and relocated modules from saved evidence', async () => {
  const prof = await DkProf.create(
    new Uint8Array(readFileSync(new URL('../../../web/public/dkprof.wasm', import.meta.url))),
  );
  const stock = {
    format: 'dk-addrmap',
    version: 1,
    kind: 'dev',
    key: { image_sha256: hash },
    entries: [{ cpu: 'sharc', addr: 0x100, size: 4, name: 'biquad', layer: 'fw' }],
    review: { cpu: 'sharc', path: '/tmp/review.html', url: 'file:///tmp/review.html', entries: [0x100] },
  };
  const moduleHash = 'ef'.repeat(32);
  const packed = (h: string) => ({
    format: 'dk-addrmap',
    version: 1,
    kind: 'module',
    key: { module: 'test/effect', dkm_sha256: h },
    entries: [{ cpu: 'sharc', addr: 4, size: 4, name: 'render', layer: 'our', module: 'test/effect' }],
  });
  const profile = readSavedProfile({
    schema: 'dk.profile/1',
    clock: { unit: 'insn' },
    run: {
      target: 'fixture',
      firmware_sha256: hash,
      module_sha256: [moduleHash],
      loads: {
        modules: [
          { id: 'test/effect', base: 0x2000, dsp_base: 0x250 },
          { id: 'test/effect', base: 0x2400, dsp_base: 0x300 },
        ],
      },
    },
    raw: [
      { cpu: 'sharc', addr: 0x101, count: 6 },
      { cpu: 'sharc', addr: 0x305, count: 4 },
    ],
  });
  const { pcs: result } = await resolveSavedHotspots(profile, { maps: async () => [stock] }, async () => prof, {
    readMaps: async () => ({ ships: [], modules: [{ id: 'test/effect', json: JSON.stringify(packed(moduleHash)) }] }),
  });
  assert.deepEqual(result.sharc[0]?.review, {
    path: '/tmp/review.html',
    url: 'file:///tmp/review.html',
    sha256: hash,
    entry: 0x100,
    fragment: 'r-0x100',
  });
  assert.equal(result.sharc[0]?.unit, 'insn');
  assert.equal(result.sharc[0]?.owner, 'firmware');
  assert.equal(result.sharc[1]?.entry, 0x304);
  assert.equal(result.sharc[1]?.module_base, 0x300);
  assert.equal(result.sharc[1]?.module_entry, 4);
  assert.equal(result.sharc[1]?.owner, 'test/effect');
  assert.equal(result.sharc[1]?.offset, 1);

  const { pcs: wrong } = await resolveSavedHotspots(profile, { maps: async () => [stock] }, async () => prof, {
    readMaps: async () => ({ ships: [], modules: [{ id: 'test/effect', json: JSON.stringify(packed(otherHash)) }] }),
  });
  assert.equal(wrong.sharc[1]?.addr, 0x305);
  assert.equal(wrong.sharc[1]?.name, undefined);
});

test('a calibration profile adds its full stock hash while retaining the short fit key', () => {
  const run = { firmware_sha256: hash.slice(0, 16), stock_firmware_sha256: hash };
  assert.equal(readSavedProfile({ ...doc, run }).sha256, hash);
  assert.equal(readSavedProfile({ ...doc, run: { ...run, stock_firmware_sha256: otherHash } }).sha256, undefined);
});

test('a rig export names SHARC instructions without a firmware identity or symbol files', async () => {
  const prof = await DkProf.create(
    new Uint8Array(readFileSync(new URL('../../../web/public/dkprof.wasm', import.meta.url))),
  );
  const rig = {
    map: {
      format: 'dk-addrmap',
      version: 1,
      kind: 'rig',
      key: { device: 'dt2' },
      entries: [{ cpu: 'sharc', addr: 4, size: 4, name: 'render', layer: 'our', module: 'test/rig' }],
    },
  };
  const doc = {
    schema: 'dk.profile/1',
    clock: { source: 'sharc-rig.insn', unit: 'instructions', sampling: 'each instruction' },
    run: { id: 'rig:test/rig', target: 'dt2', loads: { modules: [{ id: 'test/rig', base: 0, dsp_base: 0x300 }] } },
    raw: [{ cpu: 'sharc', addr: 0x305, count: 6 }],
    metrics: [{ name: 'sharc.blocks', value: 2, unit: 'blocks' }],
    rig,
  };
  const named = await resolveSavedHotspots(readSavedProfile(doc), undefined, async () => prof);
  assert.equal(named.pcs.sharc[0]?.name, 'render+0x1');
  assert.equal(named.pcs.sharc[0]?.review, undefined);
  assert.equal(named.sharc?.functions[0]?.module, 'test/rig');
  assert.equal(named.sharc?.functions[0]?.per_block, 3);

  const wrong = readSavedProfile({ ...doc, run: { ...doc.run, target: 'dn2' } });
  assert.equal((await resolveSavedHotspots(wrong, undefined, async () => prof)).pcs.sharc[0]?.name, undefined);
});
