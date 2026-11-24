import { createRoot, createSignal } from 'solid-js';
import { expect, test } from 'vitest';
import type { SavedEvidence, SavedProfile } from '../src/profile/hotspots';
import { createProfileReference } from '../src/profile/reference';
import { createSavedPerformance } from '../src/saved-performance';

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
  const firstSymbols = deferred<SavedEvidence>();
  const slowFile = deferred<string>();
  const latestPcs = namedPc(3, 'latest');
  const { reference, saved, dispose } = createRoot((dispose) => {
    const saved = createSavedPerformance();
    const reference = createProfileReference({
      capture: saved.current,
      visible: () => true,
      resolve: (profile) => (profile.pcs.cf[0].addr === 1 ? firstSymbols.promise : Promise.resolve({ pcs: latestPcs })),
    });
    return { dispose, saved, reference };
  });
  try {
    await saved.open({ name: 'first.json', text: async () => profileText(1) });
    expect(reference.evidence()?.pcs?.cf[0]).toMatchObject({ addr: 1 });
    const pending = saved.open({ name: 'slow.json', text: () => slowFile.promise });
    await saved.open({ name: 'latest.json', text: async () => profileText(3) });

    firstSymbols.resolve({ pcs: namedPc(1, 'first') });
    slowFile.resolve(profileText(2));
    await pending;
    expect(saved.current()?.label).toBe('latest.json');
    expect(reference.evidence()?.pcs).toEqual(latestPcs);
    expect(saved.error()).toBe('');
  } finally {
    dispose();
  }
});

test('the file survives hiding Profile and late replies leave a disposed comparison intact', async () => {
  const symbols = deferred<SavedEvidence>();
  const file = deferred<string>();
  const [visible, setVisible] = createSignal(true);
  const { reference, saved, dispose } = createRoot((dispose) => {
    const saved = createSavedPerformance();
    const reference = createProfileReference({ capture: saved.current, visible, resolve: () => symbols.promise });
    return { dispose, saved, reference };
  });
  await saved.open({ name: 'saved.json', text: async () => profileText(1) });
  setVisible(false);
  expect(saved.current()?.label).toBe('saved.json');
  expect(reference.evidence()?.pcs?.cf[0]).toEqual({ addr: 1, weight: 10, share: 1 });
  setVisible(true);

  const pending = saved.open({ name: 'invalid.json', text: () => file.promise });
  dispose();
  symbols.resolve({ pcs: namedPc(1, 'late symbol') });
  file.resolve('invalid JSON');
  await pending;
  expect(saved.current()?.label).toBe('saved.json');
  expect(reference.evidence()?.pcs?.cf[0]).toEqual({ addr: 1, weight: 10, share: 1 });
  expect(saved.error()).toBe('');
});

test('clearing a saved profile invalidates in-flight file and symbol reads', async () => {
  const symbols = deferred<SavedEvidence>();
  const file = deferred<string>();
  const { reference, saved, dispose } = createRoot((dispose) => {
    const saved = createSavedPerformance();
    const reference = createProfileReference({
      capture: saved.current,
      visible: () => true,
      resolve: () => symbols.promise,
    });
    return { dispose, saved, reference };
  });
  try {
    await saved.open({ name: 'saved.json', text: async () => profileText(1) });
    const pending = saved.open({ name: 'pending.json', text: () => file.promise });
    saved.returnToLive();

    symbols.resolve({ pcs: namedPc(1, 'late symbol') });
    file.resolve(profileText(2));
    await pending;
    expect(saved.current()).toBeUndefined();
    expect(reference.evidence()).toBeUndefined();
    expect(saved.error()).toBe('');
  } finally {
    dispose();
  }
});
