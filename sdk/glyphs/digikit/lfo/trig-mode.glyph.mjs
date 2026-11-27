import { BOX, colPlot, sine } from '../wave/shape.mjs';
// LFO trig modes: a short wave (12 columns per period) with the trig marker above it.
const WB = { ...BOX, y0: 5 };
const P = 12;
const s = (i) => sine(i / P);
const levels = {
  // free running: no marker
  FRE: { fn: (i) => s(i) },
  // restart at the marker: the old cycle is cut off, the wave starts again from zero
  TRG: { mark: 8, fn: (i) => (i < 8 ? s(i + 3) : s(i - 8)) },
  // hold: the wave stops at the marker and the value stays
  HLD: { mark: 6, fn: (i) => s(Math.min(i, 5)) },
  // one full period after the marker, then flat
  ONE: { mark: 1, fn: (i) => (i >= 1 && i <= 13 ? s(i - 1) : 0) },
  // half a period after the marker, then flat
  HLF: { mark: 1, fn: (i) => (i >= 1 && i <= 7 ? s(i - 1) : 0) },
};
const order = ['FRE', 'TRG', 'HLD', 'ONE', 'HLF'];
export default {
  title: 'LFO trig mode',
  size: 'knob',
  states: order,
  tags: ['lfo'],
  draw(p, i) {
    const { fn, mark } = levels[order[i]];
    colPlot(p, fn, WB);
    if (mark === undefined) return;
    // marker: 3x2 solid triangle pointing down, just above the wave at the trigger column
    const x = WB.x0 + mark;
    p.line(x - 1, 2, x + 1, 2);
    p.px(x, 3);
  },
};
