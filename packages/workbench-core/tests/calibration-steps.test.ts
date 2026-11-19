import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { MidiMap } from '@digi/devices';
import {
  parseInlineSteps,
  parseStepFile,
  parseTracks,
  resolveRef,
  resolveSteps,
  substitute,
  type StepLibrary,
} from '../src/calibration/steps';

const midi = (channels: number[]): MidiMap => ({
  trackChannels: channels,
  params: {
    filter_freq: { cc: 74, nrpn: [1, 20] },
    amp_release: { cc: 83, nrpn: [1, 34] },
    only_nrpn: { nrpn: [1, 99] },
  },
});
const ctx = (m: MidiMap | undefined = midi([1, 2, 3, 4, 5, 6, 7, 8])) => ({ midi: m, where: 't' });

test('the dk:dev key strings parse to the same steps as the Python parser makes', () => {
  // the cases of tools/patchkit/pk_devreload.py parse_keys: tap:KEY, wait:MS, turn:ENC[:BY], shot:NAME, *N, # comments
  assert.deepEqual(
    parseInlineSteps('tap:NO,tap:SETTINGS,wait:400,tap:DOWN*3,turn:ENC A:-2,turn:LEVEL,shot:module # done'),
    [
      { action: 'key', key: 'NO' },
      { action: 'key', key: 'SETTINGS' },
      { action: 'wait', ms: 400 },
      { action: 'key', key: 'DOWN' },
      { action: 'key', key: 'DOWN' },
      { action: 'key', key: 'DOWN' },
      { action: 'turn', encoder: 'ENC A', by: -2 },
      { action: 'turn', encoder: 'LEVEL', by: 1 },
      { action: 'shot', name: 'module' },
    ],
  );
  assert.deepEqual(parseInlineSteps('tap:YES\nwait:600\n\n'), [
    { action: 'key', key: 'YES' },
    { action: 'wait', ms: 600 },
  ]);
  assert.deepEqual(parseInlineSteps('wait:MOD_LOAD'), [{ action: 'wait', event: 'MOD_LOAD' }]);
  assert.throws(() => parseInlineSteps('press:YES'), /cannot read/);
  assert.throws(() => parseInlineSteps('tap'), /cannot read/);
  assert.throws(() => parseInlineSteps('turn:ENC A:x'), /cannot read/);
});

test('inline steps resolve to key, wait, turn and shot operations', () => {
  const ops = resolveSteps(parseInlineSteps('tap:YES,wait:5,turn:ENC A:2,shot:a'), {}, {}, ctx());
  assert.deepEqual(ops, [
    { op: 'key', key: 'YES' },
    { op: 'wait', ms: 5 },
    { op: 'turn', encoder: 'ENC A', by: 2 },
    { op: 'shot', name: 'a' },
  ]);
});

test('substitution fills {arg}, keeps a whole-string argument typed and refuses a missing one', () => {
  const v = substitute({ a: '{n}', b: 'x{n}y', c: ['{s}'] }, { n: 7, s: 'q' }, 'w');
  assert.deepEqual(v, { a: 7, b: 'x7y', c: ['q'] });
  assert.throws(() => substitute('{missing}', {}, 'step 2'), /step 2: argument \{missing\} has no value/);
});

test('track lists expand and refuse what is not a track', () => {
  assert.deepEqual(parseTracks('1,3,5-6', 'w'), [1, 3, 5, 6]);
  assert.deepEqual(parseTracks(2, 'w'), [2]);
  assert.throws(() => parseTracks('0', 'w'), /tracks are 1 to 16/);
  assert.throws(() => parseTracks('2-17', 'w'), /tracks are 1 to 16/);
  assert.throws(() => parseTracks('a', 'w'), /bad track list/);
});

test('named parameters resolve through the device table, one operation per track', () => {
  const def = { action: 'midi.cc', track: '1-2', param: 'filter_freq', value: 64 };
  assert.deepEqual(resolveSteps([def], {}, {}, ctx(midi([5, 6]))), [
    { op: 'midi.cc', ch: 5, cc: 74, value: 64 },
    { op: 'midi.cc', ch: 6, cc: 74, value: 64 },
  ]);
  // another device, another number and channel for the same step
  const other: MidiMap = { trackChannels: [9], params: { filter_freq: { cc: 16, nrpn: [1, 20] } } };
  assert.deepEqual(resolveSteps([{ ...def, track: 1 }], {}, {}, ctx(other)), [
    { op: 'midi.cc', ch: 9, cc: 16, value: 64 },
  ]);
  assert.deepEqual(resolveSteps([{ action: 'midi.nrpn', track: 1, param: 'amp_release', value: 127 }], {}, {}, ctx()), [
    { op: 'midi.nrpn', ch: 1, msb: 1, lsb: 34, value14: 16383 },
  ]);
  // a 7-bit value covers the whole 14-bit range: 0 is 0, 127 is 16383, 64 is 8256
  const nrpn = (value: number) =>
    (
      resolveSteps([{ action: 'midi.nrpn', track: 1, param: 'amp_release', value }], {}, {}, ctx())[0] as {
        value14: number;
      }
    ).value14;
  assert.deepEqual([nrpn(0), nrpn(64), nrpn(127)], [0, 8256, 16383]);
  assert.deepEqual(resolveSteps([{ action: 'midi.nrpn', track: 1, param: 'only_nrpn', value14: 5 }], {}, {}, ctx()), [
    { op: 'midi.nrpn', ch: 1, msb: 1, lsb: 99, value14: 5 },
  ]);
});

test('"all" is every track the device lists a channel for', () => {
  const ops = resolveSteps([{ action: 'midi.note', track: 'all', key: 60 }], {}, {}, ctx(midi([1, 2, 3])));
  assert.deepEqual(
    ops.map((o) => (o as { ch: number }).ch),
    [1, 2, 3],
  );
});

test('a wrong step is refused with where and why', () => {
  const r = (d: object, m?: MidiMap) => () => resolveSteps([d as never], {}, {}, ctx(m));
  assert.throws(r({ action: 'midi.cc', track: 1, param: 'nope', value: 1 }), /no MIDI parameter nope/);
  assert.throws(r({ action: 'midi.cc', track: 1, param: 'only_nrpn', value: 1 }), /only_nrpn has no CC/);
  assert.throws(
    r({ action: 'midi.cc', track: 9, param: 'filter_freq', value: 1 }),
    /no factory MIDI channel for track 9/,
  );
  assert.throws(
    r({ action: 'midi.cc', track: 1, param: 'filter_freq', value: 128 }),
    /value must be an integer 0 to 127/,
  );
  assert.throws(r({ action: 'midi.note', track: 1, key: 60, vel: 1 }), /unknown field vel/);
  assert.throws(
    r({ action: 'midi.cc', track: 1, param: 'filter_freq', value: 1 }, { trackChannels: [1], params: {} }),
    /no MIDI parameter/,
  );
  assert.throws(r({ action: 'wait', ms: 1, event: 'X' }), /exactly one of/);
  assert.throws(r({ action: 'capture', mark: 'middle' }), /start or stop/);
  assert.throws(r({ action: 'setup', machine: 1, filter: 2, track: 1 }), /one of machine/);
});

test('a step file parses, applies its defaults and takes the caller args over them', () => {
  const file = parseStepFile(
    `schema = "dk.steps/1"\nname = "n"\n[args]\ntracks = "1"\nkey = 60\n[[step]]\naction = "midi.note"\ntrack = "{tracks}"\nkey = "{key}"\n`,
    'n.toml',
  );
  const lib: StepLibrary = { n: file };
  assert.deepEqual(resolveRef({ steps: 'n' }, lib, ctx()), [{ op: 'midi.note', ch: 1, key: 60, velocity: 100 }]);
  assert.deepEqual(resolveRef({ steps: 'n', args: { tracks: '2', key: 40 } }, lib, ctx()), [
    { op: 'midi.note', ch: 2, key: 40, velocity: 100 },
  ]);
  assert.throws(() => resolveRef({ steps: 'zz' }, lib, ctx()), /no step file "zz"/);
  assert.deepEqual(resolveRef({ inline: 'wait:3' }, lib, ctx()), [{ op: 'wait', ms: 3 }]);
});

test('a step file with a wrong schema, an unknown key or an unknown action is refused', () => {
  const ok = `schema = "dk.steps/1"\nname = "n"\n[[step]]\naction = "midi.start"\n`;
  parseStepFile(ok, 'f');
  assert.throws(() => parseStepFile(ok.replace('dk.steps/1', 'dk.steps/2'), 'f'), /schema/);
  assert.throws(() => parseStepFile('extra = 1\n' + ok, 'f'), /unknown key extra/);
  assert.throws(() => parseStepFile(ok.replace('midi.start', 'midi.wiggle'), 'f'), /unknown action/);
  assert.throws(() => parseStepFile('schema = "dk.steps/1"\nname = "n"\n', 'f'), /no \[\[step\]\]/);
});
