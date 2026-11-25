import type { DkProf } from '@digi/dk-session/dkprof';
import type { Host } from '@digi/workbench-core/host/host.ts';
import {
  readProfileHotspots,
  resolveProfileHotspots,
  verifiedProfileMaps,
  type Hotspot,
  type ProfileHotspots,
} from '@digi/workbench-core/profile/hotspots.ts';

export { sampledPcs, type Hotspot } from '@digi/workbench-core/profile/hotspots.ts';

export interface SavedProfile extends ProfileHotspots {
  document: unknown;
}

export function readSavedProfile(doc: unknown): SavedProfile {
  return { ...readProfileHotspots(doc), document: doc };
}

interface SharcCostRow {
  self_count: number;
  per_block?: number;
  share: number;
}

interface SharcCosts {
  functions: (SharcCostRow & { name: string; module?: string; layer?: string })[];
  loops?: (SharcCostRow & { function?: string; module: string; start: number; end: number; count: string })[];
}

export interface SavedEvidence {
  pcs: SavedProfile['pcs'];
  sharc?: SharcCosts & { unit: string };
}

type ProfileReader = Pick<DkProf, 'addrLookup'> & Partial<Pick<DkProf, 'profileReport'>>;

export async function resolveSavedHotspots(
  profile: SavedProfile,
  symbols?: Pick<NonNullable<Host['symbols']>, 'maps'>,
  prof?: () => Promise<ProfileReader>,
  library?: Pick<Host['library'], 'readMaps'>,
): Promise<SavedEvidence> {
  const { sha256, pcs, target, moduleHashes, rig } = profile;
  const result: SavedEvidence = { pcs };
  if ((!sha256 && !rig) || !prof || !(pcs.cf.length || pcs.sharc.length)) return result;
  try {
    const [maps, modules] = await Promise.all([
      sha256 ? (symbols?.maps(sha256) ?? []) : [],
      sha256 && target && moduleHashes.length
        ? (library?.readMaps(target).then(({ modules }) => modules.map(({ json }) => JSON.parse(json))) ?? [])
        : [],
    ]);
    const verified = verifiedProfileMaps(profile, [...maps, ...modules]);
    if (!verified.length) return result;
    const reader = await prof();
    result.pcs = resolveProfileHotspots(profile, verified, reader);
    if (pcs.sharc.length && reader.profileReport) {
      const report = reader.profileReport({
        profile: profile.document,
        maps: verified,
        analyses: rig?.analyses,
        top: 8,
      }) as { unit: string; series: (SharcCosts & { cpu: string; series: string })[] };
      const sharc = report.series.find((row) => row.cpu === 'sharc' && row.series === 'pc');
      if (sharc) result.sharc = { ...sharc, unit: report.unit };
    }
  } catch {
    // Keep the raw PCs if the maps fail, or the named PCs if the aggregate report fails.
  }
  return result;
}

export function hotspotLabel({ addr, name }: Hotspot): string {
  const hex = `0x${addr.toString(16)}`;
  return name ? `${hex} ${name}` : hex;
}

export const fmtPct = (share: number) => `${(share * 100).toFixed(share > 0 && share < 0.001 ? 2 : 1)}%`;
