import { expect, test } from 'vitest';
import { decodeFault, settingsValue } from '../runtime/fault.ts';

function record(entries: [string, Uint8Array][]): Uint8Array {
  const body = entries.flatMap(([key, value]) => [
    key.length,
    ...Buffer.from(key),
    value.length >> 8,
    value.length & 255,
    ...value,
  ]);
  const out = new Uint8Array(18 + body.length);
  out.set(Buffer.from('DKST'));
  new DataView(out.buffer).setUint32(10, out.length);
  out.set(body, 18);
  return out;
}

function faultValue(kind: number, site: number, stage: number, id: number, step: number, at: number, name: string) {
  const value = new Uint8Array(40);
  const view = new DataView(value.buffer);
  view.setUint32(0, 0x464c5431);
  view.setUint32(4, kind | (site << 8) | (stage << 16));
  view.setUint32(8, id);
  view.setUint32(12, step);
  view.setUint32(16, at);
  value.set(Buffer.from(name), 24);
  return value;
}

test('a value is found by its key among the entries of a settings record', () => {
  const rec = record([
    ['fx/route/test/gain', Uint8Array.of(1)],
    ['dk/fault', Uint8Array.of(1, 2, 3)],
  ]);
  expect([...settingsValue(rec, 'dk/fault')!]).toEqual([1, 2, 3]);
  expect(settingsValue(rec, 'dk/other')).toBeNull();
  expect(settingsValue(rec.subarray(0, 20), 'dk/fault')).toBeNull();
});

test('dk/fault decodes to the site, the steps and the module', () => {
  const f = decodeFault(faultValue(2, 3, 4, 96, 3, 0x600304, 'test/dyn-fx-hang'))!;
  expect(f).toMatchObject({ kind: 'dsp', site: 3, stage: 4, id: 96, step: 3, module: 'test/dyn-fx-hang' });
  expect(decodeFault(faultValue(1, 0, 0, 0, 0, 0x12a4f0, 'x'))).toMatchObject({ kind: 'cpu', at: 0x12a4f0 });
  expect(decodeFault(faultValue(3, 0, 0, 0, 0, 0, 'runtime'))).toMatchObject({ kind: 'panic', module: 'runtime' });
  expect(decodeFault(new Uint8Array(40))).toBeNull();
  expect(decodeFault(faultValue(7, 0, 0, 0, 0, 0, 'x'))).toBeNull();
});
