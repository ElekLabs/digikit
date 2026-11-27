// The data side of the desktop's default modules: the list comes from the device files and the modules' own module.toml; the wait for the runtime.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError } from '@digi/dk-session';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { entries, summaryOf, waitForRuntime } from '../src/default-modules-data.ts';

test('every runtime device file lists digikit/debug first, and each entry has the summary of its module.toml', () => {
  for (const device of registry.runtimeIds) {
    const list = entries(device);
    assert.deepEqual(
      list.map((e) => e.id),
      registry.get(device).defaultModules,
    );
    assert.equal(list[0]!.id, 'digikit/debug');
    assert.match(list[0]!.summary, /log/);
    for (const e of list.slice(1)) assert.equal(e.id, 'digikit/remote');
    for (const e of list.slice(1)) assert.match(e.summary, /USB MIDI/);
  }
  assert.deepEqual(entries('st1'), []);
});

test('summaryOf finds a module by id and says nothing for an unknown one', () => {
  const texts = ['[module]\nid = "x/one"\nsummary = "One."\n', '[module]\nid = "x/two"\nsummary = "Two."\n'];
  assert.equal(summaryOf(texts, 'x/two'), 'Two.');
  assert.equal(summaryOf(texts, 'x/none'), '');
});

test('waitForRuntime asks again until a runtime answers, and gives up with the last reason', async () => {
  let t = 0;
  const now = () => t;
  const sleep = async (ms: number) => void (t += ms);
  const answers: (() => { runtime: number })[] = [
    () => {
      throw new Error('no reply from the device');
    },
    () => ({ runtime: 0 }),
    () => ({ runtime: 5 }),
  ];
  let i = 0;
  await waitForRuntime({ info: async () => answers[i++]!() as never }, { ms: 10_000, everyMs: 1000 }, sleep, now);
  assert.equal(i, 3);
  assert.equal(t, 2000);
  await assert.rejects(
    waitForRuntime(
      {
        info: async () => {
          throw new Error('no reply from the device');
        },
      },
      { ms: 3000, everyMs: 1000 },
      sleep,
      now,
    ),
    /does not answer INFO within 3 s \(no reply from the device\)/,
  );
  await assert.rejects(
    waitForRuntime(
      {
        info: async () => {
          throw new DkError({ error: 'cancelled', kind: 'cancelled' });
        },
      },
      { ms: 3000, everyMs: 1000 },
      sleep,
      now,
    ),
    DkError,
  );
});
