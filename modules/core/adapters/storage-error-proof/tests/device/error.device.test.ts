import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { button, repoRoot } from '@digi/test-support';
import { expect, test } from 'vitest';
import { record } from '@digi/regression/runtime/artifacts.ts';
import {
  bootInfo,
  bootInfoLoaded,
  cardInfo,
  currentRecord,
  encodeRecord,
  CMD,
  postRaw,
  readSectors,
  setSelection,
} from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { CARD_TIMEOUT } from '@digi/regression/runtime/debug-kit.ts';
import { analysisJson, rpc, word } from '@digi/regression/runtime/fx-live.ts';
import { drainInputs, shot } from '@digi/regression/runtime/fx-session.ts';
import { panel } from '@digi/regression/runtime/panel.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { waitEmulatedFrames } from '@digi/regression/runtime/waits.ts';

const sceneSymbols = [
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
  'BOOT_GUARD_ATTEMPTS',
  'BOOT_GUARD_REQUESTS',
  'BOOT_GUARD_ENTERED',
  'BOOT_GUARD_RETURNED',
];
const faultSymbols = [
  'BOOT_IO_MODE',
  'BOOT_IO_MATCH_READS',
  'BOOT_IO_MATCH_WRITES',
  'BOOT_IO_REJECT_READS',
  'BOOT_IO_REJECT_WRITES',
  'BOOT_IO_LAST_SECTOR',
  'BOOT_IO_LAST_BYTES',
  'BOOT_IO_LAST_OP',
];
const source = fs.readFileSync(path.join(repoRoot(), 'tests/fixtures/modules/boot-dkbs-valid/src/asset.c'), 'utf8');
const valid = Buffer.from([...source.matchAll(/0x([0-9a-f]{2})/g)].map((match) => Number.parseInt(match[1]!, 16)));
const sourceHash = createHash('sha256').update(valid).digest('hex');
const sceneHash = '63053142ed4218204f369f72b315a6294777a0b9180d5a158de4810b28814a66';
const stockLastDrawingHash = '5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef';

interface Scenario {
  name: string;
  config: string;
  mode: number;
  rejectReads: number;
  rejectWrites: number;
  storedCounter: number;
  scene: 'active' | 'payload' | 'none';
  lastSector?: 'selA' | 'selB' | 'first' | 'store';
  lastBytes?: number;
  lastOp?: number;
  reason?: number;
  held?: boolean;
  flags?: number;
  persistent?: boolean;
  reset?: boolean;
}
const scenarios: Scenario[] = [
  {
    name: 'payload-reset',
    config: 'payload-reset',
    mode: 8,
    rejectReads: 0,
    rejectWrites: 0,
    storedCounter: 0,
    scene: 'none',
    reason: 1,
    reset: true,
  },
  ...([6, 7] as const).map((mode) => ({
    name: mode === 6 ? 'persistent-one' : 'persistent-both',
    config: mode === 6 ? 'persistent-one' : 'persistent-both',
    mode,
    rejectReads: mode === 6 ? 3 : 5,
    rejectWrites: 0,
    storedCounter: 0,
    scene: 'none' as const,
    reason: 6,
    persistent: true,
    lastSector: 'selB' as const,
    lastBytes: 512,
    lastOp: 0,
  })),
  { name: 'pass', config: 'pass', mode: 0, rejectReads: 0, rejectWrites: 0, storedCounter: 1, scene: 'active' },
  {
    name: 'one-copy',
    config: 'one-copy',
    mode: 1,
    rejectReads: 1,
    rejectWrites: 0,
    storedCounter: 1,
    scene: 'none',
    lastSector: 'selB',
    lastBytes: 512,
    lastOp: 0,
  },
  {
    name: 'both-copies',
    config: 'both-copies',
    mode: 2,
    rejectReads: 2,
    rejectWrites: 0,
    storedCounter: 1,
    scene: 'none',
    lastSector: 'selB',
    lastBytes: 512,
    lastOp: 0,
  },
  {
    name: 'payload',
    config: 'payload',
    mode: 3,
    rejectReads: 1,
    rejectWrites: 0,
    storedCounter: 1,
    scene: 'payload',
    lastSector: 'store',
    lastBytes: 2560,
    lastOp: 0,
  },
  {
    name: 'write-error',
    config: 'write-error',
    mode: 4,
    rejectReads: 0,
    rejectWrites: 2,
    storedCounter: 0,
    scene: 'none',
    lastSector: 'selA',
    lastBytes: 512,
    lastOp: 1,
  },
  {
    name: 'probe',
    config: 'probe',
    mode: 5,
    rejectReads: 1,
    rejectWrites: 0,
    storedCounter: 1,
    scene: 'none',
    lastSector: 'first',
    lastBytes: 512,
    lastOp: 0,
  },
  {
    name: 'held-write-error',
    config: 'write-error',
    mode: 4,
    rejectReads: 0,
    rejectWrites: 0,
    storedCounter: 0,
    scene: 'none',
    reason: 2,
    held: true,
  },
  {
    name: 'safe-once-write-error',
    config: 'write-error',
    mode: 4,
    rejectReads: 0,
    rejectWrites: 1,
    storedCounter: 0,
    scene: 'none',
    lastSector: 'selA',
    lastBytes: 512,
    lastOp: 1,
    reason: 3,
    flags: 1,
  },
];

for (const expected of scenarios) {
  const scenario = expected.name;
  const inputs: string[] = [];
  const active = expected.scene === 'active';
  const payload = expected.scene === 'payload';
  const safe = expected.reason !== undefined;
  cardRigs(
    `boot returned I/O ${scenario}`,
    {
      roots: [...RT, `test/boot-io-${expected.config}`],
      symbols: [...CARD_SYMBOLS, ...sceneSymbols, ...faultSymbols, 'DKRT_STAGE'],
      startEmpty: true,
      ...(expected.reset ? { relaxFs: true, baselineTimeoutMs: 3 * 60_000, env: { DIGI_TRACE_PATCH_BOOT: '1' } } : {}),
      initial: () => {
        const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-boot-io-input-'));
        const file = path.join(directory, 'active.dkbs');
        fs.writeFileSync(file, valid);
        inputs.push(directory);
        return {
          modules: ['test/c-demo', { name: 'bootscreens/active.dkbs', file }],
          recs: [
            encodeRecord({ paths: [], seq: 1 }),
            encodeRecord({ paths: ['modules/c-demo.dkm'], seq: 10, flags: expected.flags ?? 0 }),
          ],
        };
      },
      initialButtons: expected.held
        ? [{ code: button('RECORD'), down: true }]
        : [
            { code: button('STOP'), down: true },
            { code: button('STOP'), down: false },
          ],
      onClose(h) {
        const retained = fs.mkdtempSync(path.join(os.tmpdir(), `digi-boot-io-evidence-${h.device}-${scenario}-`));
        for (const name of ['fault-proof.json', 'startup-empty.json', 'lifetime-safety.json', 'scene-intro.png']) {
          const source = path.join(h.artifacts, name);
          if (fs.existsSync(source)) fs.copyFileSync(source, path.join(retained, name));
        }
        for (const directory of inputs.splice(0)) fs.rmSync(directory, { recursive: true, force: true });
        console.info('retained returned I/O evidence:', retained);
      },
    },
    (h) => {
      test(
        'preserves fresh selection and suppresses early callbacks after a returned selection error',
        async () => {
          await expect.poll(async () => (await bootInfo(h.session, h.plan)).uiReached, { timeout: 30_000 }).toBe(1);
          const first = await bootInfo(h.session, h.plan);
          const info = !safe && first.loaded < first.count ? await bootInfoLoaded(h.session, h.plan) : first;
          const observed = Object.fromEntries(
            await Promise.all(
              sceneSymbols.map(async (name) => [name, await word(h.session, planSymbol(h.plan, name))]),
            ),
          );
          const fault = Object.fromEntries(
            await Promise.all(
              faultSymbols.map(async (name) => [name, await word(h.session, planSymbol(h.plan, name))]),
            ),
          );
          const resets = expected.reset
            ? [
                ...fs
                  .readFileSync(path.join(h.session.runDir, 'daemon.stderr'), 'utf8')
                  .matchAll(/phase=poll\.reset unix_ms=\d+ resets=(\d+)/g),
              ].map((match) => Number(match[1]))
            : undefined;
          const copies = await h.records();
          const selection = currentRecord(copies);
          const card = await cardInfo(h.session, h.plan);
          const storeWrites = await h.storeWrites();
          const intro = expected.held
            ? undefined
            : await h.session.client.framebufferGet({ source: 'intro', data: true, hash: true });
          if (intro?.data_b64) {
            fs.writeFileSync(path.join(h.artifacts, 'scene-intro.oled'), Buffer.from(intro.data_b64, 'base64'));
            await analysisJson(
              'tools.patchkit.pk_digikit_fxroutes',
              ['render-shots', h.artifacts],
              h.artifacts,
              'rendered-scene',
            );
          }
          // Persist complete failure evidence before recovery assertions: the first red run is not retried.
          record(h.artifacts, 'fault-proof', {
            scenario,
            resets,
            first,
            info,
            observed,
            fault,
            copies,
            selection,
            card,
            storeWrites,
            intro,
            sourceHash,
            sourceBytes: valid.length,
          });
          if (expected.reset) expect(resets).toEqual([1, 2]);
          expect(fault).toMatchObject({
            BOOT_IO_MODE: expected.mode,
            BOOT_IO_REJECT_READS: expected.rejectReads,
            BOOT_IO_REJECT_WRITES: expected.rejectWrites,
          });
          if (expected.mode === 4) expect(fault.BOOT_IO_MATCH_WRITES).toBe(expected.rejectWrites);
          if (expected.lastSector) {
            expect(fault).toMatchObject({ BOOT_IO_LAST_BYTES: expected.lastBytes, BOOT_IO_LAST_OP: expected.lastOp });
            if (expected.lastSector === 'store') {
              expect(fault.BOOT_IO_LAST_SECTOR).toBeGreaterThanOrEqual(h.store.first);
              expect(fault.BOOT_IO_LAST_SECTOR).toBeLessThan(h.store.end);
            } else expect(fault.BOOT_IO_LAST_SECTOR).toBe(h.store[expected.lastSector]);
          }
          expect(info).toMatchObject({
            safe: safe ? 1 : 0,
            reason: expected.reason ?? 0,
            count: expected.persistent ? 0 : 1,
            loaded: safe ? 0 : 1,
            counter: expected.reset ? 2 : 0,
            seq: expected.reset ? 12 : expected.persistent ? (expected.mode === 6 ? 1 : 0) : 10,
            copy: expected.persistent ? 0 : 1,
            ...(safe ? { allResults: Array(8).fill(-44) } : {}),
            ...(expected.persistent ? { results: [] } : {}),
            ...(!safe ? { results: [0] } : {}),
          });
          expect(selection).toMatchObject({ counter: expected.storedCounter, paths: ['modules/c-demo.dkm'] });
          expect(selection!.seq).toBeGreaterThanOrEqual(10);
          if (!expected.reset)
            expect(copies[1]).toMatchObject({
              seq: 10,
              counter: 0,
              flags: expected.flags ?? 0,
              paths: ['modules/c-demo.dkm'],
            });
          if (expected.storedCounter === 0 && !expected.reset) {
            expect(copies[0]).toMatchObject({ seq: 1, counter: 0, flags: 0, paths: [] });
            expect(storeWrites).toEqual([]);
          }
          expect(observed).toMatchObject({
            BOOT_SCENE_CALLS: active ? 4 : 0,
            BOOT_SCENE_LAST_INDEX: active ? 3 : 0,
            BOOT_SCENE_STATUS: active ? 1 : payload ? 2 : 0,
            BOOT_SCENE_VALIDATIONS: active || payload ? 1 : 0,
            BOOT_SCENE_DECODED_RECORDS: active ? 3 : 0,
            BOOT_SCENE_ERRORS: 0,
            BOOT_CARD_ATTEMPTS: active || payload ? 1 : 0,
            BOOT_CARD_STATS: active || payload ? 1 : 0,
            BOOT_CARD_READS: active || payload ? 1 : 0,
            BOOT_CARD_OWNED_BYTES: active ? valid.length : 0,
            BOOT_CARD_DISCARDS: payload ? 1 : 0,
            ...(!expected.held ? { BOOT_GUARD_ATTEMPTS: 1 } : {}),
            BOOT_GUARD_REQUESTS: expected.held ? 0 : 1,
            BOOT_GUARD_ENTERED: active || payload ? 1 : 0,
            BOOT_GUARD_RETURNED: active || payload ? 1 : 0,
          });
          if (scenario === 'probe') expect(card).toMatchObject({ state: 1, stateName: 'ours', readErrors: 1 });
          if (scenario === 'write-error') expect(card.readErrors).toBe(0);
          if (intro) expect(intro.sha256).toBe(active ? sceneHash : stockLastDrawingHash);
          if (active) {
            const pointer = observed.BOOT_CARD_POINTER!;
            const stage = planSymbol(h.plan, 'DKRT_STAGE');
            expect(pointer).toBeGreaterThan(0);
            expect(pointer % 16).toBe(0);
            expect(
              pointer + valid.length <= stage || pointer >= stage + deviceProfile(h.device).limits.stage_bytes!,
            ).toBe(true);
            const retained = Buffer.alloc(Math.ceil(valid.length / 4) * 4);
            for (let offset = 0; offset < retained.length; offset += 4)
              retained.writeUInt32BE(await word(h.session, pointer + offset), offset);
            expect(retained.subarray(0, valid.length)).toEqual(valid);
          } else {
            expect(observed.BOOT_SCENE_PUBLISHED).toBe(0);
            expect(observed.BOOT_CARD_POINTER).toBe(0);
          }
          if (expected.persistent) {
            expect(info).toMatchObject({ stable: 1 });
            const before = await h.after('persistent-before');
            const original = [
              encodeRecord({ paths: [], seq: 1 }),
              encodeRecord({ paths: ['modules/c-demo.dkm'], seq: 10 }),
            ];
            for (const [index, sector] of [h.store.selA, h.store.selB].entries())
              expect(readSectors(before, sector, 1)).toEqual(original[index]);
            expect(await setSelection(h.session, h.plan, [])).toBe(-3);
            for (const copy of [0, 1])
              expect(await postRaw(h.session, h.plan, CMD.SELECT_ONE, Buffer.from([copy, 0, 0]))).toBe(-3);
            const after = await h.after('persistent-after-api');
            for (const [index, sector] of [h.store.selA, h.store.selB].entries())
              expect(readSectors(after, sector, 1)).toEqual(original[index]);
            expect(await h.storeWrites()).toEqual([]);
          }
          await rpc(h.session, 'run.mode', { mode: 'free' });
          try {
            if (expected.held) await rpc(h.session, 'input.button', { code: button('RECORD'), down: false });
            await waitEmulatedFrames(h.session, 150);
            expect((await h.session.status()).cf.main_ui_reached).toBe(true);
            if (h.device === 'dt2')
              await expect
                .poll(async () => (await h.session.client.framebufferGet({ hash: true })).sha256, { timeout: 30_000 })
                .toBe('f55c67e4d946992137a1381c0883f9a3e76991a650ef2ceb31996b5dab0df817');
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
            if (expected.persistent) {
              expect(await h.storeWrites()).toEqual([]);
              expect(await word(h.session, planSymbol(h.plan, 'BOOT_IO_REJECT_READS'))).toBe(
                expected.rejectReads + (expected.mode === 6 ? 3 : 6),
              );
            }
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
