import { golden, pollUntil, repoRoot, type Device } from '@digi/test-support';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect } from 'vitest';
import { bootInfo } from '@digi/regression/runtime/guest-mailbox.ts';
import { moduleStorePath } from '@digi/regression/runtime/install.ts';
import type { CardRig } from '@digi/regression/runtime/card-rig.ts';
import { decodeSettings, settingsMap, settingsRecord } from '@digi/regression/runtime/project-kit.ts';
import type { TestRuntime } from '@digi/regression/support.ts';

/** Pinned frames of the MOD page (`golden` of test-support; written with UPDATE_GOLDENS=1 after the captured picture was looked at). */
export const GOLDENS = fileURLToPath(new URL('./goldens.json', import.meta.url));

/** `golden(GOLDENS, name, hash)`; with UPDATE_GOLDENS=1 the captured frame `png` is also copied to `out/runs/goldens/<golden file stem>/<name>.png` to look at what is pinned. */
export function pinFrame(name: string, hash: string, png: string): void {
  if (process.env.UPDATE_GOLDENS === '1') {
    const dest = path.join(repoRoot(), 'out', 'runs', 'goldens', path.basename(GOLDENS, '.json'), `${name}.png`);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(png, dest);
    console.info(`pinned frame ${name}: ${dest}`);
  }
  golden(GOLDENS, name, hash);
}

/** The shape fixtures: RAMP (`test/lfo-shape`) first, STEP (`test/lfo-shape-b`) second, so the service gives them the wave numbers 7 and 8. */
export const SHAPE_A = 'test/lfo-shape';
export const SHAPE_B = 'test/lfo-shape-b';
export const storePath = (device: Device, id: string) => moduleStorePath(device, id);
/** The settings key of the service's sticky map (`records::SETTING`): 8 words, big endian, `hash24 << 8 | wave number`. */
export const SHAPES_KEY = 'lfo.shapes';

/** Boot a card that holds `ids` (bound `lfo.dks` goes on it with them) and selects them in this order; every module must load. */
export async function bootWith(h: CardRig, tag: string, ids: string[]): Promise<TestRuntime> {
  const rt = await h.load(tag, { modules: ids, selection: ids.map((id) => storePath(h.device, id)) });
  const info = await bootInfo(rt.session, h.plan);
  expect(info.results.slice(0, ids.length), `${ids.join(' and ')} load`).toEqual(ids.map(() => 0));
  return rt;
}

/** The words of the `lfo.shapes` setting once the card holds them (the service saves after a load; the runtime writes the record after its 2 s delay). */
export async function shapeWords(h: CardRig, rt: TestRuntime): Promise<number[]> {
  const rec = await pollUntil(
    rt.clock,
    async () => {
      try {
        return await h.usb(rt, settingsRecord);
      } catch {
        return null; // no record yet
      }
    },
    (r) => r !== null && (r.flags & 1) === 0 && r.copy !== 2,
    { withinMs: 10_000, intervalMs: 250, message: 'the card holds the settings record' },
  );
  const hex = settingsMap(decodeSettings(rec!.raw))[SHAPES_KEY];
  expect(hex, `the settings record has ${SHAPES_KEY}`).toBeDefined();
  const bytes = Buffer.from(hex!, 'hex');
  expect(bytes.length, 'the sticky map is 8 words').toBe(32);
  return Array.from({ length: 8 }, (_, i) => bytes.readUInt32BE(4 * i));
}
