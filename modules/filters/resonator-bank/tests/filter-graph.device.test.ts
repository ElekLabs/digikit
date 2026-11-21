// device-literals: allow
import { scheduledTurns } from '@digi/test-support';
import { expect, test } from 'vitest';
import { bootInfo } from '@digi/regression/runtime/card-kit.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { bounded, REDRAW, stableFrameHash, word, type Rt } from '@digi/regression/runtime/fx-live.ts';
import { halfword, selectTrack } from '@digi/regression/runtime/fx-track-ui.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/install.ts';

const BANK = 'digikit/resonator-bank';
// CAL KERNELS activates machines.dks before the first filter is loaded.
const BOOT_MODULES = [DEBUG_MODULE, 'digikit/remote', 'test/cal-kernels'];
const STOCK_FILTERS = 6;
const BANK_TYPE = 16; // The only module filter on this card takes the first module type.
const MIN_CHANGED_PIXELS = 20;
// Sound type and FLTR mirror indices from findings 148 and 158.
const LAYOUT = {
  dt2: { typeOffset: 0xa2, freq: 0x24, spread: 0x25, q: 0x23 },
  dn2: { typeOffset: 0xde, freq: 0x43, spread: 0x44, q: 0x42 },
};

async function responsePixels(rt: Rt): Promise<number[]> {
  await stableFrameHash(rt);
  const frame = await bounded(() => rt.session.client.framebufferGet({ data: true }));
  const bitmap = Buffer.from(frame.data_b64 ?? '', 'base64');
  expect(bitmap.byteLength, 'decoded firmware OLED frame').toBe(1024);

  // Response rectangle x21..71, y7..23 in the bottom-up FLTR bitmap. In top-down OLED coordinates it is
  // y40..56, below the stock envelope. Read the pixels individually to exclude the rest of the display.
  const pixels: number[] = [];
  for (let y = 40; y <= 56; y++)
    for (let x = 21; x <= 71; x++) pixels.push((bitmap[8 * x + 7 - (y >> 3)]! >> (y & 7)) & 1);
  return pixels;
}

cardRigs(
  'Resonator Bank response',
  {
    devices: ['dt2', 'dn2'],
    dsp: 'sharc', // The real Bank pack contains a DSP section that must be acknowledged at load.
    hold: true,
    initial: ({ mod }) => ({ modules: [...BOOT_MODULES, BANK], selection: BOOT_MODULES.map((id) => mod(id).name) }),
  },
  (rig) => {
    test('the first live filter can be selected and its response moves with FREQ, SPRD and Q', async () => {
      const rt = await rig.first();
      const { session, ui } = rt;
      const boot = await bootInfo(session, rig.plan);
      expect({ loaded: boot.loaded, safe: boot.safe, results: boot.results.slice(0, boot.count) }).toEqual({
        loaded: BOOT_MODULES.length,
        safe: 0,
        results: BOOT_MODULES.map(() => 0),
      });

      const bankPath = rig.mod(BANK).name;
      await rig.usb(rt, async (dk) => {
        const names = (await dk.modules()).map((m) => m.name).sort();
        expect(names, 'only the boot modules are active').toEqual([...BOOT_MODULES].sort());
        expect(await dk.loadAndWait(bankPath), 'the first live filter loads').toEqual({ result: 0, unloaded: false });
        const row = (await dk.loadStates()).find((m) => m.name === bankPath.split('/').at(-1));
        expect(row, 'the filter is active after the live job').toMatchObject({ state: 'ACTIVE' });
      });
      expect((await session.status()).epoch, 'the load kept the boot epoch').toBe(rt.epoch);

      const { fx } = deviceProfile(rig.device);
      const layout = LAYOUT[rig.device as keyof typeof LAYOUT];
      const sound = (await word(session, fx.kit_ptr)) + fx.sound_off;
      const types = () => halfword(session, sound + layout.typeOffset);
      await selectTrack(rt, 1);
      const before = await types();
      expect(before & 0xff, 'track 1 starts on a stock filter').toBeLessThan(STOCK_FILTERS);

      await ui.tap('FLTR', { until: { ms: 200, why: REDRAW } });
      // SRC and SYN share a panel key code. The stock chooser starts at the active filter.
      await ui.tap('SRC', { held: 'FUNC', until: { ms: 200, why: REDRAW } });
      for (let i = 0; i < STOCK_FILTERS; i++) await ui.tap('UP', { until: { ms: 80, why: REDRAW } });
      for (let i = 0; i < STOCK_FILTERS; i++) await ui.tap('DOWN', { until: { ms: 80, why: REDRAW } });
      await ui.tap('YES', {
        until: {
          state: types,
          done: (value) => value === ((before & 0xff00) | BANK_TYPE),
          withinMs: 1000,
          what: 'track 1 selects Resonator Bank and keeps its machine',
        },
      });
      await ui.tap('NO', { until: { ms: 200, why: REDRAW } });

      const turns = scheduledTurns(session, rt.clock);
      const controls = [
        { label: 'FREQ', encoder: 'ENC E', index: layout.freq, initial: 127, next: 48 },
        { label: 'SPRD', encoder: 'ENC F', index: layout.spread, initial: 29, next: 92 },
        { label: 'Q', encoder: 'ENC G', index: layout.q, initial: 3, next: 96 },
      ];
      async function set(control: (typeof controls)[number], value: number) {
        const read = () => halfword(session, sound + 0x14 + 2 * control.index);
        await turns.turnTo(control.encoder, read, value << 8, { tolerance: 0x7f, maxBursts: 80 });
      }
      for (const control of controls) await set(control, control.initial);
      let previous = await responsePixels(rt);
      expect(previous.filter(Boolean).length, 'response pixels at FREQ 127, SPRD 29, Q 3').toBeGreaterThan(30);
      for (const control of controls) {
        await set(control, control.next);
        const pixels = await responsePixels(rt);
        expect(pixels.filter((pixel, i) => pixel !== previous[i]).length, `${control.label} moves the FLTR response`).toBeGreaterThan(
          MIN_CHANGED_PIXELS,
        );
        previous = pixels;
      }
      await rig.usb(rt, async (dk) => {
        expect(await dk.reloadAndWait(bankPath), 'the selected filter can be replaced').toEqual({ result: 0, unloaded: true });
        expect((await dk.modules()).map((m) => m.name).sort()).toEqual([...BOOT_MODULES, BANK].sort());
      });
      expect(await types(), 'replacement keeps the assigned filter type').toBe((before & 0xff00) | BANK_TYPE);
    }, 900_000);
  },
);
