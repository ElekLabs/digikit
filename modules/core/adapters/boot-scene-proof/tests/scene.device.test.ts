import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { button } from '@digi/test-support';
import { expect, test } from 'vitest';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { bootInfo, bootInfoLoaded, currentRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { CARD_TIMEOUT } from '@digi/regression/runtime/debug-kit.ts';
import { analysisJson, rpc, word } from '@digi/regression/runtime/fx-live.ts';
import { drainInputs, shot } from '@digi/regression/runtime/fx-session.ts';
import { panel } from '@digi/regression/runtime/panel.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { waitEmulatedFrames } from '@digi/regression/runtime/waits.ts';

const symbols = [
  'BOOT_SCENE_PUBLISHED',
  'BOOT_SCENE_CALLS',
  'BOOT_SCENE_LAST_INDEX',
  'BOOT_SCENE_STATUS',
  'BOOT_SCENE_VALIDATIONS',
  'BOOT_SCENE_DECODED_RECORDS',
  'BOOT_SCENE_ERRORS',
  'BOOT_GUARD_REQUESTS',
  'BOOT_GUARD_ENTERED',
  'BOOT_GUARD_RETURNED',
];
const expected = Buffer.alloc(1024);
for (const [x0, y0, x1, y1] of [
  [0, 0, 5, 2],
  [123, 61, 127, 63],
  [17, 11, 18, 18],
  [48, 32, 51, 39],
]) {
  for (let x = x0!; x <= x1!; x++)
    for (let y = y0!; y <= y1!; y++) expected[8 * x + 7 - Math.floor(y / 8)]! |= 1 << (y % 8);
}
const expectedHash = createHash('sha256').update(expected).digest('hex');

for (const scenario of ['active', 'missing', 'held', 'corrupt', 'absent'] as const) {
  const requested = scenario !== 'missing' && scenario !== 'held';
  cardRigs(
    `RAM scene ${scenario}`,
    {
      roots: [
        ...RT,
        scenario === 'absent'
          ? 'core/boot-scene-proof'
          : `test/boot-dkbs-${scenario === 'corrupt' ? 'corrupt' : 'valid'}`,
      ],
      symbols: [...CARD_SYMBOLS, ...symbols],
      startEmpty: true,
      initial: () => ({ modules: ['test/c-demo'], selection: ['modules/c-demo.dkm'] }),
      initialButtons: requested
        ? [
            { code: button('STOP'), down: true },
            { code: button('STOP'), down: false },
          ]
        : scenario === 'held'
          ? [{ code: button('RECORD'), down: true }]
          : [],
      onClose(h) {
        const retained = fs.mkdtempSync(path.join(os.tmpdir(), `digi-boot-scene-evidence-${h.device}-`));
        for (const name of ['scene-proof.json', 'startup-empty.json', 'lifetime-safety.json', 'scene-intro.png']) {
          const source = path.join(h.artifacts, name);
          if (fs.existsSync(source)) fs.copyFileSync(source, path.join(retained, name));
        }
        console.info('retained RAM scene evidence:', retained);
      },
    },
    (h) => {
      test(
        'validates a resident DKBS before preserving stock setup and completion',
        async () => {
          await expect.poll(async () => (await bootInfo(h.session, h.plan)).uiReached, { timeout: 30_000 }).toBe(1);
          const first = await bootInfo(h.session, h.plan);
          expect(first).toMatchObject({
            safe: scenario === 'held' ? 1 : 0,
            reason: scenario === 'held' ? 2 : 0,
            count: 1,
          });
          const info = scenario === 'held' ? first : await bootInfoLoaded(h.session, h.plan);
          expect(info.loaded).toBe(scenario === 'held' ? 0 : 1);
          const observed = Object.fromEntries(
            await Promise.all(symbols.map(async (name) => [name, await word(h.session, planSymbol(h.plan, name))])),
          );
          expect(observed).toMatchObject({
            BOOT_SCENE_CALLS: scenario === 'active' ? 4 : 0,
            BOOT_SCENE_LAST_INDEX: scenario === 'active' ? 3 : 0,
            BOOT_SCENE_STATUS: scenario === 'active' ? 1 : scenario === 'corrupt' ? 2 : 0,
            BOOT_SCENE_VALIDATIONS: scenario === 'active' || scenario === 'corrupt' ? 1 : 0,
            BOOT_SCENE_DECODED_RECORDS: scenario === 'active' ? 3 : 0,
            BOOT_SCENE_ERRORS: 0,
            BOOT_GUARD_REQUESTS: requested ? 1 : 0,
            BOOT_GUARD_ENTERED: requested ? 1 : 0,
            BOOT_GUARD_RETURNED: requested ? 1 : 0,
          });
          if (scenario === 'active') expect(observed.BOOT_SCENE_PUBLISHED).toBeGreaterThan(0);
          else expect(observed.BOOT_SCENE_PUBLISHED).toBe(0);
          const selection = currentRecord(await h.records());
          expect(selection).toMatchObject({ counter: scenario === 'held' ? 0 : 1, paths: ['modules/c-demo.dkm'] });
          let intro;
          if (scenario !== 'held') {
            intro = await h.session.client.framebufferGet({ source: 'intro', data: true, hash: true });
            expect(intro.source).toBe('intro');
            expect(intro.data_b64).toBeTruthy();
            const raw = Buffer.from(intro.data_b64!, 'base64');
            expect(raw.length).toBe(1024);
            if (scenario === 'active') {
              expect(raw).toEqual(expected);
              expect(intro.sha256).toBe(expectedHash);
            } else expect(intro.sha256).not.toBe(expectedHash);
            fs.writeFileSync(path.join(h.artifacts, 'scene-intro.oled'), raw);
            await analysisJson(
              'tools.patchkit.pk_digikit_fxroutes',
              ['render-shots', h.artifacts],
              h.artifacts,
              'rendered-scene',
            );
          }
          record(h.artifacts, 'scene-proof', { scenario, info, observed, selection, intro, expectedHash });
          await rpc(h.session, 'run.mode', { mode: 'free' });
          try {
            if (scenario === 'held') await rpc(h.session, 'input.button', { code: button('RECORD'), down: false });
            await waitEmulatedFrames(h.session, 150);
            expect((await h.session.status()).cf.main_ui_reached).toBe(true);
            if (h.device === 'dt2') {
              // Existing selected-c-demo empty-card oracle excludes stock verification/loading overlays.
              await expect
                .poll(async () => (await h.session.client.framebufferGet({ hash: true })).sha256, { timeout: 30_000 })
                .toBe('f55c67e4d946992137a1381c0883f9a3e76991a650ef2ceb31996b5dab0df817');
            }
            const before = await h.session.status();
            const irqs = before.cf.input_irqs;
            if (typeof irqs !== 'number') throw new Error('input response requires numeric IRQ count');
            const main = await shot(h.session, h.artifacts, 'scene-main');
            const keys = panel(h.session);
            await keys.tap('SETTINGS');
            expect(await shot(h.session, h.artifacts, 'scene-settings')).not.toBe(main);
            await keys.tap('NO');
            expect(await shot(h.session, h.artifacts, 'scene-return')).toBe(main);
            const after = await h.session.status();
            expect(after.cf.input_irqs).toBeGreaterThan(irqs);
            expect(after.cf.input_pending).toBe(0);
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
