// Labels shared by the module cards. No manifest imports here, so a client island can use them.
import { deviceIds, deviceShort, type DeviceId } from '@digi/workbench-core/data/device-profiles.ts';

export type Category = 'fx' | 'lfo' | 'machines' | 'core' | 'test' | 'ui' | 'tools';

export const CATEGORY_LABEL: Record<Category, string> = {
  fx: 'Effect',
  lfo: 'LFO shape',
  machines: 'Machine',
  core: 'Framework',
  test: 'Test fixture',
  ui: 'Interface',
  tools: 'Tool',
};

export const DEVICE_SHORT: Record<DeviceId, string> = Object.fromEntries(deviceIds.map((id) => [id, deviceShort(id)]));
