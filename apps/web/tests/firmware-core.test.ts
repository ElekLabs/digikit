// The Firmware page's rules (apps/web/src/firmware/firmware-core.ts): the check labels, when Flash is allowed, the device match, the progress
// text and the verdict after the restart.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { PreparedFlash } from '../src/patch/patchkit-core.ts';
import {
  IDLE,
  canFlash,
  confirmText,
  checkRows,
  groups,
  imageFileName,
  judge,
  matchView,
  modelOfPort,
  progressView,
  refusalText,
  stepMarks,
  timeLeft,
  type DeviceState,
  type FlashInputs,
  type FlashState,
} from '../src/firmware/firmware-core.ts';

const CHECKS = [
  'stock',
  'patch',
  'preflight',
  'verify',
  'product',
  'build floor',
  'DSP scan',
  'round trip',
  'image id',
];
function prepared(over: Partial<PreparedFlash> = {}): PreparedFlash {
  return {
    ok: true,
    restore: false,
    device: 'dn2',
    os: '1.12',
    shaStock: '88bc' + '0'.repeat(60),
    shaImage: '1'.repeat(64),
    imageId: 'c5d8ccb27e139cbc',
    runtime: '0.5.2',
    paceMs: 2,
    checks: CHECKS.map((name) => ({ name, ok: true, detail: `${name} detail` })),
    ends: Array.from({ length: 19290 }, (_, i) => (i + 1) * 128),
    image: new Uint8Array(0),
    ...over,
  };
}
const connected = (model: 'dt2' | 'dn2' | undefined, os = '1.12'): DeviceState => ({
  kind: 'connected',
  d: { port: 'Elektron Digitone II', model, os },
});
const inputs = (over: Partial<FlashInputs> = {}): FlashInputs => ({
  prepare: { kind: 'ready', name: 'Digitone_II_OS1.12.syx', size: 1, p: prepared() },
  device: connected('dn2'),
  ack: true,
  run: IDLE,
  route: 'menu',
  ...over,
});

test('the checks have page names, and the runtime version joins the patch', () => {
  const rows = checkRows(prepared());
  assert.equal(rows.length, CHECKS.length);
  assert.equal(rows.find((r) => r.name === 'patch')?.label, 'Patched (runtime 0.5.2)');
  assert.equal(rows.find((r) => r.name === 'verify')?.label, 'Checksums, HMAC and size');
  assert.ok(rows.every((r) => r.ok && r.detail));
  // a check this page does not know is shown as patchkit named it
  assert.equal(checkRows(prepared({ checks: [{ name: 'new check', ok: false, detail: 'x' }] }))[0]?.label, 'new check');
});

test('Flash is on only with passed checks, the right device and the ticked box', () => {
  assert.deepEqual(canFlash(inputs()), { ok: true, reason: '' });
  // no file, a file being checked, a file that cannot be used
  for (const prepare of [
    { kind: 'none' },
    { kind: 'working', name: 'x' },
    { kind: 'error', name: 'x', message: 'no' },
  ] as const) {
    assert.equal(canFlash(inputs({ prepare })).ok, false, prepare.kind);
  }
  // one failed check
  const bad = prepared({ ok: false, checks: [{ name: 'verify', ok: false, detail: 'HMAC fails' }] });
  assert.match(canFlash(inputs({ prepare: { kind: 'ready', name: 'x', size: 1, p: bad } })).reason, /check failed/);
  // the device: none, looking, lost, another model, no model
  const none: DeviceState = { kind: 'none', why: 'No MIDI port.' };
  assert.equal(canFlash(inputs({ device: none })).reason, 'No MIDI port.');
  assert.equal(canFlash(inputs({ device: { kind: 'checking' } })).ok, false);
  assert.equal(canFlash(inputs({ device: { kind: 'lost', reason: 'x' } })).ok, false);
  assert.match(
    canFlash(inputs({ device: connected('dt2') })).reason,
    /This is a Digitakt II\. The image is for a Digitone II/,
  );
  assert.match(canFlash(inputs({ device: connected(undefined) })).reason, /does not say which device/);
  // the box
  assert.match(canFlash(inputs({ ack: false })).reason, /WAITING FOR SYSEX/);
  // a run in progress
  const sending = { phase: 'sending', sent: 1, total: 2, startedMs: 0, nowMs: 1 } as const;
  assert.equal(canFlash(inputs({ run: sending })).ok, false);
});

test('the OS of the image is not compared with the OS of the device: both are shown', () => {
  const m = matchView(prepared(), connected('dn2', '1.10'));
  assert.equal(m.state, 'match');
  assert.match(m.os!, /The device runs OS 1\.10\. The image installs OS 1\.12 with digikit runtime 0\.5\.2\./);
  assert.equal(canFlash(inputs({ device: connected('dn2', '1.10') })).ok, true);
  const r = matchView(prepared({ restore: true, runtime: null, imageId: null }), connected('dn2', '1.12'));
  assert.match(r.os!, /installs OS 1\.12 \(stock\)/);
  assert.equal(matchView(prepared(), { kind: 'checking' }).state, 'none');
});

test('a port name tells the model when INFO cannot', () => {
  assert.equal(modelOfPort('Elektron Digitakt II'), 'dt2');
  assert.equal(modelOfPort('Digitone II MIDI 1'), 'dn2');
  assert.equal(modelOfPort('IAC Driver Bus 1'), undefined);
});

test('the progress reads as sent of total and the time left', () => {
  assert.equal(groups(9812), '9 812');
  assert.equal(groups(18674), '18 674');
  // 9 812 of 18 674 messages in 20 s: 8 862 are left at 2.04 ms each, 18 s
  assert.equal(timeLeft(9812, 18674, 0, 20_000), '18 s left');
  assert.equal(timeLeft(10, 18674, 0, 100), ''); // the rate is not known yet
  assert.equal(timeLeft(1000, 19290, 0, 2000), '37 s left');
  assert.match(timeLeft(1000, 100_000, 0, 2000), /^3 min \d+ s left$/);
  const v = progressView({ phase: 'sending', sent: 9812, total: 18674, startedMs: 0, nowMs: 20_000 });
  assert.equal(v.text, '9 812 / 18 674 · 18 s left');
  assert.ok(Math.abs(v.fraction - 0.5254) < 0.001);
});

test('after a patch the device must report the runtime and the image id of the patch', () => {
  const p = prepared();
  const ok = judge(p, { os: '1.12', runtime: '0.5.2', imageId: 'c5d8ccb27e139cbc' });
  assert.equal(ok.ok, true);
  const old = judge(p, { os: '1.12', runtime: '0.5.1', imageId: 'c5d8ccb27e139cbc' });
  assert.deepEqual(old.ok === false && old.code, 'runtime');
  const other = judge(p, { os: '1.12', runtime: '0.5.2', imageId: '0000000000000000' });
  assert.deepEqual(other.ok === false && other.code, 'image-id');
  assert.match(other.ok ? '' : other.message, /device reports 0000000000000000, the patch made c5d8ccb27e139cbc/);
  const noId = judge(p, { os: '1.12', runtime: '0.5.2' });
  assert.deepEqual(noId.ok === false && noId.code, 'image-id');
});

test('after a restore the device reports the stock OS and no runtime', () => {
  const p = prepared({ restore: true, runtime: null, imageId: null });
  assert.equal(judge(p, { os: '1.12' }).ok, true);
  const still = judge(p, { os: '1.12', runtime: '0.5.2', imageId: 'c5d8ccb27e139cbc' });
  assert.deepEqual(still.ok === false && still.code, 'still-runtime');
  const os = judge(p, { os: '1.11' });
  assert.deepEqual(os.ok === false && os.code, 'os');
  assert.deepEqual(judge(p, {}).ok === false && 'os', 'os');
});

test('the step markers follow the inputs', () => {
  assert.deepEqual(stepMarks(inputs({ prepare: { kind: 'none' }, device: { kind: 'checking' }, ack: false })), [
    'now',
    'later',
    'later',
    'later',
    'later',
    'later',
  ]);
  assert.deepEqual(stepMarks(inputs({ ack: false })), ['done', 'done', 'done', 'now', 'later', 'later']);
  assert.deepEqual(stepMarks(inputs()), ['done', 'done', 'done', 'done', 'now', 'later']);
  assert.deepEqual(stepMarks(inputs({ device: connected('dt2') })).slice(2, 3), ['bad']);
  // sending: the steps before stay done; a failed run marks the last two
  const sending = { phase: 'sending', sent: 1, total: 2, startedMs: 0, nowMs: 1 } as const;
  assert.deepEqual(stepMarks(inputs({ ack: false, run: sending })), ['done', 'done', 'done', 'done', 'now', 'later']);
  assert.deepEqual(stepMarks(inputs({ run: { phase: 'failed', code: 'runtime', message: 'm' } })).slice(4), [
    'bad',
    'bad',
  ]);
  assert.deepEqual(
    stepMarks(inputs({ ack: false, run: { phase: 'done', message: 'm', lines: [] } })),
    Array(6).fill('done'),
  );
  const bad = prepared({ ok: false });
  assert.deepEqual(stepMarks(inputs({ prepare: { kind: 'ready', name: 'x', size: 1, p: bad } })).slice(0, 3), [
    'done',
    'bad',
    'later',
  ]);
});

test('the image file name tells the device, the OS and the runtime', () => {
  assert.equal(imageFileName(prepared()), 'Digitone_II_OS1.12+digikit-0.5.2.syx');
  assert.equal(imageFileName(prepared({ restore: true, runtime: null })), 'Digitone_II_OS1.12.syx');
});

test('a refused file gets a lead-in unless patchkit already says which file it was', () => {
  assert.equal(
    refusalText('expected F0 at offset 0'),
    'This is not an Elektron firmware update file: expected F0 at offset 0',
  );
  assert.equal(
    refusalText('this file is a patched image; give the stock file it was made from (sha256 abc)'),
    'This file is a patched image; give the stock file it was made from (sha256 abc)',
  );
});

test('the RPC route needs no box, and its seven steps follow the run', () => {
  assert.deepEqual(canFlash(inputs({ route: 'rpc', ack: false })), { ok: true, reason: '' });
  assert.match(canFlash(inputs({ route: 'menu', ack: false })).reason, /WAITING FOR SYSEX/);
  const at = (run: FlashState) => stepMarks(inputs({ route: 'rpc', ack: false, run }));
  assert.deepEqual(at(IDLE), ['done', 'done', 'done', 'done', 'now', 'later', 'later']);
  const sending: FlashState = { phase: 'sending', sent: 1, total: 2, startedMs: 0, nowMs: 1, unit: 'bytes' };
  assert.deepEqual(at(sending), ['done', 'done', 'done', 'done', 'now', 'later', 'later']);
  assert.deepEqual(at({ phase: 'confirm', total: 2 }), ['done', 'done', 'done', 'done', 'done', 'now', 'later']);
  assert.deepEqual(at({ phase: 'rebooting', stage: 'leaving', sinceMs: 0, nowMs: 1, rpc: true }), [
    'done',
    'done',
    'done',
    'done',
    'done',
    'done',
    'now',
  ]);
  assert.deepEqual(at({ phase: 'stopped', reason: 'error', sent: 1, total: 2, message: 'm', unit: 'bytes' }).slice(4), [
    'bad',
    'later',
    'later',
  ]);
  assert.deepEqual(at({ phase: 'stopped', reason: 'cancelled', sent: 1, total: 2, message: 'm' }).slice(4), [
    'now',
    'later',
    'later',
  ]);
  assert.deepEqual(at({ phase: 'done', message: 'm', lines: [] }), Array(7).fill('done'));
  // the question is part of a running flash: nothing else may start
  assert.equal(canFlash(inputs({ route: 'rpc', run: { phase: 'confirm', total: 2 } })).ok, false);
});

test('the RPC progress reads in megabytes, and says when the device checks the image', () => {
  const s = { phase: 'sending', sent: 1_531_152, total: 1_531_152, startedMs: 0, nowMs: 5000, unit: 'bytes' } as const;
  assert.match(progressView({ ...s, sent: 765_576 }).text, /^received 0\.77 MB of 1\.53 MB/);
  assert.equal(progressView({ ...s, checking: true }).text, 'received 1.53 MB · the device is checking the image…');
  assert.equal(confirmText(prepared()), 'The Digitone II has the new firmware and accepted it.');
  assert.equal(confirmText(prepared({ restore: true })), 'The Digitone II has the stock firmware and accepted it.');
});
