import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { registry, stockFirmware } from '@digi/devices/node';
import { stockImage, stockSyx } from '../src/node/stock-image.ts';

describe('stock image names come from the device files', () => {
  test('every device has a stock .syx name from its firmware list', () => {
    for (const id of registry().ids) {
      const fw = stockFirmware(registry().get(id));
      expect(fw?.filename, id).toBeTruthy();
      expect(stockSyx(id), id).toBe(fw?.filename);
    }
  });

  test('the stock firmware is the acceptance version, else the last listed', () => {
    const fw = (version: string, filename: string) => ({ version, filename, sha256: '', port: false });
    const firmware = [fw('1', 'a.syx'), fw('2', 'b.syx'), fw('3', 'c.syx')];
    expect(stockFirmware({ firmware })?.filename).toBe('c.syx');
    expect(stockFirmware({ firmware, acceptance: { firmware: '2', readyDir: '', sampleCard: false } })?.filename).toBe(
      'b.syx',
    );
  });

  test('the path is in the firmware directory of the environment', () => {
    const id = registry().ids[0] as string;
    expect(stockImage(id, '/repo', {})).toBe(path.join('/repo', 'firmware', stockSyx(id)));
    expect(stockImage(id, '/repo', { DIGI_FIRMWARE_DIR: '/fw' })).toBe(path.join('/fw', stockSyx(id)));
    expect(() => stockImage('nope', '/repo', {})).toThrow(/no stock firmware/);
  });
});
