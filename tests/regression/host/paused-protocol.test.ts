import { expect, test } from 'vitest';
import { RuntimeDeadlineError, type Session } from '@digi/test-support';
import { withPausedFxProtocol } from '../runtime/fx-session.ts';
import { type PatchedSession } from '../test-runtime.ts';

const runtimeOf = (call: () => Promise<unknown>) =>
  ({ session: { status: async () => ({ mode: 'paused' }), client: { call } } as unknown as Session }) as Pick<
    PatchedSession,
    'session'
  >;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

test('an operation longer than the hang bound ends when it ends: wall time guards a hang, not a long operation', async () => {
  const result = await withPausedFxProtocol(
    runtimeOf(async () => ({})),
    async () => {
      await sleep(150);
      return 'done';
    },
    { hangMs: 50 },
  );
  expect(result).toBe('done');
});

test('a control call that never answers fails after one hang bound', async () => {
  const never = () => new Promise<never>(() => {});
  await expect(withPausedFxProtocol(runtimeOf(never), (dk) => dk.reloadState(), { hangMs: 50 })).rejects.toBeInstanceOf(
    RuntimeDeadlineError,
  );
});
