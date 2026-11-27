import fs from 'node:fs';
import path from 'node:path';
import { repoRoot, type Device } from '@digi/test-support';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { expect, test } from 'vitest';
import { moduleManifests, resolveModule } from '@digi/regression/runtime/memaudit-kit.ts';
import { bootWith, shapeWords } from './lfo-cards.ts';
import {
  evalTable,
  fitShape,
  LfoRig,
  OK,
  PAIR_MARGIN,
  STOCK_EVAL,
  WAVE,
  type Fit,
  type Sample,
} from './lfo-rig.ts';

/**
 * `mise run lfo:report -- <author/name> [--device dn2|dt2]` (tools/devtools/lfo_report.sh sets DIGI_LFO_MODULE and DIGI_LFO_DEVICE): the output report of an LFO shape module.
 * It is no gate test: it is not registered in `runtimeAcceptance` and joins no gate (the `.manual.` name keeps it out of the Nx selection). Without DIGI_LFO_MODULE it is skipped.
 * After it writes all outputs it fails when a shape has the verdict FAIL or SCALE (CLOSE is a warning).
 *
 * The module is loaded with the bound lfo service. For each of its shapes the report sets WAVE to the shape's number by MIDI CC, lets the LFO of track 1 run into one
 * destination word (`LfoRig.run`, the setup of lfo-shape-output.device.test.ts), reads the output over several periods in emulated time and fits it (`fitShape`, no phase or
 * rate needed) against the shape's own table, every other table of the module and the stock waves. The tables come from `SHAPES` in the module's `src/module.c`, the wave
 * numbers from the service's sticky map (`lfo.shapes`) matched with the keys of `build/gen/module_ids.h`. The report is a table on stdout (and in summary.txt), one SVG per shape and report.json.
 */
const MODULE = process.env.DIGI_LFO_MODULE;
const ONLY = process.env.DIGI_LFO_DEVICE as Device | undefined;
const READS = 100;
const GAP_MS = 12;
/** The peak-to-peak swing of a full-scale shape (-1 to +1) on the rig's destination: DEPTH CC 80 is +-0x2000 (lfo-rig.ts). */
const FULL_SWING = 0x4000;
const STOCK_NAME: Record<number, string> = { [WAVE.TRI]: 'TRI', [WAVE.SINE]: 'SINE', [WAVE.SQR]: 'SQR', [WAVE.SAW]: 'SAW', [WAVE.RAMP]: 'RAMP' };

interface ShapeSource {
  name: string;
  key: number;
  samples: number[];
}

function moduleDir(id: string): string {
  const stack = [path.join(repoRoot(), 'modules')];
  while (stack.length > 0) {
    const dir = stack.pop()!;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory() && !['node_modules', 'build', 'src', 'target'].includes(entry.name)) stack.push(full);
      else if (entry.name === 'module.toml' && new RegExp(`^id\\s*=\\s*"${id}"`, 'm').test(fs.readFileSync(full, 'utf8'))) return dir;
    }
  }
  throw new Error(`no module.toml with id ${id} below modules/`);
}

/** The shapes of `SHAPES` in `src/module.c` (name and 32 samples) with their keys (`KEYS` and `DK_KEY_<ID>` of `build/gen/module_ids.h`), in record order. */
function readShapes(dir: string): ShapeSource[] {
  const c = fs.readFileSync(path.join(dir, 'src/module.c'), 'utf8');
  const ids = fs.readFileSync(path.join(dir, 'build/gen/module_ids.h'), 'utf8');
  const defs = new Map([...ids.matchAll(/#define (DK_KEY_\w+) (0x[0-9a-f]+)u/g)].map((m) => [m[1]!, Number(m[2])] as const));
  const keyList = /KEYS\[\]\s*=\s*\{([^}]*)\}/.exec(c)?.[1];
  if (keyList === undefined) throw new Error('module.c has no KEYS table');
  const keys = [...keyList.matchAll(/DK_KEY_\w+/g)].map((m) => {
    const key = defs.get(m[0]);
    if (key === undefined) throw new Error(`${m[0]} is not in build/gen/module_ids.h: run mise run gen:modules`);
    return key;
  });
  const table = /SHAPES\[\]\s*=\s*\{([\s\S]*?)\n\};/.exec(c)?.[1];
  if (table === undefined) throw new Error('module.c has no SHAPES table');
  const shapes = [...table.matchAll(/\{\s*\{\s*'(.)',\s*'(.)',\s*'(.)',\s*'(.)'\s*\},\s*\{([^}]*)\}/g)].map((m, i) => ({
    name: m.slice(1, 5).join(''),
    key: keys[i]!,
    samples: m[5]!.split(',').map((x) => x.trim()).filter(Boolean).map(Number),
  }));
  if (shapes.length !== keys.length || shapes.some((s) => s.samples.length !== 32 || s.samples.some(Number.isNaN)))
    throw new Error(`module.c: ${shapes.length} shapes of 32 samples expected for ${keys.length} keys`);
  return shapes;
}

interface Candidate {
  label: string;
  eval: (phase: number) => number;
}

interface Row {
  name: string;
  number: number;
  own: number;
  closest: { label: string; err: number };
  fits: Record<string, number>;
  min: number;
  max: number;
  /** The measured peak-to-peak of the output as a fraction of the full swing. */
  swing: number;
  /** The same for the declared table: (max - min) / 65535. */
  tableSwing: number;
  verdict: string;
  svg: string;
  samples: Sample[];
}

/** The measured swing may differ from the table swing by this fraction of the full swing (measured runs differ by at most 2 points from read spacing). */
const SCALE_TOLERANCE = 0.05;

/** The fit is shape-only (both curves are rescaled to 0..1); the scale check compares the swings. FAIL and CLOSE come before SCALE. */
function verdict(own: number, others: [string, number][], swing: number, tableSwing: number): string {
  if (own >= OK) return `FAIL: output does not follow the table (error ${own.toFixed(3)} >= ${OK})`;
  const near = others.filter(([, e]) => e <= own + PAIR_MARGIN).map(([l]) => l);
  if (near.length > 0) return `CLOSE to ${near.join(', ')}: tables too similar to tell apart (margin ${PAIR_MARGIN})`;
  if (Math.abs(swing - tableSwing) > SCALE_TOLERANCE)
    return `SCALE: swing ${Math.round(swing * 100)}% differs from the table's ${Math.round(tableSwing * 100)}% by more than ${SCALE_TOLERANCE * 100} points`;
  return 'OK';
}

/** Measured samples over the declared table, both normalised to 0..1 as the fit does, the samples at the phase of the best fit. */
function plot(row: Omit<Row, 'svg'>, table: number[], fit: Fit): string {
  const W = 640;
  const H = 300;
  const m = { l: 44, r: 12, t: 34, b: 28 };
  const x = (p: number) => m.l + p * (W - m.l - m.r);
  const y = (u: number) => H - m.b - u * (H - m.t - m.b);
  const lo = Math.min(...row.samples.map((s) => s.v));
  const hi = Math.max(...row.samples.map((s) => s.v));
  const curve = Array.from({ length: 513 }, (_, k) => evalTable(table, Math.floor((k / 512) * 2 ** 32) >>> 0));
  const clo = Math.min(...curve);
  const chi = Math.max(...curve);
  const path = curve.map((v, k) => `${k === 0 ? 'M' : 'L'}${x(k / 512).toFixed(1)} ${y((v - clo) / (chi - clo)).toFixed(1)}`).join(' ');
  const t0 = row.samples[0]!.t;
  const dots = row.samples
    .map((s) => {
      const c = fit.phi + fit.f * (s.t - t0);
      return `<circle cx="${x(c - Math.floor(c)).toFixed(1)}" cy="${y((s.v - lo) / (hi - lo)).toFixed(1)}" r="2.5" fill="#d1495b" fill-opacity="0.7"/>`;
    })
    .join('');
  const grid = [0, 0.5, 1].map((u) => `<line x1="${m.l}" x2="${W - m.r}" y1="${y(u)}" y2="${y(u)}" stroke="#ccc"/><text x="${m.l - 6}" y="${y(u) + 4}" text-anchor="end" font-size="11">${u}</text>`).join('');
  const title = `${row.name}  wave ${row.number}  own fit ${row.own.toFixed(3)}  closest ${row.closest.label} ${row.closest.err.toFixed(3)}  swing ${(row.swing * 100).toFixed(0)}% of full scale  ${row.verdict}`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" font-family="sans-serif"><rect width="${W}" height="${H}" fill="#fff"/>${grid}<text x="${m.l}" y="18" font-size="12">${title.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</text><path d="${path}" fill="none" stroke="#2e6f95" stroke-width="2"/>${dots}<text x="${m.l}" y="${H - 8}" font-size="11">phase 0 to 1 (line: declared table; dots: measured, ${row.samples.length} reads)</text></svg>`;
}

/** Candidate keys are `own:NAME` and `stock:NAME`; the printed name is `NAME` or `NAME (stock)`. */
const show = (key: string): string => (key.startsWith('stock:') ? `${key.slice(6)} (stock)` : key.slice(4));

if (!MODULE) test.skip('set DIGI_LFO_MODULE=<module id> (mise run lfo:report)', () => {});
else
  cardRigs(
  `lfo shape report: ${MODULE}`,
  { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true, devices: ONLY ? [ONLY] : undefined },
  (h) => {
    test(`report ${MODULE}`, async () => {
      const id = resolveModule(MODULE, moduleManifests());
      const shapes = readShapes(moduleDir(id));
      const rt = await bootWith(h, 'report', [id]);
      const words = await shapeWords(h, rt);
      const numberOf = (key: number): number => {
        const w = words.find((x) => x >>> 8 === key >>> 8);
        if (w === undefined) throw new Error(`key ${key.toString(16)} has no number in lfo.shapes (${words.map((x) => x.toString(16)).join(' ')})`);
        return w & 0xff;
      };
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      const rows: Row[] = [];
      for (const shape of shapes) {
        const number = numberOf(shape.key);
        await rig.setWave(number);
        const samples = await rig.sample(dest, READS, GAP_MS);
        const candidates: Candidate[] = [
          ...shapes.map((s) => ({ label: `own:${s.name}`, eval: (p: number) => evalTable(s.samples, p) })),
          ...Object.entries(STOCK_EVAL).map(([w, fn]) => ({ label: `stock:${STOCK_NAME[Number(w)]!}`, eval: fn })),
        ];
        const fits: Record<string, Fit> = Object.fromEntries(candidates.map((c) => [c.label, fitShape(samples, c.eval)]));
        const ownKey = `own:${shape.name}`;
        const own = fits[ownKey]!;
        const others = Object.entries(fits)
          .filter(([l]) => l !== ownKey)
          .map(([l, f]) => [show(l), f.err] as [string, number])
          .sort((a, b) => a[1] - b[1]);
        const vs = samples.map((s) => s.v);
        const swing = (Math.max(...vs) - Math.min(...vs)) / FULL_SWING;
        const tableSwing = (Math.max(...shape.samples) - Math.min(...shape.samples)) / 65535;
        const partial = {
          name: shape.name,
          number,
          own: own.err,
          closest: { label: others[0]![0], err: others[0]![1] },
          fits: Object.fromEntries(Object.entries(fits).map(([l, f]) => [l, Number(f.err.toFixed(4))])),
          min: Math.min(...vs),
          max: Math.max(...vs),
          swing,
          tableSwing,
          verdict: verdict(own.err, others, swing, tableSwing),
          samples,
        };
        rows.push({ ...partial, svg: plot(partial, shape.samples, own) });
      }

      const dir = path.join(repoRoot(), 'out/runs/lfo-shape-report', h.device, id.split('/').at(-1)!);
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(
        path.join(dir, 'report.json'),
        JSON.stringify(
          { module: id, device: h.device, thresholds: { OK, PAIR_MARGIN }, reads: READS, gapMs: GAP_MS, destinationWord: dest, shapes: rows.map(({ svg: _svg, ...r }) => r) },
          null,
          2,
        ),
      );
      const pad = (v: string | number, n: number) => String(v).padEnd(n);
      const lines = [
        `lfo shape report: ${id} on ${h.device} (${READS} reads, ${GAP_MS} emulated ms apart; swing is the measured peak-to-peak over the full swing 0x${FULL_SWING.toString(16)}, "table" the declared one)`,
        `${pad('shape', 6)}${pad('wave', 5)}${pad('own err', 9)}${pad('closest other', 18)}${pad('min..max (hex)', 16)}${pad('swing', 7)}${pad('table', 7)}verdict`,
        ...rows.map(
          (r) =>
            `${pad(r.name, 6)}${pad(r.number, 5)}${pad(r.own.toFixed(3), 9)}${pad(`${r.closest.label} ${r.closest.err.toFixed(3)}`, 18)}${pad(`${r.min.toString(16)}..${r.max.toString(16)}`, 16)}${pad(`${Math.round(r.swing * 100)}%`, 7)}${pad(`${Math.round(r.tableSwing * 100)}%`, 7)}${r.verdict}`,
        ),
        `plots and report.json: ${dir}`,
      ];
      fs.writeFileSync(path.join(dir, 'summary.txt'), `${lines.join('\n')}\n`);
      for (const r of rows) fs.writeFileSync(path.join(dir, `${r.number}-${r.name.replace(/[^A-Za-z0-9_-]/g, '_')}.svg`), r.svg);
      console.info(`\n${lines.join('\n')}\n`);
      await rt.session.expectNoFault('lfo shape report');
      const bad = rows.filter((r) => r.verdict.startsWith('FAIL') || r.verdict.startsWith('SCALE'));
      expect(bad.map((r) => `${r.name}: ${r.verdict}`), 'shapes with verdict FAIL or SCALE').toEqual([]);
    }, 1_800_000);
  },
);
