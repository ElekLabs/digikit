import assert from 'node:assert/strict';
import { test } from 'vitest';
import { readSamplePanes } from '../src/samples-read.ts';

test('the local pane loads while the device read is still waiting', async () => {
  let finishDevice!: () => void;
  const device = new Promise<void>((resolve) => (finishDevice = resolve));
  let localVisible = false;
  let complete = false;
  const read = readSamplePanes(
    async () => void (localVisible = true),
    () => device,
    String,
  ).then(() => void (complete = true));
  await Promise.resolve();
  assert.equal(localVisible, true);
  assert.equal(complete, false, 'the device is still waiting');
  finishDevice();
  await read;
  assert.equal(complete, true);
});

test('both panes are attempted and both errors are reported', async () => {
  let deviceRead = false;
  await assert.rejects(
    readSamplePanes(
      async () => {
        throw new Error('folder unavailable');
      },
      async () => {
        deviceRead = true;
        throw new Error('device unavailable');
      },
      (e) => (e as Error).message,
    ),
    /^Error: folder unavailable\ndevice unavailable$/,
  );
  assert.equal(deviceRead, true);
});
