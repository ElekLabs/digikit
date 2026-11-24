import { createRoot, createSignal } from 'solid-js';
import { expect, test } from 'vitest';
import type { SavedProfile } from '../src/profile/hotspots';
import { createProfileReference } from '../src/profile/reference';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

function profileText(addr: number) {
  return JSON.stringify({ schema: 'dk.profile/1', raw: [{ cpu: 'cf', series: 'pc', addr, count: 10 }] });
}

const namedPc = (addr: number, name: string): SavedProfile['pcs'] => ({
  cf: [{ addr, name, weight: 10, share: 1 }],
  sharc: [],
});

test('late file and symbol reads leave the latest selected comparison intact', async () => {
  const firstSymbols = deferred<SavedProfile['pcs']>();
  const slowFile = deferred<string>();
  const latestPcs = namedPc(3, 'latest');
  const { reference, dispose } = createRoot((dispose) => ({
    dispose,
    reference: createProfileReference({
      visible: () => true,
      resolve: (profile) => (profile.pcs.cf[0].addr === 1 ? firstSymbols.promise : Promise.resolve(latestPcs)),
    }),
  }));
  try {
    await reference.open({ name: 'first.json', text: async () => profileText(1) });
    expect(reference.pcs()?.cf[0]).toMatchObject({ addr: 1 });
    const pending = reference.open({ name: 'slow.json', text: () => slowFile.promise });
    await reference.open({ name: 'latest.json', text: async () => profileText(3) });

    firstSymbols.resolve(namedPc(1, 'first'));
    slowFile.resolve(profileText(2));
    await pending;
    expect(reference.selection()?.name).toBe('latest.json');
    expect(reference.pcs()).toEqual(latestPcs);
    expect(reference.error()).toBe('');
  } finally {
    dispose();
  }
});

test('the file survives hiding Profile and late replies leave a disposed comparison intact', async () => {
  const symbols = deferred<SavedProfile['pcs']>();
  const file = deferred<string>();
  const [visible, setVisible] = createSignal(true);
  const { reference, dispose } = createRoot((dispose) => ({
    dispose,
    reference: createProfileReference({ visible, resolve: () => symbols.promise }),
  }));
  await reference.open({ name: 'saved.json', text: async () => profileText(1) });
  setVisible(false);
  expect(reference.selection()?.name).toBe('saved.json');
  expect(reference.pcs()?.cf[0]).toEqual({ addr: 1, weight: 10, share: 1 });
  setVisible(true);

  const pending = reference.open({ name: 'invalid.json', text: () => file.promise });
  dispose();
  symbols.resolve(namedPc(1, 'late symbol'));
  file.resolve('invalid JSON');
  await pending;
  expect(reference.selection()?.name).toBe('saved.json');
  expect(reference.pcs()?.cf[0]).toEqual({ addr: 1, weight: 10, share: 1 });
  expect(reference.error()).toBe('');
});
