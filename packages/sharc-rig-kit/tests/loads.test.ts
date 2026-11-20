import { describe, expect, it } from 'vitest';
import { loadFailures, mergeRanges, wordToFloat, type Load, type LoadRule } from '../src/loads.ts';
import type { CallResult, Op, RigPort } from '../src/rig.ts';
import { BACK, StereoSession } from '../src/stereo.ts';

const TABLE: [number, number] = [0x28000000 + 0x26e9c8, 0x28000000 + 0x26e9c8 + 512];
const rule = (r: Partial<LoadRule> = {}): LoadRule => ({ label: 'table', pcs: [10, 11], within: [TABLE], ...r });
const at = (pc: number, address: number, width = 4): Load => [pc, address, width];

describe('loadFailures', () => {
  it('passes loads of the covered pcs inside the range and ignores the loads of other pcs', () => {
    const loads = [at(10, TABLE[0]), at(11, TABLE[1] - 4), at(99, 0x1234)];
    expect(loadFailures(loads, [rule({ bytes: 8 })])).toEqual([]);
  });

  it('names a load of the right pc at a wrong address, which a load count alone passes', () => {
    const loads = [at(10, TABLE[0]), at(11, TABLE[1])];
    const failures = loadFailures(loads, [rule({ bytes: 8 })]);
    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain('table: 1 of 2 loads outside the range');
    expect(failures[0]).toContain(`pc b address ${TABLE[1].toString(16)} width 4`);
  });

  it('refuses a load that starts inside the range and ends past it', () => {
    expect(loadFailures([at(10, TABLE[1] - 2)], [rule()])).toHaveLength(1);
  });

  it('counts bytes, not loads, and checks a multiple', () => {
    const loads = [at(10, TABLE[0], 1), at(10, TABLE[0] + 1, 1), at(10, TABLE[0] + 4, 4)];
    expect(loadFailures(loads, [rule({ bytes: 6 })])).toEqual([]);
    expect(loadFailures(loads, [rule({ bytes: 8 })])).toEqual(['table: 6 bytes loaded, expected 8']);
    expect(loadFailures(loads, [rule({ bytesMultipleOf: 4 })])).toEqual(['table: 6 bytes loaded, not a multiple of 4']);
    expect(loadFailures([], [rule({ bytes: 0, bytesMultipleOf: 4 })])).toEqual([]);
  });

  it('applies a rule without pcs to every load of the call', () => {
    const loads = [at(10, TABLE[0]), at(77, 0x5000)];
    expect(loadFailures(loads, [{ label: 'all', within: [TABLE] }])).toHaveLength(1);
  });

  it('accepts a load across two adjacent ranges and refuses an unexpected width', () => {
    const within: [number, number][] = [
      [100, 104],
      [104, 108],
    ];
    expect(loadFailures([at(1, 102, 4)], [{ label: 'two words', within }])).toEqual([]);
    expect(loadFailures([at(1, 100, 8)], [{ label: 'wide', within, widths: [4] }])[0]).toContain('width 8');
  });
});

describe('mergeRanges', () => {
  it('sorts and joins touching and overlapping ranges', () => {
    expect(
      mergeRanges([
        [8, 12],
        [0, 4],
        [4, 6],
        [10, 20],
        [30, 31],
      ]),
    ).toEqual([
      [0, 6],
      [8, 20],
      [30, 31],
    ]);
  });
});

describe('wordToFloat', () => {
  it('reads the little-endian single the word holds', () => {
    expect(wordToFloat(0x3fc00000)).toBe(1.5);
    expect(wordToFloat(0xc1200000)).toBe(-10);
  });
});

describe('the loads option', () => {
  it('is sent only when asked for and the loads come back with the result', async () => {
    const calls: unknown[] = [];
    const port: RigPort = {
      async run(batch: Op[]) {
        const out: CallResult[] = [];
        for (const op of batch) {
          if (!('call' in op)) continue;
          calls.push(op.call);
          out.push({
            instructions: 1,
            halt: 'breakpoint',
            final_pc: BACK,
            regs: {},
            reads: [],
            stores: [],
            loads: [[5, 0x28000010, 4]],
          });
        }
        return out;
      },
      async close() {},
    };
    const s = new StereoSession(port);
    const r = await s.raw(0x1000, {}, { loads: true });
    await s.raw(0x1000, {});
    expect(r.loads).toEqual([[5, 0x28000010, 4]]);
    expect((calls[0] as { loads?: boolean }).loads).toBe(true);
    expect('loads' in (calls[1] as object)).toBe(false);
  });
});
