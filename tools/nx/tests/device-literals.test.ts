// device-literals: allow (the fixtures name devices on purpose)
import { describe, expect, test } from 'vitest';
import { allowedPath, compare, countLiterals, literalPatterns, scan } from '../device-literals.mjs';

const registry = {
  devices: [
    { id: 'dt2', name: 'Digitakt II' },
    { id: 'dn2', name: 'Digitone II' },
  ],
};
const p = literalPatterns(registry);

describe('countLiterals', () => {
  test('counts ids at word edges and device names anywhere bounded', () => {
    expect(countLiterals('a.ts', "if (dev === 'dt2') use('Digitone II')", p.text)).toBe(2);
    expect(countLiterals('a.ts', 'const adt2x = 1; // dt2_foo', p.text)).toBe(1);
    expect(countLiterals('a.ts', 'digitakt2 dt2fx', p.text)).toBe(0);
  });

  test('the marker must be a comment line of its own in the first 5 lines', () => {
    expect(countLiterals('a.ts', "// device-literals: allow\nconst d = 'dt2';", p.text)).toBe(0);
    expect(countLiterals('a.py', "# device-literals: allow\nd = 'dt2'", p.text)).toBe(0);
    expect(countLiterals('a.ts', "const d = 'dt2'; // device-literals: allow", p.text)).toBe(1);
    expect(countLiterals('a.ts', "\n\n\n\n\n// device-literals: allow\nconst d = 'dt2';", p.text)).toBe(1);
  });

  test('a manifest exempts table headers and the whole value of ports and devices, not other lines', () => {
    const toml = [
      'ports = ["dt2-1.17"]',
      'devices = [',
      '  "dt2",',
      '  "dn2",',
      ']',
      '[rule.dt2]',
      '[[gen.dt2]]',
      '"dn2-1.12" = "untested"',
      'note = "dt2 only"',
      'x = ["dt2"]',
    ].join('\n');
    expect(countLiterals('module.toml', toml, p.text)).toBe(3);
    expect(countLiterals('module.json', 'ports = "dt2"', p.text)).toBe(1);
  });
});

describe('scan and compare', () => {
  test('docs, devices/ and bounded path tokens are not scanned; a name that only starts with a literal is', () => {
    for (const path of [
      'devices/x.toml',
      'docs/a.md',
      'tools/foo_dt2.py',
      'tests/regression/dn2/a.ts',
      'a/device-dt2.inc',
    ])
      expect(allowedPath(path, p.path), path).toBe(true);
    for (const path of ['README.md', 'tools/foo.py', 'digitakt2/a.ts', 'tools/dt2fx.py'])
      expect(allowedPath(path, p.path), path).toBe(false);
    const { found, binary } = scan(
      ['tools/foo.py', 'tools/foo_dt2.py', 'b.bin'],
      (f) => (f === 'b.bin' ? undefined : "x = 'dt2'"),
      p,
    );
    expect(found).toEqual({ 'tools/foo.py': 1 });
    expect(binary).toBe(1);
  });

  test('growth, new files and shrinking are all differences', () => {
    const r = compare({ a: 3, b: 1, c: 2 }, { a: 2, b: 2, c: 2, gone: 4 });
    expect(r.grew).toEqual([{ path: 'a', was: 2, now: 3 }]);
    expect(r.shrank.map((s) => s.path).sort()).toEqual(['b', 'gone']);
    expect(compare({ n: 1 }, {}).grew).toHaveLength(1);
    expect(compare({ a: 1 }, { a: 1 })).toEqual({ grew: [], shrank: [] });
  });
});
