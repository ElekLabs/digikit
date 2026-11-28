// Selector rules of the merge gate over fixture file lists (docs/TESTING.md "Runtime areas", "Nx gate"): the platform area map, the users of
// a changed port declaration and the rule for a hand-edited device file.
import { describe, expect, it } from 'vitest';
import { deviceShorts } from '../device-files.mjs';
import { ownershipSelection } from '../device-owners.mjs';
import { gateContext } from '../gate-context.mjs';
import { declaredUsers, portEntryUsers } from '../port-entries.mjs';
import { checkComplete, classify, loadAreas, portAreaSelection } from '../runtime-areas.mjs';

const areas = loadAreas();
const [DEV] = deviceShorts();

describe('platform areas', () => {
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

describe('declaring area of a changed port binding', () => {
  const inputArea = {
    scope: ['runtime/platform/**'],
    areas: { 'platform-input': { globs: ['runtime/platform/input/**'], tests: ['core/ui:ui.device.test.ts'] } },
  };
  it('selects the area tests of a platform user of the changed declaration', () => {
    const got = portAreaSelection(projects, [{ file: 'ports/fw/input.toml', project: 'core/input' }], inputArea);
    expect(got.tests.map((t) => [t.project, t.changed])).toEqual([['core/ui', ['ports/fw/input.toml']]]);
  });

  it('ignores a user outside the platform modules and a file that is no port file', () => {
    const got = portAreaSelection(
      [...projects, { name: 'svc', root: 'services/svc', targets: [] }],
      [
        { file: 'ports/fw/input.toml', project: 'svc' },
        { file: 'runtime/platform/input/module.toml', project: 'core/input' },
      ],
      inputArea,
    );
    expect(got.tests).toEqual([]);
  });
});

describe('gate context against the base', () => {
  const decl = (users) =>
    `[[hook]]\nname = "cf.input.hook.key_event"\nusers = [${users.map((u) => `"${u}"`).join(', ')}]\n`;
  const ctx = ({ now = {}, before = {}, ...rest }) =>
    gateContext('/x', 'base' in rest ? rest.base : 'main', {
      read: (f) => now[f],
      show: (f) => before[f],
      declarationTexts: () => [decl(['core/input'])],
      deviceGlobs: () => [{ short: DEV, globs: [`ports/${DEV}-*.toml`] }],
    });
  const F = 'ports/fw/input.toml';

  it('names the users of a declaration added since the base', () => {
    expect(ctx({ now: { [F]: decl(['core/input']) } }).portUsers(F)).toEqual(['core/input']);
  });

  it('names the users of a changed declaration, old and new, not those of an unchanged one', () => {
    const two = `${decl(['core/input'])}[[hook]]\nname = "b"\nusers = ["core/ui"]\n`;
    const three = `${decl(['core/input'])}[[hook]]\nname = "b"\nusers = ["core/ui", "core/x"]\n`;
    expect(
      ctx({ now: { [F]: three }, before: { [F]: two } })
        .portUsers(F)
        .sort(),
    ).toEqual(['core/ui', 'core/x']);
  });

  it('names the users of a declaration removed since the base: the file is gone, so none', () => {
    expect(ctx({ before: { [F]: decl(['core/input']) } }).portUsers(F)).toEqual([]);
  });

  it('counts every declaration as changed when no base is given', () => {
    const none = ctx({ now: { [F]: decl(['core/a']) }, before: { [F]: decl(['core/b']) }, base: undefined });
    expect(none.portUsers(F)).toEqual(['core/a']);
  });

  it('reads a generated port file against the base text', () => {
    const file = `ports/${DEV}-1.0.toml`;
    const port = (mode) => GENERATED(mode);
    const changed = ctx({ now: { [file]: port('before') }, before: { [file]: port('replace') } }).portEntries(file);
    expect(changed).toEqual({ users: ['core/input'], wide: false });
  });

  it('is wide for a generated port file without a base, a new one or a removed one', () => {
    const file = `ports/${DEV}-1.0.toml`;
    expect(ctx({ now: { [file]: GENERATED('before') }, base: undefined }).portEntries(file).wide).toBe(true);
    expect(ctx({ before: { [file]: GENERATED('before') } }).portEntries(file).wide).toBe(true);
    expect(ctx({ now: { [file]: GENERATED('before') } }).portEntries(file).wide).toBe(true);
  });

  it('leaves a file that is no generated port file alone', () => {
    expect(ctx({}).portEntries('ports/fw/input.toml')).toBeUndefined();
  });
});
