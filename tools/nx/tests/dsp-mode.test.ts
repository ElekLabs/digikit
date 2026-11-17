import { expect, it } from 'vitest';
import { checkDspModes, dspModeOf, taskMode } from '../dsp-mode.mjs';
import { startAtOnce } from '../resources.mjs';

const helpers = [
  `export function withFx() { return startSession({ dsp: 'sharc' }); }
   export function withFxSuite() { return withFx(); }
   export function plain() { return startSession({}); }
   export function upload(opts: { sharc?: boolean }) { return rig({ dsp: opts.sharc ? 'sharc' : 'zero' }); }`,
];

it('derives sharc from a literal, from a helper chain, and from an option set to true', () => {
  expect(dspModeOf(`start({ dsp: 'sharc' });`)).toBe('sharc');
  expect(dspModeOf(`withFxSuite();`, helpers)).toBe('sharc');
  expect(dspModeOf(`upload(body, { sharc: true });`, helpers)).toBe('sharc');
});

it('derives zero when no coupled session is started, and ignores comments and the other option value', () => {
  expect(dspModeOf(`plain(); // pass dsp: 'sharc' for audio\n/* dsp: 'sharc' */`, helpers)).toBe('zero');
  expect(dspModeOf(`upload(body, { sharc: false });`, helpers)).toBe('zero');
  expect(dspModeOf(`start({ dsp: 'zero' });`)).toBe('zero');
});

it('names the file and the mode when a record disagrees with its source', () => {
  const none = () => [];
  expect(() => checkDspModes('/nowhere', 'p', [{ files: ['a.device.test.ts'], dsp: 'warm' }], none)).toThrow(
    /not zero or sharc/,
  );
  expect(taskMode(['a.ts'], [{ files: ['a.ts', 'b.ts'], dsp: 'sharc' }])).toBe('sharc');
  expect(taskMode(['a.ts'], [{ files: ['a.ts'] }, { files: ['b.ts'], dsp: 'sharc' }])).toBe('zero');
});

it('starts coupled tasks first within the core budget', () => {
  expect(startAtOnce({ sharc: 57, zero: 9 }, 12, 12)).toEqual({ sharc: 4, zero: 0 });
  expect(startAtOnce({ sharc: 2, zero: 20 }, 12, 12)).toEqual({ sharc: 2, zero: 6 });
  expect(startAtOnce({ sharc: 0, zero: 20 }, 12, 12)).toEqual({ sharc: 0, zero: 12 });
});

it('counts a dsp value it cannot resolve as coupled', () => {
  expect(dspModeOf(`start({ dsp: mode });`)).toBe('sharc');
  expect(dspModeOf(`start({ dsp: await pick() });`)).toBe('sharc');
  expect(dspModeOf(`start({ dsp: fast ? 'zero' : slow });`)).toBe('sharc');
  expect(dspModeOf(`start({ dsp: 'zero' });`)).toBe('zero');
  // the caller's own value passes through; the caller decides
  expect(dspModeOf(`start({ dsp: opts.dsp });`)).toBe('zero');
  expect(dspModeOf(`start({ ...opts, dsp: resolveDsp(given) });`)).toBe('zero');
});

it('turns an option-conditional helper coupled only for a caller that sets the option', () => {
  const kit = [`export function rig(o) { return start({ dsp: o.initial === 'small' ? 'cold' : o.dsp }); }`];
  expect(dspModeOf(`rig({ initial: 'small' });`, kit)).toBe('sharc');
  expect(dspModeOf(`rig({ initial: 'big' });`, kit)).toBe('zero');
});
