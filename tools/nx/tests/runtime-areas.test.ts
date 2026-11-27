import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { DEVICES } from '../device-owners.mjs';
import { areaSelection, checkComplete, classify, danglingEntries, entriesOf, loadAreas } from '../runtime-areas.mjs';
import { loadProjects } from '../test-subset.mjs';

const areas = loadAreas();
const SRC = 'runtime/core/src/rs/src';

// A graph with every project and file the map names, so that each entry resolves to one device test.
const projects = (() => {
  const files = new Map<string, Set<string>>();
  for (const area of Object.keys(areas.areas))
    for (const entry of entriesOf(area, areas)) {
      const at = entry.indexOf(':');
      const [name, file] = [entry.slice(0, at), entry.slice(at + 1)];
      files.set(name, files.get(name) ?? new Set());
      files.get(name)!.add(file === '*' ? `${name.replace(/\W/g, '-')}.device.test.ts` : file);
    }
  // A project named with '*' also has one named file, so '*' and the file entries agree.
  return [...files].map(([name, fs]) => ({
    name,
    root: name,
    targets: [...fs].map((f) => `test-device--${f.replace(/\.device\.test\.ts$/, '')}--${DEVICES[0]}`),
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
  image: 'runtime/core/abi/Cargo.toml',
};

// The areas a file selects: its probe area plus any other area its globs reach (a union).
const areasOf = (file: string): string[] => {
  const c = classify(file, areas);
  return c?.kind === 'area' ? c.areas : [];
};

describe('runtime areas', () => {
  it('classifies a probe file of each area into that area', () => {
    for (const [area, file] of Object.entries(PROBES)) expect(areasOf(file), file).toContain(area);
  });

  it('every area has a probe or is a platform module area', () => {
    for (const area of Object.keys(areas.areas))
      expect(area in PROBES || area.startsWith('platform-'), area).toBe(true);
  });

  it('a probe selects the tests its area names and nothing else', () => {
    for (const [area, file] of Object.entries(PROBES)) {
      const sel = areaSelection(projects, [file], areas);
      const wanted = new Set(
        areasOf(file)
          .flatMap((a) => entriesOf(a, areas))
          .flatMap((entry: string) => {
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
      'runtime/core/vectors/dkp-vectors.json',
      `${SRC}/host_io.rs`,
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

  it('overlapping areas are a union: a file selects the tests of every area that matches it', () => {
    expect(classify(`${SRC}/package/loader/install.rs`, areas)).toEqual({
      kind: 'area',
      areas: ['live-reload', 'module-load'],
    });
    expect(classify(`${SRC}/package/loader/discover.rs`, areas)).toEqual({ kind: 'area', areas: ['module-load'] });
    for (const file of ['runtime/core/lifecycle/src/live.rs', 'runtime/core/lifecycle/src/manager/reload.rs'])
      expect(areasOf(file), file).toEqual(expect.arrayContaining(['live-reload']));
    const sel = areaSelection(projects, [`${SRC}/package/loader/install.rs`], areas);
    const keys = sel.tests.map((t) => `${t.project}:${t.file}`);
    expect(keys).toContain('core/runtime-tests:core-runtime-tests.device.test.ts');
    expect(keys).toContain('core/runtime-capacity-tests:core-runtime-capacity-tests.device.test.ts');
  });

  it('an area match wins over hostProven and the gate prints the reason of each file', () => {
    const file = 'runtime/core/abi/src/dev_addr.rs';
    expect(areas.hostProven.some((g: string) => path.matchesGlob(file, g))).toBe(false);
    const both = { ...areas, hostProven: [file] };
    expect(classify(file, both)).toMatchObject({ kind: 'area' });
    const sel = areaSelection(projects, [file, `${SRC}/fake_storage.rs`], areas);
    expect(sel.notes).toContain(`${file} -> area platform-seam`);
    expect(sel.notes).toContain(`${SRC}/fake_storage.rs -> host-proven (no device test)`);
  });

  it('no firmware-facing file is host-proven: build outputs, device layouts and C headers are in areas', () => {
    for (const file of [
      'runtime/core/build/runtime.dko.json',
      `runtime/core/build/gen/device-${DEVICES[0]}.inc`,
      'runtime/core/abi/src/mem.rs',
      'runtime/core/abi/src/resident/card.rs',
      'runtime/core/abi.toml',
      'runtime/core/include/dkrt_pdw.h',
      'runtime/core/include/dkrt_sdhc.h',
      'runtime/core/sdk/src/mailbox.rs',
      'runtime/core/sdk/src/store/lease.rs',
    ])
      expect(classify(file, areas)?.kind, file).toBe('area');
  });

  it('a Cargo.toml of a runtime crate is the image area: every area test runs', () => {
    const file = 'runtime/core/lifecycle/Cargo.toml';
    expect(areasOf(file)).toContain('image');
    const keys = new Set(areaSelection(projects, [file], areas).tests.map((t) => `${t.project}:${t.file}`));
    const every = new Set(
      Object.keys(areas.areas).flatMap((a) =>
        areaSelection(projects, [PROBES[a] ?? `runtime/platform/${a.slice(9)}/x`], areas).tests.map(
          (t) => `${t.project}:${t.file}`,
        ),
      ),
    );
    for (const k of every) expect(keys.has(k), k).toBe(true);
  });

  it('every area entry names a test of the real Nx project graph', () => {
    const root = path.resolve(__dirname, '../../..');
    expect(danglingEntries(areas, loadProjects(root))).toEqual([]);
  }, 180_000);
});
