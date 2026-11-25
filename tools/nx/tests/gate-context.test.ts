import { resolve } from 'node:path';
import { expect, it } from 'vitest';
import { gateContext } from '../gate-context.mjs';

const root = resolve(__dirname, '../../..');
const ctx = gateContext(root);

it('reads the users of a firmware declaration file of the checkout', () => {
  expect(ctx.portUsers('ports/fw/no-such-point.toml')).toEqual([]);
  const users = ctx.portUsers('ports/fw/draw.toml');
  expect(users.length).toBeGreaterThan(0);
  expect(users.every((u: string) => typeof u === 'string')).toBe(true);
});

it('names the device a device file belongs to, and no device for another file', () => {
  expect(ctx.deviceOf('devices/digitakt-ii.toml')).toBeTruthy();
  expect(ctx.deviceOf('docs/TESTING.md')).toBeUndefined();
});
