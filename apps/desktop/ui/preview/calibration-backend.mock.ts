// The Calibration section's backend in the preview (replaces ../src/calibration-backend.ts): the profile list is the repository's own files, a run
// plays made-up captures with the runner's progress lines, and the fit is the same arithmetic as dkprof's (fit minimising the squared relative error,
// budget the worst ratio) over the worst-case pairs, without wasm. The numbers show layout, not measurements. `cal=slow` stretches a capture to 3 s.
import { loadCalibrationFiles, parseRunLine, worstValues } from '@digi/workbench-core/profile/calibration-view.ts';
import type { CalibrationDoc, CalProfileDoc } from '@digi/workbench-core/profile/calibration-view.ts';
import type { Calibrator, CalibrationDeps } from '../src/calibration-backend';

const slow = new URLSearchParams(location.search).get('cal') === 'slow';

/** A made-up capture: a stock case with no rows, and module cases with one row each. */
function capture(emu: boolean, caseId: string, n: number): Record<string, unknown> {
  const k = (caseId.charCodeAt(0) % 7) + 1;
  const insn = 9_000 * k;
  const v = emu ? insn : Math.round(insn * (1.28 + 0.02 * k) + n * 3);
  return {
    schema: 'dk.profile/1',
    run: { id: `${emu ? 'emu' : 'dev'}-20261120T120000-${caseId}-${n}`, case_id: caseId, target: 'dn2' },
    dsp: { unit: emu ? 'insn' : 'cycles', blocks: 400, max_stock_block_delta: emu ? 0 : 61_000 },
    dsp_rows:
      caseId === 'stock'
        ? []
        : [{ row: k, kind: 'machine', calls: 400, sum_cycles: v * 380, max_call: Math.round(v * 0.8), max_block: v }],
  };
}

export function createCalibrator(deps: CalibrationDeps): Calibrator {
  const files = loadCalibrationFiles(deps.texts());
  return {
    profiles: () =>
      files.profiles.map((p) => ({
        name: p.name,
        description: p.description,
        seconds: p.cases.length * p.repeats * (p.warmup_s + p.capture_s),
        profile: p,
      })),
    async run(opts) {
      const profile = files.profiles.find((p) => p.name === opts.profile)!;
      const emulator = !!deps.rpc;
      const captures: { caseId: string; repeat: number; file: string; profile: Record<string, unknown> }[] = [];
      const wait = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
      for (const c of profile.cases.slice(0, 3)) {
        for (let n = 1; n <= profile.repeats; n++) {
          if (opts.signal?.aborted)
            return { profile: profile.name, emulator, captures, skipped: [], failed: [], stopped: true };
          opts.onEvent?.(parseRunLine(`case ${c.id}: capture ${n} of ${profile.repeats}`));
          await wait(slow ? 3000 : 250);
          const cap = {
            caseId: c.id,
            repeat: n,
            file: `${c.id}-${n}.profile.json`,
            profile: capture(emulator, c.id, n),
          };
          captures.push(cap);
          opts.onCapture?.(cap);
        }
      }
      return { profile: profile.name, emulator, captures, skipped: [], failed: [], stopped: false };
    },
    async fit(emu, dev) {
      const m = worstValues(emu.captures as CalProfileDoc[]);
      const d = worstValues(dev.captures as CalProfileDoc[]);
      const pairs = [...m.keys()]
        .filter((k) => d.has(k))
        .map((key) => {
          const [kase, id, stat] = key.split('|') as [string, string, string];
          return { case: kase, id, stat, model: m.get(key)!.value, device: d.get(key)!.value };
        });
      const sx = pairs.reduce((t, p) => t + p.model / p.device, 0);
      const sxx = pairs.reduce((t, p) => t + (p.model / p.device) ** 2, 0);
      const fit = sx / sxx;
      const budget = Math.max(...pairs.map((p) => p.device / p.model));
      const full = pairs.map((p) => ({
        ...p,
        ratio: p.device / p.model,
        error: (fit * p.model - p.device) / p.device,
      }));
      const under = Math.max(0, ...full.map((p) => -p.error));
      const doc: CalibrationDoc = {
        schema: 'dk.calibration/1',
        key: { device: 'dn2', device_unit: 'cycles' },
        dsp: {
          factor: { fit, budget },
          error: {
            n: pairs.length,
            mean: 0,
            worst_under: under,
            loo_worst: pairs.length > 2 ? under * 1.2 : null,
            loo_budget_under: pairs.length > 2 ? 0 : null,
          },
          steady: { n: 0, mean: 0, worst_under: 0 },
          stock: { avg: 58_000, max: 61_000, cases: 1 },
          pairs: full,
        },
        refused: [],
        warnings: ['block_cycles not given: the share of a block is not computed'],
      };
      return doc;
    },
  };
}
