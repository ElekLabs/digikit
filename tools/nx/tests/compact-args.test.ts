import { expect, it } from 'vitest';
import { compactArgs } from '../nx.mjs';

it('adds the quiet style before the arguments for the target', () => {
  expect(compactArgs(['run', 'p:t', '--', '-t', 'x'], {})).toEqual([
    'run',
    'p:t',
    '--output-style=static-failures-only',
    '--',
    '-t',
    'x',
  ]);
});

it('keeps Nx output with --verbose, DIGI_NX_VERBOSE, another style, or a command that runs no task', () => {
  expect(compactArgs(['run', 'p:t', '--verbose'], {})).toBeUndefined();
  expect(compactArgs(['run', 'p:t'], { DIGI_NX_VERBOSE: '1' })).toBeUndefined();
  expect(compactArgs(['run', 'p:t', '--output-style=stream'], {})).toBeUndefined();
  expect(compactArgs(['show', 'projects'], {})).toBeUndefined();
  expect(compactArgs(['run', 'p:t'], { DIGI_NX_VERBOSE: '0' })).toBeDefined();
});

it('takes the style the gate already passes', () => {
  const args = ['affected', '--output-style=static-failures-only'];
  expect(compactArgs(args, {})).toEqual(args);
});
