// device-literals: allow
import type { SymbolsHost } from '@digi/workbench-core/host/host.ts';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import { createFirmwareSymbols } from '../src/firmware-symbols';

export const previewSymbolImage = { device: 'dt2', firmware: '1.17', sha256: 'a'.repeat(64) };

export function createPreviewSymbols(): SymbolsHost {
  const paths = {
    cf: '/Users/preview/symbols/coldfire.sqlite',
    sharc: '/Users/preview/symbols/sharc.sqlite',
    map: '/Users/preview/symbols/firmware.json',
  };
  let pickedMap = false;
  return createFirmwareSymbols(memoryKeyValue(), {
    async pick(kind) {
      if (kind === 'map') {
        if (pickedMap) return null;
        pickedMap = true;
      }
      return paths[kind];
    },
    async read(kind) {
      if (kind === 'sharc') throw new Error('The file is missing the SHARC symbols table.');
      const { device, firmware, sha256 } =
        kind === 'cf' ? previewSymbolImage : { device: 'dn2', firmware: '1.12', sha256: 'b'.repeat(64) };
      const json = JSON.stringify({
        format: 'dk-addrmap',
        version: 1,
        kind: 'dev',
        key: { device, firmware, image_sha256: sha256 },
        entries: [{ cpu: 'cf', addr: 4096, size: 32, name: 'preview_function', layer: 'fw' }],
      });
      return [{ device, firmware, sha256, json }];
    },
  });
}
