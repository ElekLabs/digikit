import { expect } from 'vitest';
import type { DspStats, RuntimeEvents } from '@digi/dk-session';
import type { TestRuntime } from '@digi/regression/test-runtime.ts';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { pickSine } from '@digi/regression/runtime/panel-waits.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/debug-module.ts';
import { selectTrack } from '@digi/regression/runtime/fx-track-ui.ts';
import { sendCc, sendNote } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/runtime-tests.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * The DSP stats window (docs/plans/profiling.md 13.1, P8d) through the debug module's PROFILE_DSP, on the firmware (SHARC core): `test/sine` on track 1 holds a note and
 * `test/dyn-fx-gain` is the effect on the master route. The spans of the dispatch code in `machines.dks` and `fx.dks`, the fold in the dsp-link block tail, the pages in the
 * acknowledgement slot, the runtime's mirror and the command are all in the path; the emulator counts instructions where the device counts cycles.
 *
 * - Off: no counters, no sweep.
 * - Two tracks have the machine (one row of the registry) and only track 1 plays: the idle voice calls no module and takes no span, so the row counts one call per block, not two.
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

/** No DSP_SWEEP event within `ms` of emulated time. A sweep whose last page was already on the way when the DSP stopped may still complete: it is drained once, then a second one fails. */
/** Returns how many sweeps were drained first. */
async function expectNoSweepEvent(events: RuntimeEvents, ms: number, what: string): Promise<number> {
  for (let drained = 0; ; drained++) {
    await events.listen();
    if ((await events.tryUntil('DSP_SWEEP', undefined, { withinMs: ms })) === null) return drained;
    expect(drained, `${what}: a sweep appeared again`).toBeLessThan(1);
  }
}

async function expectDspStats(rt: TestRuntime): Promise<void> {
  const { dk, session } = rt;
  const artifacts = liveArtifacts(session, 'debug-dsp-stats');
  await cliInstall(session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE, 'test/dyn-fx-gain']);
  const device = session.options.device as 'dt2' | 'dn2';
  await pickSine(rt); // track 1 plays
  await selectTrack(rt, 2);
  await pickSine(rt); // track 2 has the same machine and stays silent
  await selectTrack(rt, 1);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].hold, 127);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].decay, 64);
  for (const cc of SEND_CC[device]) await sendCc(rt, MIDI_CHANNEL, cc, 0);

  const idle = await dk.profileDsp('get');
  expect(idle.on, 'stats are off at the start').toBe(false);
  expect(idle.sweep, 'and there is no sweep').toBeUndefined();

  // the runtime logs DSP_SWEEP once per complete sweep: every wait below is on that event, not on the clock
  const events = await rt.listen();
  expect((await dk.profileDsp('start')).on).toBe(true);
  await sendNote(rt, MIDI_CHANNEL, NOTE, 100); // AMP HOLD is NOTE: the voice runs while the note is held

  /** The newest sweep once the log says a sweep of `blocks` blocks or more was published (and one newer than `after`). */
  const sweepAfter = async (after: number, what: string, blocks = 0): Promise<NonNullable<DspStats['sweep']>> => {
    await events.until('DSP_SWEEP', (e) => Number(e.fields.sweep) > after && Number(e.fields.blocks) >= blocks, {
      withinMs: 20_000,
    });
    const sweep = (await dk.profileDsp('get')).sweep;
    expect(sweep, `${what}: the event came with a sweep to read`).toBeDefined();
    return sweep!;
  };

  const first = await sweepAfter(0, 'first sweep');
  const second = await sweepAfter(first.number, 'second sweep', first.blocks + BLOCKS_PER_S); // about a second of blocks later
  const blocks = second.blocks - first.blocks;
  expect(blocks, 'about a second of blocks between the two sweeps').toBeGreaterThan(BLOCKS_PER_S * 0.8);
  for (const [name, row] of [
    ['machine', MACHINE_ROW],
    ['effect', EFFECT_ROW],
  ] as const) {
    const a = first.rows[row]!;
    const b = second.rows[row]!;
    const ratio = (b.calls - a.calls) / blocks;
    expect(ratio, `${name} row: calls per block (one playing voice, the silent track adds none; one effect)`).toBeGreaterThan(0.9);
    expect(ratio, `${name} row: calls per block`).toBeLessThan(1.1);
    expect(b.sumCycles - a.sumCycles, `${name} row: cycles`).toBeGreaterThan(0n);
    expect(b.maxCall, `${name} row: the largest call`).toBeGreaterThan(0);
    expect(b.maxBlock, `${name} row: the largest block`).toBeGreaterThan(0);
  }
  expect(second.maxStockBlockDelta, 'the stock block delta of the engine').toBeGreaterThan(0);

  const stopped = await dk.profileDsp('stop');
  expect(stopped.on).toBe(false);
  const drained = await expectNoSweepEvent(events, 2000, 'stopped: the mirror publishes no sweep');
  const frozen = (await dk.profileDsp('get')).sweep!.number;
  // a sweep that was complete on the way when the DSP stopped may have been published first; none moves it after
  if (drained === 0) expect(frozen, 'stopped: the sweep number does not move').toBe(stopped.sweep!.number);
  else expect(frozen).toBeGreaterThan(stopped.sweep!.number);

  expect((await dk.profileDsp('clear')).sweep, 'a new clear generation drops the old sweep').toBeUndefined();
  await events.listen();
  expect((await dk.profileDsp('start')).on).toBe(true);
  const again = await sweepAfter(0, 'sweep after the clear');
  expect(again.blocks, 'the counts start again from 0').toBeLessThan(second.blocks);
  expect(again.blocks).toBeLessThan(BLOCKS_PER_S);
  expect(again.rows[MACHINE_ROW]!.calls, 'rows of one sweep are up to 50 blocks apart').toBeLessThanOrEqual(again.blocks + 50);
  await dk.profileDsp('stop');
  await session.expectNoFault('DSP stats');
}

runtimeTests('PROFILE_DSP counts one call per block for a machine and an effect, stops and clears', expectDspStats, { dsp: 'sharc' });
