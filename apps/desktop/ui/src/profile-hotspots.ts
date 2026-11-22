import type { Cpu, DkProf } from '@digi/dk-session/dkprof';
import { stockImageHash } from '@digi/dk-session/log-names';
import type { Host } from '@digi/workbench-core/host/host.ts';
import { readReference } from '@digi/workbench-core/profile/profile-core.ts';

export interface Hotspot {
  addr: number;
  weight: number;
  share: number;
  name?: string;
}

export function sampledPcs(raw: unknown[], cpu: Cpu, top = 10): Hotspot[] {
  const samples = new Map<number, number>();
  for (const value of raw) {
    const row = value as { cpu?: unknown; series?: unknown; addr?: unknown; count?: unknown } | null;
    if (!row || row.cpu !== cpu || row.series !== 'pc') continue;
    const { addr, count } = row;
    if (typeof addr !== 'number' || !Number.isInteger(addr) || addr < 0 || addr > 0xffffffff) continue;
    if (typeof count !== 'number' || !Number.isFinite(count) || count <= 0) continue;
    samples.set(addr, (samples.get(addr) ?? 0) + count);
  }
  const total = [...samples.values()].reduce((sum, count) => sum + count, 0);
  return [...samples]
    .sort(([addrA, countA], [addrB, countB]) => countB - countA || addrA - addrB)
    .slice(0, top)
    .map(([addr, weight]) => ({ addr, weight, share: weight / total }));
}

export interface SavedProfile {
  sha256?: string;
  shares?: Map<string, number>;
  pcs: { cf: Hotspot[]; sharc: Hotspot[] };
}

export function readSavedProfile(doc: unknown): SavedProfile {
  const shares = readReference(doc);
  const { run, raw } = doc as { run?: { firmware_sha256?: unknown }; raw?: unknown };
  const hash = run?.firmware_sha256;
  const sha256 = typeof hash === 'string' && /^[a-f0-9]{64}$/i.test(hash) ? hash.toLowerCase() : undefined;
  const rows = Array.isArray(raw) ? raw : [];
  const pcs = { cf: sampledPcs(rows, 'cf'), sharc: sampledPcs(rows, 'sharc') };
  return { sha256, shares, pcs };
}

export async function resolveSavedHotspots(
  { sha256, pcs }: SavedProfile,
  symbols?: Pick<NonNullable<Host['symbols']>, 'maps'>,
  prof?: () => Promise<Pick<DkProf, 'addrLookup'>>,
): Promise<SavedProfile['pcs']> {
  if (!sha256 || !symbols || !prof || !(pcs.cf.length || pcs.sharc.length)) return pcs;
  try {
    const dkprof = await prof();
    const maps = (await symbols.maps(sha256)).filter((map) => stockImageHash(map) === sha256);
    if (!maps.length) return pcs;
    const addrs = [
      ...pcs.cf.map(({ addr }) => ({ cpu: 'cf' as const, addr })),
      ...pcs.sharc.map(({ addr }) => ({ cpu: 'sharc' as const, addr })),
    ];
    const { results } = dkprof.addrLookup({ maps, addrs });
    const named = (rows: Hotspot[], start: number) =>
      rows.map((row, i) => {
        const name = results[start + i]?.text;
        return name ? { ...row, name } : row;
      });
    return { cf: named(pcs.cf, 0), sharc: named(pcs.sharc, pcs.cf.length) };
  } catch {
    return pcs;
  }
}

export function hotspotLabel({ addr, name }: Hotspot): string {
  const hex = `0x${addr.toString(16)}`;
  return name ? `${hex} ${name}` : hex;
}
