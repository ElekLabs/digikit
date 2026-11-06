import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { InitialButton } from '@digi/control-client';
import { button } from '@digi/test-support';
import { expect, test } from 'vitest';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { bootInfo, bootInfoLoaded, currentRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { CARD_TIMEOUT } from '@digi/regression/runtime/debug-kit.ts';
import { rpc, word } from '@digi/regression/runtime/fx-live.ts';
import { drainInputs } from '@digi/regression/runtime/fx-session.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { waitEmulatedFrames } from '@digi/regression/runtime/waits.ts';

const scenarios: { name: string; buttons: InitialButton[]; decision: number; selected?: boolean; held?: boolean }[] = [
  {
    name: 'fresh released',
    buttons: [
      { code: button('STOP'), down: true },
      { code: button('STOP'), down: false },
    ],
    decision: 1,
  },
  { name: 'missing', buttons: [], decision: 0, selected: true },
  { name: 'held', buttons: [{ code: button('RECORD'), down: true }], decision: 3, held: true, selected: true },
  {
    name: 'record down then up',
    buttons: [
      { code: button('RECORD'), down: true },
      { code: button('RECORD'), down: false },
    ],
    decision: 3,
    selected: true,
  },
];
const symbols = [
  'BOOT_GUARD_PANEL_BITS',
  'BOOT_GUARD_CLOSED',
  'BOOT_GUARD_DECISION',
  'BOOT_GUARD_ATTEMPTS',
  'BOOT_GUARD_REQUESTS',
  'BOOT_GUARD_ENTERED',
  'BOOT_GUARD_RETURNED',
  'BOOT_EARLY_CALLS',
  'BOOT_GUARD_SETTINGS_BEFORE',
  'BOOT_GUARD_SETTINGS_AFTER',
  'DKRT_SET',
];

for (const scenario of scenarios) {
  cardRigs(
    `early panel ${scenario.name}`,
    {
      roots: [...RT, 'core/boot-guard', 'test/boot-early-probe'],
      symbols: [...CARD_SYMBOLS, ...symbols],
      startEmpty: true,
      initialButtons: scenario.buttons,
      initial: () =>
        scenario.selected ? { modules: ['test/c-demo'], selection: ['modules/c-demo.dkm'] } : { selection: [] },
      onClose(h) {
        const retained = fs.mkdtempSync(path.join(os.tmpdir(), `digi-early-panel-evidence-${h.device}-`));
        for (const name of ['early-panel.json', 'startup-empty.json', 'lifetime-safety.json']) {
          const source = path.join(h.artifacts, name);
          if (fs.existsSync(source)) fs.copyFileSync(source, path.join(retained, name));
        }
        console.info('retained early panel evidence:', retained);
      },
    },
    (h) => {
      test(
        'captures UART-only freshness before intro without bypassing normal late boot',
        async () => {
          await expect.poll(async () => (await bootInfo(h.session, h.plan)).uiReached, { timeout: 30_000 }).toBe(1);
          const first = await bootInfo(h.session, h.plan);
          expect(first).toMatchObject({ safe: scenario.held ? 1 : 0, reason: scenario.held ? 2 : 0 });
          const info = scenario.selected && !scenario.held ? await bootInfoLoaded(h.session, h.plan) : first;
          expect(info).toMatchObject({
            count: scenario.selected ? 1 : 0,
            loaded: scenario.selected && !scenario.held ? 1 : 0,
          });
          const observed = Object.fromEntries(
            await Promise.all(symbols.map(async (name) => [name, await word(h.session, planSymbol(h.plan, name))])),
          );
          expect(observed).toMatchObject({
            BOOT_GUARD_PANEL_BITS: scenario.decision,
            BOOT_GUARD_DECISION: scenario.decision,
            BOOT_GUARD_CLOSED: 1,
            BOOT_GUARD_ATTEMPTS: 1,
            BOOT_GUARD_REQUESTS: scenario.decision === 1 ? 1 : 0,
            BOOT_GUARD_ENTERED: scenario.decision === 1 ? 1 : 0,
            BOOT_GUARD_RETURNED: scenario.decision === 1 ? 1 : 0,
            BOOT_EARLY_CALLS: scenario.decision === 1 ? 1 : 0,
            BOOT_GUARD_SETTINGS_BEFORE: 0,
            BOOT_GUARD_SETTINGS_AFTER: 0,
            DKRT_SET: 1,
          });
          const selection = currentRecord(await h.records());
          expect(selection).toMatchObject({
            counter: !scenario.held && (scenario.selected || scenario.decision === 1) ? 1 : 0,
            paths: scenario.selected ? ['modules/c-demo.dkm'] : [],
          });
          record(h.artifacts, 'early-panel', { scenario: scenario.name, info, observed, selection });
          await rpc(h.session, 'run.mode', { mode: 'free' });
          try {
            if (scenario.held) await rpc(h.session, 'input.button', { code: button('RECORD'), down: false });
            await waitEmulatedFrames(h.session, 150);
            expect(await word(h.session, planSymbol(h.plan, 'BOOT_GUARD_DECISION'))).toBe(scenario.decision);
            expect((await h.session.status()).cf.main_ui_reached).toBe(true);
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
