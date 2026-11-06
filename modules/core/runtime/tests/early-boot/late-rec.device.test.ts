import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { button } from '@digi/test-support';
import { expect, test } from 'vitest';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { bootInfo, bootInfoLoaded, currentRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { CARD_TIMEOUT } from '@digi/regression/runtime/debug-kit.ts';
import { rpc } from '@digi/regression/runtime/fx-live.ts';
import { drainInputs } from '@digi/regression/runtime/fx-session.ts';
import { waitEmulatedFrames } from '@digi/regression/runtime/waits.ts';

for (const held of [true, false]) {
  cardRigs(
    `direct REC ${held ? 'held' : 'released'}`,
    {
      roots: RT,
      symbols: CARD_SYMBOLS,
      startEmpty: true,
      initial: () => ({ modules: ['test/c-demo'], selection: ['modules/c-demo.dkm'] }),
      initialButtons: held
        ? [{ code: button('RECORD'), down: true }]
        : [
            { code: button('RECORD'), down: true },
            { code: button('RECORD'), down: false },
          ],
      onClose(h) {
        const retained = fs.mkdtempSync(path.join(os.tmpdir(), `digi-direct-rec-evidence-${h.device}-`));
        for (const name of ['direct-rec.json', 'startup-empty.json', 'lifetime-safety.json']) {
          const source = path.join(h.artifacts, name);
          if (fs.existsSync(source)) fs.copyFileSync(source, path.join(retained, name));
        }
        console.info('retained direct REC evidence:', retained);
      },
    },
    (h) => {
      test(
        'uses current key state for late safe mode without the early adapter',
        async () => {
          await expect.poll(async () => (await bootInfo(h.session, h.plan)).uiReached, { timeout: 30_000 }).toBe(1);
          // Check the decision immediately: a wrong safe result must fail, not poll for a different boot.
          const first = await bootInfo(h.session, h.plan);
          expect(first).toMatchObject({ safe: held ? 1 : 0, reason: held ? 2 : 0, count: 1 });
          const info = held ? first : await bootInfoLoaded(h.session, h.plan);
          expect(info).toMatchObject({ loaded: held ? 0 : 1 });
          const selection = currentRecord(await h.records());
          expect(selection).toMatchObject({ counter: held ? 0 : 1, paths: ['modules/c-demo.dkm'] });
          record(h.artifacts, 'direct-rec', { held, info, selection });
          await rpc(h.session, 'run.mode', { mode: 'free' });
          try {
            if (held) await rpc(h.session, 'input.button', { code: button('RECORD'), down: false });
            await waitEmulatedFrames(h.session, 150);
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
