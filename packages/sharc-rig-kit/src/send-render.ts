/** Run the actual send hook against independent dry and send stems. */
import { createHash } from 'node:crypto';
import { FS } from './analysis.ts';
import { f2w } from './context.ts';
import { FxRig } from './fx.ts';
import { listeningStems, LISTENING_SEED, LISTENING_TEMPO, type ListeningScene } from './listening-stems.ts';
import { buildFx } from './place.ts';
import { fxDescription } from './conform.ts';
import type { Device, FxSpec } from './spec.ts';
import type { PcCounts } from './profile.ts';

const TRACK = 2;
const SEED = 0xd1617;
const ZERO = Array<number>(64).fill(0);

export interface SendOptions {
  amount: number;
  params: readonly number[];
  profileBlock?: number;
}

export async function renderSend(sp: FxSpec, device: Device, scene: ListeningScene, options: SendOptions) {
  const { amount, params, profileBlock } = options;
  if (!Number.isInteger(amount) || amount < 0 || amount > 127)
    throw new Error('Send amount must be an integer from 0 to 127.');
  const built = buildFx(sp.sources, device);
  const desc = fxDescription(built, sp.desc);
  if (params.length > desc[2]! || params.some((n) => !Number.isFinite(Math.fround(n))))
    throw new Error(`Expected up to ${desc[2]} finite float32 parameter values.`);
  const { dry, send, beats } = listeningStems(scene);
  const blocks = dry.length / 32;
  if (profileBlock !== undefined && (!Number.isSafeInteger(profileBlock) || profileBlock < 0 || profileBlock >= blocks))
    throw new Error(`Profile block must be an index from 0 to ${blocks - 1}.`);
  const rig = await FxRig.open([{ pc: sp.process, state: sp.state, desc: sp.desc, params: params.map(f2w) }], {
    built,
    seed: SEED,
    sources: sp.sources,
  });
  const samples: number[] = [];
  let worstBlock = 0;
  let worstSteady = 0;
  let profile: { block: number; counts: PcCounts } | undefined;
  try {
    rig.setAmounts(0, [0, 0, amount]);
    // One populated bus. The identity map also points the corresponding voice slot at it.
    rig.setSlotmap(Array.from({ length: 16 }, (_, n) => n));
    for (let i = 0; i < blocks; i++) {
      const at = i * 32;
      const dryBlock = Array.from(dry.subarray(at, at + 32));
      const sentBlock = Array.from(send.subarray(at, at + 32));
      const master = [...dryBlock, ...dryBlock];
      rig.setTrack(TRACK, [...sentBlock, ...sentBlock]);
      rig.setReturn(ZERO);
      const b = await rig.sendBlock(master, 'friendly', {}, i === profileBlock);
      if (!b.returned || b.faults[0] || b.ctxBad.length || b.regsBad.length)
        throw new Error(`${scene} block ${i}: send adapter failed (${b.halt}, ${b.faults[0]} faults).`);
      if (
        b.master.some((x, n) => x !== master[n]) ||
        b.ret.some((x) => !Number.isFinite(x) || (amount === 0 && x !== 0))
      )
        throw new Error(`${scene} block ${i}: unexpected master or return samples.`);
      worstBlock = Math.max(worstBlock, b.instructions);
      if (i > 0) worstSteady = Math.max(worstSteady, b.instructions);
      if (i === profileBlock) profile = { block: i, counts: b.profile };
      // The hook produces a return. Model the downstream unity return mix explicitly.
      for (let n = 0; n < 32; n++) {
        samples.push(Math.fround(b.master[n]! + b.ret[n]!), Math.fround(b.master[n + 32]! + b.ret[n + 32]!));
      }
    }
  } finally {
    await rig.close();
  }
  const inputSha256 = createHash('sha256')
    .update(Buffer.from(dry.buffer))
    .update(Buffer.from(send.buffer))
    .digest('hex');
  return {
    built,
    settings: {
      scene,
      inputSha256,
      route: 'send',
      sendTrack: TRACK,
      amount,
      paramsF32: params.map(Math.fround),
      rate: FS,
      tempoBpm: LISTENING_TEMPO,
      beats,
      stimulusSeed: LISTENING_SEED,
      returnGain: 1,
    },
    cost: { blocks, instructionUnit: 'rig-instruction' as const, worstBlock, worstSteady },
    pcm: { rate: FS, channels: 2, samples },
    profile,
  };
}
