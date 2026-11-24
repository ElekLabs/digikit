import type { DkProf } from '@digi/dk-session/dkprof';
import type { Host } from '@digi/workbench-core/host/host.ts';
import {
  readProfileHotspots,
  resolveProfileHotspots,
  verifiedProfileMaps,
  type Hotspot,
  type ProfileHotspots,
} from '@digi/workbench-core/profile/hotspots.ts';
import { readReference } from '@digi/workbench-core/profile/profile-core.ts';

export { sampledPcs, type Hotspot } from '@digi/workbench-core/profile/hotspots.ts';

export interface SavedProfile extends ProfileHotspots {
  shares?: Map<string, number>;
}

export function readSavedProfile(doc: unknown): SavedProfile {
  return { ...readProfileHotspots(doc), shares: readReference(doc) };
}

export async function resolveSavedHotspots(
  profile: SavedProfile,
  symbols?: Pick<NonNullable<Host['symbols']>, 'maps'>,
  prof?: () => Promise<Pick<DkProf, 'addrLookup'>>,
  library?: Pick<Host['library'], 'readMaps'>,
): Promise<SavedProfile['pcs']> {
  const { sha256, pcs, target, moduleHashes } = profile;
  if (!sha256 || !prof || !(pcs.cf.length || pcs.sharc.length)) return pcs;
  try {
    const [maps, modules] = await Promise.all([
      symbols?.maps(sha256) ?? [],
      target && moduleHashes.length
        ? (library?.readMaps(target).then(({ modules }) => modules.map(({ json }) => JSON.parse(json))) ?? [])
        : [],
    ]);
    if (!verifiedProfileMaps(profile, [...maps, ...modules]).length) return pcs;
    return resolveProfileHotspots(profile, [...maps, ...modules], await prof());
  } catch {
    return pcs;
  }
}

export function hotspotLabel({ addr, name }: Hotspot): string {
  const hex = `0x${addr.toString(16)}`;
  return name ? `${hex} ${name}` : hex;
}

export const fmtPct = (share: number) => `${(share * 100).toFixed(share > 0 && share < 0.001 ? 2 : 1)}%`;
