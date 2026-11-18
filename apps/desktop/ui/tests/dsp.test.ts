import { describe, expect, it } from 'vitest';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import {
  DSP_KEY,
  dspLine,
  dspOption,
  dspPending,
  hasDspChoice,
  readDspChoice,
  startupDsp,
  writeDspChoice,
} from '../src/dsp-core';

describe('DSP tick', () => {
  it('is offered by a device with a DSP or a second CPU, from the device data', () => {
    const offered = registry.devices.filter((d) => d.capabilities.dsp === 'sharc' || d.container.cf2 !== undefined);
    expect(offered.length).toBeGreaterThan(0);
    for (const d of registry.devices) expect(hasDspChoice(d)).toBe(offered.includes(d));
  });
  it('is pending when the tick differs from the running image', () => {
    expect(dspPending(true, { enabled: true, flag: null })).toBe(false);
    expect(dspPending(false, { enabled: true, flag: null })).toBe(true);
    expect(dspPending(true, { enabled: false, flag: null })).toBe(true);
    expect(dspPending(false, undefined)).toBe(false);
  });
  it('takes the launcher flag over the saved choice', () => {
    expect(startupDsp(true, { enabled: false, flag: false })).toBe(false);
    expect(startupDsp(false, { enabled: true, flag: true })).toBe(true);
    expect(startupDsp(false, { enabled: true, flag: null })).toBe(false);
    expect(startupDsp(true, undefined)).toBe(true);
  });
  it('says "DSP off" in Metrics', () => {
    expect(dspLine({ enabled: false, flag: null })).toBe('DSP off');
    expect(dspLine({ enabled: true, flag: null })).toBe('DSP on');
    expect(dspLine(undefined)).toBe('—');
  });
  it('keeps the saved choice: on unless saved off, and only an explicit write changes it', () => {
    const storage = memoryKeyValue();
    expect(readDspChoice(storage)).toBe(true);
    writeDspChoice(storage, false);
    expect(storage.data.get(DSP_KEY)).toBe('off');
    // the launcher's flag is adopted for the session (startupDsp) and never written: the saved choice stays
    expect(startupDsp(readDspChoice(storage), { enabled: true, flag: true })).toBe(true);
    expect(readDspChoice(storage)).toBe(false);
    writeDspChoice(storage, true);
    expect(readDspChoice(storage)).toBe(true);
  });
  it('reads "on" and ignores a write when the storage throws', () => {
    const broken = {
      get: () => {
        throw new Error('blocked');
      },
      set: () => {
        throw new Error('blocked');
      },
    };
    expect(readDspChoice(broken)).toBe(true);
    expect(() => writeDspChoice(broken, false)).not.toThrow();
  });
  it('sends the tick only for a device that offers the choice', () => {
    for (const d of registry.devices) expect(dspOption(d, false)).toBe(hasDspChoice(d) ? false : undefined);
    expect(dspOption(undefined, false)).toBe(false);
  });
});
