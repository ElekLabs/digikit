import { appendFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { parseDepfile, patchkitKeys } from '../patchkit-key.mjs';

const scratch = mkdtempSync(join(tmpdir(), 'digikit-patchkit-key-'));
afterAll(() => rmSync(scratch, { recursive: true, force: true }));
const FACTS = { rustc: 'rustc 1', profileEnv: '[]', rustflags: '' };
const toml = (short: string) => `[device]\nshort = "${short}"\n[acceptance]\nready_dir = "x"\n`;

let n = 0;
const makeRepo = (files: Record<string, string> = {}) => {
  const repo = join(scratch, `repo${n++}`);
  const all: Record<string, string> = {
    'devices/aa.toml': toml('aa'),
    'devices/bb.toml': toml('bb'),
    'devices/aa-leds.toml': 'leds = "a"\n',
    'devices/bb-leds.toml': 'leds = "b"\n',
    'ports/aa-1.toml': 'pa',
    'ports/bb-1.toml': 'pb',
    'Cargo.toml': 'w',
    'Cargo.lock': 'l',
    'packages/patchkit/Cargo.toml': 'p',
    'packages/patchkit/src/main.rs': 'fn main() {}',
    'packages/patchkit/src/apply.rs': 'a',
    'packages/patchkit/src/lib.rs': '#[cfg(test)]\nmod tests;\n',
    'packages/patchkit/src/tests.rs': 't',
    ...files,
  };
  for (const [name, text] of Object.entries(all)) {
    mkdirSync(dirname(join(repo, name)), { recursive: true });
    writeFileSync(join(repo, name), text);
  }
  return repo;
};
// what cargo writes: absolute paths, `..` entries, a directory, build output, a continuation line
const depText = (repo: string) =>
  [
    `${repo}/dist/cargo/release/patchkit: ${repo}/packages/patchkit/src/main.rs ${repo}/packages/patchkit/src/apply.rs \\`,
    ` ${repo}/packages/patchkit/src/lib.rs ${repo}/packages/patchkit/src/tests.rs ${repo}/packages/patchkit/../../devices/aa.toml`,
    ` ${repo}/packages/patchkit/../../devices/bb.toml ${repo}/packages/patchkit/../../devices ${repo}/dist/cargo/release/build/x/out/device_files.rs`,
    '',
  ].join('\n');
const keys = (repo: string, text = depText(repo)) => patchkitKeys({ depText: text, workspaceRoot: repo, facts: FACTS });
// An edit appends a comment line, so a TOML file stays valid TOML.
const edit = (repo: string, name: string) => appendFileSync(join(repo, name), `\n# ${Math.random()}\n`);

describe('parseDepfile', () => {
  it('reads continuation lines and escaped spaces, and skips comments and empty targets', () => {
    const text = 't: /a/x.rs /a/with\\ space.rs \\\n /b.rs\n/a/x.rs:\n# env-dep:K=v: y\n';
    expect(parseDepfile(text)).toEqual(['/a/x.rs', '/a/with space.rs', '/b.rs']);
  });
});

describe('patchkit key of a device', () => {
  it('follows the sources, the device itself and the build facts, not the other devices or test-only files', () => {
    const repo = makeRepo();
    const base = keys(repo);
    expect(base.get('aa')).toContain('ports/aa-1.toml'); // not in the dependency file: added from the device's files
    expect(base.get('aa')).not.toContain('devices/bb');
    expect(base.get('aa')).not.toContain('tests.rs');
    expect(base.get('aa')).not.toContain('device_files.rs'); // build output
    edit(repo, 'devices/bb.toml');
    expect(keys(repo).get('aa')).toBe(base.get('aa'));
    expect(keys(repo).get('bb')).not.toBe(base.get('bb'));
    const afterB = keys(repo);
    edit(repo, 'ports/aa-1.toml');
    expect(keys(repo).get('aa')).not.toBe(afterB.get('aa'));
    expect(keys(repo).get('bb')).toBe(afterB.get('bb'));
    const afterPorts = keys(repo);
    edit(repo, 'packages/patchkit/src/tests.rs');
    expect(keys(repo).get('aa')).toBe(afterPorts.get('aa'));
    edit(repo, 'packages/patchkit/src/apply.rs');
    expect(keys(repo).get('aa')).not.toBe(afterPorts.get('aa'));
    expect(keys(repo).get('bb')).not.toBe(afterPorts.get('bb'));
    expect(
      patchkitKeys({ depText: depText(repo), workspaceRoot: repo, facts: { ...FACTS, rustc: 'rustc 2' } }).get('aa'),
    ).not.toBe(keys(repo).get('aa'));
  });

  it('is the same in another checkout of the same files', () => {
    const files = { 'packages/patchkit/src/apply.rs': 'same' };
    const [a, b] = [makeRepo(files), makeRepo(files)];
    expect(keys(a).get('aa')).toBe(keys(b).get('aa'));
    expect(keys(a).get('aa')).not.toContain(a);
  });

  it('fails when the dependency file is stale or not the patchkit build', () => {
    const repo = makeRepo();
    expect(() => keys(repo, `t: /elsewhere/x.rs\n`)).toThrow(/outside the repository/);
    expect(() => keys(repo, `t: ${repo}/packages/patchkit/src/gone.rs\n`)).toThrow(/build patchkit again/);
    expect(() => keys(repo, `t: ${repo}/devices/aa.toml\n`)).toThrow(/lists no patchkit source/);
    const bad = makeRepo({ 'packages/patchkit/src/lib.rs': 'mod tests;\n' }); // test-only file compiled in
    expect(() => keys(bad)).toThrow(/not behind cfg\(test\)/);
  });
});
