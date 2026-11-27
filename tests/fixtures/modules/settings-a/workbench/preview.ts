// The preview's answer to BootAtInit: the module reads its "boot" setting once in init, so the preview reads the store when the instance
// loads (the setting as it was before the page changed it).
import { memoryQueryService, type MemoryQueryService } from '@digi/module-ui';
import { SettingsA } from './gen/settings_a_pb.js';

export default function previewQueries(loaded: (settingId: string) => Uint8Array | undefined): MemoryQueryService {
  const boot = loaded('boot');
  return memoryQueryService(SettingsA, {
    bootAtInit: () => ({ present: boot !== undefined, value: boot ? new TextDecoder().decode(boot) : '' }),
  });
}
