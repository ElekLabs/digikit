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
    expect(() =>
      parse({ 's.toml': st.replace('readonly = [1, 2, 4, 6, 7, 8]', 'readonly = [1, 2, 4, 6, 8]') }),
    ).toThrow(/cf2 section 7 must be in readonly/);
    expect(() =>
      parse({ 's.toml': st.replace('readonly = [1, 2, 4, 6, 7, 8]', 'readonly = [2, 4, 6, 7, 8]') }),
    ).toThrow(/fpga section 1 must be in readonly/);
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

  test('the file stem is kept, and the boot fields of a firmware go together', () => {
    const h = 'a'.repeat(64);
    const boot = `main_sha256 = "${h}"\nboot_contract = "c"\nsymbol_profile = "s"\n`;
    const fw = `[[firmware]]\nversion = "1.0"\nsha256 = "AB"\n${boot}`;
    const rec = parse({ 'dev-one.toml': identity('x', fw) }).get('x');
    expect(rec.stem).toBe('dev-one');
    expect(rec.patchSets).toEqual({});
    expect(rec.defaultModules).toEqual([]);
    const withDefaults =
      identity('x').replace('short = "x"', 'short = "x"\nsyx_id = 0x30') +
      '[patch]\ncontainer_max = 1\n[digikit]\nid = 1\ndefault_modules = ["a/b"]\n';
    expect(parse({ 'x.toml': withDefaults }).get('x').defaultModules).toEqual(['a/b']);
    expect(() => parse({ 'x.toml': withDefaults.replace('["a/b"]', '"a/b"') })).toThrow(/not a list of module ids/);
    expect(
      parse({ 'x.toml': identity('x', '[patch_sets]\ndevice = ["core/a", "core/b"]\n') }).get('x').patchSets,
    ).toEqual({ device: ['core/a', 'core/b'] });
    expect(rec.firmware[0]?.boot).toEqual({ mainSha256: h, contract: 'c', symbolProfile: 's' });
    expect(() =>
      parse({ 'x.toml': identity('x', `[[firmware]]\nversion = "1"\nsha256 = "AB"\nboot_contract = "c"\n`) }),
    ).toThrow(/go together/);
  });

  test('[storage] is optional, checked against its words, and refuses unknown keys', () => {
    expect(parse({ 'x.toml': identity('x') }).get('x').storage).toBeUndefined();
    const ok = identity('x', '[storage]\nsamples = "digikit"\nmodules = "digikit"\n');
    expect(parse({ 'x.toml': ok }).get('x').storage).toEqual({ samples: 'digikit', modules: 'digikit' });
    expect(() => parse({ 'x.toml': ok.replace('samples = "digikit"', 'samples = "tape"') })).toThrow(
      /\[storage\] samples/,
    );
    expect(() => parse({ 'x.toml': ok + 'extra = 1\n' })).toThrow(/\[storage\] has an unknown key extra/);
    expect(() => parse({ 'x.toml': identity('x', '[storage]\nsamples = "digikit"\n') })).toThrow(/\[storage\] modules/);
  });

  test('[storage.slots] belongs to stock-slots, is checked, and refuses what the converter does not make', () => {
    const slots = (extra = '') =>
      `[storage]\nsamples = "stock-slots"\nmodules = "digikit"\n[storage.slots]\npath = "/samples"\ncount = 64\nbytes_max = 0x7ffc0\nrate = 48000\nchannels = 1\nbits = 16\nbyte_order = "big"\napply = "restart"\n${extra}`;
    expect(parse({ 'x.toml': identity('x', slots()) }).get('x').storage?.slots).toEqual({
      path: '/samples',
      count: 64,
      bytesMax: 0x7ffc0,
      rate: 48000,
      channels: 1,
      bits: 16,
      byteOrder: 'big',
      apply: 'restart',
    });
    expect(() => parse({ 'x.toml': identity('x', slots().replace('bits = 16', 'bits = 24')) })).toThrow(/mono 16-bit/);
    expect(() => parse({ 'x.toml': identity('x', slots().replace('bytes_max = 0x7ffc0', 'bytes_max = 7')) })).toThrow(
      /even/,
    );
    expect(() => parse({ 'x.toml': identity('x', slots().replace('apply = "restart"', 'apply = "later"')) })).toThrow(
      /apply must be one of/,
    );
    expect(() => parse({ 'x.toml': identity('x', slots('extra = 1\n')) })).toThrow(/unknown key extra/);
    expect(() =>
      parse({ 'x.toml': identity('x', '[storage]\nsamples = "stock-slots"\nmodules = "digikit"\n') }),
    ).toThrow(/needed by samples/);
    expect(() =>
      parse({ 'x.toml': identity('x', slots().replace('samples = "stock-slots"', 'samples = "digikit"')) }),
    ).toThrow(/needed by samples/);
  });

  test('[midi] is optional and checks channels, cc and nrpn ranges', () => {
    const midi = (params: string, chans = '[1, 2]') =>
      identity('x', `[midi]\ntrack_channels = ${chans}\n[midi.params]\n${params}\n`);
    expect(parse({ 'x.toml': identity('x') }).get('x').midi).toBeUndefined();
    expect(parse({ 'x.toml': midi('a = { cc = 74, nrpn = [1, 20] }\nb = { nrpn = [1, 2] }') }).get('x').midi).toEqual({
      trackChannels: [1, 2],
      params: { a: { cc: 74, nrpn: [1, 20] }, b: { nrpn: [1, 2] } },
    });
    expect(() => parse({ 'x.toml': midi('a = { cc = 128 }') })).toThrow(/cc must be 0 to 127/);
    expect(() => parse({ 'x.toml': midi('a = { nrpn = [1] }') })).toThrow(/nrpn must be/);
    expect(() => parse({ 'x.toml': midi('a = {}') })).toThrow(/needs cc or nrpn/);
    expect(() => parse({ 'x.toml': midi('a = { cc = 1 }', '[0]') })).toThrow(/track_channels/);
  });

  test('a TOML syntax error names the file', () => {
    expect(() => parse({ 'broken.toml': '[device\n' })).toThrow(/devices\/broken\.toml/);
  });
});

describe('the repository device files', () => {
  test('both loaders read the same registry, and every test cell is on a runtime target', () => {
    const r = loadDevices();
    expect(bundled.devices).toEqual(r.devices);
    expect(r.ids).toEqual(expect.arrayContaining(['dt2', 'dn2']));
    for (const d of r.devices) {
      expect(d.firmware.length, d.id).toBeGreaterThan(0);
      if (d.acceptance) expect(d.level, d.id).toBe('runtime');
    }
    expect(r.get('dt2').acceptance).toBeDefined();
    expect(r.get('dn2').acceptance).toBeDefined();
    expect(r.get('dt2').capabilities.card).toBe('path-fs');
  });

  test('the audio devices carry the MIDI map the calibration runner uses', () => {
    const r = loadDevices();
    for (const id of ['dt2', 'dn2']) {
      const m = r.get(id).midi;
      expect(m, id).toBeDefined();
      for (const name of [
        'track_mute',
        'track_level',
        'filter_freq',
        'filter_reso',
        'filter_env_depth',
        'amp_release',
        'send_delay',
        'send_reverb',
        'send_chorus',
      ])
        expect(m?.params[name], `${id} ${name}`).toBeDefined();
    }
  });

  test('the slot device keeps its samples in numbered slots', () => {
    const st = loadDevices().get('st').storage;
    expect(st).toMatchObject({ samples: 'stock-slots', slots: { path: '/samples', count: 64, bytesMax: 0x7ffc0 } });
  });
});
