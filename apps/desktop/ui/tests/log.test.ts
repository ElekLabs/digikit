// The pure logic of the workbench Log drawer (apps/desktop/ui/src/log-core.ts).
import assert from 'node:assert/strict';
import { test } from 'vitest';
import {
  LOG_CAP,
  addEntry,
  autoOpens,
  clearEntries,
  copyText,
  emptyLog,
  flashEntry,
  filterEntries,
  formatTime,
  healthChanges,
  healthStart,
  reloadEntry,
  reloadFailureEntry,
  transferEntry,
  unseenErrors,
  type LogInput,
  type LogState,
} from '../src/log-core.ts';

const at = (h: number, m: number, s: number) => new Date(2026, 9, 28, h, m, s).getTime();
const info = (text: string, source: LogInput['source'] = 'reload'): LogInput => ({ source, level: 'info', text });
const bad = (text: string, source: LogInput['source'] = 'reload'): LogInput => ({ source, level: 'error', text });
const fill = (inputs: LogInput[]): LogState => inputs.reduce((s, i, n) => addEntry(s, i, at(14, 0, n)), emptyLog());

test('the log keeps the newest 500 entries', () => {
  let s = emptyLog();
  for (let n = 0; n < LOG_CAP + 25; n++) s = addEntry(s, info(`e${n}`), 0);
  assert.equal(s.entries.length, LOG_CAP);
  assert.equal(s.entries[0].text, 'e25');
  assert.equal(s.entries.at(-1)!.text, `e${LOG_CAP + 24}`);
});

test('ids keep growing after a clear', () => {
  const s = clearEntries(fill([info('a'), info('b')]));
  assert.equal(s.entries.length, 0);
  assert.equal(addEntry(s, info('c'), 0).entries[0].id, 3);
});

test('the filter takes one source and errors only', () => {
  const s = fill([info('a'), bad('b', 'build'), info('c', 'card'), bad('d', 'card')]);
  const text = (f: Parameters<typeof filterEntries>[1]) => filterEntries(s.entries, f).map((e) => e.text);
  assert.deepEqual(text({ source: 'all', errorsOnly: false }), ['a', 'b', 'c', 'd']);
  assert.deepEqual(text({ source: 'card', errorsOnly: false }), ['c', 'd']);
  assert.deepEqual(text({ source: 'all', errorsOnly: true }), ['b', 'd']);
  assert.deepEqual(text({ source: 'card', errorsOnly: true }), ['d']);
});

test('the badge counts errors after the last one seen', () => {
  const s = fill([bad('a'), info('b'), bad('c'), bad('d')]);
  assert.equal(unseenErrors(s.entries, 0), 3);
  assert.equal(unseenErrors(s.entries, 3), 1);
  assert.equal(unseenErrors(s.entries, 4), 0);
});

test('the drawer opens for a failed reload, a failed build and an emulator fault only', () => {
  assert.equal(autoOpens(bad('x', 'reload')), true);
  assert.equal(autoOpens(bad('x', 'build')), true);
  assert.equal(autoOpens(bad('x', 'emulator')), true);
  assert.equal(autoOpens(info('x', 'reload')), false);
  assert.equal(autoOpens(bad('x', 'transfer')), false);
  assert.equal(autoOpens(bad('x', 'card')), false);
  assert.equal(autoOpens(bad('x', 'firmware')), false);
});

test('copy text has the time, the source, and the indented detail', () => {
  const s = addEntry(
    addEntry(emptyLog(), info('built a (248 ms)'), at(14, 2, 11)),
    { ...bad('build of a failed', 'build'), detail: 'line 1\nline 2\n' },
    at(14, 2, 40),
  );
  assert.equal(formatTime(at(9, 5, 3)), '09:05:03');
  assert.equal(
    copyText(s.entries),
    [
      '14:02:11  reload    built a (248 ms)',
      '14:02:40  build ✕   build of a failed',
      '          line 1',
      '          line 2',
    ].join('\n'),
  );
});

test('a reload that worked names the build and the swap', () => {
  const e = reloadEntry({
    path: 'swap',
    ms: 2300,
    ranges: 1,
    bytes: 1412,
    owners: ['test/ui-demo'],
    build: { built: [{ id: 'test/ui-demo', objects: [], ms: 248 }], skipped: [], ms: 248 },
  });
  assert.equal(e.text, 'built test/ui-demo (248 ms) · swapped 1412 B in test/ui-demo (2.3 s)');
  assert.equal(
    reloadEntry({ path: 'boot', seconds: 5.3, reason: 'core/runtime changed' }).text,
    'booted the new image, core/runtime changed (5.3 s)',
  );
});

test('a build error is a build entry with the compiler output as detail', () => {
  const e = reloadFailureEntry({
    status: 'Reload failed: build of test/ui-demo failed: ui_demo.c:117: error: invalid',
    full: 'Reload failed: build of test/ui-demo failed:\nui_demo.c:117: error: invalid',
  });
  assert.equal(e.source, 'build');
  assert.equal(e.level, 'error');
  assert.equal(e.text, 'build of test/ui-demo failed: ui_demo.c:117: error: invalid');
  assert.match(e.detail ?? '', /^build of test\/ui-demo failed:\nui_demo/);
  const other = reloadFailureEntry({ status: 'Reload failed: the running image has no patch record', full: '' });
  assert.deepEqual(other, { source: 'reload', level: 'error', text: 'failed: the running image has no patch record' });
});

test('a transfer run counts the files sent and skipped', () => {
  const lines = [
    'Installing a, b on dn2',
    'Uploading modules/a.dkm (2048 bytes)',
    'b: already on the card, not uploaded',
  ];
  const ok = transferEntry({ what: 'Installing', target: 'the emulator', message: 'On the card.', lines });
  assert.equal(ok.text, 'installed on the emulator · sent 1, skipped 1 · On the card.');
  assert.equal(ok.detail, lines.join('\n'));
  const failed = transferEntry({ what: 'Uploading', target: 'the emulator', error: 'no reply\nmore', lines: [] });
  assert.deepEqual([failed.level, failed.text], ['error', 'uploading failed: no reply']);
});

test('a fault, a stall and a DSP halt are logged once, when they start', () => {
  let h = healthStart();
  const step = (r: Parameters<typeof healthChanges>[1]) => {
    const out = healthChanges(h, r);
    h = out.health;
    return out.entries.map((e) => `${e.level} ${e.text}`);
  };
  assert.deepEqual(step({ error: null }), []);
  assert.deepEqual(step({ error: 'GUEST_FAULT at 0x4011' }), ['error fault: GUEST_FAULT at 0x4011']);
  assert.deepEqual(step({ error: 'GUEST_FAULT at 0x4011' }), []);
  assert.deepEqual(step({ stalled: true, stallMs: 1200 }), ['error stall: the main loop stopped for more than 1.2 s']);
  assert.deepEqual(step({ stalled: true }), []);
  assert.deepEqual(step({ stalled: false }), ['info stall ended']);
  assert.deepEqual(step({ halted: 'bad opcode' }), ['error DSP halt: bad opcode']);
  assert.deepEqual(step({ halted: 'bad opcode', error: 'GUEST_FAULT at 0x4011' }), []);
  // a reading that leaves a field out does not clear it
  assert.equal(h.error, 'GUEST_FAULT at 0x4011');
});

test('a flash of the Firmware page is logged as the firmware source, an error as an error', () => {
  assert.deepEqual(flashEntry({ level: 'info', text: 'flash dn2 OS 1.12 to Elektron Digitone II: 19290 messages' }), {
    source: 'firmware',
    level: 'info',
    text: 'flash dn2 OS 1.12 to Elektron Digitone II: 19290 messages',
  });
  const done = flashEntry({
    level: 'info',
    text: 'flash done: Restarted',
    detail: 'Runtime 0.5.2\nImage id c5d8ccb27e139cbc',
  });
  assert.equal(done.detail, 'Runtime 0.5.2\nImage id c5d8ccb27e139cbc');
  const failed = flashEntry({ level: 'error', text: 'flash failed: the image id differs' });
  assert.equal(failed.level, 'error');
  assert.equal(autoOpens(failed), false); // the Firmware page shows its own error; the Log does not open by itself
});
