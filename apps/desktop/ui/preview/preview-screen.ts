import { panels } from '@digi/workbench-core/data/faceplates.ts';
import { queryFrame, screenBytes } from './frame-from-png';

/** The fake panel keeps its display between polls, just like a device. Render only on input. */
export function createPreviewScreen(device: string) {
  const initial = queryFrame();
  const panel = panels[device];
  const held = new Set<number>();
  const positions = new Map<number, number>();
  let pixels: Uint8Array | undefined;
  let revision = 1;

  function display(label: string, value: string) {
    const canvas = document.createElement('canvas');
    canvas.width = 128;
    canvas.height = 64;
    const ctx = canvas.getContext('2d')!;
    ctx.fillRect(0, 0, 128, 64);
    ctx.fillStyle = 'white';
    ctx.font = 'bold 12px monospace';
    ctx.fillText(label, 4, 25, 120);
    ctx.font = '11px monospace';
    ctx.fillText(value, 4, 47, 120);
    pixels = screenBytes(ctx.getImageData(0, 0, 128, 64));
    revision++;
  }

  return {
    revision: () => revision,
    read: async () => (pixels ?? (await initial)).slice(),
    key(code: number, down: boolean) {
      if (down) held.add(code);
      else held.delete(code);
      const key = panel?.keys.find((k) => k.code === code);
      display(key?.legend || key?.icon || `Key ${code}`, down ? 'Pressed' : 'Released');
    },
    turn(encoder: number, delta: number) {
      const value = (positions.get(encoder) ?? 0) + delta;
      positions.set(encoder, value);
      display(panel?.knobs.find((k) => k.encoder === encoder)?.label || `Encoder ${encoder}`, String(value));
    },
    release() {
      if (held.size) display('Keys', 'Released');
      held.clear();
    },
  };
}
