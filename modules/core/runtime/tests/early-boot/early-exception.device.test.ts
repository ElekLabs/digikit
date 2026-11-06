import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readFault } from '@digi/dk-session';
import { button } from '@digi/test-support';
import { expect, test } from 'vitest';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { bootInfoLoaded, crashInfo, currentRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { CARD_TIMEOUT, pausedProtocol } from '@digi/regression/runtime/debug-kit.ts';
import { rpc, word } from '@digi/regression/runtime/fx-live.ts';
import { drainInputs, shot } from '@digi/regression/runtime/fx-session.ts';
import { panel } from '@digi/regression/runtime/panel.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { waitEmulatedFrames } from '@digi/regression/runtime/waits.ts';

const scenarios = [
  { name: 'pending fault zero counter', counter: 0, selected: true, safe: false },
  { name: 'pending fault seeded counter', counter: 1, selected: false, safe: true },
] as const;

for (const scenario of scenarios) {
  cardRigs(
    `early CPU exception ${scenario.name}`,
    {
      roots: [...RT, 'core/boot-guard', 'test/boot-early-exception'],
      symbols: [
        ...CARD_SYMBOLS,
        'dk_boot_early',
        'BOOT_GUARD_ATTEMPTS',
        'BOOT_GUARD_REQUESTS',
        'BOOT_GUARD_ENTERED',
        'BOOT_GUARD_RETURNED',
        'BOOT_GUARD_SETTINGS_BEFORE',
        'BOOT_GUARD_SETTINGS_AFTER',
        'DKRT_SET',
      ],
      startEmpty: true,
      initial: () => ({
        counter: scenario.counter,
        modules: scenario.selected ? ['test/c-demo'] : [],
        selection: scenario.selected ? ['modules/c-demo.dkm'] : [],
      }),
      initialButtons: [
        { code: button('STOP'), down: true },
        { code: button('STOP'), down: false },
      ],
      deliverExceptions: true,
      faults: true,
      relaxFs: true,
      // One real exception, then pending-fault deferral. Seed 1 is prior history, not a second exception.
      // Retain the existing bounded recovery window; no global timeout change.
      baselineTimeoutMs: 3 * 60_000,
      env: { DIGI_TRACE_PATCH_BOOT: '1' },
      onClose(h) {
        // Preserve only bounded JSON evidence, never firmware/card data, outside automatic session cleanup.
        const retained = fs.mkdtempSync(path.join(os.tmpdir(), `digi-early-exception-evidence-${h.device}-`));
        for (const name of ['early-exception.json', 'startup-empty.json', 'lifetime-safety.json']) {
          const source = path.join(h.artifacts, name);
          if (fs.existsSync(source)) fs.copyFileSync(source, path.join(retained, name));
        }
        console.info('retained early exception evidence:', retained);
      },
    },
    (h) => {
      test(
        'one pre-intro guest exception is persisted by normal late handling without a second callback',
        async () => {
          await h.ready(scenario.safe);
          const info = await bootInfoLoaded(h.session, h.plan);
          expect(info).toMatchObject({
            safe: scenario.safe ? 1 : 0,
            reason: scenario.safe ? 1 : 0,
            counter: scenario.counter + 1,
            count: scenario.selected ? 1 : 0,
            loaded: scenario.selected ? 1 : 0,
            uiReached: 1,
          });
          const pc = planSymbol(h.plan, 'dk_boot_early');
          const crash = await crashInfo(h.session, h.plan);
          // found=1/bad=0 means at_boot validated the version and CRC of the retained record.
          // Linked code is deliberately not attributed to a card-loaded module.
          expect(crash).toMatchObject({
            found: 1,
            bad: 0,
            pc,
            vec: 4,
            module: 0xffffffff,
            stable: 0,
            ramKept: 1,
            saved: 1,
            hang: 0,
            panic: 0,
          });
          expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_ATTEMPTS'))).toBe(1);
          expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_REQUESTS'))).toBe(1);
          expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_ENTERED'))).toBe(0);
          expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_RETURNED'))).toBe(0);
          expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_SETTINGS_BEFORE'))).toBe(0);
          expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_SETTINGS_AFTER'))).toBe(0);
          expect(await word(h.session, planSymbol(h.plan, 'DKRT_SET'))).toBe(1);
          const selection = currentRecord(await h.records());
          expect(selection).toMatchObject({
            counter: scenario.safe ? 0 : 2,
            paths: scenario.selected ? ['modules/c-demo.dkm'] : [],
          });
          const fault = await pausedProtocol(h.runtime, readFault);
          expect(fault).toMatchObject({ kind: 'cpu', at: pc, module: '' });
          // This fixture's single dk/fault entry fits one sector under the current settings format.
          // Settings flush writes that first sector in either alternating copy, never the store headers/data.
          const written = await h.storeWrites();
          const allowed = [h.store.selA, h.store.selB, h.store.setA, h.store.setB];
          expect(
            written.filter((sector) => !allowed.includes(sector)),
            'only selection and fault settings sectors',
          ).toEqual([]);
          expect(
            written.some((sector) => sector === h.store.setA || sector === h.store.setB),
            'fault settings were written',
          ).toBe(true);
          h.allowStoreWrites([h.store.setA, h.store.setB]);
          record(h.artifacts, 'early-exception', {
            initialCounter: scenario.counter,
            info,
            pc,
            crash,
            fault,
            selection,
            written,
            allowed,
          });
          expect((await h.session.status()).cf.filesystem_verified).not.toBe(false);

          await rpc(h.session, 'run.mode', { mode: 'free' });
          try {
            await waitEmulatedFrames(h.session, 150);
            if (h.device === 'dt2') {
              // Same empty-card unobscured stock main frame as the reset proof; exclude boot overlays.
              await expect
                .poll(async () => (await h.session.client.framebufferGet({ hash: true })).sha256, { timeout: 30_000 })
                .toBe('f55c67e4d946992137a1381c0883f9a3e76991a650ef2ceb31996b5dab0df817');
            }
            const before = await h.session.status();
            const inputIrqs = before.cf.input_irqs;
            if (typeof inputIrqs !== 'number')
              throw new Error('panel response proof requires a numeric input IRQ count');
            const main = await shot(h.session, h.artifacts, 'safe-main');
            const keys = panel(h.session);
            await keys.tap('SETTINGS', 3000);
            expect(await shot(h.session, h.artifacts, 'safe-settings'), 'the guest opened SETTINGS').not.toBe(main);
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
}
