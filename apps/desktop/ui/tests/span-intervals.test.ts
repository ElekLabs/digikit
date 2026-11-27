import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { ModuleRef, Snapshot, SpanIn } from '@digi/workbench-core/profile/profile-core.ts';
import { createSpanIntervals } from '../src/span-intervals';

const modules: ModuleRef[] = [{ index: 0, name: 'test/sine', kind: 'machine' }];
const span = (calls: number, sumTicks: bigint, maxTicks = 1): SpanIn => ({
  row: 0,
  kind: 'module',
  index: 0,
  calls,
  sumTicks,
  maxTicks,
});
const reading = (elapsedTicks: bigint, spans: SpanIn[], options?: Partial<Snapshot>): Snapshot => ({
  running: true,
  hz: 1_000,
  elapsedTicks,
  spans,
  ...options,
});

test('uses per-owner calls and ticks over the target elapsed ticks', () => {
  const history = createSpanIntervals();
  const poll = (calls: number, sumTicks: bigint): SpanIn => ({
    ...span(calls, sumTicks),
    row: 1,
    kind: 'poll',
  });
  const polls: Snapshot['polls'] = [{ owner: 0, fn: 0x5000, name: 'tick' }];
  history.push(250, reading(200n, [span(10, 100n), poll(2, 10n)], { polls }), modules);
  assert.deepEqual(history.intervals(), []);
  history.push(1_250, reading(300n, [poll(3, 15n), span(12, 120n)], { polls }), modules);
  assert.deepEqual(history.intervals(), [
    {
      start: 250,
      end: 1_250,
      rows: [
        { id: 'module:0', name: 'test/sine', calls: 2, share: 0.2 },
        { id: 'poll:0', name: 'test/sine poll: tick', calls: 1, share: 0.05 },
      ],
    },
  ]);
});

test('uses the stopped reading once and starts a fresh history on the next recording', () => {
  const history = createSpanIntervals();
  history.push(0, reading(0n, [span(0, 0n)]), modules);
  const stopped = reading(120n, [span(2, 30n)], { running: false });
  history.push(1_000, stopped, modules);
  history.push(2_000, stopped, modules);
  assert.deepEqual(history.intervals(), [
    { start: 0, end: 1_000, rows: [{ id: 'module:0', name: 'test/sine', calls: 2, share: 0.25 }] },
  ]);

  history.push(3_000, reading(130n, [span(3, 35n)]), modules);
  assert.deepEqual(history.intervals(), []);
  history.push(4_000, reading(140n, [span(5, 40n)]), modules);
  assert.equal(history.intervals()[0]?.start, 3_000);
  assert.equal(history.intervals()[0]?.rows[0]?.calls, 2);
});

test.each([
  ['elapsed', reading(5n, [span(4, 40n, 10)])],
  ['calls', reading(300n, [span(1, 40n, 10)])],
  ['sum', reading(300n, [span(4, 1n, 10)])],
  ['maximum', reading(300n, [span(4, 40n, 1)])],
])('drops the old recording on a %s counter reset', (_, reset) => {
  const history = createSpanIntervals();
  history.push(0, reading(100n, [span(2, 20n, 10)]), modules);
  history.push(1_000, reading(200n, [span(3, 30n, 10)]), modules);
  assert.equal(history.intervals().length, 1);
  history.push(2_000, reset, modules);
  assert.deepEqual(history.intervals(), []);
  const previous = reset.spans[0]!;
  history.push(
    3_000,
    reading(reset.elapsedTicks + 100n, [span(previous.calls + 1, previous.sumTicks + 20n, 10)]),
    modules,
  );
  assert.deepEqual(history.intervals(), [
    { start: 2_000, end: 3_000, rows: [{ id: 'module:0', name: 'test/sine', calls: 1, share: 0.2 }] },
  ]);
});

test('a break, a long gap and a repeated timestamp produce no placeholder intervals', () => {
  const history = createSpanIntervals();
  const push = (at: number, elapsed: bigint) => history.push(at, reading(elapsed, [span(0, 0n)]), modules);
  push(0, 0n);
  push(1_000, 100n);
  history.break();
  push(2_000, 200n);
  push(8_000, 300n);
  push(8_000, 400n);
  push(9_000, 400n);
  push(10_000, 500n);
  assert.deepEqual(history.intervals(), [
    { start: 0, end: 1_000, rows: [] },
    { start: 9_000, end: 10_000, rows: [] },
  ]);
  history.clear();
  push(11_000, 600n);
  assert.deepEqual(history.intervals(), []);
});

test('does not attribute work across module, poll or span row reuse', () => {
  const history = createSpanIntervals();
  const swapped = [{ ...modules[0]!, name: 'digikit/pulse' }];
  const origPoll = { owner: 0, fn: 0x1000, name: 'poll' };
  const newPoll = { ...origPoll, fn: 0x2000 };
  const push = (i: number, module: ModuleRef[], poll: typeof origPoll, row = 0) => {
    history.push(i * 1_000, reading(BigInt(i * 100), [{ ...span(i, BigInt(i * 10)), row }], { polls: [poll] }), module);
  };
  push(0, modules, origPoll);
  push(1, modules, origPoll);
  push(2, swapped, origPoll);
  push(3, swapped, newPoll);
  push(4, swapped, newPoll, 1);
  push(5, swapped, newPoll, 1);
  assert.deepEqual(history.intervals(), [
    { start: 0, end: 1_000, rows: [{ id: 'module:0', name: 'test/sine', calls: 1, share: 0.1 }] },
    { start: 4_000, end: 5_000, rows: [{ id: 'module:0', name: 'digikit/pulse', calls: 1, share: 0.1 }] },
  ]);
});

test('retains only intervals that overlap the latest 60 seconds', () => {
  const history = createSpanIntervals();
  for (let i = 0; i <= 31; i++) {
    history.push(i * 2_000, reading(BigInt(i), []), []);
  }
  assert.equal(history.intervals().length, 30);
  assert.equal(history.intervals()[0]?.start, 2_000);
  history.push(121_000, reading(32n, []), []);
  assert.deepEqual(history.intervals(), [{ start: 60_000, end: 62_000, rows: [] }]);
});
