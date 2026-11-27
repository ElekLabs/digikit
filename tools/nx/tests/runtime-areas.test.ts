import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { areaSelection, checkComplete, classify, loadAreas } from '../runtime-areas.mjs';

const areas = loadAreas();
const SRC = 'runtime/core/src/rs/src';

// A graph with every project and file the map names, so that each entry resolves to one device test.
const projects = (() => {
  const files = new Map<string, Set<string>>();
  for (const a of Object.values(areas.areas) as { tests: string[] }[])
    for (const entry of a.tests) {
      const at = entry.indexOf(':');
      const [name, file] = [entry.slice(0, at), entry.slice(at + 1)];
      files.set(name, files.get(name) ?? new Set());
      files.get(name)!.add(file === '*' ? `${name.replace(/\W/g, '-')}.device.test.ts` : file);
    }
  // A project named with '*' also has one named file, so '*' and the file entries agree.
  return [...files].map(([name, fs]) => ({
    name,
    root: name,
    targets: [...fs].map((f) => `test-device--${f.replace(/\.device\.test\.ts$/, '')}--dt2`),
    metadata: { runtimeAcceptance: [{ files: [...fs] }] },
  }));
})();

// One probe file per area, and the tests the area names.
const PROBES: Record<string, string> = {
  'live-reload': `${SRC}/package/loader/install.rs`,
  'module-load': `${SRC}/package/loader/discover.rs`,
  dsp: `${SRC}/package/dsp/mod.rs`,
  services: `${SRC}/package/handlers.rs`,
  'ui-menu': `${SRC}/package/menu.rs`,
  boot: `${SRC}/stage1.rs`,
  store: `${SRC}/storage/store.rs`,
  usb: `${SRC}/transport.rs`,
  resident: `${SRC}/state.rs`,
  'platform-seam': `${SRC}/platform.rs`,
  'platform-usb-bulk': 'runtime/platform/usb-bulk/src/x.c',
};

describe('runtime areas', () => {
  it('classifies a probe file of each area into that area', () => {
    for (const [area, file] of Object.entries(PROBES)) expect(classify(file, areas)).toEqual({ kind: 'area', area });
  });

  it('every area has a probe or is a platform module area', () => {
    for (const area of Object.keys(areas.areas)) expect(area in PROBES || area.startsWith('platform-')).toBe(true);
  });

  it('a probe selects the tests its area names and nothing else', () => {
    for (const [area, file] of Object.entries(PROBES)) {
      const sel = areaSelection(projects, [file], areas);
      const wanted = new Set(
        areas.areas[area].tests.flatMap((entry: string) => {
          const name = entry.slice(0, entry.indexOf(':'));
          const f = entry.slice(entry.indexOf(':') + 1);
          const fs = projects.find((p) => p.name === name)!.metadata.runtimeAcceptance[0].files;
          return (f === '*' ? fs : [f]).map((x: string) => `${name}:${x}`);
        }),
      );
      expect(new Set(sel.tests.map((t) => `${t.project}:${t.file}`)), area).toEqual(wanted);
      expect(sel.unclassified).toEqual([]);
      expect(sel.governed.has(file)).toBe(true);
    }
  });

  it('a host-proven file selects no device test of its own', () => {
    for (const file of [
      'runtime/core/abi/src/plan.rs',
      `${SRC}/fake_storage.rs`,
      'runtime/core/build/runtime.dko.json',
    ]) {
      expect(classify(file, areas)).toEqual({ kind: 'host-proven' });
      const sel = areaSelection(projects, [file], areas);
      expect(sel.tests).toEqual([]);
      expect(sel.unclassified).toEqual([]);
    }
  });

  it('a runtime file in no area and not host-proven is unclassified and fails the check', () => {
    const file = 'runtime/core/newdir/thing.rs';
    expect(classify(file, areas)).toEqual({ kind: 'unclassified' });
    expect(areaSelection(projects, [file], areas).unclassified).toEqual([file]);
    expect(checkComplete([file, 'runtime/core/abi/src/plan.rs'], areas)).toEqual([file]);
  });

  it('every tracked runtime file is classified', () => {
    const root = path.resolve(__dirname, '../../..');
    const files = execFileSync('git', ['ls-files', 'runtime/core'], { cwd: root, encoding: 'utf8' })
      .split('\n')
      .filter(Boolean);
    expect(checkComplete(files, areas)).toEqual([]);
  });

  it('unit-test sources and device test projects select no area test', () => {
    expect(classify(`${SRC}/package/loader/tests.rs`, areas)).toEqual({ kind: 'host-proven' });
    expect(classify(`${SRC}/package/loader/tests/a.rs`, areas)).toEqual({ kind: 'host-proven' });
    expect(classify('runtime/core/tests/boot/safe-mode.device.test.ts', areas)).toBeUndefined();
    expect(classify('modules/x/y.rs', areas)).toBeUndefined();
    const sel = areaSelection(projects, [`${SRC}/package/loader/tests.rs`], areas);
    expect(sel.tests).toEqual([]);
    expect(sel.unclassified).toEqual([]);
  });

  it('the first matching area wins: install.rs is live-reload, discover.rs is module-load', () => {
    expect(classify(`${SRC}/package/loader/install.rs`, areas)).toEqual({ kind: 'area', area: 'live-reload' });
    expect(classify(`${SRC}/package/loader/discover.rs`, areas)).toEqual({ kind: 'area', area: 'module-load' });
  });
});
