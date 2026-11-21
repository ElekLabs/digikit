import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, expect, it } from 'vitest';
import { RawRig, f2w, nw, sharcIncludeDirs, w2f } from '@digi/sharc-rig-kit';

const MODULE = path.resolve(import.meta.dirname, '..');
const RETURN_SW = 0xb9f000;
const STACK = 0x08021000;
const REGS = { I6: STACK, I7: STACK, I13: RETURN_SW, M5: 0, M6: 1, M7: 0xffffffff, M13: 0, M14: 1, MODE1: 0x10000 };
const BLOCK = 32;
const STATE_A = 0x20080000;
const STATE_STRIDE = 568; // The adapter rounds the descriptor's 564 bytes up to eight-byte alignment.
const STATE_B = STATE_A + STATE_STRIDE;
const PARAMS = 0x20081000;
const PARAM_OFFSET = 104;
const COPY_OFFSET = 109;

// DN2 1.12 passes a DDR byte address in R8. The rig's host pokes and reads use the DDR normal-word alias.
const AUDIO_A = 0x804acf90;
const AUDIO_B = AUDIO_A + BLOCK * 4;
const ddrWord = (byte: number): number => 0x10000000 + (byte - 0x80000000) / 4;
const SILENCE = Array.from({ length: BLOCK }, () => 0);

let rig: RawRig;

beforeAll(async () => {
  rig = await RawRig.open({
    objects: [{ text: readFileSync(path.join(MODULE, 'src/dsp/resonator_bank.s'), 'utf8'), includes: sharcIncludeDirs(MODULE), base: 0xb90000 }],
    returnSw: RETURN_SW,
  });
}, 120_000);

afterAll(async () => {
  await rig?.close();
});

async function call(entry: string, state: number, arg: number, arg2: number, reads?: [number, number][]) {
  const result = await rig.call(entry, { ...REGS, R4: state, R8: arg, R12: arg2 }, { maxSteps: 5000, readRegs: ['R0'], reads });
  expect(result.halt).toBe('breakpoint');
  expect(result.regs.R0).toBe(0);
  return result.reads;
}

async function coef(state: number, cutoff: number, spread: number, q: number) {
  rig.poke(nw(PARAMS), spread);
  rig.poke(nw(PARAMS) + 1, q);
  await call('_rbnk_coef', state, f2w(cutoff), PARAMS);
}

async function process(state: number, audio: number, input: readonly number[]) {
  const word = ddrWord(audio);
  input.forEach((value, i) => rig.poke(word + i, value));
  return call('_rbnk_process', state, audio, BLOCK, [[word, BLOCK], [nw(state) + COPY_OFFSET, BLOCK]]);
}

it('processes the live DDR block in place and keeps coefficient and audio history per voice', async () => {
  for (let i = 0; i < (2 * STATE_STRIDE) / 4; i++) rig.poke(nw(STATE_A) + i, 0);
  await coef(STATE_A, 0.75, 0x1800, 0x2400);
  await coef(STATE_B, 0.25, 0x3800, 0x4800);
  expect(await rig.peekMany([[nw(STATE_A) + PARAM_OFFSET, 3], [nw(STATE_B) + PARAM_OFFSET, 3]])).toEqual([
    [24384, 0x1800, 0x2400],
    [8128, 0x3800, 0x4800],
  ]);

  const guard = 0x5a5a5a5a;
  rig.poke(ddrWord(AUDIO_A) - 1, guard);
  rig.poke(ddrWord(AUDIO_B) + BLOCK, guard);
  const input = Array.from({ length: BLOCK }, (_, i) => f2w((i + 1) / 64));
  const [output, copied] = await process(STATE_A, AUDIO_A, input);
  expect(copied).toEqual(input);
  expect(output).not.toEqual(input);
  expectSignal(output!);

  const [silent] = await process(STATE_B, AUDIO_B, SILENCE);
  expect(silent).toEqual(SILENCE);
  expect(await rig.peek(ddrWord(AUDIO_A), BLOCK)).toEqual(output);

  const [tail] = await process(STATE_A, AUDIO_A, SILENCE);
  expectSignal(tail!);
  expect(await rig.peekMany([[ddrWord(AUDIO_A) - 1, 1], [ddrWord(AUDIO_B) + BLOCK, 1]])).toEqual([[guard], [guard]]);
});

function expectSignal(words: (number | null)[]): void {
  const samples = words.map((word) => (word === null ? NaN : w2f(word)));
  expect(samples.every(Number.isFinite)).toBe(true);
  expect(samples.some((sample) => sample !== 0)).toBe(true);
}
