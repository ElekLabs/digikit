// Selector rules of the merge gate over fixture file lists (docs/TESTING.md "Runtime areas", "Nx gate"): the platform area map, the users of
// a changed port declaration and the rule for a hand-edited device file.
import { describe, expect, it } from 'vitest';
import { deviceShorts } from '../device-files.mjs';
import { ownershipSelection } from '../device-owners.mjs';
import { declaredUsers, portEntryUsers } from '../port-entries.mjs';
import { checkComplete, classify, loadAreas } from '../runtime-areas.mjs';

const areas = loadAreas();
const [DEV] = deviceShorts();

describe('platform areas', () => {
  it('maps a platform module file to the area of its module', () => {
    expect(classify('runtime/platform/lfo-table/src/cf/table.c', areas)).toEqual({
      kind: 'area',
      areas: ['platform-lfo-table'],
    });
  });

  it('fails a file of a platform module that has no area', () => {
    const bare = { ...areas, areas: { ...areas.areas } };
    delete bare.areas['platform-lfo-table'];
    expect(
      checkComplete(['runtime/platform/lfo-table/src/cf/table.c', 'runtime/platform/input/module.toml'], bare),
    ).toEqual(['runtime/platform/lfo-table/src/cf/table.c']);
  });

  it('keeps the host crate of ui and unit-test sources host-proven', () => {
    expect(classify('runtime/platform/ui/host/src/lib.rs', areas)).toEqual({ kind: 'host-proven' });
    expect(classify('runtime/platform/new-module/tests/x.c', areas)).toEqual({ kind: 'host-proven' });
  });

  it('leaves a path outside the runtime scope to ownership', () => {
    expect(classify('services/shortcuts/dks/src/lib.rs', areas)).toBeUndefined();
  });
});

const GENERATED = (hook, extra = '') => `
[port]
device = "${DEV}"
version = "1.17"
${extra}
[[anchor]]
name = "cf.draw.fn.rect"
addr = 1
[[hook]]
name = "cf.input.hook.key_event"
mode = "${hook}"
`;
const DECLARED = declaredUsers([
  `[[fn]]\nname = "cf.draw.fn.rect"\nusers = ["core/ui"]\n[[hook]]\nname = "cf.input.hook.key_event"\nusers = ["core/input"]\n`,
]);

describe('users of a changed generated port file', () => {
  it('selects the users of the entries that differ', () => {
    const got = portEntryUsers({ head: GENERATED('before'), base: GENERATED('replace'), declared: DECLARED });
    expect(got).toEqual({ users: ['core/input'], wide: false });
  });

  it('sees a change nested inside an entry', () => {
    const nested = (v) =>
      `[port]\ndevice = "${DEV}"\n[[hook]]\nname = "cf.input.hook.key_event"\n[hook.args]\nreg = { a = 1, b = ${v} }\n`;
    const got = portEntryUsers({ head: nested(2), base: nested(1), declared: DECLARED });
    expect(got).toEqual({ users: ['core/input'], wide: false });
  });

  it('is wide when the header or a limit changes', () => {
    const got = portEntryUsers({
      head: GENERATED('before', 'sha = "b"'),
      base: GENERATED('before', 'sha = "a"'),
      declared: DECLARED,
    });
    expect(got.wide).toBe(true);
  });

  it('is wide for an entry no declaration names', () => {
    const got = portEntryUsers({
      head: GENERATED('before'),
      base: GENERATED('before').replace('cf.draw.fn.rect', 'cf.x'),
      declared: DECLARED,
    });
    expect(got.wide).toBe(true);
  });
});

const project = (name, root, files) => ({
  name,
  root,
  subjects: [],
  targets: files.map((f) => `test-device--${f}--${DEV}`),
  metadata: { runtimeAcceptance: [{ files: files.map((f) => `${f}.device.test.ts`) }] },
});
const projects = [
  project('core/input', 'runtime/platform/input', ['input-adapter']),
  project('core/ui', 'runtime/platform/ui', ['ui']),
];
const select = (changed, hooks) =>
  ownershipSelection(projects, {
    changed,
    portEntries: (f) => (f.startsWith(`ports/${DEV}-`) ? hooks : undefined),
    deviceOf: (f) => (f === `devices/${DEV}.toml` || f.startsWith(`ports/${DEV}-`) ? DEV : undefined),
  });

describe('port bindings and device files', () => {
  it('selects the tests of the users of a changed generated entry, no device', () => {
    const got = select([`ports/${DEV}-1.0.toml`], { users: ['core/input'], wide: false });
    expect(got.tests.map((t) => t.project)).toEqual(['core/input']);
    expect(got.devices).toEqual([]);
  });

  it('keeps every half of the device for a wide change of a port file', () => {
    const got = select([`ports/${DEV}-1.0.toml`], { users: [], wide: true });
    expect(got.tests).toEqual([]);
    expect(got.devices).toEqual([DEV]);
  });

  it('keeps every half of the device for a hand-edited device file', () => {
    const got = select([`devices/${DEV}.toml`], undefined);
    expect(got.devices).toEqual([DEV]);
  });
});
