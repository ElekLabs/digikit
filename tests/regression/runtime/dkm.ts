import { patchkit } from './patchkit.ts';
import { fxExpectationsWith, GAIN_PAGE_DEFAULTS, type FxExpectations, type ModuleRange } from './dkm-parse.ts';

export * from './dkm-parse.ts';

/** Offline oracles over a .dkm. The needs and the fit rule are the Rust ones (`patchkit needs|fit`). */

/** What a load of each file takes, from `patchkit needs --json`; throws for a file the loader would refuse. */
export async function needsOf(files: string[]): Promise<Record<string, number>[]> {
  const { stdout } = await patchkit(['needs', '--json', ...files]).catch((error: { stdout?: string }) => {
    if (error.stdout) return { stdout: error.stdout };
    throw error;
  });
  const out = JSON.parse(stdout) as Record<string, Record<string, number> & { error?: string }>;
  return files.map((file) => {
    if ('error' in out[file]!) throw new Error(`patchkit needs ${file}: ${out[file]!.error}`);
    return out[file]!;
  });
}

/** The fit rule (`patchkit fit --json -`) for NEEDS against CAPS; EMPTY makes CAPS the capacity of a runtime that loaded nothing. */
export async function fit(caps: unknown, needs: Record<string, number>[], empty = false) {
  const { stdout } = await patchkit(['fit', '--json', '-'], { input: JSON.stringify({ caps, needs, empty }) });
  return JSON.parse(stdout);
}

/** Relocation and row oracle of a one-effect module placed at BASE; the needs come from `patchkit needs` at run time. */
export async function fxExpectations(
  file: string,
  base: number,
  pageDefaults = GAIN_PAGE_DEFAULTS,
): Promise<FxExpectations> {
  return fxExpectationsWith(file, base, (await needsOf([file]))[0]!, pageDefaults);
}

/** The DSP ranges of the modules are pairwise disjoint. */
function rangesOk(mods: ModuleRange[]): boolean {
  const rs = mods.filter((m) => m.dsp_len).map((m) => [m.dsp_off, m.dsp_off + m.dsp_len] as const);
  rs.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  return rs.every((r, i) => i === rs.length - 1 || r[1] <= rs[i + 1]![0]);
}

/** Range and native fit oracles of the full FX chain: the last effect against the live capacity, all of them against an empty device. */
export async function chainCheck(snapshot: { modules: ModuleRange[]; capacity: unknown }, effectFiles: string[]) {
  const needs = await needsOf(effectFiles);
  return {
    ranges_ok: rangesOk(snapshot.modules),
    live: await fit(snapshot.capacity, [needs.at(-1)!]),
    empty: await fit(snapshot.capacity, needs, true),
  };
}
