import { waitClock, pollUntil } from '@digi/test-support';
import { expect } from 'vitest';
import type { DspStats } from '@digi/dk-session';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { pickSine } from '@digi/regression/runtime/fx-live.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/install.ts';
import { sendCc, sendNote } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * The DSP stats window (docs/plans/profiling.md 13.1, P8d) through the debug module's PROFILE_DSP, on the firmware (SHARC core): `test/sine` on track 1 holds a note and
 * `test/dyn-fx-gain` is the effect on the master route. The spans of the dispatch code in `machines.dks` and `fx.dks`, the fold in the dsp-link block tail, the pages in the
 * acknowledgement slot, the runtime's mirror and the command are all in the path; the emulator counts instructions where the device counts cycles.
 *
 * - Off: no counters, no sweep.
 * - Start: a sweep appears (50 blocks, one page each). Over the time between two sweeps the machine row (stats row 0) counts one call per block (one voice) and the effect row
 *   (row 8) one call per block, both with cycles. The rows of one sweep are up to 50 blocks apart, so the ratio is taken over about 1500 blocks and has a few per cent of slack.
 * - Stop: the sweep number stays when the DSP stops.
 * - Clear, start: the counts start again from 0 (the header's block count falls below the old one).
 */
const MIDI_CHANNEL = 1;
const NOTE = 57;
const AMP_CC = { dt2: { hold: 80, decay: 81 }, dn2: { hold: 85, decay: 86 } } as const;
const SEND_CC = { dt2: [84, 85, 12], dn2: [30, 31, 29] } as const;
const BLOCKS_PER_S = 1500;
const MACHINE_ROW = 0;
const EFFECT_ROW = 8;

async function expectDspStats(rt: TestRuntime): Promise<void> {
  const { dk, session } = rt;
  const artifacts = liveArtifacts(session, 'debug-dsp-stats');
  await cliInstall(session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE, 'test/dyn-fx-gain']);
  const device = session.options.device as 'dt2' | 'dn2';
  await pickSine(rt);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].hold, 127);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].decay, 64);
  for (const cc of SEND_CC[device]) await sendCc(rt, MIDI_CHANNEL, cc, 0);

  const idle = await dk.profileDsp('get');
  expect(idle.on, 'stats are off at the start').toBe(false);
  expect(idle.sweep, 'and there is no sweep').toBeUndefined();

  expect((await dk.profileDsp('start')).on).toBe(true);
  await sendNote(rt, MIDI_CHANNEL, NOTE, 100); // AMP HOLD is NOTE: the voice runs while the note is held

  const sweepAfter = (after: number, what: string): Promise<NonNullable<DspStats['sweep']>> =>
    pollUntil(
      rt.clock,
      async () => (await dk.profileDsp('get')).sweep,
      (s) => s !== undefined && s.number > after,
      { withinMs: 20_000, intervalMs: 100, message: () => `${what}: no sweep newer than ${after}` },
    ) as Promise<NonNullable<DspStats['sweep']>>;

  const first = await sweepAfter(0, 'first sweep');
  await waitClock(rt.clock, 1000); // about 1500 blocks
  const second = await sweepAfter(first.number, 'second sweep');
  const blocks = second.blocks - first.blocks;
  expect(blocks, 'about a second of blocks between the two sweeps').toBeGreaterThan(BLOCKS_PER_S * 0.8);
  for (const [name, row] of [
    ['machine', MACHINE_ROW],
    ['effect', EFFECT_ROW],
  ] as const) {
    const a = first.rows[row]!;
    const b = second.rows[row]!;
    const ratio = (b.calls - a.calls) / blocks;
    expect(ratio, `${name} row: calls per block (one voice, one effect)`).toBeGreaterThan(0.9);
    expect(ratio, `${name} row: calls per block`).toBeLessThan(1.1);
    expect(b.sumCycles - a.sumCycles, `${name} row: cycles`).toBeGreaterThan(0n);
    expect(b.maxCall, `${name} row: the largest call`).toBeGreaterThan(0);
    expect(b.maxBlock, `${name} row: the largest block`).toBeGreaterThan(0);
  }
  expect(second.maxStockBlockDelta, 'the stock block delta of the engine').toBeGreaterThan(0);

  const stopped = await dk.profileDsp('stop');
  expect(stopped.on).toBe(false);
  await waitClock(rt.clock, 300); // pages already on the way
  const frozen = (await dk.profileDsp('get')).sweep!.number;
  await waitClock(rt.clock, 2000);
  expect((await dk.profileDsp('get')).sweep!.number, 'stopped: the sweep number does not move').toBe(frozen);

  expect((await dk.profileDsp('clear')).sweep, 'a new clear generation drops the old sweep').toBeUndefined();
  expect((await dk.profileDsp('start')).on).toBe(true);
  const again = await sweepAfter(0, 'sweep after the clear');
  expect(again.blocks, 'the counts start again from 0').toBeLessThan(second.blocks);
  expect(again.blocks).toBeLessThan(BLOCKS_PER_S);
  expect(again.rows[MACHINE_ROW]!.calls).toBeLessThanOrEqual(again.blocks);
  await dk.profileDsp('stop');
  await session.expectNoFault('DSP stats');
}

runtimeTests('PROFILE_DSP counts one call per block for a machine and an effect, stops and clears', expectDspStats, { dsp: 'sharc' });
