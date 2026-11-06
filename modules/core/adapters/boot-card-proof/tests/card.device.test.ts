import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { crc32 } from 'node:zlib';
import { button, repoRoot } from '@digi/test-support';
import { expect, test } from 'vitest';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { bootInfo, bootInfoLoaded, currentRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
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
  'BOOT_CARD_ATTEMPTS',
  'BOOT_CARD_STATS',
  'BOOT_CARD_READS',
  'BOOT_CARD_OWNED_BYTES',
  'BOOT_CARD_POINTER',
  'BOOT_CARD_DISCARDS',
  'BOOT_GUARD_REQUESTS',
  'BOOT_GUARD_ENTERED',
  'BOOT_GUARD_RETURNED',
];
const expectedHash = '63053142ed4218204f369f72b315a6294777a0b9180d5a158de4810b28814a66';
const stockLastDrawingHash = '5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef';
const root = repoRoot();
function fixture(name: string): Buffer {
  const source = fs.readFileSync(path.join(root, `tests/fixtures/modules/boot-dkbs-${name}/src/asset.c`), 'utf8');
  return Buffer.from([...source.matchAll(/0x([0-9a-f]{2})/g)].map((match) => Number.parseInt(match[1]!, 16)));
}
const valid = fixture('valid');

for (const scenario of ['valid', 'missing', 'corrupt', 'oversized', 'held', 'empty', 'counter', 'transfer'] as const) {
  const inputs: string[] = [];
  const bypass = scenario === 'held' || scenario === 'counter';
  const active = scenario === 'valid' || scenario === 'empty' || scenario === 'transfer';
  const read = active || scenario === 'corrupt';
  cardRigs(
    `card DKBS ${scenario}`,
    {
      roots: [...RT, 'core/boot-card-proof'],
      symbols: [...CARD_SYMBOLS, ...symbols, 'DKRT_STAGE'],
      startEmpty: true,
      uploads: scenario === 'transfer',
      initial: () => {
        const modules: (string | { name: string; file: string })[] = ['test/c-demo'];
        if (scenario !== 'missing') {
          const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-boot-card-input-'));
          const file = path.join(directory, 'active.dkbs');
          fs.writeFileSync(
            file,
            scenario === 'oversized' ? Buffer.alloc(65537) : scenario === 'corrupt' ? fixture('corrupt') : valid,
          );
          modules.push({ name: 'bootscreens/active.dkbs', file });
          inputs.push(directory);
        }
        return {
          modules,
          selection: scenario === 'empty' ? [] : ['modules/c-demo.dkm'],
          counter: scenario === 'counter' ? 2 : 0,
        };
      },
      initialButtons:
        scenario !== 'held'
          ? [
              { code: button('STOP'), down: true },
              { code: button('STOP'), down: false },
            ]
          : [{ code: button('RECORD'), down: true }],
      onClose(h) {
        const retained = fs.mkdtempSync(path.join(os.tmpdir(), `digi-boot-card-evidence-${h.device}-`));
        for (const name of [
          'scene-proof.json',
          'startup-empty.json',
          'lifetime-safety.json',
          'scene-intro.png',
          'transfer-proof.json',
        ]) {
          const source = path.join(h.artifacts, name);
          if (fs.existsSync(source)) fs.copyFileSync(source, path.join(retained, name));
        }
        for (const directory of inputs.splice(0)) fs.rmSync(directory, { recursive: true, force: true });
        console.info('retained card DKBS evidence:', retained);
      },
    },
    (h) => {
      test(
        'loads an owned card DKBS before preserving stock setup and completion',
        async () => {
          await expect.poll(async () => (await bootInfo(h.session, h.plan)).uiReached, { timeout: 30_000 }).toBe(1);
          const first = await bootInfo(h.session, h.plan);
          expect(first).toMatchObject({
            safe: bypass ? 1 : 0,
            reason: scenario === 'held' ? 2 : scenario === 'counter' ? 1 : 0,
            count: scenario === 'empty' ? 0 : 1,
          });
          const info = bypass || scenario === 'empty' ? first : await bootInfoLoaded(h.session, h.plan);
          expect(info.loaded).toBe(bypass || scenario === 'empty' ? 0 : 1);
          if (scenario === 'counter') expect(info.counter).toBe(2);
          const observed = Object.fromEntries(
            await Promise.all(symbols.map(async (name) => [name, await word(h.session, planSymbol(h.plan, name))])),
          );
          expect(observed).toMatchObject({
            BOOT_SCENE_CALLS: active ? 4 : 0,
            BOOT_SCENE_LAST_INDEX: active ? 3 : 0,
            BOOT_SCENE_STATUS: active ? 1 : bypass ? 0 : 2,
            BOOT_SCENE_VALIDATIONS: bypass ? 0 : 1,
            BOOT_SCENE_DECODED_RECORDS: active ? 3 : 0,
            BOOT_SCENE_ERRORS: 0,
            BOOT_CARD_ATTEMPTS: bypass ? 0 : 1,
            BOOT_CARD_STATS: bypass ? 0 : 1,
            BOOT_CARD_READS: read ? 1 : 0,
            BOOT_CARD_OWNED_BYTES: active ? valid.length : 0,
            BOOT_CARD_DISCARDS: scenario === 'corrupt' ? 1 : 0,
            BOOT_GUARD_REQUESTS: scenario === 'held' ? 0 : 1,
            BOOT_GUARD_ENTERED: bypass ? 0 : 1,
            BOOT_GUARD_RETURNED: bypass ? 0 : 1,
          });
          const retainedCopy = async (pointer: number) => {
            const retained = Buffer.alloc(Math.ceil(valid.length / 4) * 4);
            for (let offset = 0; offset < retained.length; offset += 4)
              retained.writeUInt32BE(await word(h.session, pointer + offset), offset);
            const bytes = retained.subarray(0, valid.length);
            expect(bytes).toEqual(valid);
            return createHash('sha256').update(bytes).digest('hex');
          };
          if (active) {
            expect(observed.BOOT_SCENE_PUBLISHED).toBeGreaterThan(0);
            const pointer = observed.BOOT_CARD_POINTER!;
            const stage = planSymbol(h.plan, 'DKRT_STAGE');
            expect(pointer).toBeGreaterThan(0);
            expect(pointer % 16).toBe(0);
            expect(
              pointer + valid.length <= stage || pointer >= stage + deviceProfile(h.device).limits.stage_bytes!,
            ).toBe(true);
            await retainedCopy(pointer);
          } else {
            expect(observed.BOOT_SCENE_PUBLISHED).toBe(0);
            expect(observed.BOOT_CARD_POINTER).toBe(0);
          }
          const selection = currentRecord(await h.records());
          expect(selection).toMatchObject({
            counter: bypass ? 0 : 1,
            paths: scenario === 'empty' ? [] : ['modules/c-demo.dkm'],
          });
          let intro;
          if (scenario !== 'held') {
            intro = await h.session.client.framebufferGet({ source: 'intro', data: true, hash: true });
            expect(intro.source).toBe('intro');
            expect(intro.data_b64).toBeTruthy();
            const raw = Buffer.from(intro.data_b64!, 'base64');
            expect(raw.length).toBe(1024);
            if (active) {
              expect(intro.sha256).toBe(expectedHash);
            } else expect(intro.sha256).toBe(stockLastDrawingHash);
            fs.writeFileSync(path.join(h.artifacts, 'scene-intro.oled'), raw);
            await analysisJson(
              'tools.patchkit.pk_digikit_fxroutes',
              ['render-shots', h.artifacts],
              h.artifacts,
              'rendered-scene',
            );
          }
          record(h.artifacts, 'scene-proof', { scenario, info, observed, selection, intro, expectedHash });
          if (scenario === 'transfer') {
            const activePath = 'bootscreens/active.dkbs';
            const replacement = fixture('corrupt');
            replacement.writeUInt32BE(crc32(replacement.subarray(16)), 12);
            expect(replacement.length).toBe(40);
            const snapshots: object[] = [];
            const unchanged = async (stage: string) => {
              const current = Object.fromEntries(
                await Promise.all(symbols.map(async (name) => [name, await word(h.session, planSymbol(h.plan, name))])),
              );
              expect(current).toEqual(observed);
              const ownedSha256 = await retainedCopy(current.BOOT_CARD_POINTER!);
              const drawing = await h.session.client.framebufferGet({ source: 'intro', hash: true });
              expect(drawing.sha256).toBe(expectedHash);
              const selected = currentRecord(await h.records());
              expect(selected).not.toBeNull();
              expect(selected!.paths).toEqual(selection!.paths);
              expect(selected!.flags).toBe(selection!.flags);
              const loaded = await bootInfo(h.session, h.plan);
              expect(loaded).toMatchObject({ safe: 0, reason: 0, count: 1, loaded: 1, results: [0] });
              snapshots.push({
                stage,
                observed: current,
                ownedSha256,
                introHash: drawing.sha256,
                selection: { paths: selected!.paths, flags: selected!.flags },
              });
            };
            await rpc(h.session, 'run.mode', { mode: 'paused' });
            await h.usb(h.runtime, async (dk) => {
              await unchanged('before PUT');
              await dk.put(activePath, replacement);
              const verified = await dk.verifyFile(activePath, replacement);
              expect(verified).toBe(true);
              const listing = await dk.list();
              expect(listing.find((entry) => entry.path === activePath)).toMatchObject({ size: 40 });
              await unchanged('after PUT / VERIFY / LIST');
              await dk.delete(activePath);
              const afterDelete = await dk.list();
              const absent = !afterDelete.some((entry) => entry.path === activePath);
              expect(absent).toBe(true);
              await unchanged('after DELETE / absence LIST');
              const modules = await dk.modules();
              expect(modules).toHaveLength(1);
              expect(modules[0]!.faults).toBe(0);
              record(h.artifacts, 'transfer-proof', {
                activePath,
                replacementSize: replacement.length,
                verified,
                listing,
                absent,
                afterDelete,
                modules,
                snapshots,
              });
            });
          }
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
