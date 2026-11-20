import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'vitest';
import { loadDevices } from '@digi/devices/node';
import { addModuleCases, canonicalJson, parseCalCase, parseCalProfile } from '../src/calibration/calprofile';
import { resolveRef, parseStepFile, type StepLibrary } from '../src/calibration/steps';

const base = `schema = "dk.calprofile/1"\nname = "p"\nwarmup_s = 1\ncapture_s = 2\nrepeats = 2\n`;
const stock = `[[case]]\nid = "stock"\nsteps = "s"\n`;

test('a profile parses with its cases and defaults', () => {
  const p = parseCalProfile(
    base + 'modules = ["a/b"]\n' + stock + `[[case]]\nid = "x"\nmodules = ["a/b"]\nsteps = "s"\nargs = { t = 1 }\n`,
    'p.toml',
  );
  assert.equal(p.repeats, 2);
  assert.deepEqual(
    p.cases.map((c) => [c.id, c.modules]),
    [
      ['stock', []],
      ['x', ['a/b']],
    ],
  );
  assert.deepEqual(p.cases[1]!.steps, { steps: 's', args: { t: 1 } });
});

test('a profile without the module-free case "stock" is refused, and so is a stock case that loads a module', () => {
  assert.throws(() => parseCalProfile(base + `[[case]]\nid = "x"\nsteps = "s"\n`, 'p'), /module-free case "stock"/);
  assert.throws(
    () =>
      parseCalProfile(base + 'modules = ["a/b"]\n' + `[[case]]\nid = "stock"\nmodules = ["a/b"]\nsteps = "s"\n`, 'p'),
    /"stock" loads no modules/,
  );
});

test('other mistakes are refused with the file name', () => {
  assert.throws(
    () => parseCalProfile(base.replace('repeats = 2', 'repeats = 0') + stock, 'p.toml'),
    /p\.toml: repeats/,
  );
  assert.throws(() => parseCalProfile(base + stock + stock, 'p'), /used twice/);
  assert.throws(() => parseCalProfile(base + `[[case]]\nid = "stock"\n`, 'p'), /needs steps/);
  assert.throws(
    () => parseCalProfile(base + stock + `[[case]]\nid = "m"\nmodules = ["q/r"]\nsteps = "s"\n`, 'p'),
    /not in the profile's modules/,
  );
  assert.throws(() => parseCalProfile('oops = 1\n' + base + stock, 'p'), /unknown key oops/);
  assert.throws(() => parseCalProfile(base.replace('dk.calprofile/1', 'x') + stock, 'p'), /schema/);
});

test('module cases are discovered from the files the modules ship, and only by a profile that asks', () => {
  const p = parseCalProfile(base + 'discover = "modules"\n' + stock, 'p');
  const found = [
    parseCalCase('schema = "dk.calcase/1"\nid = "zz"\nmodule = "x/z"\nsteps = "s"\nargs = { machine = "x/z" }\n', 'a'),
    parseCalCase('schema = "dk.calcase/1"\nid = "aa"\nmodule = "x/a"\nsteps = "s"\n', 'b'),
  ];
  const out = addModuleCases(p, found);
  assert.deepEqual(
    out.cases.map((c) => [c.id, c.modules]),
    [
      ['stock', []],
      ['aa', ['x/a']],
      ['zz', ['x/z']],
    ],
  );
  assert.deepEqual(out.modules, ['x/a', 'x/z']);
  assert.equal(addModuleCases(parseCalProfile(base + stock, 'p'), found).cases.length, 1);
  assert.throws(() => addModuleCases(p, [{ ...found[0]!, id: 'stock' }]), /repeats a case id/);
  assert.throws(() => parseCalCase('schema = "dk.calcase/1"\nid = "q"\nsteps = "s"\n', 'c'), /module is missing/);
});

test('the same value hashes the same whatever the key order', () => {
  assert.equal(canonicalJson({ b: 1, a: [2, { d: 1, c: undefined }] }), '{"a":[2,{"d":1}],"b":1}');
});

// the shipped files: every profile parses, every step it names exists and resolves on both audio devices
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const read = (d: string) =>
  fs
    .readdirSync(path.join(root, 'devices/calibration', d))
    .filter((f) => f.endsWith('.toml'))
    .map((f) => [f, fs.readFileSync(path.join(root, 'devices/calibration', d, f), 'utf8')] as const);

test('the shipped profiles and step files are valid and resolve on every device with a [midi] table', () => {
  const lib: StepLibrary = Object.fromEntries(
    read('steps').map(([f, t]) => {
      const s = parseStepFile(t, f);
      return [s.name, s];
    }),
  );
  const found = ['machines', 'fx', 'tools'].flatMap((kind) =>
    fs
      .readdirSync(path.join(root, 'modules', kind))
      .map((n) => path.join(root, 'modules', kind, n, 'calibration.toml'))
      .filter((f) => fs.existsSync(f))
      .map((f) => parseCalCase(fs.readFileSync(f, 'utf8'), f)),
  );
  assert.deepEqual(found.map((c) => c.id).sort(), ['analog', 'bd', 'freqshift', 'pluck', 'shot', 'tb303']);
  const profiles = read('profiles').map(([f, t]) => addModuleCases(parseCalProfile(t, f), found));
  assert.deepEqual(profiles.map((p) => p.name).sort(), ['heavy', 'kernels', 'light', 'modules']);
  const devices = loadDevices().devices.filter((d) => d.midi);
  assert.ok(devices.length >= 2);
  for (const d of devices)
    for (const p of profiles)
      for (const c of p.cases) {
        const ctx = { midi: d.midi, where: `${d.id} ${p.name} ${c.id}` };
        assert.ok(resolveRef(c.steps, lib, ctx).length > 0);
        if (p.teardown) resolveRef(p.teardown, lib, ctx);
        if (c.teardown) resolveRef(c.teardown, lib, ctx);
      }
  // kernels: the stock case and one case per machine of the calibration module, in the order of its [[machine]] entries
  const kernels = profiles.find((p) => p.name === 'kernels')!;
  assert.deepEqual(
    kernels.cases.map((c) => [c.id, 'args' in c.steps ? c.steps.args?.machine : undefined]),
    [
      ['stock', undefined],
      ['alu', 'test/cal-kernels#1'],
      ['dm', 'test/cal-kernels#2'],
      ['mem', 'test/cal-kernels#3'],
      ['branch', 'test/cal-kernels#4'],
      ['mix', 'test/cal-kernels#5'],
    ],
  );
  assert.ok(kernels.cases.slice(1).every((c) => c.modules.join() === 'test/cal-kernels' && c.teardown));
  // heavy plays every audio track of the device
  const heavy = profiles.find((p) => p.name === 'heavy')!;
  for (const d of devices) {
    const notes = resolveRef(heavy.cases[0]!.steps, lib, { midi: d.midi, where: 'h' }).filter(
      (o) => o.op === 'midi.note',
    );
    assert.equal(notes.length, d.midi!.trackChannels.length);
  }
});
