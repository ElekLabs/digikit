// Distinct pitches make a stale frame after a target switch visible in the preview.
import type { AnalyzerHost } from '@digi/workbench-core/host/host.ts';

export function createPreviewAnalyzer(): AnalyzerHost {
  const rate = 48000;
  let token = 0;
  let pitch = 0;
  let offset = 0;
  return {
    async open(source) {
      pitch = source === 'remote' ? 880 : 440;
      offset = 0;
      return ++token;
    },
    async read(id) {
      if (id !== token || !pitch) return null;
      const left = Array.from({ length: 2048 }, (_, i) => 0.5 * Math.sin((2 * Math.PI * pitch * (i + offset)) / rate));
      offset += left.length;
      return { generation: id, rate, left, right: left.map((sample) => sample * 0.5) };
    },
    async close(id) {
      if (id === token) pitch = 0;
    },
  };
}
