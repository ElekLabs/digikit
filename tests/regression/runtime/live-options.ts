import type { Device } from '@digi/test-support';
import { cachedRunDir, daemonBinary, FIRMWARE } from './devices.ts';
import { liveCard } from './live-card.ts';

/**
 * The session options of a live patched session. It starts on the cached card of `liveCard`; `blankCard: true` leaves the card to the
 * daemon (a test of the stock initialisation of a blank card); `stableBlocks` replaces the stable period of the card's selection record (0: the
 * runtime's default of 30 s, for a test of the boot counter). Mirror sessions and `fresh: true` keep a fresh run dir (the mirror folder
 * must start empty); every session shares the snapshot store, which is keyed by what the card holds, not by the run dir.
 */
export function liveOptions(
  device: Device,
  extra: { patchSet?: string; mirror?: boolean; fresh?: boolean; blankCard?: boolean; stableBlocks?: number } = {},
) {
  const binary = daemonBinary(device);
  const { fresh, blankCard, stableBlocks, ...rest } = extra;
  const claim = cachedRunDir(device);
  return {
    device,
    firmware: FIRMWARE[device],
    binary,
    ...(blankCard ? {} : { cardImage: () => liveCard(device, FIRMWARE[device], stableBlocks) }),
    ...rest,
    runDir: extra.mirror || fresh ? undefined : claim.dir,
    snapshotDir: claim.snapshotDir,
  };
}
