import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cachedCard, sourcesHash } from '../runtime/card-cache.ts';

let root: string;
beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-card-cache-'));
  vi.stubEnv('DIGI_CARD_CACHE', root);
});
afterEach(() => {
  vi.unstubAllEnvs();
  fs.rmSync(root, { recursive: true, force: true });
});

const build = (work: string): Promise<string> => {
  const dir = path.join(work, 'out');
  fs.mkdirSync(dir);
  fs.writeFileSync(path.join(dir, 'card.img'), 'card');
  return Promise.resolve(path.join(dir, 'card.img'));
};

test('a built card carries its key as the content id the daemon puts in its snapshot key', async () => {
  const card = await cachedCard('abc123', build);
  expect(fs.readFileSync(path.join(path.dirname(card), 'card.key'), 'utf8')).toBe('abc123');
});

test('the hash of the card builder sources changes when a source changes', () => {
  fs.writeFileSync(path.join(root, 'builder.ts'), 'one');
  const before = sourcesHash(root, ['builder.ts']);
  expect(sourcesHash(root, ['builder.ts'])).toBe(before);
  fs.writeFileSync(path.join(root, 'builder.ts'), 'two');
  expect(sourcesHash(root, ['builder.ts'])).not.toBe(before);
});

test('a card dir of an earlier build gets its content id when it is reused', async () => {
  const dir = path.join(root, 'abc123');
  fs.mkdirSync(dir);
  fs.writeFileSync(path.join(dir, 'card.img'), 'card');
  const card = await cachedCard('abc123', () => Promise.reject(new Error('must not rebuild')));
  expect(fs.readFileSync(path.join(path.dirname(card), 'card.key'), 'utf8')).toBe('abc123');
});
