import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { Device } from '@digi/test-support';
import { FIRMWARE } from '../runtime/devices.ts';
import { cachedCard, cardSpecKey, sourcesHash } from '../runtime/card-cache.ts';

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

test('a card built on a base, whose build already wrote a read-only card.key, takes the cache key', async () => {
  // linkCard writes the derived card's id read-only (write-safety.ts); the cache then sets its own key on the same file
  const card = await cachedCard('abc123', (work) => {
    const dir = path.join(work, 'out');
    fs.mkdirSync(dir);
    fs.writeFileSync(path.join(dir, 'card.img'), 'card');
    fs.writeFileSync(path.join(dir, 'card.key'), 'base-and-overlay', { mode: 0o444 });
    return Promise.resolve(path.join(dir, 'card.img'));
  });
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

const device = Object.keys(FIRMWARE)[0] as Device;
const put = (file: string, text: string) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
  return file;
};

test('a card spec key follows the content of drive files and folder, not their paths', () => {
  const at = (dir: string, text: string) => ({
    modules: [],
    drive: [{ name: 'Samples/a.bin', file: put(path.join(root, dir, 'a.bin'), text) }],
    folder: path.join(root, dir, 'folder'),
  });
  put(path.join(root, 'one', 'folder', 'x.wav'), 'x');
  put(path.join(root, 'two', 'folder', 'x.wav'), 'x');
  const one = cardSpecKey(device, at('one', 'same'));
  expect(cardSpecKey(device, at('two', 'same'))).toBe(one);
  expect(cardSpecKey(device, at('two', 'other'))).not.toBe(one);
  put(path.join(root, 'two', 'folder', 'x.wav'), 'y');
  expect(cardSpecKey(device, at('two', 'same'))).not.toBe(one);
});
