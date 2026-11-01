import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { button, type Session } from '@digi/test-support';
import { expect, test, vi } from 'vitest';
import type { PatchedSession } from '../support.ts';
import { evidence, pickSine } from '../runtime/fx-live.ts';

test('numeric input hint controls list movement and a failed chord still releases FUNC', async () => {
  const client = { inputButton: vi.fn(), inputTap: vi.fn(), step: vi.fn() };
  const session = { options: { device: 'dn2' }, client } as unknown as Session;
  await pickSine(session, 2);
  expect(client.inputTap.mock.calls.filter(([code]) => code === button('DOWN'))).toHaveLength(2);
  client.inputTap.mockRejectedValueOnce(new Error('panel failed'));
  await expect(pickSine(session, 0)).rejects.toThrow('panel failed');
  expect(client.inputButton).toHaveBeenLastCalledWith(button('FUNC'), false);
});

test('silent evidence preserves mandatory finite, guard and no-fault checks; default still requires sound', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-fx-evidence-'));
  let nonfinite = 0;
  let hits = 0;
  const expectNoFault = vi.fn();
  const session = {
    client: {
      guardReport: vi.fn(async () => ({ total: hits, hits: [] })),
      call: vi.fn(async (method: string) =>
        method === 'pcm.tap' ? { peak: 0, non_finite: nonfinite } : { sectors: 0 },
      ),
    },
    status: vi.fn(async () => ({ mode: 'paused' })),
    expectNoFault,
  } as unknown as Session;
  const runtime = { session } as PatchedSession;
  const overlays: string[] = [];
  try {
    await expect(evidence(runtime, directory, 'silent', overlays, false)).resolves.toBeUndefined();
    expect(expectNoFault).toHaveBeenCalledWith('silent');
    await expect(evidence(runtime, directory, 'default', overlays)).rejects.toThrow('input is audible');
    nonfinite = 1;
    await expect(evidence(runtime, directory, 'nonfinite', overlays, false)).rejects.toThrow();
    nonfinite = 0;
    hits = 1;
    await expect(evidence(runtime, directory, 'guard', overlays, false)).rejects.toThrow('guard:');
    hits = 0;
    expectNoFault.mockRejectedValueOnce(new Error('DSP halt'));
    await expect(evidence(runtime, directory, 'fault', overlays, false)).rejects.toThrow('DSP halt');
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
