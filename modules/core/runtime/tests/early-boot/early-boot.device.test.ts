import { expect, test } from 'vitest';
import { bootInfoLoaded, currentRecord, encodeRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { CARD_TIMEOUT } from '@digi/regression/runtime/debug-kit.ts';
import { rpc, word } from '@digi/regression/runtime/fx-live.ts';
import { drainInputs, shot } from '@digi/regression/runtime/fx-session.ts';
import { panel } from '@digi/regression/runtime/panel.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { waitEmulatedFrames } from '@digi/regression/runtime/waits.ts';

const guard = [...RT, 'core/boot-guard'];
const proof = [
  'BOOT_GUARD_ATTEMPTS',
  'BOOT_GUARD_ENTERED',
  'BOOT_GUARD_RETURNED',
  'BOOT_GUARD_BEFORE',
  'BOOT_GUARD_AFTER',
];

cardRigs('early boot without callback', { roots: guard, symbols: [...CARD_SYMBOLS, ...proof] }, (h) => {
  test(
    'the absent callback does not prepare early or arm an empty selection',
    async () => {
      await h.boot(await h.card({ selection: [] }));
      const info = await h.ready();
      expect(info).toMatchObject({ safe: 0, count: 0, loaded: 0, counter: 0 });
      for (const symbol of proof.slice(0, 3)) expect(await word(h.session, planSymbol(h.plan, symbol)), symbol).toBe(0);
      expect(currentRecord(await h.records())).toMatchObject({ counter: 0, paths: [] });
    },
    CARD_TIMEOUT,
  );
});

cardRigs(
  'early boot probe',
  {
    roots: [...guard, 'test/boot-early-probe'],
    symbols: [...CARD_SYMBOLS, ...proof, 'BOOT_EARLY_CALLS'],
  },
  (h) => {
    test(
      'arms zero selected modules before callback, then reaches the UI without another increment',
      async () => {
        await h.boot(await h.card({ selection: [] }));
        const info = await h.ready();
        expect(info).toMatchObject({ safe: 0, count: 0, loaded: 0, counter: 0, uiReached: 1 });
        for (const symbol of ['BOOT_GUARD_ATTEMPTS', 'BOOT_GUARD_ENTERED', 'BOOT_GUARD_RETURNED', 'BOOT_EARLY_CALLS'])
          expect(await word(h.session, planSymbol(h.plan, symbol)), symbol).toBe(1);
        // These typed runtime snapshots are captured around the callback, after successful persistence and hook validation.
        // The generated before-hook continues through the original installer only after the callback returns.
        for (const symbol of ['BOOT_GUARD_BEFORE', 'BOOT_GUARD_AFTER']) {
          const address = planSymbol(h.plan, symbol);
          expect(await word(h.session, address)).toBe(0x444b4254);
          expect(await word(h.session, address + 32), 'no module ran before intro installation').toBe(0);
          expect(await word(h.session, address + 36), 'main UI has not run').toBe(0);
        }
        expect(currentRecord(await h.records())).toMatchObject({ counter: 1, paths: [] });
      },
      CARD_TIMEOUT,
    );

    test(
      'an owned store with two corrupt selection records denies early work and boots normally',
      async () => {
        const torn = encodeRecord({ paths: [] });
        torn[100] = torn[100]! ^ 0xff;
        await h.boot(await h.card({ recs: [torn, torn] }));
        expect(await h.ready()).toMatchObject({ safe: 0, count: 0, loaded: 0, counter: 0, seq: 0 });
        expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_ATTEMPTS'))).toBe(1);
        expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_ENTERED'))).toBe(0);
        expect(await word(h.session, planSymbol(h.plan, 'BOOT_EARLY_CALLS'))).toBe(0);
        expect(await h.records()).toEqual([null, null]);
      },
      CARD_TIMEOUT,
    );

    test(
      'the later boot hook still loads a selected module once',
      async () => {
        const mod = h.mod('test/c-demo');
        await h.boot(await h.card({ modules: [mod], selection: [mod.name] }));
        await h.ready();
        expect(await bootInfoLoaded(h.session, h.plan)).toMatchObject({ count: 1, loaded: 1, safe: 0 });
        expect(await word(h.session, planSymbol(h.plan, 'BOOT_EARLY_CALLS'))).toBe(1);
        expect(currentRecord(await h.records())).toMatchObject({ counter: 1, paths: [mod.name] });
      },
      CARD_TIMEOUT,
    );

    test(
      'safe-once skips the callback, clears the flag and preserves selection',
      async () => {
        const mod = h.mod('test/c-demo');
        await h.boot(await h.card({ modules: [mod], selection: [mod.name], flags: 1 }));
        expect(await h.ready(true)).toMatchObject({ safe: 1, reason: 3, loaded: 0 });
        expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_ATTEMPTS'))).toBe(1);
        expect(await word(h.session, planSymbol(h.plan, 'BOOT_EARLY_CALLS'))).toBe(0);
        expect(currentRecord(await h.records())).toMatchObject({ flags: 0, counter: 0, paths: [mod.name] });
      },
      CARD_TIMEOUT,
    );
  },
);

cardRigs(
  'early boot reset recovery',
  {
    // A reset intentionally skips the DT2 filesystem worker; use the existing MainPanel baseline contract.
    relaxFs: true,
    startEmpty: true,
    initial: () => ({ selection: [] }),
    roots: [...guard, 'test/boot-early-reset'],
    symbols: [...CARD_SYMBOLS, ...proof],
  },
  (h) => {
    test(
      'an empty coupled startup reaches safe mode after two pre-intro resets with zero selected modules',
      async () => {
        expect(await h.ready(true)).toMatchObject({ safe: 1, reason: 1, counter: 2, count: 0, loaded: 0 });
        expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_ENTERED'))).toBe(0);
        expect(currentRecord(await h.records())).toMatchObject({ counter: 0, paths: [] });
        expect((await h.session.status()).cf.filesystem_verified).not.toBe(false);
        // Exporting the records pauses the daemon. These clock/input helpers require free-running emulation.
        await rpc(h.session, 'run.mode', { mode: 'free' });
        try {
          await waitEmulatedFrames(h.session, 150);
          const before = await h.session.status();
          const inputIrqs = before.cf.input_irqs;
          if (typeof inputIrqs !== 'number') throw new Error('panel response proof requires a numeric input IRQ count');
          const main = await shot(h.session, h.artifacts, 'safe-main');
          const keys = panel(h.session);
          await keys.tap('SETTINGS', 3000);
          const settings = await shot(h.session, h.artifacts, 'safe-settings');
          expect(settings, 'the guest opened SETTINGS after a delivered key').not.toBe(main);
          await keys.tap('NO', 3000);
          expect(await shot(h.session, h.artifacts, 'safe-return'), 'NO returns to the same main screen').toBe(main);
          const after = await h.session.status();
          expect(after.cf.input_irqs, 'the guest received panel interrupts').toBeGreaterThan(inputIrqs);
          expect(after.cf.input_pending, 'the guest processed the input packets').toBe(0);
        } finally {
          try {
            await drainInputs(h.session);
          } finally {
            await rpc(h.session, 'run.mode', { mode: 'paused' });
          }
        }
      },
      CARD_TIMEOUT,
    );
  },
);

cardRigs(
  'early boot empty coupled control',
  {
    devices: ['dn2'],
    // Paired with reset recovery: same empty startup/card/MainPanel contract, with a returning callback.
    startEmpty: true,
    initial: () => ({ selection: [] }),
    relaxFs: true,
    roots: [...guard, 'test/boot-early-probe'],
    symbols: [...CARD_SYMBOLS, ...proof, 'BOOT_EARLY_CALLS'],
  },
  (h) => {
    test(
      'a returning callback reaches the normal coupled UI and advances the DSP from empty startup',
      async () => {
        expect(await h.ready()).toMatchObject({ safe: 0, reason: 0, uiReached: 1, count: 0, loaded: 0, counter: 0 });
        for (const symbol of ['BOOT_GUARD_ENTERED', 'BOOT_GUARD_RETURNED', 'BOOT_EARLY_CALLS'])
          expect(await word(h.session, planSymbol(h.plan, symbol)), symbol).toBe(1);
        expect(currentRecord(await h.records())).toMatchObject({ counter: 1, paths: [] });
        await rpc(h.session, 'run.mode', { mode: 'free' });
        try {
          await waitEmulatedFrames(h.session, 150);
        } finally {
          try {
            await drainInputs(h.session);
          } finally {
            await rpc(h.session, 'run.mode', { mode: 'paused' });
          }
        }
      },
      CARD_TIMEOUT,
    );
  },
);
