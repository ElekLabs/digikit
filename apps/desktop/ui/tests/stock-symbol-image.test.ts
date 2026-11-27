import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { Host } from '@digi/workbench-core/host/host.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { selectedSymbolImage } from '../src/log-names-backend';

const stock = 'ab'.repeat(32);
const patched = 'ab'.repeat(31) + 'cd';

function host(response: (method: string) => unknown): Host {
  return { rpc: { call: async <T>(method: string) => response(method) as T } } as Host;
}

function binding(check: () => void = () => {}): TargetBinding {
  return {
    target: { kind: 'emulator' },
    check,
    beforeLeave: () => () => true,
    session: async () => {
      throw new Error('emulator identity comes from the controller');
    },
  };
}

test('a stock emulator can match registered symbols using the loaded SYX hash', async () => {
  const control = host((method) => {
    if (method === 'runtime.maps') throw new Error('the running image has no patch record');
    return { firmware: { syx_sha256: stock.toUpperCase() } };
  });
  assert.equal(await selectedSymbolImage(control, binding()), stock);
});

test('missing maps cannot turn a patched image into a match for the stock base', async () => {
  const control = host((method) => {
    if (method === 'runtime.maps') throw new Error('the stock image is missing');
    return { firmware: { base_sha256: stock, syx_sha256: patched } };
  });
  assert.equal(await selectedSymbolImage(control, binding()), patched);
});

test('an explicit patched map mismatch refuses identity even when status has the stock hash', async () => {
  const control = host((method) =>
    method === 'runtime.maps'
      ? { ship: JSON.stringify({ key: { image_sha256: stock } }), symbols: {}, matches: false }
      : { firmware: { syx_sha256: stock } },
  );
  assert.equal(await selectedSymbolImage(control, binding()), undefined);
});

test('the selected target changing during status prevents use of the fallback identity', async () => {
  let current = true;
  const control = host((method) => {
    if (method === 'runtime.maps') throw new Error('the running image has no patch record');
    current = false;
    return { firmware: { syx_sha256: stock } };
  });
  await assert.rejects(
    selectedSymbolImage(
      control,
      binding(() => assert.ok(current, 'target changed')),
    ),
    /target changed/,
  );
});
