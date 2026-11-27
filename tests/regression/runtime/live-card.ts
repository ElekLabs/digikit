import fs from 'node:fs';
import path from 'node:path';
import { cachedCard, cardKey, fileSha, folderSha } from './card-cache.ts';
import { buildCard } from './card-store.ts';
import { deviceProfile } from './device-data.ts';
import { resolveInputs, repoRoot, type Device } from '@digi/test-support';

/**
 * Stable period of a live session's card, in DSP blocks (1500 blocks are one second). A card with no selection record gives the
 * runtime's default of 30 s (`STABLE_BLOCKS`): a test that boots three times in a row never reaches it, and the third boot is safe mode.
 * One second still lets every boot reach the UI loop (the period counts from the first UI-loop pass) and keeps the boot counter raised
 * long enough to be seen, so `ui_pass` clears it as it does in a real session. The record keeps the value through every later selection
 * (`next_record`).
 */
export const LIVE_STABLE_BLOCKS = 1500;

/** The sample folder that live sessions mount on a device with a path filesystem (made by pk_harness), or undefined for another device. */
function liveSampleFolder(device: Device): string | undefined {
  if (deviceProfile(device).capabilities.card !== 'path-fs') return undefined;
  const dir = path.join(repoRoot(), 'out/runs/patchkit/samples');
  if (!fs.existsSync(dir)) {
    throw new Error(
      `sample card folder missing: generate ${dir} (tools.emulator.card_check.make_samples, run by pk_harness)`,
    );
  }
  return dir;
}

/**
 * The card of a live session: the empty store, the sample folder (a device with a path filesystem, with the boot project of its
 * firmware, as `card.load` builds it) and a selection record that carries `stableBlocks` (0: the runtime's default of 30 s). The folder goes to `plusdrive build` as
 * it is, so it is scanned by the same `scan_folder` as `card.load`. Content-named in the card cache, so equal inputs give one card.
 */
export async function liveCard(device: Device, firmware: string, stableBlocks = LIVE_STABLE_BLOCKS): Promise<string> {
  const folder = liveSampleFolder(device);
  const syx = folder ? resolveInputs(device, firmware).syx : undefined;
  const spec = { selection: [] as string[], stableBlocks, folder, syx };
  const key = cardKey(device, {
    ...spec,
    folder: folder ? folderSha(folder) : undefined,
    syx: syx ? fileSha(syx) : undefined,
  });
  return cachedCard(key, (work) => buildCard(device, { ...spec, dir: work, name: 'card' }));
}
