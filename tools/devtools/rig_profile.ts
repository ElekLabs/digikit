// Instructions per function and per DO loop for one conformance case of a machine or effect module, on the SHARC-only rig (no emulator): the rig is
// a producer for `wb profile report` (packages/dkprof), the profiler of the emulator and the device.
//
//   node tools/devtools/rig_profile.ts MODULE --device dt2|dn2 [--case GROUP/NAME] [--blocks 40,41] [--out DIR] [--top 20] [--print]
//
// MODULE is a module id (`<author/name>`). --case is the case as `group/name` (`sample/stereo-fwd-oneshot/note60`) or only the name; default
// the first case of the module. Effects run the conformance suite's master chain with one effect. By default, all effect blocks or the
// machine's steady blocks are added (DN2 stages the note one block after the trig); --blocks names the zero-based blocks to
// add instead. The tool writes to DIR (default out/runs/rig-profile/<module>/<device>/):
//   profile.json       `dk.profile/1`: raw SHARC counters per pc, metric sharc.blocks, the load of the module's DSP image
//   rig.addrmap.json   the labels of the placed objects (module labels relative to its DSP image)
//   module.dkm         `patchkit pack` of the module for the device (its DSP section is what dksharc analyses)
// and runs `node tools/devtools/wb.mjs profile report profile.json --map rig.addrmap.json --analyse ID=module.dkm:DSP.dko.json`, so functions
// come from the map and loops from dksharc. --print shows that command instead of running it (needs `mise run wasm:dkprof` to run).
// The rig binary and patchkit come from DIGI_SHARC_RIG and DIGI_PATCHKIT, else dist/ (`nx run sharc-rig:build`, `nx run patchkit:cli`).
// DIGI_RIG_ROOT=PATH reads the module sources, SDK and device files of another checkout (read only), for example a lane branch.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DkProf } from '../../packages/dk-session/src/dkprof.ts';
import {
  buildFx,
  fxBudget,
  fxCases,
  fxDescription,
  fxSpec,
  machineCases,
  machineSpec,
  MachineRig,
  moduleBaseOf,
  patchkit,
  publishSample,
  rigAddrMap,
  rigBinary,
  rigProfile,
  runFxCase,
  sumCounts,
  wordsAt,
  type Device,
  type FxSpec,
  type MachineSpec,
  type PcCounts,
} from '../../packages/sharc-rig-kit/src/index.ts';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../..');

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(name);
  return i < 0 ? undefined : process.argv[i + 1];
}

const module = process.argv[2];
const device = arg('--device') as Device | undefined;
if (!module || module.startsWith('--') || (device !== 'dt2' && device !== 'dn2')) {
  console.error(
    'usage: node tools/devtools/rig_profile.ts MODULE --device dt2|dn2 [--case GROUP/NAME] [--blocks 40,41] [--out DIR] [--top 20] [--print]',
  );
  process.exit(2);
}
const want = arg('--case');
const asked = arg('--blocks')?.split(',').map(Number);
if (asked?.some((k) => !Number.isSafeInteger(k) || k < 0)) throw new Error('--blocks must be zero-based block indices');

function selectCase<C extends { group: string; name: string; blocks: number }>(cases: C[]) {
  const c = want ? cases.find((k) => `${k.group}/${k.name}` === want || k.name === want) : cases[0];
  if (!c) throw new Error(`no case ${want}; cases: ${cases.map((k) => `${k.group}/${k.name}`).join(', ')}`);
  const blocks = asked ? Math.max(...asked) + 1 : c.blocks;
  if (blocks > c.blocks) throw new Error(`case ${c.name} has ${c.blocks} blocks`);
  return { c, blocks, caseName: `${c.group}/${c.name}` };
}

async function profileMachine(sp: MachineSpec) {
  const { c, blocks, caseName } = selectCase(machineCases(sp));
  const trig = new Set(c.trig);
  const noteOn = new Set(trig);
  if (sp.device === 'dn2') for (const k of trig) noteOn.add(k + 1);
  const rig = await MachineRig.open(sp, { profile: true });
  const calls: PcCounts[] = [];
  try {
    if (c.setup === 'sample-pool') publishSample(rig, sp, c);
    rig.setNote(c.note, c.vel);
    for (let k = 0; k < blocks; k++) {
      rig.setParams(wordsAt(c, k));
      if (trig.has(k)) rig.trigger();
      const b = await rig.block();
      if (!b.returned) throw new Error(`block ${k} did not return (${b.halt})`);
      if (asked ? asked.includes(k) : !noteOn.has(k)) calls.push(b.profile);
    }
    return { built: rig.built, caseName, calls };
  } finally {
    await rig.close();
  }
}

async function profileEffect(sp: FxSpec, target: Device) {
  const built = buildFx(sp.sources, target);
  const desc = fxDescription(built, sp.desc);
  const { c, blocks, caseName } = selectCase(fxCases(desc[2]!));
  const calls: PcCounts[] = [];
  const result = await runFxCase(sp, { ...c, blocks }, fxBudget(), desc, 0xd1617, {
    built,
    profile: true,
    onBlock(block, index) {
      if (!asked || asked.includes(index)) calls.push(block.profile);
    },
  });
  if (result.failure) throw new Error(result.failure);
  return { built, caseName, calls };
}

const machine = machineSpec(module, device);
const effect = machine ? null : fxSpec(module);
if (!machine && !effect) throw new Error(`${module} is no machine or effect with a DSP object for ${device}`);
const { built, caseName, calls } = machine ? await profileMachine(machine) : await profileEffect(effect!, device);
if (!calls.length) throw new Error(`case ${caseName}: no block to add`);
const moduleBase = moduleBaseOf(built);

const dir = path.resolve(arg('--out') ?? path.join(repo, 'out/runs/rig-profile', module, device));
mkdirSync(dir, { recursive: true });
const profileFile = path.join(dir, 'profile.json');
const mapFile = path.join(dir, 'rig.addrmap.json');
const dkmFile = path.join(dir, 'module.dkm');
const binarySha256 = createHash('sha256').update(readFileSync(rigBinary())).digest('hex');
const map = rigAddrMap(built, module, device);
writeFileSync(mapFile, JSON.stringify(map) + '\n');

// The module's DSP section and object for the loops: the pack of the module, and its DSP object in the build cache.
const report = [path.join(here, 'wb.mjs'), 'profile', 'report', profileFile, '--top', String(arg('--top') ?? 20)];
let moduleSha256: string | undefined;
let analyses: Record<string, unknown> | undefined;
try {
  patchkit(['pack', module, '-o', dkmFile, '--device', device]);
  const packed = readFileSync(dkmFile);
  moduleSha256 = createHash('sha256').update(packed).digest('hex');
  const objects =
    (JSON.parse(patchkit(['build', '--paths', module])) as Record<string, Record<string, string>>)[module] ?? {};
  const dko = Object.entries(objects).find(([k]) => k.endsWith('-dsp.dko.json'))?.[1];
  if (!dko) throw new Error(`${module} has no DSP object`);
  const wasm = path.join(repo, 'apps/web/public/dkprof.wasm');
  if (existsSync(wasm)) {
    const prof = await DkProf.create(Uint8Array.from(readFileSync(wasm)));
    analyses = {
      [module]: prof.moduleAnalyse(Uint8Array.from(packed), Uint8Array.from(readFileSync(dko))),
    };
  } else {
    report.push('--analyse', `${module}=${dkmFile}:${dko}`);
  }
} catch (error) {
  console.error(`no loop rows: ${(error as Error).message}`);
}
const profile = rigProfile(
  { module, device, caseName, blocks: calls.length, binarySha256, moduleSha256 },
  sumCounts(calls),
  moduleBase,
);
writeFileSync(profileFile, JSON.stringify({ ...profile, rig: { map, analyses, blocks: asked } }) + '\n');
console.error(
  `${caseName} on ${device}: ${calls.length} block${calls.length === 1 ? '' : 's'} added; wrote ${profileFile}`,
);
if (process.argv.includes('--print')) {
  console.log(['node', ...report].join(' '));
} else {
  const r = spawnSync('node', report, { stdio: 'inherit', cwd: repo });
  process.exit(r.status ?? 1);
}
