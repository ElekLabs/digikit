import type { Device, Session } from '@digi/test-support';
import { portFile } from './runtime/device-data.ts';

export interface FreeRegion {
  name: string;
  lo: number;
  hi: number;
}

/** Free ColdFire regions of ports/<device>-<firmware>.toml: every `[[region]]` with
 * `cpu = "cf"` (e.g. `cf.flash_cave`, `cf.expansion`), minus its `excludes` (`[lo, hi]` pairs).
 * `hi` is the file's `end`. */
export function freeRegions(device: Device, firmware: string): FreeRegion[] {
  const file = `${device}-${firmware}.toml`;
  const out: FreeRegion[] = [];
  for (const { name, cpu, start, end, excludes = [] } of portFile(device, firmware).region ?? []) {
    if (cpu !== 'cf') continue;
    if (name === undefined || start === undefined || end === undefined) {
      throw new Error(`${file}: malformed region block`);
    }
    let lo = start;
    for (const [xl, xh] of [...excludes].sort((a, b) => a[0] - b[0])) {
      if (xl > lo) out.push({ name, lo, hi: xl });
      lo = Math.max(lo, xh);
    }
    if (lo < end) out.push({ name, lo, hi: end });
  }
  if (out.length === 0) throw new Error(`${file}: no cf regions found`);
  return out;
}

/** Arm the guard on the free regions of the device and firmware; no PC is allowed (stock firmware). */
export async function armFreeRegionGuard(session: Session): Promise<void> {
  const { device, firmware } = session.options;
  const protect = freeRegions(device, firmware).map((r) => ({ lo: r.lo, hi: r.hi, label: r.name }));
  await session.client.guardSet({ protect, allow_pc: [] });
}

/** Fails when the guard saw a write into a free region; lists the first hits. */
export async function expectNoGuardHits(session: Session): Promise<void> {
  const report = await session.client.guardReport();
  if (report.total === 0) return;
  const hex = (n: number) => `0x${n.toString(16)}`;
  const lines = report.hits
    .slice(0, 8)
    .map((h) => `${h.label} pc=${hex(h.pc)} addr=${hex(h.addr)} size=${h.size} icount=${h.icount}`);
  throw new Error(`guard: ${report.total} write(s) into free regions:\n${lines.join('\n')}`);
}
