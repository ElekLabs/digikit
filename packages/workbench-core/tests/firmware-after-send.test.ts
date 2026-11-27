// The end of a flash (firmware-flow.ts `afterSend`) for an image with and without the runtime: a patch without it (the Syntakt's first step) is done when the
// stock OS answers and no runtime does; a patch with it still fails when the device comes back without its runtime.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { afterSend, LIMITS } from '../src/firmware/firmware-flow.ts';
import type { Answer, FlashState } from '../src/firmware/firmware-core.ts';
import type { PreparedFlash } from '../src/patch/patchkit-core.ts';

const prepared = (over: Partial<PreparedFlash>): PreparedFlash =>
  ({
    ok: true,
    restore: false,
    device: 'dn2',
    os: '1.12',
    shaStock: '',
    shaImage: '',
    imageId: 'c5d8ccb27e139cbc',
    runtime: '0.5.2',
    paceMs: 2,
    checks: [],
    ends: [],
    image: new Uint8Array(0),
    ...over,
  }) as PreparedFlash;

/** A device that left and came back at once, then answers with `answer`. The clock is virtual: a sleep moves it. */
async function end(p: PreparedFlash, answer: Answer): Promise<FlashState> {
  let t = 0;
  let polls = 0;
  const limits = { ...LIMITS, leaveMs: 1000, returnMs: 5000, askMs: 20_000 };
  return afterSend(
    {
      now: () => t,
      sleep: async (ms) => {
        t += ms;
      },
      release: async () => {},
      // the port is gone on the first poll and back on the next
      ports: async () => (polls++ === 0 ? [] : ['Dev']),
      ask: async () => answer,
      port: 'Dev',
    },
    p,
    new AbortController().signal,
    () => {},
    limits,
    true,
  );
}

test('a patch without the runtime is done when the stock OS answers and no runtime does', async () => {
  const s = await end(prepared({ imageId: null, runtime: null }), {
    os: '1.12',
    runtime: undefined,
    imageId: undefined,
  });
  assert.equal(s.phase, 'done');
  assert.match((s as { message: string }).message, /patched without a runtime/);
});

test('a patch without the runtime fails when a runtime still answers', async () => {
  const s = await end(prepared({ imageId: null, runtime: null }), { os: '1.12', runtime: '0.5.2', imageId: 'aa' });
  assert.equal(s.phase, 'failed');
  assert.equal((s as { code: string }).code, 'still-runtime');
});

test('a patch with the runtime fails when the device comes back without one', async () => {
  const s = await end(prepared({}), { os: '1.12', runtime: undefined, imageId: undefined });
  assert.equal(s.phase, 'failed');
  assert.equal((s as { code: string }).code, 'runtime');
});

test('a patch with the runtime is done when its runtime and image id answer', async () => {
  const s = await end(prepared({}), { os: '1.12', runtime: '0.5.2', imageId: 'c5d8ccb27e139cbc' });
  assert.equal(s.phase, 'done');
});
