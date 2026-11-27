/** Render the same named workloads used by the rig conformance suite. */
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { FS } from './analysis.ts';
import { fxCases, machineCases } from './cases.ts';
import { fxDescription, machineBudget, runFxCase, runMachineCase } from './conform.ts';
import { buildFx, buildMachine, type FxBuilt, type MachineBuilt } from './place.ts';
import { rigBinary } from './rig.ts';
import { checkMachineGallery, fxBudget, fxSpec, fxWord, machineSpec, type Device } from './spec.ts';
import { LISTENING_SCENES, type ListeningScene } from './listening-stems.ts';
import { renderSend, type SendOptions } from './send-render.ts';
import { moduleBaseOf, rigAddrMap, rigProfile } from './profile.ts';

const SEED = 0xd1617;

export interface RenderedCase {
  module: string;
  device: Device;
  case: string;
  tap: 'master-insert' | 'voice' | 'send-mix';
  workload: { sha256: string; seed: number; settings: Record<string, unknown> };
  build: { linkedSha256: string; rigSha256: string };
  cost: {
    blocks: number;
    instructionUnit: 'rig-instruction';
    worstBlock: number;
    worstSteady: number;
    worstNoteOn?: number;
  };
  pcm: { rate: number; channels: number; samples: number[] };
  profile?: object;
}

function selectCase<T extends { group: string; name: string }>(cases: readonly T[], name: string): T {
  const matching = cases.filter((c) => `${c.group}/${c.name}` === name);
  if (matching.length === 1) return matching[0]!;
  throw new Error(`Unknown case "${name}". Choose one of: ${cases.map((c) => `${c.group}/${c.name}`).join(', ')}`);
}

function buildIdentity(built: FxBuilt | MachineBuilt): RenderedCase['build'] {
  const hash = createHash('sha256');
  for (const code of built.linked) hash.update(`code:${code.baseSw}:${code.codeBoot}\n`);
  for (const init of built.inits) {
    hash.update(`data:${init.address}:${init.bytes.length}:${init.bss}\n`);
    hash.update(init.bytes);
  }
  return {
    linkedSha256: hash.digest('hex'),
    rigSha256: createHash('sha256').update(readFileSync(rigBinary())).digest('hex'),
  };
}

function workload(settings: Record<string, unknown>): RenderedCase['workload'] {
  return {
    sha256: createHash('sha256')
      .update(JSON.stringify({ version: 1, seed: SEED, settings }))
      .digest('hex'),
    seed: SEED,
    settings,
  };
}

export async function renderCase(
  module: string,
  device: Device,
  name: string,
  send?: SendOptions,
): Promise<RenderedCase> {
  if (send) {
    if (!LISTENING_SCENES.includes(name as ListeningScene)) throw new Error('Send options need a listening scene.');
    const sp = fxSpec(module);
    if (!sp) throw new Error(`${module} is not an effect with a DSP object.`);
    const { built, settings, cost, pcm, profile } = await renderSend(sp, device, name as ListeningScene, send);
    const build = buildIdentity(built);
    const work = workload(settings);
    return {
      module,
      device,
      case: name,
      tap: 'send-mix',
      workload: work,
      build,
      cost,
      pcm,
      ...(profile
        ? {
            profile: {
              ...rigProfile(
                {
                  module,
                  device,
                  caseName: name,
                  blocks: 1,
                  binarySha256: build.rigSha256,
                  inputSha256: settings.inputSha256,
                  id: `rig:${module}:${device}:${name}:${work.sha256.slice(0, 12)}:${build.linkedSha256.slice(0, 12)}:block${profile.block}`,
                },
                new Map(profile.counts),
                moduleBaseOf(built),
              ),
              rig: {
                map: rigAddrMap(built, module, device),
                blocks: [profile.block],
                linkedSha256: build.linkedSha256,
              },
            },
          }
        : {}),
    };
  }
  if (LISTENING_SCENES.includes(name as ListeningScene))
    throw new Error('Provide a send amount for the listening scene.');
  const samples: number[] = [];
  const machine = machineSpec(module, device);
  if (machine) {
    checkMachineGallery(machine);
    const c = selectCase(machineCases(machine), name);
    const built = buildMachine(machine);
    let inputSha256: string | undefined;
    const result = await runMachineCase(machine, c, machineBudget(machine, built), SEED, {
      built,
      onBlock: ({ samples: block }) => samples.push(...block),
      onSample: ({ left, right }) => {
        inputSha256 = createHash('sha256')
          .update(Buffer.from(Int16Array.from(left).buffer))
          .update(Buffer.from(Int16Array.from(right).buffer))
          .digest('hex');
      },
    });
    if (result.failure) throw new Error(result.failure);
    return {
      module,
      device,
      case: name,
      tap: 'voice',
      workload: workload({ ...c, inputSha256, rate: FS, tap: 'voice' }),
      build: buildIdentity(built),
      cost: {
        blocks: result.blocks,
        instructionUnit: 'rig-instruction',
        worstBlock: Math.max(result.steady, result.noteon),
        worstSteady: result.steady,
        worstNoteOn: result.noteon,
      },
      pcm: { rate: FS, channels: 1, samples },
    };
  }

  const effect = fxSpec(module);
  if (!effect) throw new Error(`${module} is not a machine or effect with a DSP object for ${device}`);
  const built = buildFx(effect.sources, device);
  const desc = fxDescription(built, effect.desc);
  const c = selectCase(fxCases(desc[2]!), name);
  const inputHash = createHash('sha256');
  const result = await runFxCase(effect, c, fxBudget(), desc, SEED, {
    built,
    onBlock({ master }, _index, input) {
      inputHash.update(Buffer.from(Float32Array.from(input).buffer));
      const frames = master.length / 2;
      for (let i = 0; i < frames; i++) samples.push(master[i]!, master[frames + i]!);
    },
  });
  if (result.failure) throw new Error(result.failure);
  return {
    module,
    device,
    case: name,
    tap: 'master-insert',
    workload: workload({
      ...c,
      words: c.words.map((row) => row.map((word, j) => fxWord(effect, j, word))),
      inputSha256: inputHash.digest('hex'),
      rate: FS,
      tap: 'master-insert',
      mix: 1,
    }),
    build: buildIdentity(built),
    cost: {
      blocks: result.blocks,
      instructionUnit: 'rig-instruction',
      worstBlock: result.steady,
      worstSteady: result.steady,
    },
    pcm: { rate: FS, channels: 2, samples },
  };
}
