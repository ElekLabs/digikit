import path from 'node:path';
import type { Device } from '@digi/test-support';
import { expect, test } from 'vitest';
import { bootInfo, bootInfoLoaded } from '@digi/regression/runtime/guest-mailbox.ts';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { type CardRig } from '@digi/regression/runtime/card-rig-api.ts';
import { capture } from '@digi/regression/runtime/note-capture.ts';
import { peak, readFloatWav } from '@digi/regression/runtime/pcm-files.ts';
import { introNow } from '@digi/regression/runtime/intro-frame.ts';
import { ringEvents } from '@digi/regression/runtime/runtime-menu.ts';
import { bootScreenCard, IB, lastFrame } from './introblank-support.ts';

/**
 * core/introblank: the stock intro draws black (the adapter's scene table replaces the stock one at the intro setup) until a boot screen module or the
 * safe-mode screen takes over; when nothing takes over, the runtime ends the intro after the module load. The tests run on an image with
 * the adapter. The framebuffer is read at the ready point: the intro's last frame. The stock intro's length is a constant (STOCK_TICKS, the five stock
 * scene tables), so no run on the stock image is needed as a reference. REC held is in tools/bootscreen bootscreen-safe (the same boot, the safe-mode frame).
 */

/** The stock intro's length in ticks (the five stock scene tables and the adapter's table). */
const STOCK_TICKS = 175;
/** The intro's drawn-frame counter (`_DAT_43153a0c` on the DT2, `_DAT_42c4cad8` on the DN2: the setup function clears it). */
const TICK_COUNTER: Record<Device, number> = { dt2: 0x43153a0c, dn2: 0x42c4cad8, st: 0 }; // st: no intro test
/** ColdFire instructions per DSP block (132 M a second, 1500 blocks a second). */
const INSNS_PER_BLOCK = 88_000;

const BLACK = Buffer.alloc(1024);

/** What the boot reached, from the runtime's UI_FIRST event (DSP blocks at the first UI pass) and the intro's own counter. */
async function uiReached(h: CardRig) {
  const events = await ringEvents(h.session, h.plan, 'UI_FIRST');
  const blocks = events.at(-1)!.args[0]! >>> 0;
  const ticks = (await h.session.client.memPeek(TICK_COUNTER[h.device]))! >>> 0;
  return { blocks, insns: blocks * INSNS_PER_BLOCK, ticks };
}

/** A note of the sine machine reaches the PCM tap: the DSP runs and the machine list is complete. */
async function playsANote(h: CardRig) {
  const sine = h.mod('test/sine', 'sine');
  await h.load('note', { modules: [sine], selection: [sine.name] });
  const info = await bootInfoLoaded(h.session, h.plan);
  expect(info).toMatchObject({ safe: 0, loaded: 1, uiReached: 1 });
  expect((await introNow(h.session)).equals(BLACK), 'black: the adapter ended the intro').toBe(true);
  const status = await h.session.status();
  expect(status.dsp).toBe('sharc');
  expect(status.audio?.halted ?? false).toBeFalsy();
  // the first SRC list after a boot may not take (the project round trip tries the list position up to four times): same position twice, then its neighbours
  let best = 0;
  let tries = 0;
  for (const hint of [undefined, undefined, 6, 8]) {
    const wav = path.join(h.artifacts, `note-${tries++}.wav`);
    await capture(h.runtime, wav, hint);
    best = peak(readFloatWav(wav));
    if (best > 0.01) break;
  }
  console.log(`INTROBLANK ${h.device} adapter note peak=${best} tries=${tries}`);
  expect(best, 'the machine makes sound').toBeGreaterThan(0.01);
}

cardRigs('introblank', { dsp: 'sharc', relaxFs: true, roots: IB, symbols: CARD_SYMBOLS }, (h) => {
  test(
    'no boot screen: the intro is black and ends before the stock length',
    async () => {
      await h.load('black', { modules: [], selection: [] });
      const info = await bootInfo(h.session, h.plan);
      expect(info).toMatchObject({ safe: 0, uiReached: 1 });
      const ui = await uiReached(h);
      console.log(`INTROBLANK ${h.device} adapter blocks=${ui.blocks} insns=${ui.insns} ticks=${ui.ticks}`);
      expect((await introNow(h.session)).equals(BLACK), 'the intro framebuffer is all black').toBe(true);
      expect(ui.ticks, 'the intro ended early').toBeLessThan(STOCK_TICKS);
    },
  );

  test(
    'a boot screen is selected: the module takes over and its last frame stays',
    async () => {
      await h.load('screen', bootScreenCard(h));
      const info = await bootInfoLoaded(h.session, h.plan);
      expect(info).toMatchObject({ safe: 0, loaded: 1, uiReached: 1 });
      expect((await introNow(h.session)).equals(lastFrame), 'the module frame').toBe(true);
    },
  );

  test(
    'after the early end the boot is complete: a note plays and the DSP runs',
    () => playsANote(h),
  );
});
