import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import { DkProf } from '@digi/dk-session/dkprof';
import { makeResolver } from '@digi/dk-session/log-names';

function map(image: string, name: string) {
  return {
    format: 'dk-addrmap',
    version: 1,
    kind: 'dev',
    key: { image_sha256: image },
    entries: [{ cpu: 'cf', addr: 0x40000000, size: 0x10, name, layer: 'fw' }],
  };
}

test('simultaneous Log resolvers each prepare their own map once', async () => {
  const wasm = new Uint8Array(readFileSync(new URL('../../../web/public/dkprof.wasm', import.meta.url)));
  const prof = await DkProf.create(wasm);
  const loads = { modules: [] };
  const emulatorMap = map('emulator', 'emulator_fn');
  let encodes = 0;
  const countedMap = {
    ...emulatorMap,
    toJSON() {
      encodes += 1;
      return emulatorMap;
    },
  };
  const emulator = makeResolver(prof, [countedMap], loads);
  const device = makeResolver(prof, [map('device', 'device_fn')], loads);

  assert.equal(emulator('cf', 0x40000000), 'emulator_fn');
  assert.equal(device('cf', 0x40000004), 'device_fn+0x4');
  assert.equal(emulator('cf', 0x40000010, true), 'emulator_fn+0xe');
  assert.equal(device('cf', 0x40000010), null);
  assert.equal(encodes, 1);
});
