import type { CardBridge } from '../src/runtime/cards.ts';
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { cardLine, folderFiles, folderName, startCard } from '../src/runtime/cards.ts';

// Only the File members consumed by these helpers are needed by the fake.
const file = (name: string, path: string, bytes = [1, 2, 3]) =>
  ({
    name,
    webkitRelativePath: path,
    arrayBuffer: async () => new Uint8Array(bytes).buffer,
  }) as File;

test('every file of the chosen folder is used with its path inside it, sorted', () => {
  const list = [
    file('b.wav', 'drums/b.wav'),
    file('A.WAV', 'drums/A.WAV'),
    file('note.txt', 'drums/note.txt'),
    file('deep.wav', 'drums/sub/deep.wav'),
    file('0000.bin', 'drums/.mmcfs/projects/0000.bin'),
    file('.DS_Store', 'drums/.DS_Store'),
    file('loose.wav', ''),
  ];
  assert.deepEqual(
    folderFiles(list).map((f) => f.path),
    ['.mmcfs/projects/0000.bin', 'A.WAV', 'b.wav', 'loose.wav', 'note.txt', 'sub/deep.wav'],
  );
  assert.deepEqual(folderFiles([]), []);
});

test('the toolbar line follows the controller status', () => {
  assert.equal(cardLine(undefined), '');
  assert.equal(cardLine({ state: 'idle' }), 'Choose samples to load a folder of WAVs');
  assert.equal(cardLine({ state: 'error', message: 'no .wav files' }), 'Card failed: no .wav files');
  assert.equal(
    cardLine({ state: 'working', message: 'Building the card (2/3 samples)', fraction: 0.07 }),
    'Building the card (2/3 samples) (7%)',
  );
  assert.equal(
    cardLine({ state: 'working', message: 'Booting the Digitakt II with the card (42%)', fraction: 0.42 }),
    'Booting the Digitakt II with the card (42%)',
  );
  assert.equal(cardLine({ state: 'ready', samples: 3, name: 'Kick drums' }), 'Samples: Kick drums (3 files)');
  assert.equal(cardLine({ state: 'ready', samples: 1, ready_cached: true }), 'Samples: card (1 file)');
});

test('startCard resets, stages every file in order, then starts the job', async () => {
  const calls: string[] = [];
  const bridge: CardBridge = {
    reset: async () => void calls.push('reset'),
    stage: async (name, bytes) => void calls.push(`stage ${name} ${bytes.length}`),
    load: async (name) => void calls.push(`load ${name}`),
    status: async () => ({ state: 'idle' }),
  };
  const progress: [number, number][] = [];
  await startCard(
    bridge,
    folderFiles([file('a.wav', 'x/a.wav', [1]), file('b.wav', 'x/sub/b.wav', [1, 2])]),
    (done, total) => progress.push([done, total]),
    'x',
  );
  assert.deepEqual(calls, ['reset', 'stage a.wav 1', 'stage sub/b.wav 2', 'load x']);
  assert.deepEqual(progress, [
    [1, 2],
    [2, 2],
  ]);
});

test('the folder name is the first path part of the chosen files', () => {
  assert.equal(folderName([file('a.wav', 'Kick drums/a.wav')]), 'Kick drums');
  assert.equal(folderName([file('a.wav', '')]), '');
});
