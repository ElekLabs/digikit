import { expect, test, vi } from 'vitest';
import { DkError } from '@digi/dk-session';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { processorsOf } from '@digi/workbench-core/profile/processor.ts';
import type { EmuRpc } from '@digi/workbench-core/host/host.ts';
import { createCaptureCapabilities } from '../src/profile/capabilities';

const dspDevices = registry.devices.filter((device) => device.capabilities.dsp === 'sharc');
const peerDevice = registry.devices.find((device) => device.container.cf2 !== undefined)!;
const state = { on: false, generation: 0, counts: { sweeps: 0, gaps: 0, ignored: 0 } };

test.each(dspDevices)('$name supports block costs independent of the optional DSP sampler', async (device) => {
  const call = vi.fn(async () => ({ dsp_pc: false, dsp_active: true, main_pc: true }));
  const profileDsp = vi.fn(async () => state);
  const session = { profileDsp };
  const recorder = createCaptureCapabilities({ call: call as EmuRpc['call'] });
  const capability = await recorder.inspect(device.id, session, 'same', true);
  expect(processorsOf(device)).toEqual([
    { role: 'main', architecture: 'coldfire', addressUnit: 'byte' },
    { role: 'dsp', architecture: 'sharc', addressUnit: 'word' },
  ]);
  expect(capability).toMatchObject({ dspBlocks: true, dspHotspots: expect.stringContaining('emulator build') });
  expect(await recorder.inspect(device.id, session, 'same', true)).toEqual(capability);
  expect(profileDsp).toHaveBeenCalledTimes(1);
  expect(call).toHaveBeenCalledTimes(1);
});

test('an audio peer is a distinct processor and is not a DSP profiling target', async () => {
  const profileDsp = vi.fn(async () => state);
  const capabilities = await createCaptureCapabilities().inspect(peerDevice.id, { profileDsp }, 'peer', true);
  expect(processorsOf(peerDevice)).toEqual([
    { role: 'main', architecture: 'coldfire', addressUnit: 'byte' },
    { role: 'audio-peer', architecture: 'coldfire', addressUnit: 'byte' },
  ]);
  expect(capabilities.hasDsp).toBe(false);
  expect(capabilities.dspBlocks).toMatch(/no DSP/i);
  expect(profileDsp).not.toHaveBeenCalled();
});

test('distinguishes the inactive DSP from an older runtime without block statistics', async () => {
  const profileDsp = vi.fn(async () => {
    throw new DkError({ error: 'unknown command', kind: 'device', status: 'E_PROTO' });
  });
  const call = vi
    .fn()
    .mockResolvedValueOnce({ dsp_pc: true, dsp_active: false, main_pc: true })
    .mockResolvedValueOnce({ dsp_pc: true, dsp_active: true, main_pc: true });
  const recorder = createCaptureCapabilities({ call });
  const session = { profileDsp };
  expect(await recorder.inspect(dspDevices[0]!.id, session, 'off', true)).toMatchObject({
    dspBlocks: expect.stringMatching(/DSP is off/),
  });
  expect(profileDsp).not.toHaveBeenCalled();
  expect(await recorder.inspect(dspDevices[0]!.id, session, 'on', true)).toMatchObject({
    dspHotspots: true,
    dspBlocks: expect.stringMatching(/runtime does not support/),
  });
});
