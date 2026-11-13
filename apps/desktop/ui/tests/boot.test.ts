import { describe, expect, it } from 'vitest';
import { deviceOfSyx } from '../../../web/src/patch/identify';
import { registry } from '../../../web/src/data/device-profiles';
import { booting, bootingScreen, bootingStatus, loadFailure } from '../src/boot-core';

const [first, second] = registry.runtimeIds as [string, string];
const framed = (id: number) => new Uint8Array([0xf0, 0x00, 0x20, 0x3c, id, 0x00, 0x7f, 0xf7]);

describe('loading state', () => {
  it('knows the device from the device byte of the framing, not from a name', () => {
    expect(deviceOfSyx(framed(registry.get(first).syxId!))).toBe(first);
    expect(deviceOfSyx(framed(registry.get(second).syxId!))).toBe(second);
    expect(deviceOfSyx(framed(0x00))).toBeUndefined();
    expect(deviceOfSyx(new Uint8Array([1, 2, 3]))).toBeUndefined();
    expect(deviceOfSyx(new Uint8Array([0xf0, 0x00, 0x21, 0x3c, registry.get(first).syxId!]))).toBeUndefined();
  });
  it('names the device, else the file, else nothing', () => {
    const name = registry.get(first).name;
    expect(bootingStatus(booting('image.syx', first))).toBe(`Booting ${name}…`);
    expect(bootingStatus(booting('image.syx'))).toBe('Booting image.syx…');
    expect(bootingStatus(booting(''))).toBe('Booting…');
    expect(bootingScreen(booting('image.syx', first))).toBe(
      `Booting ${name}… the first boot of a firmware can take a minute`,
    );
  });
  it('puts the error in the failure line', () => {
    expect(loadFailure('a.syx', 'bad image')).toBe('Loading a.syx failed: bad image');
  });
});
