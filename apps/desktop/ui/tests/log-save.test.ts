// The Save button of the Log drawer (apps/desktop/ui/src/log-save.ts) over a FakeHost.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';
import { saveLog } from '../src/log-save.ts';
import { saveName, saveText, type LogEntry } from '../src/log-core.ts';

const entries: LogEntry[] = [{ id: 1, at: 1000, source: 'transfer', level: 'info', text: 'module sine packed' }];

test('the text of the visible entries goes to the save dialog under the dated name', async () => {
  const host = createFakeHost({ 'fs.saveLog': '/Users/em/Downloads/log.log' });
  const where = await saveLog(host.fs, entries, 5000);
  assert.equal(where, '/Users/em/Downloads/log.log');
  const [[bytes, name]] = host.argsOf('fs.saveLog') as [[Uint8Array, string]];
  assert.equal(new TextDecoder().decode(bytes), saveText(entries, 5000));
  assert.equal(name, saveName(5000));
});

test('a cancelled dialog answers null', async () => {
  const host = createFakeHost({ 'fs.saveLog': null });
  assert.equal(await saveLog(host.fs, entries, 5000), null);
});
