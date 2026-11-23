import type { SampleSourceFiles } from '../src/samples/source-files';
import { SAMPLES_ROOT, control, delay, listLocal, localKindOf, localSizeOf, sim } from './sim';

export function createSampleSourceFiles(): SampleSourceFiles {
  return {
    pick: async () => SAMPLES_ROOT,
    async list(_root, path, purpose) {
      if (purpose !== 'walk' && sim.folder === 'none') return undefined;
      if (purpose === 'read') {
        control.counts.localList++;
        await delay(60);
      } else if (purpose === 'poll') {
        control.counts.localPoll++;
      } else {
        await delay(20);
      }
      return listLocal(path);
    },
    async read(_root, path, head) {
      const size = localSizeOf(path);
      const out = new Uint8Array(head === undefined ? size : Math.min(size, head));
      const kind = localKindOf(path);
      if (kind === 'converted' && out.length >= 12) {
        out.set(new TextEncoder().encode('RIFF'), 0);
        out.set(new TextEncoder().encode('WAVE'), 8);
      } else if (kind === 'native' && out.length >= 64) {
        const header = new DataView(out.buffer);
        header.setUint32(4, size - 64);
        header.setUint32(8, 48000);
        out[0x14] = 0x7f;
      }
      return out;
    },
  };
}
