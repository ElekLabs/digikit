import { nameHash, runtimeEvent } from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect, test } from 'vitest';

const event = (args: number[]) => runtimeEvent({ seq: 9, time: 3, kind: LOG_KIND.LIFECYCLE, flags: 0, args });
const info = (phase: number, step: number, kind: number, record: number) =>
  phase | (step << 8) | (kind << 16) | ((record << 24) >>> 0);

test('a LIFECYCLE event names its operation, package, phase, step and status', () => {
  const hash = nameHash('modules/local-ping-user.dkm');
  const e = event([4, hash, info(1, 3, 0, 2), 0xfffffffb, 0]);
  expect(e.kind).toBe('LIFECYCLE');
  expect(e.fields).toEqual({
    op: 4,
    hash: hash.toString(16).padStart(8, '0'),
    phase: 'end',
    step: 'start',
    kind: 'module',
    package: 2,
    status: -5,
    detail: 0,
  });
});

test('a service and a package that has no record yet read as such', () => {
  const e = event([1, 0, info(2, 17, 1, 0xff), 0, 1]);
  expect(e.fields).toMatchObject({ phase: 'note', step: 'park_wait', kind: 'service', package: null, detail: 1 });
});

test('the name hash is the FNV-1a of the file stem, the same for a path and a bare name', () => {
  expect(nameHash('services/ping.dks')).toBe(nameHash('ping'));
  expect(nameHash('a')).toBe(0xe40c292c);
});
