import { describe, expect, it } from 'vitest';
import { deviceOfSyx } from '@digi/workbench-core/patch/identify.ts';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import {
  booting,
  bootingScreen,
  bootingStatus,
  bootOnEmulatorPage,
  keepDeferredBoot,
  loadFailure,
  shouldBootOnStart,
} from '../src/boot-core';

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
  it('boots at start for a command-line file, or for Auto-load firmware with the emulator as target', () => {
    expect(shouldBootOnStart('a.syx', false, 'midi')).toBe(true);
    expect(shouldBootOnStart('a.syx', false, 'emulator')).toBe(true);
    expect(shouldBootOnStart(null, true, 'emulator')).toBe(true);
    expect(shouldBootOnStart(null, true, 'midi')).toBe(false);
    expect(shouldBootOnStart(null, true, 'none')).toBe(false);
    expect(shouldBootOnStart(null, false, 'emulator')).toBe(false);
    expect(shouldBootOnStart(undefined, false, 'emulator')).toBe(false);
  });
  it('boots a deferred start on the Emulator page with the emulator as target, once nothing runs', () => {
    expect(bootOnEmulatorPage(true, 'run', 'emulator', false, false)).toBe(true);
    expect(bootOnEmulatorPage(false, 'run', 'emulator', false, false)).toBe(false);
    expect(bootOnEmulatorPage(true, 'files', 'emulator', false, false)).toBe(false);
    expect(bootOnEmulatorPage(true, 'run', 'midi', false, false)).toBe(false);
    expect(bootOnEmulatorPage(true, 'run', 'emulator', true, false)).toBe(false);
    expect(bootOnEmulatorPage(true, 'run', 'emulator', false, true)).toBe(false);
  });
  it('drops a deferred start once a firmware runs or starts', () => {
    expect(keepDeferredBoot(false, false)).toBe(true);
    expect(keepDeferredBoot(true, false)).toBe(false);
    expect(keepDeferredBoot(false, true)).toBe(false);
  });
});
