// device-literals: allow (the last test checks the repository's own device files)
import { describe, expect, test } from 'vitest';
import { loadDevices, parse } from '../src/node.ts';
import { registry as bundled } from '../src/vite.ts';

const CAPS = '[capabilities]\ndsp = "none"\nseal = "none"\nsample_pool = "none"\ncard = "unknown"\n';
const identity = (short: string, extra = '') =>
  `[device]\nname = "Dev ${short}"\nshort = "${short}"\n${CAPS}[container]\nmain = 1\nreadonly = [2]\n${extra}`;

describe('parse', () => {
  test('a device file with only an identity is a device of level identity', () => {
    const r = parse({ 'x.toml': identity('x') });
    expect(r.ids).toEqual(['x']);
    expect(r.get('x').level).toBe('identity');
    expect(r.get('x').syxId).toBeUndefined();
  });

  test('levels follow the tables that are present', () => {
    const base = identity('x').replace('short = "x"', 'short = "x"\nsyx_id = 0x30');
    expect(parse({ 'x.toml': base + '[patch]\ncontainer_max = 1\n' }).get('x').level).toBe('patchable');
    expect(parse({ 'x.toml': base + '[patch]\ncontainer_max = 1\n[digikit]\nid = 1\n' }).get('x').level).toBe(
      'runtime',
    );
    expect(parse({ 'x.toml': identity('x') }).get('x').level).toBe('identity');
  });

  test('files without [device] (the LED maps) are skipped', () => {
    expect(parse({ 'x-leds.toml': '[leds]\na = 1\n', 'x.toml': identity('x') }).ids).toEqual(['x']);
  });

  test('device() accepts a known id and names the known ids otherwise', () => {
    const r = parse({ 'a.toml': identity('a'), 'b.toml': identity('b') });
    expect(r.device('a')).toBe('a');
    expect(() => r.device('c')).toThrow(/unknown device "c" \(known: a, b\)/);
  });

  test('capabilities are required and checked against their words', () => {
    expect(() => parse({ 'x.toml': '[device]\nname = "X"\nshort = "x"\n' })).toThrow(/\[capabilities\]/);
    expect(() => parse({ 'x.toml': identity('x').replace('card = "unknown"', 'card = "floppy"') })).toThrow(/card/);
    expect(() => parse({ 'x.toml': identity('x').replace('seal = "none"', 'seal = "none"\nextra = 1') })).toThrow(
      /unknown key/,
    );
  });

  test('the container must agree with the capabilities and keep roles apart', () => {
    expect(() => parse({ 'x.toml': identity('x').replace('main = 1', 'main = 1\ndsp = 7') })).toThrow(
      /dsp is present exactly/,
    );
    const sharc = identity('x').replace('dsp = "none"', 'dsp = "sharc"');
    expect(() => parse({ 'x.toml': sharc })).toThrow(/dsp is present exactly/);
    expect(parse({ 'x.toml': sharc.replace('main = 1', 'main = 1\ndsp = 7') }).get('x').container.dsp).toBe(7);
    expect(() => parse({ 'x.toml': identity('x').replace('readonly = [2]', 'readonly = [1]') })).toThrow(/read-only/);
  });

  test('a Syntakt-like container has read-only role sections; a runtime target must be patchable', () => {
    const st =
      '[device]\nname = "S"\nshort = "st"\n[capabilities]\ndsp = "none"\nseal = "none"\nsample_pool = "none"\ncard = "unknown"\n' +
      '[container]\nmain = 3\ncf2 = 7\nfpga = 1\nreadonly = [1, 2, 4, 6, 7, 8]\n';
    const c = parse({ 's.toml': st }).get('st').container;
    expect([c.cf2, c.fpga, c.readonly]).toEqual([7, 1, [1, 2, 4, 6, 7, 8]]);
    expect(() => parse({ 's.toml': st + '[digikit]\nid = 5\n' })).toThrow(/must be patchable/);
  });

  test('a patchable device needs its transport id, and a device is described once', () => {
    expect(() => parse({ 'x.toml': identity('x', '[patch]\ncontainer_max = 1\n') })).toThrow(/syx_id/);
    expect(() => parse({ 'a.toml': identity('x'), 'b.toml': identity('x') })).toThrow(/twice/);
  });

  test('the acceptance firmware must be one of the device firmware versions', () => {
    const fw = '[[firmware]]\nversion = "1.0"\nsha256 = "AB"\n';
    const acc = (v: string) => `[acceptance]\nfirmware = "${v}"\nready_dir = "out/r"\nsample_card = false\n`;
    expect(parse({ 'x.toml': identity('x', fw + acc('1.0')) }).get('x').acceptance).toEqual({
      firmware: '1.0',
      readyDir: 'out/r',
      sampleCard: false,
    });
    expect(() => parse({ 'x.toml': identity('x', fw + acc('2.0')) })).toThrow(/not a \[\[firmware\]\] version/);
  });

  test('a TOML syntax error names the file', () => {
    expect(() => parse({ 'broken.toml': '[device\n' })).toThrow(/devices\/broken\.toml/);
  });
});

describe('the repository device files', () => {
  test('both loaders read the same registry, and each device states its test cell', () => {
    const r = loadDevices();
    expect(bundled.devices).toEqual(r.devices);
    expect(r.ids).toEqual(expect.arrayContaining(['dt2', 'dn2']));
    for (const d of r.devices) {
      expect(d.level).toBe('runtime');
      expect(d.acceptance, d.id).toBeDefined();
      expect(d.firmware.length).toBeGreaterThan(0);
    }
    expect(r.get('dt2').capabilities.card).toBe('path-fs');
    expect(r.get('dn2').acceptance?.readyDir).toBe('out/ready-fix-dn2-112');
  });
});
