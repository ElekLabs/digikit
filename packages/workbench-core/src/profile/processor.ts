import type { DeviceRecord } from '@digi/devices';

export type Processor = Readonly<{
  role: 'main' | 'dsp' | 'audio-peer';
  architecture: 'coldfire' | 'sharc';
  addressUnit: 'byte' | 'word';
}>;

export const processorLabels = {
  main: 'Main CPU',
  dsp: 'DSP',
  'audio-peer': 'Audio peer',
} as const satisfies Record<Processor['role'], string>;

/** The device topology is independent of how it is being run or profiled. */
export function processorsOf(device?: Pick<DeviceRecord, 'capabilities' | 'container'>): readonly Processor[] {
  return [
    { role: 'main', architecture: 'coldfire', addressUnit: 'byte' },
    ...(device?.capabilities.dsp === 'sharc'
      ? [{ role: 'dsp', architecture: 'sharc', addressUnit: 'word' } as const]
      : []),
    ...(device?.container.cf2 !== undefined
      ? [{ role: 'audio-peer', architecture: 'coldfire', addressUnit: 'byte' } as const]
      : []),
  ];
}
