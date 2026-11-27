// Simulated device I/O for the real Samples workflow in the desktop previews.
import { noteActivity } from '@digi/workbench-core/transfer/activity.ts';
import { sourceKind, type Prepared } from '@digi/dk-session/dksample';
import type { SampleEntry, SamplesBackend } from '@digi/dk-session/samples';
import type { SampleConnection, SampleDeviceContext, SampleDeviceSession } from '../src/samples/device-session';
import { control, delay, deviceRead, sampleTree, sim } from './sim';

const NAME = 'Digitakt II';

export function createSampleDeviceSessions({ connection }: { connection: () => SampleConnection }) {
  let puts = 0;
  let sent = 0;
  return ({ binding, check, current, setLink }: SampleDeviceContext): SampleDeviceSession => {
    const clean = (folder: string) => folder.replace(/^\/+|\/+$/g, '');
    const backend: SamplesBackend = {
      kind: 'stock-rpc',
      prepare(name, bytes): Prepared {
        const kind = sourceKind(bytes, bytes.length);
        return { name: kind === 'converted' ? name.replace(/\.wav$/i, '') : name, bytes, stored: bytes, kind };
      },
      sizeOf(file) {
        return file.stored.length;
      },
      checksAtBulkEnd() {
        return true;
      },
      async list(folder = ''): Promise<SampleEntry[]> {
        await deviceRead(binding.session);
        check();
        return [...(sampleTree.get(clean(folder)) ?? [])];
      },
      async space() {
        return { free: 3.1 * 2 ** 30, total: 4 * 2 ** 30 };
      },
      async canMkdir() {
        return sim.mkdir;
      },
      async mkdir(folder: string) {
        await delay(200);
        check();
        const f = clean(folder);
        const parent = f.split('/').slice(0, -1).join('/');
        sampleTree.set(f, []);
        sampleTree.set(parent, [
          ...(sampleTree.get(parent) ?? []),
          { name: f.split('/').pop() as string, kind: 'folder', size: 0 },
        ]);
      },
      async rmdir(folder: string) {
        const f = clean(folder);
        sampleTree.delete(f);
        const parent = f.split('/').slice(0, -1).join('/');
        sampleTree.set(
          parent,
          (sampleTree.get(parent) ?? []).filter((e) => e.name !== f.split('/').pop()),
        );
      },
      async removeTree(folder, o) {
        const root = clean(folder);
        const files: [string, string][] = [];
        const dirs: string[] = [];
        const walk = (dir: string) => {
          for (const e of sampleTree.get(dir) ?? []) {
            if (e.kind === 'folder') walk(`${dir}/${e.name}`);
            else files.push([dir, e.name]);
          }
          dirs.push(dir);
        };
        walk(root);
        o?.onProgress?.(0, files.length);
        let removed = 0;
        for (const [dir, name] of files) {
          if (o?.shouldStop?.()) return { removed, total: files.length, stopped: true };
          await delay(150);
          check();
          sampleTree.set(
            dir,
            (sampleTree.get(dir) ?? []).filter((e) => e.name !== name),
          );
          removed++;
          o?.onProgress?.(removed, files.length);
        }
        for (const d of dirs) await backend.rmdir(d);
        return { removed, total: files.length, stopped: false };
      },
      sameOnDevice(file) {
        return sim.skip.split(',').includes(file.name);
      },
      async put(folder, file, onProgress) {
        if (sim.link === 'fallback' && sent++ === 1 && current())
          setLink({ state: 'fallback', reason: 'the device did not answer within 2 s' });
        if (sim.putfail && puts++ === 1) throw new Error(`open /tst/${file.name}: the device answered status 0`);
        const total = file.stored.length;
        const step = Math.max(1, Math.round(sim.speed * 1024 * 1024 * 0.1));
        for (let sent = 0; sent < total;) {
          await delay(100);
          check();
          sent = Math.min(total, sent + Math.round(step * (0.85 + 0.3 * Math.abs(Math.sin(sent / step)))));
          noteActivity();
          onProgress?.(sent, total);
        }
        const f = clean(folder);
        const list = (sampleTree.get(f) ?? []).filter((e) => e.name !== file.name);
        sampleTree.set(f, [...list, { name: file.name, kind: 'file', size: total }]);
      },
      async verify(_folder, file) {
        await delay(sim.verify);
        check();
        const size = file.stored.length;
        return {
          ok: true,
          device: 'preview',
          host: 'preview',
          size,
          sent: size,
          mismatchAt: null,
          compared: 'device content hash matches',
          message: `${size} bytes compared`,
        };
      },

      async remove(folder, name) {
        await delay(150);
        check();
        const f = clean(folder);
        sampleTree.set(
          f,
          (sampleTree.get(f) ?? []).filter((e) => e.name !== name),
        );
      },
      async canRename() {
        return true;
      },
      async rename(folder, name, toFolder, toName) {
        await delay(150);
        check();
        const [f, t] = [clean(folder), clean(toFolder)];
        const entry = (sampleTree.get(f) ?? []).find((e) => e.name === name);
        if (!entry) return;
        sampleTree.set(
          f,
          (sampleTree.get(f) ?? []).filter((e) => e.name !== name),
        );
        sampleTree.set(t, [...(sampleTree.get(t) ?? []).filter((e) => e.name !== toName), { ...entry, name: toName }]);
      },
    };

    return {
      async open(purpose) {
        if (purpose === 'read') control.counts.deviceRead++;
        const { asked, noteConnection } = connection();
        await asked(() => deviceRead(binding.session));
        check();
        if (sim.link !== 'none' && !(sim.link === 'fallback' && sent > 1))
          setLink(
            sim.link === 'sysex'
              ? { state: 'sysex' }
              : sim.link === 'wrongmode'
                ? { state: 'sysex', wrongMode: true }
                : sim.link === 'reason'
                  ? { state: 'sysex', reason: "none of 1 candidate(s) returned this device's nonce" }
                  : { state: 'bulk' },
          );
        noteConnection({ type: 'answered', device: { id: 'dt2', name: NAME, runtime: '0.5.2' } });
        return {
          name: NAME,
          runtime: '0.9.4',
          hasRuntime: true,
          samples: 'stock-rpc',
          apply: 'live',
          backend,
          link: 'sysex',
        };
      },
      async release() {},
    };
  };
}
