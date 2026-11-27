import type { GlyphJson } from '../src/export.ts';
import { canvasOf, decode, drawBits } from './bitmap.ts';
import { el } from './element.ts';
import type { ViewState } from './state.ts';

const DETAIL_SCALE = 16;
const DETAIL_MAX_WIDTH = 420;

function draw(canvas: HTMLCanvasElement, g: GlyphJson, hex: string, s: ViewState, scale = DETAIL_SCALE): void {
  drawBits(canvas.getContext('2d')!, decode(hex, g.w, g.h), g.w, g.h, 0, 0, {
    scale,
    grid: s.grid,
    inverted: s.inverted,
  });
}

/** Detail panel: large view, frame strip, value slider, state buttons, transition playback and check messages. */
export function renderDetail(root: HTMLElement, g: GlyphJson | undefined, s: ViewState): void {
  root.replaceChildren();
  if (!g || !s.set) {
    root.append(el('p', { class: 'muted' }, 'Click a glyph for details.'));
    return;
  }
  const bigScale = Math.max(1, Math.min(DETAIL_SCALE, Math.floor(DETAIL_MAX_WIDTH / g.w)));
  const big = canvasOf(g.w, g.h, bigScale);
  const show = (hex: string) => draw(big, g, hex, s, bigScale);
  const info = `${g.id}  ${g.w}x${g.h}  ${g.frames.length} frame${g.frames.length === 1 ? '' : 's'}  ${g.kind}${g.range ? ` ${g.range}` : ''}`;
  root.append(el('h2', {}, 'Detail'), el('div', {}, info), big);
  show(g.frames[0]!);

  if (g.frames.length > 1) {
    const strip = el('div', { class: 'strip' });
    for (const hex of g.frames) {
      const c = canvasOf(g.w, g.h, 4);
      draw(c, g, hex, s, 4);
      strip.append(c);
    }
    root.append(el('h2', {}, 'Frames'), strip);
    if (g.kind === 'states' && g.states) {
      for (const t of g.transitions ?? []) {
        const row = el('div', { class: 'strip' });
        for (const hex of [g.frames[t.from]!, ...t.frames, g.frames[t.to]!]) {
          const c = canvasOf(g.w, g.h, 4);
          draw(c, g, hex, s, 4);
          row.append(c);
        }
        root.append(el('div', {}, `${g.states[t.from]} → ${g.states[t.to]}`), row);
      }
    }
  }

  if (g.kind === 'value' && g.valueMap) {
    const [lo, hi] = g.range === 'bipolar' ? [-1, 1] : [0, 1];
    const n = g.valueMap.length;
    const readout = el('span', {}, '');
    const at = (frac: number) => {
      s.value = frac;
      const idx = Math.round(frac * (n - 1));
      show(g.frames[g.valueMap![idx]!]!);
      readout.textContent = `v = ${(lo + (hi - lo) * frac).toFixed(2)}  frame ${g.valueMap![idx]}`;
    };
    const slider = el('input', { type: 'range', min: 0, max: n - 1, value: Math.round(s.value * (n - 1)) });
    slider.oninput = () => at(Number(slider.value) / (n - 1));
    let timer: number | undefined;
    const play = el('button', {}, 'Sweep');
    play.onclick = () => {
      if (timer !== undefined) {
        clearInterval(timer);
        timer = undefined;
        return;
      }
      let i = 0;
      timer = window.setInterval(() => {
        const k = i % (2 * n - 2 || 1);
        const idx = k < n ? k : 2 * n - 2 - k;
        slider.value = String(idx);
        at(idx / (n - 1));
        i += 1;
      }, 60);
    };
    at(s.value);
    root.append(el('h2', {}, 'Value'), slider, el('div', { class: 'toolbar' }, play, readout));
  }

  if (g.kind === 'states' && g.states) {
    const bar = el('div', { class: 'toolbar' });
    const go = (to: number) => {
      const from = s.state;
      s.state = to;
      const tr = g.transitions?.find((t) => t.from === from && t.to === to);
      const seq = [...(tr?.frames ?? []), g.frames[to]!];
      seq.forEach((hex, k) => window.setTimeout(() => show(hex), k * 120));
    };
    g.states.forEach((name, i) => bar.append(el('button', { onclick: () => go(i) }, name)));
    show(g.frames[Math.min(s.state, g.frames.length - 1)]!);
    root.append(el('h2', {}, 'States'), bar);
  }

  const msgs = el('div', {});
  for (const m of s.set.report.errors.filter((e) => e.id === g.id))
    msgs.append(el('div', { class: 'msg error' }, m.text));
  for (const m of s.set.report.warnings.filter((e) => e.id === g.id))
    msgs.append(el('div', { class: 'msg warning' }, m.text));
  if (!msgs.childElementCount) msgs.append(el('div', { class: 'muted' }, 'No check messages.'));
  root.append(el('h2', {}, 'Checks'), msgs);
}
