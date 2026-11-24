import type { AddrMapJson, Cpu, DkProf, LoadTableJson } from '@digi/dk-session/dkprof';
import { stockImageHash } from '@digi/dk-session/log-names';

export interface ReviewLink {
  path: string;
  url?: string;
  fragment: string;
  sha256: string;
  entry: number;
}

export interface ReviewIndex {
  cpu: Cpu;
  path: string;
  url?: string;
  entries: number[];
}

export interface Hotspot {
  addr: number;
  weight: number;
  share: number;
  name?: string;
  entry?: number;
  owner?: string;
  offset?: number;
  unit?: string;
  review?: ReviewLink;
  module_base?: number;
  module_entry?: number;
}

export interface ProfileHotspots {
  sha256?: string;
  target?: string;
  loads?: LoadTableJson;
  moduleHashes: string[];
  pcs: Record<Cpu, Hotspot[]>;
}

const fingerprint = (value: unknown) =>
  typeof value === 'string' && /^[a-f0-9]{64}$/i.test(value) ? value.toLowerCase() : undefined;

export function sampledPcs(raw: unknown[], cpu: Cpu, top = 10): Hotspot[] {
  const samples = new Map<number, number>();
  for (const value of raw) {
    const row = value as { cpu?: unknown; series?: unknown; addr?: unknown; count?: unknown } | null;
    if (!row || row.cpu !== cpu || (row.series !== undefined && row.series !== 'pc')) continue;
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

export function readProfileHotspots(doc: unknown, top?: number): ProfileHotspots {
  const { run, raw, clock } = (doc ?? {}) as {
    run?: { firmware_sha256?: unknown; target?: string; loads?: LoadTableJson; module_sha256?: unknown[] };
    raw?: unknown[];
    clock?: { unit?: string };
  };
  const rows = Array.isArray(raw) ? raw : [];
  const unit = typeof clock?.unit === 'string' ? clock.unit : undefined;
  const pcs = (cpu: Cpu) => {
    const samples = sampledPcs(rows, cpu, top);
    return unit === undefined ? samples : samples.map((sample) => ({ ...sample, unit }));
  };
  return {
    sha256: fingerprint(run?.firmware_sha256),
    target: run?.target,
    loads: Array.isArray(run?.loads?.modules) ? run.loads : undefined,
    moduleHashes: Array.isArray(run?.module_sha256) ? run.module_sha256.flatMap((v) => fingerprint(v) ?? []) : [],
    pcs: { cf: pcs('cf'), sharc: pcs('sharc') },
  };
}

export function verifiedProfileMaps(profile: ProfileHotspots, maps: AddrMapJson[]): AddrMapJson[] {
  if (!profile.sha256) return [];
  return maps.filter((map) => {
    if (map.kind !== 'module') return stockImageHash(map) === profile.sha256;
    const key = map.key as { dkm_sha256?: string } | undefined;
    const hash = fingerprint(key?.dkm_sha256);
    return hash !== undefined && profile.moduleHashes.includes(hash);
  });
}

export function resolveProfileHotspots(
  profile: ProfileHotspots,
  maps: AddrMapJson[],
  prof: Pick<DkProf, 'addrLookup'>,
): ProfileHotspots['pcs'] {
  const { pcs, loads, sha256 } = profile;
  const verified = verifiedProfileMaps(profile, maps);
  if (!sha256 || !verified.length || !(pcs.cf.length || pcs.sharc.length)) return pcs;
  const addrs = [
    ...pcs.cf.map(({ addr }) => ({ cpu: 'cf' as const, addr })),
    ...pcs.sharc.map(({ addr }) => ({ cpu: 'sharc' as const, addr })),
  ];
  const { results } = prof.addrLookup({ maps: verified, loads, addrs });
  // The resolver uses the last load for an id, including CLI overrides.
  const loaded = new Map(loads?.modules.map((module) => [module.id, module]));
  const reviews = verified.flatMap((map) => (map.review as ReviewIndex | undefined) ?? []);
  const named = (rows: Hotspot[], start: number) =>
    rows.map((row, i) => {
      const hit = results[start + i];
      if (!hit?.text) return row;
      const output: Hotspot = { ...row, name: hit.text };
      const entry = hit.sym_addr;
      if (entry === null || hit.offset === null) return output;
      output.entry = entry;
      output.offset = hit.offset;
      output.owner = hit.module ?? (hit.layer === 'our' ? 'runtime' : 'firmware');
      const load = hit.module && loaded.get(hit.module);
      if (load) {
        const base = hit.cpu === 'sharc' ? load.dsp_base : load.base;
        if (base !== undefined) {
          output.module_base = base;
          output.module_entry = entry - base;
        }
      }
      if (hit.layer === 'fw') {
        const index = reviews.find((r) => r.cpu === hit.cpu && r.entries.includes(entry));
        if (index)
          output.review = {
            path: index.path,
            ...(index.url ? { url: index.url } : {}),
            fragment: 'r-0x' + entry.toString(16),
            sha256,
            entry,
          };
      }
      return output;
    });
  return { cf: named(pcs.cf, 0), sharc: named(pcs.sharc, pcs.cf.length) };
}
